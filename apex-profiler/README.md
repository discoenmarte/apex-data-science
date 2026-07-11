# apex-profiler

Paso 1 de la arquitectura **APEX DS Pipeline**: perfilado paralelo en Rust de la fuente de datos (Postgres) que extrae **schemas, no datos**, y los publica en **Neo4j** listo para GDS/embeddings.

```
Data Sources (Postgres tras VPN) ──> Parallel Profiling (Rust) ──> Schemas (Not Data) ──> Neo4j (GDS)
```

## Arquitectura (DDD)

El dominio es el corazón; la infraestructura implementa sus interfaces (ver `DDD Explicación`):

```
src/
├── domain/                     # Sin SQL, sin Cypher, sin frameworks
│   ├── entities/               #   TableProfile (Aggregate Root), ColumnProfile, DatabaseProfile
│   ├── value_objects/          #   QualifiedName, ColumnStatistics
│   ├── repositories/           #   Puertos: SchemaCatalog, KnowledgeGraph, ProfileExporter
│   └── services/               #   RelationshipResolver (grafo FK entre agregados)
├── application/
│   └── use_cases/              #   ProfileDatabaseUseCase (coordina, no contiene reglas)
└── infrastructure/
    ├── config.rs               #   Env vars (12-factor)
    └── persistence/            #   PostgresCatalogRepository, Neo4jKnowledgeGraph, JsonExporter
```

**Paralelismo**: pool de conexiones (deadpool) + `buffer_unordered(PROFILE_CONCURRENCY)` — perfila N tablas a la vez. Como solo lee catálogos (`pg_class`, `pg_attribute`, `pg_constraint`, `pg_stats`), perfilar cientos de tablas (incl. las de 60+ GB) toma segundos, no horas.

**Privacidad**: nunca se leen filas de las tablas. Las estadísticas (null_frac, n_distinct, avg_width) salen de `pg_stats`, y deliberadamente se excluyen `most_common_vals`/`histogram_bounds` porque contienen valores reales (DB clínica).

## Requisitos

- Docker (Docker Desktop o Colima — no necesitas Rust local). Con Colima, el plugin compose se instala con `brew install docker-compose` y se enlaza: `ln -sfn $(brew --prefix)/opt/docker-compose/bin/docker-compose ~/.docker/cli-plugins/docker-compose` (ya hecho en esta máquina)
- El repo `db-vpn-connect` en el directorio hermano (`../db-vpn-connect`) con su `.env/.client` (ya creado, incluye el pin real del certificado del servidor VPN)

## Ejecución

```bash
# 1. Terminal A — conectar VPN (pide password de VPN y luego código 2FA del correo)
make vpn-build          # solo la primera vez
make vpn-connect        # dejar corriendo
#    Alternativa no-interactiva (útil para agentes / scripts):
#    cd ../db-vpn-connect && VPN_PASSWORD='...' make vpn-connect-auto
#    y cuando llegue el código:  echo CODIGO > /tmp/vpn-2fa-token.txt

# 2. Terminal B — cuando la VPN reciba paquetes:
make vpn-test           # ping al host de la DB para verificar
make vpn-forward        # port forwarding 5432 -> DB por el túnel

# 3. Levantar Neo4j (descarga plugin GDS al primer arranque)
make up

# 4. Perfilar (compila la imagen la primera vez, ~2-3 min)
make profile

# 5. Explorar el grafo
make browser            # http://localhost:7474  (neo4j / apexpassword)
```

`make profile-json` corre solo el export JSON (sin Neo4j). El JSON queda en `./data/`.

### Nota sobre el certificado VPN

Si `make vpn-connect` falla con error de certificado, openconnect imprime el pin real (`pin-sha256:...`). Cópialo en `SERVER_CERT` dentro de `../db-vpn-connect/.env/.client` y reintenta.

## Modelo de grafo en Neo4j

```
(d:Database)-[:HAS_SCHEMA]->(s:Schema)-[:HAS_TABLE]->(t:Table)-[:HAS_COLUMN]->(c:Column)
(t)-[:REFERENCES {constraint, from_columns, to_columns}]->(t2:Table)
(c)-[:REFERENCES_COLUMN {constraint}]->(c2:Column)
```

Propiedades clave: `Table {fqn, row_estimate, size_bytes, n_columns, constraints[]}` y `Column {fqn, data_type, nullable, is_primary_key, null_frac, n_distinct, distinct_estimate, avg_width, correlation}`. Todo es idempotente (`MERGE`): re-ejecutar actualiza el grafo.

## Consultas útiles

```cypher
// Tablas más grandes y sus relaciones
MATCH (t:Table) RETURN t.name, t.row_estimate, t.size_bytes ORDER BY t.size_bytes DESC LIMIT 20;

// Vecindario de una tabla
MATCH (t:Table {name:'cuentas_detalle'})-[r:REFERENCES]-(o:Table) RETURN t, r, o;

// Columnas casi siempre nulas (candidatas a limpieza)
MATCH (c:Column) WHERE c.null_frac > 0.95 RETURN c.fqn, c.null_frac ORDER BY c.null_frac DESC;

// Proyección GDS + grado de conectividad de tablas
CALL gds.graph.project('esquema', ['Table'], ['REFERENCES']);
CALL gds.degree.stream('esquema') YIELD nodeId, score
RETURN gds.util.asNode(nodeId).name AS tabla, score ORDER BY score DESC LIMIT 20;

// Embeddings de nodos (FastRP) para pasos siguientes del pipeline
CALL gds.fastRP.stream('esquema', {embeddingDimension: 64})
YIELD nodeId, embedding RETURN gds.util.asNode(nodeId).fqn AS tabla, embedding LIMIT 5;
```

## Configuración (`.env`)

| Variable | Default | Descripción |
|---|---|---|
| `DB_HOST` | `host.docker.internal` | Postgres vía port-forward del contenedor VPN |
| `DB_SCHEMAS` | `public` | Esquemas a perfilar, separados por coma |
| `PROFILE_CONCURRENCY` | `8` | Tablas perfiladas en paralelo |
| `SKIP_NEO4J` | `false` | `true` = solo export JSON |
| `NEO4J_PASSWORD` | `apexpassword` | Cambia también en `compose.yml` si lo cambias |

## Troubleshooting

- **`row_estimate = -1`**: la tabla nunca fue analizada (`ANALYZE`); las estadísticas de columnas vendrán vacías. Es metadato de Postgres, no un error.
- **Timeout conectando a la DB**: verifica `make vpn-test` y que `make vpn-forward` esté corriendo. La VPN del cliente es inestable — reintenta `make vpn-connect`.
- **Una tabla falla**: el perfilado continúa; las fallas se listan al final y no bloquean el resto.
- **Estado de compilación**: verificado — `cargo build` + 7 tests unitarios + test de integración contra Postgres 17 real (PKs compuestas, FKs compuestas/cíclicas, pg_stats, comments, arrays) todo en verde.
