//! Agregado raíz superior: el perfil completo de la base de datos.

use crate::domain::entities::table::TableProfile;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseProfile {
    pub name: String,
    pub profiled_at: DateTime<Utc>,
    pub tables: Vec<TableProfile>,
}

impl DatabaseProfile {
    pub fn new(name: impl Into<String>, tables: Vec<TableProfile>) -> Self {
        Self {
            name: name.into(),
            profiled_at: Utc::now(),
            tables,
        }
    }

    pub fn schemas(&self) -> Vec<&str> {
        let mut s: Vec<&str> = self.tables.iter().map(|t| t.id.schema.as_str()).collect();
        s.sort_unstable();
        s.dedup();
        s
    }

    pub fn total_columns(&self) -> usize {
        self.tables.iter().map(|t| t.columns.len()).sum()
    }

    pub fn total_size_bytes(&self) -> i64 {
        self.tables.iter().map(|t| t.total_size_bytes).sum()
    }
}
