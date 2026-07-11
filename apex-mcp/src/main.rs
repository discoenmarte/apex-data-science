//! apex-mcp — Paso 2 de la arquitectura APEX DS Pipeline.
//! Servidor MCP (stdio) sobre la ontología Neo4j del paso 1, con consultas
//! runtime guardadas contra el Postgres origen. Federación, no replicación.

mod domain;
mod infrastructure;
mod server;

use infrastructure::config::AppConfig;
use rmcp::{transport::stdio, ServiceExt};
use server::ApexMcp;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // IMPORTANTE: logs a stderr — stdout pertenece al protocolo MCP
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "apex_mcp=info".into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cfg = AppConfig::from_env()?;
    tracing::info!(neo4j = %cfg.neo4j_uri, db = %cfg.db_name, "apex-mcp arrancando");

    let service = ApexMcp::new(cfg)
        .await?
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("error sirviendo MCP: {e:?}"))?;

    service.waiting().await?;
    Ok(())
}
