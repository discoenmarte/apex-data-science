//! Servicio de Dominio: resuelve el grafo de relaciones FK entre tablas.
//! Es un servicio porque involucra múltiples agregados (tablas), no pertenece
//! a ninguna tabla individual.

use crate::domain::entities::table::TableProfile;
use crate::domain::value_objects::identifiers::QualifiedName;
use serde::{Deserialize, Serialize};

/// Relación derivada tabla -> tabla vía foreign key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyRelation {
    pub constraint: String,
    pub from: QualifiedName,
    pub to: QualifiedName,
    /// Pares (columna_origen, columna_destino) alineados por posición.
    pub column_pairs: Vec<(String, String)>,
}

pub struct RelationshipResolver;

impl RelationshipResolver {
    pub fn resolve(tables: &[TableProfile]) -> Vec<ForeignKeyRelation> {
        let mut relations = Vec::new();
        for table in tables {
            for fk in table.foreign_keys() {
                let Some(target) = &fk.references else {
                    continue;
                };
                let pairs = fk
                    .columns
                    .iter()
                    .zip(target.columns.iter())
                    .map(|(a, b)| (a.clone(), b.clone()))
                    .collect();
                relations.push(ForeignKeyRelation {
                    constraint: fk.name.clone(),
                    from: table.id.clone(),
                    to: target.table.clone(),
                    column_pairs: pairs,
                });
            }
        }
        relations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::table::{
        ConstraintKind, ForeignKeyTarget, TableConstraint,
    };

    #[test]
    fn resolves_fk_relations_with_column_pairs() {
        let mut t = TableProfile::new(
            QualifiedName::new("public", "cuentas_detalle"),
            None,
            0,
            0,
        );
        t.add_constraint(TableConstraint {
            name: "fk_cuenta".into(),
            kind: ConstraintKind::ForeignKey,
            definition: "FOREIGN KEY (cuenta_id) REFERENCES cuentas(id)".into(),
            columns: vec!["cuenta_id".into()],
            references: Some(ForeignKeyTarget {
                table: QualifiedName::new("public", "cuentas"),
                columns: vec!["id".into()],
            }),
        });

        let rels = RelationshipResolver::resolve(&[t]);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].to.fqn(), "public.cuentas");
        assert_eq!(rels[0].column_pairs, vec![("cuenta_id".into(), "id".into())]);
    }
}
