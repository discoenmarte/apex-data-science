//! Configuración por variables de entorno (12-factor), igual que apex-profiler.

use anyhow::Context;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub neo4j_uri: String,
    pub neo4j_user: String,
    pub neo4j_password: String,
    pub db_host: String,
    pub db_port: u16,
    pub db_user: String,
    pub db_password: String,
    pub db_name: String,
    /// Timeout por consulta a la fuente (ms).
    pub query_timeout_ms: u64,
    /// Tope duro de filas por respuesta.
    pub max_rows: usize,
    /// Consultas concurrentes máximas contra la fuente (protege el OLTP).
    pub source_max_concurrency: usize,
    /// Muestreo de filas crudas (off por defecto: datos clínicos).
    pub allow_row_sampling: bool,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            neo4j_uri: env_or("NEO4J_URI", "bolt://host.docker.internal:7687"),
            neo4j_user: env_or("NEO4J_USER", "neo4j"),
            neo4j_password: env_or("NEO4J_PASSWORD", "apexpassword"),
            db_host: env_or("DB_HOST", "host.docker.internal"),
            db_port: env_or("DB_PORT", "5432")
                .parse()
                .context("DB_PORT inválido")?,
            db_user: env_or("DB_USER", "postgres"),
            db_password: env_or("DB_PASSWORD", ""),
            db_name: env_or("DB_NAME", "postgres"),
            query_timeout_ms: env_or("QUERY_TIMEOUT_MS", "15000")
                .parse()
                .context("QUERY_TIMEOUT_MS inválido")?,
            max_rows: env_or("MAX_ROWS", "500")
                .parse()
                .context("MAX_ROWS inválido")?,
            source_max_concurrency: env_or("SOURCE_MAX_CONCURRENCY", "4")
                .parse()
                .context("SOURCE_MAX_CONCURRENCY inválido")?,
            allow_row_sampling: env_or("ALLOW_ROW_SAMPLING", "false").eq_ignore_ascii_case("true"),
        })
    }

    pub fn pg_pool(&self) -> anyhow::Result<deadpool_postgres::Pool> {
        let mut cfg = deadpool_postgres::Config::new();
        cfg.host = Some(self.db_host.clone());
        cfg.port = Some(self.db_port);
        cfg.user = Some(self.db_user.clone());
        cfg.password = Some(self.db_password.clone());
        cfg.dbname = Some(self.db_name.clone());
        cfg.pool = Some(deadpool_postgres::PoolConfig::new(
            self.source_max_concurrency + 1,
        ));
        let pool = cfg.create_pool(
            Some(deadpool_postgres::Runtime::Tokio1),
            tokio_postgres::NoTls,
        )?;
        Ok(pool)
    }
}
