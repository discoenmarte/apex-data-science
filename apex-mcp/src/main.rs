//! apex-mcp — Paso 2 de la arquitectura APEX DS Pipeline.
//! Servidor MCP sobre la ontología Neo4j del paso 1, con consultas runtime
//! guardadas contra el Postgres origen. Federación, no replicación.
//!
//! Transportes:
//!   - stdio (default): para clientes locales (Claude Code, MCP Inspector)
//!   - http  (MCP_TRANSPORT=http): streamable HTTP en /mcp con Bearer API key
//!     (MCP_API_KEY) — para agentes remotos (ADK Rust) y conexión "URL + key".

mod domain;
mod infrastructure;
mod server;

use anyhow::Context;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use infrastructure::config::AppConfig;
use rmcp::transport::stdio;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::ServiceExt;
use server::ApexMcp;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // IMPORTANTE: logs a stderr — en stdio, stdout pertenece al protocolo MCP
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "apex_mcp=info".into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cfg = AppConfig::from_env()?;
    let transport = std::env::var("MCP_TRANSPORT").unwrap_or_else(|_| "stdio".into());
    tracing::info!(neo4j = %cfg.neo4j_uri, db = %cfg.db_name, %transport, "apex-mcp arrancando");

    match transport.as_str() {
        "http" => run_http(cfg).await,
        _ => run_stdio(cfg).await,
    }
}

async fn run_stdio(cfg: AppConfig) -> anyhow::Result<()> {
    let service = ApexMcp::new(cfg)
        .await?
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("error sirviendo MCP: {e:?}"))?;
    service.waiting().await?;
    Ok(())
}

async fn run_http(cfg: AppConfig) -> anyhow::Result<()> {
    let key = std::env::var("MCP_API_KEY")
        .ok()
        .filter(|k| k.trim().len() >= 16)
        .context("MCP_TRANSPORT=http requiere MCP_API_KEY (mínimo 16 caracteres)")?;
    let port: u16 = std::env::var("MCP_PORT")
        .unwrap_or_else(|_| "8977".into())
        .parse()
        .context("MCP_PORT inválido")?;

    // El servidor se construye UNA vez (pools compartidos); cada sesión HTTP lo clona.
    let apex = ApexMcp::new(cfg).await?;
    let mcp_service = StreamableHttpService::new(
        move || Ok(apex.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    let app = axum::Router::new().nest_service("/mcp", mcp_service).layer(
        axum::middleware::from_fn_with_state(Arc::new(key), require_bearer),
    );

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("apex-mcp HTTP escuchando en http://0.0.0.0:{port}/mcp (Authorization: Bearer <MCP_API_KEY>)");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Middleware: exige `Authorization: Bearer <MCP_API_KEY>` en cada request.
async fn require_bearer(State(key): State<Arc<String>>, req: Request, next: Next) -> Response {
    let expected = format!("Bearer {}", key.as_str());
    let authorized = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|got| constant_time_eq(got.as_bytes(), expected.as_bytes()))
        .unwrap_or(false);
    if authorized {
        next.run(req).await
    } else {
        (
            axum::http::StatusCode::UNAUTHORIZED,
            "unauthorized: falta o no coincide 'Authorization: Bearer <MCP_API_KEY>'",
        )
            .into_response()
    }
}

/// Comparación en tiempo constante (no filtra longitud de coincidencia).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
