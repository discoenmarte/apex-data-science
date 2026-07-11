# apex-mcp — Diseño del Paso 2: Acceso MCP a la Ontología + Insights en Runtime

> Estado: **diseño para revisión** · Autor: sesión Claude + Juan · Fecha: 2026-07-10
> Insumos: diagrama APEX ARCH / DS PIPELINE, `apex-data-profiling-pipeline.pdf`,
> `agentic_data_workflows_rust.pdf`, [rust-sdk oficial de MCP](https://github.com/modelcontextprotocol/rust-sdk), grafo real ya cargado (paso 1).

## 1. Problema y objetivo

El paso 1 dejó un **grafo de conocimiento en Neo4j** (7 esquemas, 4,542 tablas, 38,630
columnas, 7,763 FKs de la base `CLIENT_DB`, 508 GB). El paso 2 debe permitir que **agentes
LLM hagan preguntas de negocio y reciban insights rápidos** — por ejemplo *"¿cómo está la
cartera?"* — **sin migrar ni poblar la base completa**: la información se busca en runtime,
solo la que se necesita.

En el diagrama general esto es la caja **"Business Logic DDD Arch - MCP's"** y las flechas
**"MCP Communication"** / **"Knowledge Graph Access to Ontology"**: el servidor MCP es la
frontera entre los agentes (APEX ARCH / ADK Rust) y los datos.

## 2. Principio rector: federación, no replicación

Del documento de profiling del equipo tomamos la separación por costo:

| | Fase barata (siempre) | Fase costosa (solo si se necesita) |
|---|---|---|
| Paso 1 (hecho) | Perfilar **esquemas** completos | — |
| Paso 2 (este diseño) | Consultar la **ontología** local (Neo4j) | Consultas **puntuales** al Postgres origen vía VPN |
| Paso 3 (futuro) | — | Materializar tablas calientes en ClickHouse / Parquet-R2 |

La regla: **el grafo enruta, la fuente responde, y solo se materializa lo que se pregunta
repetidamente**. Nunca se copia la base entera.

## 3. Arquitectura

```
                          ┌────────────────────────────────────────────────┐
   Agentes (ADK Rust,     │                apex-mcp (Rust)                 │
   Claude, cualquier ─────┤  MCP server (rmcp): stdio + streamable HTTP    │
   cliente MCP)           │                                                │
                          │  ┌──────────────────┐  ┌─────────────────────┐ │
                          │  │ Tools de ONTOLOGÍA│  │ Tools de DATOS      │ │
                          │  │ (grafo local)     │  │ (runtime, guardados)│ │
                          │  └────────┬─────────┘  └──────────┬──────────┘ │
                          └───────────┼───────────────────────┼────────────┘
                                      │ bolt                  │ port-forward VPN
                                      ▼                       ▼
                            ┌──────────────────┐    ┌──────────────────────┐
                            │ Neo4j + GDS      │    │ Postgres CLIENT_DB    │
                            │ (ontología,      │    │ (fuente, solo SELECT │
                            │  local, sin VPN) │    │  con guardrails)     │
                            └──────────────────┘    └──────────────────────┘
```

Dos familias de tools = las dos cajas Rust del diagrama ("Agents MCP's Workflow" y
"Data Workflows"):

### 3.1 Tools de ontología (Neo4j local — funcionan sin VPN)

| Tool | Entrada | Salida | Para qué |
|---|---|---|---|
| `search_tables` | término ("cartera"), top_k | tablas rankeadas (nombre, esquema, filas, tamaño, score) | Encontrar las ~10 tablas relevantes entre 4,542 |
| `describe_table` | fqn | columnas + tipos + null_frac + PK/FKs + comentarios + tamaño | Entender una tabla antes de consultarla |
| `join_path` | fqn origen, fqn destino | camino de FKs (tabla→tabla con columnas) | Saber cómo joinear sin adivinar |
| `table_neighborhood` | fqn, profundidad | subgrafo de tablas relacionadas | Contexto alrededor de una entidad |
| `domain_overview` | esquema opcional | esquemas, tablas top por tamaño/centralidad (GDS degree) | Primer vistazo del dominio |

Búsqueda v1: **índice fulltext de Neo4j** sobre nombre + comentario de tablas/columnas
(los nombres están en español: `cartera`, `cuentas_detalle`, …) + expansión por vecindario
FK y ranking por centralidad GDS. La firma del tool queda igual cuando en v2 se cambie a
**índice vectorial + embeddings** (patrón *Schema RAG* de `agentic_data_workflows_rust.pdf`,
`get_relevant_tables`); el upgrade es interno, no rompe clientes.

### 3.2 Tools de datos (Postgres origen vía VPN — con guardrails)

| Tool | Entrada | Salida | Guardrails |
|---|---|---|---|
| `run_read_query` | SQL | filas (JSON) + metadata de ejecución | Solo `SELECT`/`WITH` (parser), 1 statement, transacción `READ ONLY`, `statement_timeout` 15s, LIMIT inyectado (máx 500 filas) |
| `aggregate_table` | fqn, métricas (count/sum/avg), group_by, filtros | agregado tabular | Plantilla SQL generada por nosotros — sin SQL libre |
| `sample_rows` | fqn, n (≤ 50) | muestra de filas | Deshabilitable por config (datos clínicos); pensado para desarrollo |
| `source_status` | — | ¿VPN/túnel vivo? latencia | Los agentes saben si la fuente está disponible antes de intentar |

**Privacidad y seguridad** (base clínica):
- Preferir `aggregate_table` (números, no registros). `sample_rows` apagable por env (`ALLOW_ROW_SAMPLING=false` por defecto en producción).
- Toda consulta queda **auditada** (tracing estructurado → en el futuro OTEL → ClickHouse, como en APEX ARCH).
- El parser rechaza DDL/DML/múltiples statements; la transacción es read-only a nivel Postgres (defensa en profundidad).
- Si la VPN está caída, los tools de datos degradan con error claro; los de ontología siguen funcionando (el grafo es local).

## 4. El flujo "cartera" de punta a punta

```
Usuario: "¿Cómo está la cartera? ¿cuánto se debe y quiénes son los mayores deudores?"

Agente (cliente MCP):
 1. search_tables("cartera")            → esquema cartera (3 tablas, 1.1 GB) +
                                          cuentas_detalle, facturas… rankeadas    [~ms, local]
 2. describe_table("cartera.saldos")    → columnas, tipos, FKs                    [~ms, local]
 3. join_path("cartera.saldos",
              "public.terceros")        → JOIN por FK explícita                   [~ms, local]
 4. aggregate_table(sum(saldo) group by tercero, top 20)                          [~s, vía VPN]
 5. Respuesta con números reales — sin haber copiado nada.
```

El grafo reduce 4,542 tablas al puñado correcto (paso barato); la fuente solo ejecuta
una agregación puntual (paso costoso, minimizado). Si mañana "cartera" se pregunta a
diario, el paso 3 materializa esas tablas en ClickHouse y `aggregate_table` cambia de
backend sin tocar a los clientes MCP.

## 5. Estructura del proyecto (DDD, workspace Cargo)

Alineada con `agentic_data_workflows_rust.pdf` (workspace de crates) y con el
`quality-check.sh` del equipo (fmt + clippy -D warnings + check + test por workspace):

```
apex-mcp/
├── Cargo.toml                  # workspace
├── quality-check.sh            # gate de calidad (el del equipo)
├── compose.yml                 # reusa red de apex-profiler (Neo4j) 
└── crates/
    ├── domain/                 # SIN neo4j, SIN sql, SIN mcp
    │   ├── entities: TableRef, ColumnProfile, JoinPath, AggregateSpec, Insight
    │   ├── ports:    Ontology, DataSource, AuditSink   (traits async)
    │   └── policy:   QueryPolicy (limits, timeouts, allow/deny) — reglas puras, testeables
    ├── application/             # casos de uso: SearchDomain, DescribeEntity,
    │                            #   PlanJoin, RunGuardedQuery, RunAggregate
    ├── infra-neo4j/             # adapter Ontology → neo4rs + GDS + fulltext (v2: vector index)
    ├── infra-postgres/          # adapter DataSource → tokio-postgres (pool deadpool,
    │                            #   read-only, timeouts) — mismo stack del profiler
    └── mcp-server/              # rmcp 0.16: #[tool_router] mapea use cases → tools
                                 #   transporte: stdio (local) y streamable HTTP (agentes remotos)
```

Decisiones:
- **rmcp** (SDK oficial, `modelcontextprotocol/rust-sdk`): macros `#[tool]`/`#[tool_router]`
  generan el JSON Schema de cada tool desde structs `serde` — el contrato queda tipado.
- El dominio no conoce Cypher ni SQL ni MCP (idéntico espíritu al `apex-profiler` y al
  documento DDD del equipo). Cambiar Neo4j→otra cosa o agregar ClickHouse = un adapter nuevo.
- `QueryPolicy` es dominio puro: las reglas de qué SQL se acepta viven donde se pueden
  testear sin infraestructura.

## 6. Pruebas

- **MCP Inspector** (`npx @modelcontextprotocol/inspector`) contra el binario stdio —
  probar tools sin LLM (referencia del equipo en reference.txt).
- Unit tests del dominio (`QueryPolicy`: SQL malicioso, límites) — sin contenedores.
- Integración ontología: contra el **Neo4j local que ya tiene el grafo real** (sin VPN).
- Integración datos: contra el Postgres semilla del smoke test del paso 1 (sin VPN).
- `quality-check.sh` como gate en cada cambio.

## 7. Roadmap

| Fase | Alcance | Depende de |
|---|---|---|
| **MVP (este diseño)** | Tools de ontología (fulltext + GDS degree) + `run_read_query`/`aggregate_table` con guardrails + stdio | Grafo del paso 1 (ya existe) |
| v1.1 | Streamable HTTP + auth (para ADK Rust remoto), auditoría estructurada | MVP |
| v2 — Schema RAG | Índice vectorial en Neo4j + embeddings (provider pluggable / FastRP estructural), `search_tables` semántico | MVP |
| v2.1 — Materialización | ClickHouse/R2-Parquet como backend caliente de `aggregate_table` (paso 2/3 del diagrama DS) | Métricas de uso del MVP |
| v3 — Frescura | CDC Debezium actualiza grafo y cachés (paso 4 del diagrama) | v2.1 |

## 8. Fuera de alcance del MVP

- Embeddings semánticos (v2) — el fulltext en español + grafo llega lejos primero.
- Migración/materialización a ClickHouse o R2 (v2.1) — solo cuando el uso real muestre
  qué está caliente.
- Escritura de cualquier tipo sobre la fuente (nunca en este componente).
- Multi-tenant (un cliente: the client; el diseño no lo impide después).

## 9. Riesgos y mitigaciones

| Riesgo | Mitigación |
|---|---|
| VPN inestable en runtime | Tools de ontología no dependen de ella; `source_status` + errores claros; timeouts cortos |
| Consulta pesada tumba/molesta al OLTP del cliente | READ ONLY + statement_timeout 15s + LIMIT + preferencia por agregados; v2: EXPLAIN cost-gate |
| Fuga de datos clínicos vía sample | `sample_rows` off por defecto; auditoría de toda consulta; agregados como camino feliz |
| El fulltext no encuentra tablas por nombres crípticos | Vecindario FK + centralidad complementan; v2 embeddings resuelve el fondo |
```
