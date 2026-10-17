//! Caso de Uso: perfilar la base de datos completa en paralelo.
//! Coordina — no contiene reglas del negocio.

use crate::domain::entities::database::DatabaseProfile;
use crate::domain::repositories::{KnowledgeGraph, ProfileExporter, SchemaCatalog};
use crate::domain::services::relationship_resolver::RelationshipResolver;
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

pub struct ProfileDatabaseOptions {
    pub schemas: Vec<String>,
    pub concurrency: usize,
}

pub struct ProfileDatabaseUseCase {
    catalog: Arc<dyn SchemaCatalog>,
    graph: Option<Arc<dyn KnowledgeGraph>>,
    exporter: Arc<dyn ProfileExporter>,
}

pub struct ProfileOutcome {
    pub profile: DatabaseProfile,
    pub failed_tables: Vec<(String, String)>,
    pub export_path: std::path::PathBuf,
}

impl ProfileDatabaseUseCase {
    pub fn new(
        catalog: Arc<dyn SchemaCatalog>,
        graph: Option<Arc<dyn KnowledgeGraph>>,
        exporter: Arc<dyn ProfileExporter>,
    ) -> Self {
        Self {
            catalog,
            graph,
            exporter,
        }
    }

    pub async fn execute(&self, opts: ProfileDatabaseOptions) -> anyhow::Result<ProfileOutcome> {
        let started = Instant::now();
        let db_name = self.catalog.database_name().await?;
        info!(db = %db_name, schemas = ?opts.schemas, "iniciando perfilado");

        // 1. Descubrir tablas
        let tables = self.catalog.list_tables(&opts.schemas).await?;
        info!(count = tables.len(), "tablas descubiertas");

        // 2. Perfilado PARALELO: fan-out limitado por `concurrency`
        let catalog = self.catalog.clone();
        let results: Vec<_> = stream::iter(tables.into_iter())
            .map(|table| {
                let catalog = catalog.clone();
                async move {
                    let result = catalog.profile_table(&table).await;
                    (table, result)
                }
            })
            .buffer_unordered(opts.concurrency.max(1))
            .collect()
            .await;

        let mut profiles = Vec::new();
        let mut failed = Vec::new();
        for (table, result) in results {
            match result {
                Ok(p) => profiles.push(p),
                Err(e) => {
                    error!(table = %table, error = %e, "fallo perfilando tabla");
                    failed.push((table.fqn(), format!("{e:#}")));
                }
            }
        }
        // Orden estable por tamaño para lecturas humanas del export
        profiles.sort_by_key(|p| std::cmp::Reverse(p.total_size_bytes));
        info!(
            ok = profiles.len(),
            failed = failed.len(),
            elapsed_s = started.elapsed().as_secs_f32(),
            "perfilado paralelo completado"
        );

        let profile = DatabaseProfile::new(db_name, profiles);

        // 3. Servicio de dominio: resolver relaciones FK
        let relations = RelationshipResolver::resolve(&profile.tables);
        info!(fk_relations = relations.len(), "relaciones resueltas");

        // 4. Exportar artefacto JSON (Schemas, Not Data)
        let export_path = self.exporter.export(&profile, &relations)?;
        info!(path = %export_path.display(), "perfil exportado");

        // 5. Persistir en el grafo de conocimiento
        if let Some(graph) = &self.graph {
            graph.initialize().await?;
            let summary = graph.persist(&profile, &relations).await?;
            info!(
                schemas = summary.schemas,
                tables = summary.tables,
                columns = summary.columns,
                fk_relations = summary.fk_relations,
                "grafo actualizado en Neo4j"
            );
        } else {
            warn!("SKIP_NEO4J activo: solo se exportó JSON");
        }

        info!(
            total_elapsed_s = started.elapsed().as_secs_f32(),
            "caso de uso finalizado"
        );
        Ok(ProfileOutcome {
            profile,
            failed_tables: failed,
            export_path,
        })
    }
}
