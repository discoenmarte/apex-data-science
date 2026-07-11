//! Configuración por variables de entorno (12-factor).

use anyhow::Context;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub db_host: String,
    pub db_port: u16,
    pub db_user: String,
    pub db_password: String,
    pub db_name: String,
    /// Esquemas a perfilar, separados por coma.
    pub db_schemas: Vec<String>,
    /// Número de tablas perfiladas en paralelo.
    pub concurrency: usize,
    pub neo4j_uri: String,
    pub neo4j_user: String,
    pub neo4j_password: String,
    pub export_dir: String,
    /// Si true, solo exporta JSON (no escribe en Neo4j).
    pub skip_neo4j: bool,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            db_host: env_or("DB_HOST", "host.docker.internal"),
            db_port: env_or("DB_PORT", "5432").parse().context("DB_PORT inválido")?,
            db_user: std::env::var("DB_USER").context("falta DB_USER")?,
            db_password: std::env::var("DB_PASSWORD").context("falta DB_PASSWORD")?,
            db_name: std::env::var("DB_NAME").context("falta DB_NAME")?,
            db_schemas: env_or("DB_SCHEMAS", "public")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            concurrency: env_or("PROFILE_CONCURRENCY", "8")
                .parse()
                .context("PROFILE_CONCURRENCY inválido")?,
            neo4j_uri: env_or("NEO4J_URI", "bolt://neo4j:7687"),
            neo4j_user: env_or("NEO4J_USER", "neo4j"),
            neo4j_password: env_or("NEO4J_PASSWORD", "apexpassword"),
            export_dir: env_or("EXPORT_DIR", "./data"),
            skip_neo4j: env_or("SKIP_NEO4J", "false").eq_ignore_ascii_case("true"),
        })
    }

    pub fn pg_pool(&self) -> anyhow::Result<deadpool_postgres::Pool> {
        let mut cfg = deadpool_postgres::Config::new();
        cfg.host = Some(self.db_host.clone());
        cfg.port = Some(self.db_port);
        cfg.user = Some(self.db_user.clone());
        cfg.password = Some(self.db_password.clone());
        cfg.dbname = Some(self.db_name.clone());
        cfg.pool = Some(deadpool_postgres::PoolConfig::new(self.concurrency + 2));
        let pool = cfg.create_pool(
            Some(deadpool_postgres::Runtime::Tokio1),
            tokio_postgres::NoTls,
        )?;
        Ok(pool)
    }
}
