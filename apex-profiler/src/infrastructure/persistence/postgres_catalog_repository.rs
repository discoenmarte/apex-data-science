//! Adaptador de infraestructura: implementa `SchemaCatalog` contra los
//! catálogos de Postgres (pg_class, pg_attribute, pg_constraint, pg_stats).
//! Solo lee METADATOS — jamás datos de las tablas (DB clínica: privacidad).

use crate::domain::entities::column::ColumnProfile;
use crate::domain::entities::table::{
    ConstraintKind, ForeignKeyTarget, TableConstraint, TableProfile,
};
use crate::domain::repositories::SchemaCatalog;
use crate::domain::value_objects::identifiers::QualifiedName;
use crate::domain::value_objects::statistics::ColumnStatistics;
use anyhow::Context;
use async_trait::async_trait;
use deadpool_postgres::Pool;

pub struct PostgresCatalogRepository {
    pool: Pool,
}

impl PostgresCatalogRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

const LIST_TABLES: &str = r#"
SELECT n.nspname::text AS schema_name, c.relname::text AS table_name
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE c.relkind = 'r'
  AND n.nspname = ANY($1)
ORDER BY pg_total_relation_size(c.oid) DESC
"#;

const TABLE_META: &str = r#"
SELECT c.reltuples::bigint AS row_estimate,
       pg_total_relation_size(c.oid) AS total_size_bytes,
       obj_description(c.oid) AS table_comment
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind = 'r'
"#;

const TABLE_COLUMNS: &str = r#"
SELECT a.attname::text                                   AS column_name,
       a.attnum                                          AS position,
       pg_catalog.format_type(a.atttypid, a.atttypmod)   AS data_type,
       NOT a.attnotnull                                  AS is_nullable,
       pg_get_expr(ad.adbin, ad.adrelid)                 AS default_value,
       col_description(a.attrelid, a.attnum)             AS comment,
       s.null_frac                                       AS null_frac,
       s.n_distinct                                      AS n_distinct,
       s.avg_width                                       AS avg_width,
       s.correlation                                     AS correlation
FROM pg_attribute a
LEFT JOIN pg_attrdef ad ON a.attrelid = ad.adrelid AND a.attnum = ad.adnum
LEFT JOIN pg_stats s
       ON s.schemaname = $1 AND s.tablename = $2 AND s.attname = a.attname
WHERE a.attrelid = (quote_ident($1) || '.' || quote_ident($2))::regclass
  AND a.attnum > 0
  AND NOT a.attisdropped
ORDER BY a.attnum
"#;

const TABLE_CONSTRAINTS: &str = r#"
SELECT con.conname::text          AS constraint_name,
       con.contype::text          AS constraint_type,
       pg_get_constraintdef(con.oid) AS definition,
       (SELECT array_agg(a.attname::text ORDER BY x.ord)
          FROM unnest(con.conkey) WITH ORDINALITY AS x(attnum, ord)
          JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = x.attnum
       ) AS columns,
       (SELECT n2.nspname::text
          FROM pg_class c2 JOIN pg_namespace n2 ON n2.oid = c2.relnamespace
         WHERE c2.oid = con.confrelid
       ) AS ref_schema,
       (SELECT c2.relname::text FROM pg_class c2 WHERE c2.oid = con.confrelid
       ) AS ref_table,
       (SELECT array_agg(a.attname::text ORDER BY x.ord)
          FROM unnest(con.confkey) WITH ORDINALITY AS x(attnum, ord)
          JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = x.attnum
       ) AS ref_columns
FROM pg_constraint con
WHERE con.conrelid = (quote_ident($1) || '.' || quote_ident($2))::regclass
"#;

#[async_trait]
impl SchemaCatalog for PostgresCatalogRepository {
    async fn database_name(&self) -> anyhow::Result<String> {
        let client = self.pool.get().await.context("obteniendo conexión del pool")?;
        let row = client.query_one("SELECT current_database()::text", &[]).await?;
        Ok(row.get(0))
    }

    async fn list_tables(&self, schemas: &[String]) -> anyhow::Result<Vec<QualifiedName>> {
        let client = self.pool.get().await.context("obteniendo conexión del pool")?;
        let rows = client.query(LIST_TABLES, &[&schemas]).await?;
        Ok(rows
            .iter()
            .map(|r| QualifiedName::new(r.get::<_, String>(0), r.get::<_, String>(1)))
            .collect())
    }

    async fn profile_table(&self, table: &QualifiedName) -> anyhow::Result<TableProfile> {
        let client = self.pool.get().await.context("obteniendo conexión del pool")?;
        let schema = &table.schema;
        let name = &table.name;

        // 1. Metadatos de la tabla
        let meta = client
            .query_one(TABLE_META, &[schema, name])
            .await
            .with_context(|| format!("metadatos de {table}"))?;
        let mut profile = TableProfile::new(
            table.clone(),
            meta.get::<_, Option<String>>("table_comment"),
            meta.get::<_, i64>("row_estimate"),
            meta.get::<_, i64>("total_size_bytes"),
        );

        // 2. Columnas + estadísticas (pg_stats, sin valores de datos)
        let col_rows = client
            .query(TABLE_COLUMNS, &[schema, name])
            .await
            .with_context(|| format!("columnas de {table}"))?;
        for row in &col_rows {
            let stats = ColumnStatistics {
                null_frac: row.get("null_frac"),
                n_distinct: row.get("n_distinct"),
                avg_width: row.get("avg_width"),
                correlation: row.get("correlation"),
            };
            let column = ColumnProfile::new(
                row.get::<_, String>("column_name"),
                row.get::<_, i16>("position"),
                row.get::<_, String>("data_type"),
                row.get::<_, bool>("is_nullable"),
                row.get::<_, Option<String>>("default_value"),
                row.get::<_, Option<String>>("comment"),
                if stats.is_empty() { None } else { Some(stats) },
            );
            profile.add_column(column)?;
        }

        // 3. Constraints (PK/FK/UNIQUE/CHECK)
        let con_rows = client
            .query(TABLE_CONSTRAINTS, &[schema, name])
            .await
            .with_context(|| format!("constraints de {table}"))?;
        for row in &con_rows {
            let kind = ConstraintKind::from_pg(&row.get::<_, String>("constraint_type"));
            let ref_schema: Option<String> = row.get("ref_schema");
            let ref_table: Option<String> = row.get("ref_table");
            let ref_columns: Option<Vec<String>> = row.get("ref_columns");
            let references = match (ref_schema, ref_table) {
                (Some(s), Some(t)) => Some(ForeignKeyTarget {
                    table: QualifiedName::new(s, t),
                    columns: ref_columns.unwrap_or_default(),
                }),
                _ => None,
            };
            profile.add_constraint(TableConstraint {
                name: row.get("constraint_name"),
                kind,
                definition: row.get("definition"),
                columns: row
                    .get::<_, Option<Vec<String>>>("columns")
                    .unwrap_or_default(),
                references,
            });
        }

        Ok(profile)
    }
}
