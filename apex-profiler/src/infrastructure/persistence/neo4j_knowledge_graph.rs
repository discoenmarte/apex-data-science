//! Adaptador de infraestructura: implementa `KnowledgeGraph` contra Neo4j.
//!
//! Modelo de grafo (pensado para GDS / embeddings):
//!   (d:Database)-[:HAS_SCHEMA]->(s:Schema)-[:HAS_TABLE]->(t:Table)-[:HAS_COLUMN]->(c:Column)
//!   (t)-[:REFERENCES {constraint, from_columns, to_columns}]->(t2)
//!   (c)-[:REFERENCES_COLUMN {constraint}]->(c2)

use crate::domain::entities::database::DatabaseProfile;
use crate::domain::entities::table::TableProfile;
use crate::domain::repositories::{GraphWriteSummary, KnowledgeGraph};
use crate::domain::services::relationship_resolver::ForeignKeyRelation;
use crate::infrastructure::persistence::bolt::to_bolt;
use async_trait::async_trait;
use neo4rs::{query, Graph};
use serde_json::json;
use tracing::info;

pub struct Neo4jKnowledgeGraph {
    graph: Graph,
}

impl Neo4jKnowledgeGraph {
    pub async fn connect(uri: &str, user: &str, password: &str) -> anyhow::Result<Self> {
        let graph = Graph::new(uri, user, password).await?;
        Ok(Self { graph })
    }

    async fn persist_table(&self, db_name: &str, table: &TableProfile) -> anyhow::Result<()> {
        let columns: Vec<serde_json::Value> = table
            .columns
            .iter()
            .map(|c| {
                let stats = c.statistics.clone().unwrap_or_default();
                json!({
                    "fqn": format!("{}.{}", table.id.fqn(), c.name),
                    "name": c.name,
                    "position": c.position as i64,
                    "data_type": c.data_type,
                    "nullable": c.is_nullable,
                    "default_value": c.default_value,
                    "comment": c.comment,
                    "is_primary_key": c.is_primary_key,
                    "null_frac": stats.null_frac.map(|v| v as f64),
                    "n_distinct": stats.n_distinct.map(|v| v as f64),
                    "distinct_estimate": stats.distinct_estimate(table.row_estimate),
                    "avg_width": stats.avg_width.map(|v| v as i64),
                    "correlation": stats.correlation.map(|v| v as f64),
                })
            })
            .collect();

        let check_constraints: Vec<serde_json::Value> = table
            .constraints
            .iter()
            .map(|c| json!(format!("{}: {}", c.name, c.definition)))
            .collect();

        let cypher = r#"
MERGE (s:Schema {fqn: $schema_fqn})
MERGE (t:Table {fqn: $fqn})
SET t.name = $name,
    t.schema = $schema,
    t.database = $db,
    t.comment = $comment,
    t.row_estimate = $row_estimate,
    t.size_bytes = $size_bytes,
    t.n_columns = $n_columns,
    t.constraints = $constraints,
    t.profiled_at = $profiled_at
MERGE (s)-[:HAS_TABLE]->(t)
WITH t
UNWIND $columns AS col
MERGE (c:Column {fqn: col.fqn})
SET c.name = col.name,
    c.table_fqn = $fqn,
    c.position = col.position,
    c.data_type = col.data_type,
    c.nullable = col.nullable,
    c.default_value = col.default_value,
    c.comment = col.comment,
    c.is_primary_key = col.is_primary_key,
    c.null_frac = col.null_frac,
    c.n_distinct = col.n_distinct,
    c.distinct_estimate = col.distinct_estimate,
    c.avg_width = col.avg_width,
    c.correlation = col.correlation
MERGE (t)-[:HAS_COLUMN]->(c)
"#;

        self.graph
            .run(
                query(cypher)
                    .param("schema_fqn", format!("{}.{}", db_name, table.id.schema))
                    .param("fqn", table.id.fqn())
                    .param("name", table.id.name.clone())
                    .param("schema", table.id.schema.clone())
                    .param("db", db_name.to_string())
                    .param("comment", table.comment.clone().unwrap_or_default())
                    .param("row_estimate", table.row_estimate)
                    .param("size_bytes", table.total_size_bytes)
                    .param("n_columns", table.columns.len() as i64)
                    .param("constraints", to_bolt(&serde_json::Value::Array(check_constraints)))
                    .param("profiled_at", chrono::Utc::now().to_rfc3339())
                    .param("columns", to_bolt(&serde_json::Value::Array(columns))),
            )
            .await?;
        Ok(())
    }
}

#[async_trait]
impl KnowledgeGraph for Neo4jKnowledgeGraph {
    async fn initialize(&self) -> anyhow::Result<()> {
        let constraints = [
            "CREATE CONSTRAINT database_name IF NOT EXISTS FOR (d:Database) REQUIRE d.name IS UNIQUE",
            "CREATE CONSTRAINT schema_fqn IF NOT EXISTS FOR (s:Schema) REQUIRE s.fqn IS UNIQUE",
            "CREATE CONSTRAINT table_fqn IF NOT EXISTS FOR (t:Table) REQUIRE t.fqn IS UNIQUE",
            "CREATE CONSTRAINT column_fqn IF NOT EXISTS FOR (c:Column) REQUIRE c.fqn IS UNIQUE",
        ];
        for c in constraints {
            self.graph.run(query(c)).await?;
        }
        Ok(())
    }

    async fn persist(
        &self,
        profile: &DatabaseProfile,
        relations: &[ForeignKeyRelation],
    ) -> anyhow::Result<GraphWriteSummary> {
        // 1. Base de datos
        self.graph
            .run(
                query("MERGE (d:Database {name: $name}) SET d.profiled_at = $ts")
                    .param("name", profile.name.clone())
                    .param("ts", profile.profiled_at.to_rfc3339()),
            )
            .await?;

        // 2. Esquemas
        let schemas = profile.schemas();
        for schema in &schemas {
            self.graph
                .run(
                    query(
                        "MATCH (d:Database {name: $db}) \
                         MERGE (s:Schema {fqn: $fqn}) SET s.name = $name, s.database = $db \
                         MERGE (d)-[:HAS_SCHEMA]->(s)",
                    )
                    .param("db", profile.name.clone())
                    .param("fqn", format!("{}.{}", profile.name, schema))
                    .param("name", schema.to_string()),
                )
                .await?;
        }

        // 3. Tablas + columnas
        for table in &profile.tables {
            self.persist_table(&profile.name, table).await?;
        }
        info!(tables = profile.tables.len(), "tablas persistidas en Neo4j");

        // 4. Relaciones FK a nivel tabla
        let rels: Vec<serde_json::Value> = relations
            .iter()
            .map(|r| {
                json!({
                    "constraint": r.constraint,
                    "from": r.from.fqn(),
                    "to": r.to.fqn(),
                    "from_columns": r.column_pairs.iter().map(|p| p.0.clone()).collect::<Vec<_>>(),
                    "to_columns": r.column_pairs.iter().map(|p| p.1.clone()).collect::<Vec<_>>(),
                })
            })
            .collect();
        if !rels.is_empty() {
            self.graph
                .run(
                    query(
                        "UNWIND $rels AS rel \
                         MATCH (a:Table {fqn: rel.from}) \
                         MATCH (b:Table {fqn: rel.to}) \
                         MERGE (a)-[r:REFERENCES {constraint: rel.constraint}]->(b) \
                         SET r.from_columns = rel.from_columns, r.to_columns = rel.to_columns",
                    )
                    .param("rels", to_bolt(&serde_json::Value::Array(rels))),
                )
                .await?;
        }

        // 5. Relaciones FK a nivel columna (útil para embeddings más ricos)
        let pairs: Vec<serde_json::Value> = relations
            .iter()
            .flat_map(|r| {
                r.column_pairs.iter().map(move |(from_col, to_col)| {
                    json!({
                        "constraint": r.constraint,
                        "from": format!("{}.{}", r.from.fqn(), from_col),
                        "to": format!("{}.{}", r.to.fqn(), to_col),
                    })
                })
            })
            .collect();
        if !pairs.is_empty() {
            self.graph
                .run(
                    query(
                        "UNWIND $pairs AS p \
                         MATCH (a:Column {fqn: p.from}) \
                         MATCH (b:Column {fqn: p.to}) \
                         MERGE (a)-[:REFERENCES_COLUMN {constraint: p.constraint}]->(b)",
                    )
                    .param("pairs", to_bolt(&serde_json::Value::Array(pairs))),
                )
                .await?;
        }

        Ok(GraphWriteSummary {
            schemas: schemas.len(),
            tables: profile.tables.len(),
            columns: profile.total_columns(),
            fk_relations: relations.len(),
        })
    }
}
