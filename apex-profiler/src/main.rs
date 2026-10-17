//! apex-profiler — Paso 1 de la arquitectura APEX DS Pipeline:
//! Fuente (Postgres) -> Perfilado paralelo (Rust) -> Schemas (Not Data) -> Neo4j (GDS).

mod application;
mod domain;
mod infrastructure;

use application::use_cases::profile_database::{ProfileDatabaseOptions, ProfileDatabaseUseCase};
use domain::repositories::{KnowledgeGraph, ProfileExporter, SchemaCatalog};
use infrastructure::config::AppConfig;
use infrastructure::persistence::json_exporter::JsonExporter;
use infrastructure::persistence::neo4j_knowledge_graph::Neo4jKnowledgeGraph;
use infrastructure::persistence::postgres_catalog_repository::PostgresCatalogRepository;
use std::sync::Arc;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "apex_profiler=info".into()),
        )
        .init();

    let config = AppConfig::from_env()?;
    info!(
        host = %config.db_host,
        db = %config.db_name,
        schemas = ?config.db_schemas,
        concurrency = config.concurrency,
        "apex-profiler arrancando"
    );

    // Composición de dependencias (la infraestructura implementa los puertos del dominio)
    let pool = config.pg_pool()?;
    let catalog: Arc<dyn SchemaCatalog> = Arc::new(PostgresCatalogRepository::new(pool));

    let graph: Option<Arc<dyn KnowledgeGraph>> = if config.skip_neo4j {
        None
    } else {
        let g = Neo4jKnowledgeGraph::connect(
            &config.neo4j_uri,
            &config.neo4j_user,
            &config.neo4j_password,
        )
        .await?;
        Some(Arc::new(g))
    };

    let exporter: Arc<dyn ProfileExporter> = Arc::new(JsonExporter::new(&config.export_dir));

    let use_case = ProfileDatabaseUseCase::new(catalog, graph, exporter);
    let outcome = use_case
        .execute(ProfileDatabaseOptions {
            schemas: config.db_schemas.clone(),
            concurrency: config.concurrency,
        })
        .await?;

    println!("\n================= RESUMEN =================");
    println!("Base de datos : {}", outcome.profile.name);
    println!("Esquemas      : {}", outcome.profile.schemas().join(", "));
    println!("Tablas        : {}", outcome.profile.tables.len());
    println!("Columnas      : {}", outcome.profile.total_columns());
    println!(
        "Tamaño total  : {:.2} GB",
        outcome.profile.total_size_bytes() as f64 / 1e9
    );
    println!("Export JSON   : {}", outcome.export_path.display());
    if !outcome.failed_tables.is_empty() {
        warn!("{} tablas fallaron:", outcome.failed_tables.len());
        for (t, e) in &outcome.failed_tables {
            warn!("  {t}: {e}");
        }
    }
    println!("===========================================\n");
    Ok(())
}
