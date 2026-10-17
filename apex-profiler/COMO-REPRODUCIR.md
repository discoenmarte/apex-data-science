# Qué es esto y cómo reproducirlo (5 minutos)

## La idea en 3 frases

Le tomamos una **"radiografía" a la base de datos de la clínica** (508 GB, 4,542 tablas) sin leer ni un solo dato de pacientes: un perfilador en Rust se conecta por la VPN y lee **solo la estructura** (tablas, columnas, llaves foráneas y estadísticas de los catálogos de Postgres). Con eso arma un **grafo de conocimiento en Neo4j**: cada tabla y columna es un nodo, cada llave foránea una relación. Sobre ese grafo ya corre Graph Data Science (embeddings, centralidad) — es el mapa que los agentes de los siguientes pasos del pipeline usarán para entender la base sin tocarla.

```
Postgres (clínica, tras VPN) ──> Perfilado paralelo (Rust) ──> "Schemas, Not Data" ──> Neo4j + GDS
```

## Qué necesitas

- Docker corriendo (Docker Desktop o Colima). Con Colima: `brew install docker-compose` y enlazar el plugin (ver README).
- Los dos repos como carpetas hermanas: `apex-profiler/` y `db-vpn-connect/` (con su `.env/.client` ya configurado).
- Acceso al correo donde llega el **código 2FA** de la VPN.

## Reproducir (todo con make, no necesitas Rust)

```bash
cd apex-profiler

# 1. Conectar la VPN (Terminal A — dejar corriendo)
make vpn-build          # solo la primera vez
make vpn-connect        # pide password de VPN y luego el código 2FA del correo

# 2. En OTRA terminal, cuando la VPN esté conectada:
make vpn-test           # ping a la DB para verificar
make vpn-forward        # túnel: localhost:5432 -> Postgres de la clínica

# 3. Levantar Neo4j y perfilar
make up                 # Neo4j con plugin GDS
make profile            # ~2-3 min: perfila 4,542 tablas y llena el grafo

# 4. Explorar
make browser            # http://localhost:7474  (neo4j / apexpassword)
```

En el browser de Neo4j prueba:

```cypher
// Las tablas más grandes
MATCH (t:Table) RETURN t.name, t.row_estimate, t.size_bytes ORDER BY t.size_bytes DESC LIMIT 20;

// El vecindario de una tabla (sus llaves foráneas)
MATCH (t:Table {name:'cuentas_detalle'})-[r:REFERENCES]-(o:Table) RETURN t, r, o;
```

## Notas

- **Privacidad**: jamás se leen filas — solo `pg_class`, `pg_attribute`, `pg_constraint`, `pg_stats` (y se excluyen los campos de pg_stats que contienen valores reales).
- **Idempotente**: correr `make profile` de nuevo solo actualiza el grafo (todo es MERGE).
- Si la VPN pide el código y se demora, tranquilo: el contenedor ya usa `--no-http-keepalive`, que arregla el error clásico de FortiGate al enviar el código tarde.
- El JSON exportado queda en `./data/` — es el artefacto que en el paso 2 del pipeline viaja a R2 como Parquet.
- Más detalle (modelo del grafo, consultas GDS, troubleshooting): ver [README.md](README.md).
