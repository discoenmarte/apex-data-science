//! Agregado: TableProfile es el Aggregate Root.
//! Una columna o constraint jamás existe sola — siempre pertenece a una tabla.
//! Las reglas del negocio (marcar PKs, validar unicidad de columnas) viven aquí,
//! no en SQL ni en la capa de infraestructura.

use crate::domain::entities::column::ColumnProfile;
use crate::domain::value_objects::identifiers::QualifiedName;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstraintKind {
    PrimaryKey,
    ForeignKey,
    Unique,
    Check,
    Exclusion,
    Trigger,
    Other,
}

impl ConstraintKind {
    /// Mapea `pg_constraint.contype`.
    pub fn from_pg(contype: &str) -> Self {
        match contype {
            "p" => Self::PrimaryKey,
            "f" => Self::ForeignKey,
            "u" => Self::Unique,
            "c" => Self::Check,
            "x" => Self::Exclusion,
            "t" => Self::Trigger,
            _ => Self::Other,
        }
    }
}

/// Destino de una foreign key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyTarget {
    pub table: QualifiedName,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableConstraint {
    pub name: String,
    pub kind: ConstraintKind,
    pub definition: String,
    pub columns: Vec<String>,
    /// Solo para ForeignKey.
    pub references: Option<ForeignKeyTarget>,
}

/// Aggregate Root del perfil de una tabla.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableProfile {
    pub id: QualifiedName,
    pub comment: Option<String>,
    /// reltuples: -1 si la tabla nunca fue analizada.
    pub row_estimate: i64,
    pub total_size_bytes: i64,
    pub columns: Vec<ColumnProfile>,
    pub constraints: Vec<TableConstraint>,
}

impl TableProfile {
    pub fn new(
        id: QualifiedName,
        comment: Option<String>,
        row_estimate: i64,
        total_size_bytes: i64,
    ) -> Self {
        Self {
            id,
            comment,
            row_estimate,
            total_size_bytes,
            columns: Vec::new(),
            constraints: Vec::new(),
        }
    }

    /// Regla: no se admiten columnas duplicadas dentro del agregado.
    pub fn add_column(&mut self, column: ColumnProfile) -> anyhow::Result<()> {
        if self.columns.iter().any(|c| c.name == column.name) {
            anyhow::bail!(
                "columna duplicada '{}' en tabla {}",
                column.name,
                self.id
            );
        }
        self.columns.push(column);
        Ok(())
    }

    /// Regla: al agregar constraints, el agregado mantiene la invariante
    /// de que las columnas PK quedan marcadas.
    pub fn add_constraint(&mut self, constraint: TableConstraint) {
        if constraint.kind == ConstraintKind::PrimaryKey {
            for col in self.columns.iter_mut() {
                if constraint.columns.contains(&col.name) {
                    col.is_primary_key = true;
                }
            }
        }
        self.constraints.push(constraint);
    }

    pub fn primary_key_columns(&self) -> Vec<&str> {
        self.columns
            .iter()
            .filter(|c| c.is_primary_key)
            .map(|c| c.name.as_str())
            .collect()
    }

    pub fn foreign_keys(&self) -> impl Iterator<Item = &TableConstraint> {
        self.constraints
            .iter()
            .filter(|c| c.kind == ConstraintKind::ForeignKey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TableProfile {
        TableProfile::new(QualifiedName::new("public", "facturas"), None, 100, 4096)
    }

    fn col(name: &str, pos: i16) -> ColumnProfile {
        ColumnProfile::new(name, pos, "integer", false, None, None, None)
    }

    #[test]
    fn rejects_duplicate_columns() {
        let mut t = table();
        t.add_column(col("id", 1)).unwrap();
        assert!(t.add_column(col("id", 2)).is_err());
    }

    #[test]
    fn primary_key_constraint_marks_columns() {
        let mut t = table();
        t.add_column(col("id", 1)).unwrap();
        t.add_column(col("valor", 2)).unwrap();
        t.add_constraint(TableConstraint {
            name: "facturas_pkey".into(),
            kind: ConstraintKind::PrimaryKey,
            definition: "PRIMARY KEY (id)".into(),
            columns: vec!["id".into()],
            references: None,
        });
        assert_eq!(t.primary_key_columns(), vec!["id"]);
    }
}
