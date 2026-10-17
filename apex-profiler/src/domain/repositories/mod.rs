//! Interfaces de Repositorio (puertos del dominio).
//! Aquí NO aparece SQL ni Cypher — la infraestructura implementa estas interfaces.

use crate::domain::entities::database::DatabaseProfile;
use crate::domain::entities::table::TableProfile;
use crate::domain::services::relationship_resolver::ForeignKeyRelation;
use crate::domain::value_objects::identifiers::QualifiedName;
use async_trait::async_trait;
use std::path::PathBuf;

/// Puerto de lectura: catálogo de la fuente de datos.
#[async_trait]
pub trait SchemaCatalog: Send + Sync {
    async fn database_name(&self) -> anyhow::Result<String>;

    /// Lista las tablas de los esquemas indicados.
    async fn list_tables(&self, schemas: &[String]) -> anyhow::Result<Vec<QualifiedName>>;

    /// Perfila una tabla completa (metadatos + columnas + estadísticas + constraints).
    async fn profile_table(&self, table: &QualifiedName) -> anyhow::Result<TableProfile>;
}

/// Puerto de escritura: grafo de conocimiento (Neo4j u otro).
#[async_trait]
pub trait KnowledgeGraph: Send + Sync {
    /// Crea constraints/índices de unicidad. Idempotente.
    async fn initialize(&self) -> anyhow::Result<()>;

    /// Persiste el perfil completo y sus relaciones. Idempotente (MERGE).
    async fn persist(
        &self,
        profile: &DatabaseProfile,
        relations: &[ForeignKeyRelation],
    ) -> anyhow::Result<GraphWriteSummary>;
}

/// Puerto de exportación de artefactos (JSON hoy, Parquet/R2 en pasos siguientes).
pub trait ProfileExporter: Send + Sync {
    fn export(
        &self,
        profile: &DatabaseProfile,
        relations: &[ForeignKeyRelation],
    ) -> anyhow::Result<PathBuf>;
}

#[derive(Debug, Default)]
pub struct GraphWriteSummary {
    pub schemas: usize,
    pub tables: usize,
    pub columns: usize,
    pub fk_relations: usize,
}
