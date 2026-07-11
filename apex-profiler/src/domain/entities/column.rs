//! Entidad: perfil de una columna dentro del agregado TableProfile.

use crate::domain::value_objects::statistics::ColumnStatistics;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnProfile {
    pub name: String,
    /// Posición ordinal (attnum).
    pub position: i16,
    pub data_type: String,
    pub is_nullable: bool,
    pub default_value: Option<String>,
    pub comment: Option<String>,
    pub is_primary_key: bool,
    /// Estadísticas de pg_stats (None si la tabla nunca fue analizada).
    pub statistics: Option<ColumnStatistics>,
}

impl ColumnProfile {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        position: i16,
        data_type: impl Into<String>,
        is_nullable: bool,
        default_value: Option<String>,
        comment: Option<String>,
        statistics: Option<ColumnStatistics>,
    ) -> Self {
        Self {
            name: name.into(),
            position,
            data_type: data_type.into(),
            is_nullable,
            default_value,
            comment,
            is_primary_key: false, // lo marca el agregado al aplicar constraints
            statistics,
        }
    }
}
