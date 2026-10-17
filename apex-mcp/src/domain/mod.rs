//! Dominio: entidades, puertos y políticas.
//! Sin Cypher, sin SQL de infraestructura, sin MCP.

pub mod policy;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------- Entidades ----------

#[derive(Debug, Clone, Serialize)]
pub struct TableHit {
    pub fqn: String,
    pub schema: String,
    pub name: String,
    pub comment: Option<String>,
    pub row_estimate: i64,
    pub size_bytes: i64,
    /// Relaciones FK entrantes+salientes (centralidad simple).
    pub fk_degree: i64,
    pub score: f64,
    /// Columnas que hicieron match en la búsqueda (si aplica).
    pub matched_columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub position: i64,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    pub null_frac: Option<f64>,
    pub distinct_estimate: Option<f64>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeignKeyInfo {
    pub direction: String, // "out" | "in"
    pub constraint: String,
    pub other_table: String,
    pub column_pairs: String, // "col_local=col_remota, ..."
}

#[derive(Debug, Clone, Serialize)]
pub struct TableDetail {
    pub fqn: String,
    pub comment: Option<String>,
    pub row_estimate: i64,
    pub size_bytes: i64,
    pub columns: Vec<ColumnInfo>,
    pub constraints: Vec<String>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JoinPath {
    pub tables: Vec<String>,
    pub hops: Vec<String>,
    pub column_pairs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaSummary {
    pub schema: String,
    pub tables: i64,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainOverview {
    pub database: String,
    pub schemas: Vec<SchemaSummary>,
    pub biggest_tables: Vec<TableHit>,
    pub most_connected_tables: Vec<TableHit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryResult {
    pub rows: Vec<serde_json::Value>,
    pub row_count: usize,
    pub truncated: bool,
    pub elapsed_ms: u128,
    /// El SQL realmente ejecutado (transparencia/linaje).
    pub executed_sql: String,
}

// ---------- Especificación de agregados (tool aggregate_table) ----------

#[derive(Debug, Clone, Deserialize)]
pub struct MetricSpec {
    /// count | sum | avg | min | max
    pub func: String,
    /// Columna; opcional solo para count.
    pub column: Option<String>,
}

// ---------- Puertos ----------

#[async_trait]
pub trait Ontology: Send + Sync {
    async fn ensure_indexes(&self) -> anyhow::Result<()>;
    async fn overview(&self) -> anyhow::Result<DomainOverview>;
    async fn search_tables(&self, term: &str, top_k: usize) -> anyhow::Result<Vec<TableHit>>;
    async fn describe_table(&self, fqn: &str) -> anyhow::Result<Option<TableDetail>>;
    async fn join_path(&self, from_fqn: &str, to_fqn: &str) -> anyhow::Result<Option<JoinPath>>;
    async fn neighborhood(&self, fqn: &str) -> anyhow::Result<Vec<ForeignKeyInfo>>;
}

#[async_trait]
pub trait DataSource: Send + Sync {
    /// Ejecuta un SELECT ya validado por la política, con guardrails.
    async fn run_select(&self, sql: &str, max_rows: usize) -> anyhow::Result<QueryResult>;
    /// Latencia de un ping (SELECT 1) o error si la fuente no responde.
    async fn ping(&self) -> anyhow::Result<u128>;
}
