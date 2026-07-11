# apex-mcp — Paso 2: MCP sobre el grafo de conocimiento

Servidor **MCP en Rust** que expone la ontología del paso 1 (Neo4j: 4,542 tablas,
7,763 FKs de `CLIENT_DB`) y ejecuta consultas puntuales de **solo lectura** contra el
Postgres real (vía VPN) en runtime. **Federación, no replicación**: nunca se copia la
base; cada pregunta paga solo lo que necesita.

- Diseño completo: [DESIGN.md](DESIGN.md) · Diagramas: [diagrama-simple-apex-mcp.pdf](diagrama-simple-apex-mcp.pdf) (pizarra) y [diagrama-arquitectura-apex-mcp.pdf](diagrama-arquitectura-apex-mcp.pdf) (detallado)

## Tools

| Tool | Fuente | Latencia | Qué hace |
|---|---|---|---|
| `domain_overview` | grafo (local) | ~ms | Esquemas, tablas top por tamaño y centralidad FK |
| `search_tables` | grafo (local) | ~ms | Fulltext (Lucene) sobre nombres/comentarios/columnas + ranking por centralidad |
| `describe_table` | grafo (local) | ~ms | Columnas, tipos, stats (null_frac, cardinalidad), constraints, FKs |
| `join_path` | grafo (local) | ~ms | Camino de JOINs más corto entre dos tablas (shortestPath ≤6 saltos) |
| `table_neighborhood` | grafo (local) | ~ms | FKs entrantes/salientes con pares de columnas |
| `aggregate_table` | fuente (VPN) | ~s | count/sum/avg/min/max + group by, por plantilla validada contra columnas reales |
| `run_read_query` | fuente (VPN) | ~s | SELECT libre con guard por AST, READ ONLY, timeout, LIMIT |
| `sample_rows` | fuente (VPN) | ~s | Muestra cruda — **apagado por defecto** (datos clínicos) |
| `source_status` | fuente (VPN) | ~ms | ¿Túnel vivo? latencia |

## Correr (todo en Docker, no necesitas Rust local)

```bash
make build      # compila el binario release (cache de cargo en volumen)
make test       # unit tests del dominio (política SQL)
make quality    # fmt + clippy -D warnings + check + test (gate del equipo)
make e2e        # prueba JSON-RPC completa contra el grafo real (sin LLM)
make inspector  # UI oficial de MCP para probar tools a mano (requiere node)
```

Para los tools de datos contra la fuente real: primero `make vpn-connect` + `make vpn-forward`
en `../apex-profiler` (el `.env` ya apunta a `host.docker.internal:5432`).

## Conectarlo a un cliente MCP

Claude Code / Claude Desktop / cualquier cliente (config `mcpServers`):

```json
{
  "mcpServers": {
    "apex": {
      "command": "docker",
      "args": ["run", "-i", "--rm",
               "--add-host", "host.docker.internal:host-gateway",
               "--env-file", "/RUTA/A/apex-mcp/.env",
               "-v", "/RUTA/A/apex-mcp:/app", "-w", "/app",
               "rust:1.90-slim", "/app/target/release/apex-mcp"]
    }
  }
}
```

(O `make image` una vez y usa `"args": ["run","-i","--rm","--add-host","host.docker.internal:host-gateway","--env-file","/RUTA/A/apex-mcp/.env","apex-mcp"]`.)

Para el **grafo de agentes ADK Rust** (APEX ARCH): mismo contrato de tools; cuando el
cliente sea remoto se agrega el transporte streamable HTTP + auth (v1.1 del roadmap) —
el código del servidor no cambia, solo el transporte.

## Decisiones de escalabilidad

| Decisión | Por qué escala |
|---|---|
| Índices **fulltext (Lucene)** en Neo4j para search | O(log n); mismo mecanismo sirve para 100k+ tablas. Se crean solos al arrancar (`IF NOT EXISTS`) |
| `shortestPath` acotado (≤6) para join planning | El grafo poda el espacio de joins; nunca se enumeran combinaciones en el LLM |
| Guard de SQL por **AST** (`sqlparser`, dialecto Postgres) | 1 statement, solo `SELECT/WITH`, sin `INTO`; imposible de burlar con comentarios/encoding como un regex |
| **Semáforo** (`SOURCE_MAX_CONCURRENCY=4`) | Tope duro de queries concurrentes contra el OLTP clínico, independiente de cuántos agentes llamen |
| Transacción `READ ONLY` + `SET LOCAL statement_timeout` | Defensa en profundidad por consulta, no por sesión |
| Envoltura `row_to_json(...) LIMIT n+1` | Tipado universal (cualquier tipo PG → JSON) + truncamiento honesto reportado al agente |
| Pools perezosos (deadpool) | La VPN puede estar caída: los tools de ontología funcionan igual; los de datos degradan con error claro |
| Respuestas compactas (top_k, LIMIT) | El contexto del LLM es el recurso más caro del sistema |

### Camino de crecimiento (sin romper el contrato)

- **v1.1**: transporte streamable HTTP + auth para agentes remotos (ADK Rust).
- **v2**: `search_tables` semántico — índice vectorial de Neo4j + embeddings (misma firma).
- **v2.1**: `aggregate_table` cambia su backend a **ClickHouse / R2-Parquet** para los
  dominios calientes (lo que la auditoría muestre que se pregunta a diario) — los pasos
  2–3 del diagrama general. Los clientes MCP no se enteran.
- **v3**: **Debezium CDC** mantiene grafo y cachés frescos.

## Verificación (2026-07-10)

- `cargo test`: 8/8 (política SQL: inyección, DML/DDL, multi-statement, SELECT INTO)
- `quality-check.sh`: fmt + clippy `-D warnings` + check + test en verde
- E2E JSON-RPC contra el **grafo real**: 9 tools; `search_tables("cartera")` → recibos_caja
  (1.5M filas, 25 FKs), rc_detalle_tesoreria_facturas (3.6M filas); `join_path(cuentas_detalle→pacientes)`
  → 3 saltos reales; guardrails rechazan `DELETE`, multi-statement y `sample_rows`
