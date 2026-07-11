//! Exporta el perfil como JSON — el artefacto "Schemas (Not Data)" del diagrama.
//! En pasos siguientes de la arquitectura este artefacto viaja a R2 como Parquet.

use crate::domain::entities::database::DatabaseProfile;
use crate::domain::repositories::ProfileExporter;
use crate::domain::services::relationship_resolver::ForeignKeyRelation;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

pub struct JsonExporter {
    dir: PathBuf,
}

impl JsonExporter {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }
}

#[derive(Serialize)]
struct ExportDocument<'a> {
    #[serde(flatten)]
    profile: &'a DatabaseProfile,
    relations: &'a [ForeignKeyRelation],
}

impl ProfileExporter for JsonExporter {
    fn export(
        &self,
        profile: &DatabaseProfile,
        relations: &[ForeignKeyRelation],
    ) -> anyhow::Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let filename = format!(
            "profile_{}_{}.json",
            profile.name,
            profile.profiled_at.format("%Y%m%dT%H%M%SZ")
        );
        let path = self.dir.join(filename);
        let doc = ExportDocument { profile, relations };
        fs::write(&path, serde_json::to_vec_pretty(&doc)?)?;
        Ok(path)
    }
}
