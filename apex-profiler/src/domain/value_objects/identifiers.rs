//! Value Objects: identidad de objetos del catálogo.
//! Sin identidad propia — dos QualifiedName iguales son el mismo valor.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Nombre calificado `schema.tabla`. Inmutable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QualifiedName {
    pub schema: String,
    pub name: String,
}

impl QualifiedName {
    pub fn new(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            name: name.into(),
        }
    }

    /// Fully-qualified name, clave única en el grafo.
    pub fn fqn(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }
}

impl fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.fqn())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fqn_joins_schema_and_name() {
        let q = QualifiedName::new("public", "hc_evoluciones");
        assert_eq!(q.fqn(), "public.hc_evoluciones");
    }

    #[test]
    fn equality_is_by_value() {
        assert_eq!(
            QualifiedName::new("public", "t"),
            QualifiedName::new("public", "t")
        );
    }
}
