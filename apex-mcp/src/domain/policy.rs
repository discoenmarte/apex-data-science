//! QueryPolicy: reglas puras sobre qué SQL se acepta.
//! Validación por AST (sqlparser), no por regex — la fuente es una base clínica
//! en producción y este es el único punto de entrada de SQL libre.

use sqlparser::ast::{SetExpr, Statement};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("SQL inválido: {0}")]
    Invalid(String),
    #[error("solo se permite exactamente 1 statement (recibidos: {0})")]
    MultipleStatements(usize),
    #[error("solo se permiten consultas SELECT/WITH de lectura, no: {0}")]
    NotReadOnly(String),
    #[error("SELECT INTO no está permitido")]
    SelectInto,
    #[error("identificador inválido: {0}")]
    BadIdentifier(String),
    #[error("columna desconocida en la tabla: {0}")]
    UnknownColumn(String),
    #[error("función de agregado no soportada: {0} (use count|sum|avg|min|max)")]
    BadAggregate(String),
    #[error("se requiere al menos una métrica")]
    NoMetrics,
}

/// Valida que el SQL sea una única consulta de lectura y lo retorna limpio
/// (sin `;` final) listo para ser envuelto por el ejecutor.
pub fn validate_read_only(sql: &str) -> Result<String, PolicyError> {
    let cleaned = sql.trim().trim_end_matches(';').trim().to_string();
    if cleaned.is_empty() {
        return Err(PolicyError::Invalid("consulta vacía".into()));
    }
    let statements = Parser::parse_sql(&PostgreSqlDialect {}, &cleaned)
        .map_err(|e| PolicyError::Invalid(e.to_string()))?;
    if statements.len() != 1 {
        return Err(PolicyError::MultipleStatements(statements.len()));
    }
    match &statements[0] {
        Statement::Query(query) => {
            if let SetExpr::Select(select) = query.body.as_ref() {
                if select.into.is_some() {
                    return Err(PolicyError::SelectInto);
                }
            }
            Ok(cleaned)
        }
        other => Err(PolicyError::NotReadOnly(
            format!("{other}").chars().take(60).collect(),
        )),
    }
}

/// Un identificador Postgres citado de forma segura ("...").
pub fn quote_ident(raw: &str) -> Result<String, PolicyError> {
    let ok = !raw.is_empty()
        && raw.len() <= 128
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ');
    if !ok {
        return Err(PolicyError::BadIdentifier(raw.to_string()));
    }
    Ok(format!("\"{raw}\""))
}

/// Cita un fqn "esquema.tabla" como "esquema"."tabla".
pub fn quote_fqn(fqn: &str) -> Result<String, PolicyError> {
    let parts: Vec<&str> = fqn.split('.').collect();
    if parts.len() != 2 {
        return Err(PolicyError::BadIdentifier(format!(
            "{fqn} (se espera esquema.tabla)"
        )));
    }
    Ok(format!(
        "{}.{}",
        quote_ident(parts[0])?,
        quote_ident(parts[1])?
    ))
}

/// Construye el SQL de un agregado seguro a partir de la especificación,
/// validando cada columna contra las columnas reales de la tabla.
pub fn build_aggregate_sql(
    table_fqn: &str,
    metrics: &[crate::domain::MetricSpec],
    group_by: &[String],
    filter_sql: Option<&str>,
    known_columns: &[String],
    limit: usize,
) -> Result<String, PolicyError> {
    if metrics.is_empty() {
        return Err(PolicyError::NoMetrics);
    }
    let table = quote_fqn(table_fqn)?;
    let check_col = |c: &str| -> Result<String, PolicyError> {
        if !known_columns.iter().any(|k| k == c) {
            return Err(PolicyError::UnknownColumn(c.to_string()));
        }
        quote_ident(c)
    };

    let mut select_parts: Vec<String> = Vec::new();
    for g in group_by {
        select_parts.push(check_col(g)?);
    }
    for (i, m) in metrics.iter().enumerate() {
        let func = m.func.to_lowercase();
        if !matches!(func.as_str(), "count" | "sum" | "avg" | "min" | "max") {
            return Err(PolicyError::BadAggregate(m.func.clone()));
        }
        let expr = match (&func[..], &m.column) {
            ("count", None) => "count(*)".to_string(),
            (_, Some(col)) => format!("{}({})", func, check_col(col)?),
            (f, None) => return Err(PolicyError::BadAggregate(format!("{f} requiere columna"))),
        };
        select_parts.push(format!("{expr} AS metric_{i}"));
    }

    let mut sql = format!("SELECT {} FROM {}", select_parts.join(", "), table);
    if let Some(f) = filter_sql {
        let f = f.trim();
        if !f.is_empty() {
            sql.push_str(&format!(" WHERE {f}"));
        }
    }
    if !group_by.is_empty() {
        let cols: Result<Vec<String>, PolicyError> =
            group_by.iter().map(|g| check_col(g)).collect();
        sql.push_str(&format!(" GROUP BY {}", cols?.join(", ")));
        // Orden por la primera métrica para que "top N" signifique algo
        sql.push_str(&format!(" ORDER BY metric_{} DESC", 0));
    }
    sql.push_str(&format!(" LIMIT {limit}"));

    // Defensa en profundidad: el SQL generado también pasa por el validador
    validate_read_only(&sql)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MetricSpec;

    #[test]
    fn accepts_plain_select() {
        assert!(validate_read_only("SELECT 1").is_ok());
        assert!(validate_read_only("  SELECT a, b FROM t WHERE x > 1 ; ").is_ok());
    }

    #[test]
    fn accepts_cte() {
        assert!(validate_read_only("WITH x AS (SELECT 1 AS n) SELECT n FROM x").is_ok());
    }

    #[test]
    fn rejects_dml_ddl() {
        assert!(validate_read_only("DELETE FROM t").is_err());
        assert!(validate_read_only("UPDATE t SET a=1").is_err());
        assert!(validate_read_only("DROP TABLE t").is_err());
        assert!(validate_read_only("INSERT INTO t VALUES (1)").is_err());
        assert!(validate_read_only("CREATE TABLE t (a int)").is_err());
    }

    #[test]
    fn rejects_multiple_statements() {
        assert!(matches!(
            validate_read_only("SELECT 1; SELECT 2"),
            Err(PolicyError::MultipleStatements(2))
        ));
    }

    #[test]
    fn rejects_select_into() {
        assert!(matches!(
            validate_read_only("SELECT a INTO nueva FROM t"),
            Err(PolicyError::SelectInto)
        ));
    }

    #[test]
    fn quotes_identifiers_and_rejects_injection() {
        assert_eq!(quote_ident("saldo").unwrap(), "\"saldo\"");
        assert!(quote_ident("x\"; DROP TABLE t; --").is_err());
        assert_eq!(
            quote_fqn("cartera.saldos").unwrap(),
            "\"cartera\".\"saldos\""
        );
        assert!(quote_fqn("sin_esquema").is_err());
    }

    #[test]
    fn builds_aggregate() {
        let cols = vec!["saldo".to_string(), "tercero".to_string()];
        let sql = build_aggregate_sql(
            "cartera.saldos",
            &[MetricSpec {
                func: "sum".into(),
                column: Some("saldo".into()),
            }],
            &["tercero".to_string()],
            None,
            &cols,
            20,
        )
        .unwrap();
        assert!(sql.contains("sum(\"saldo\") AS metric_0"));
        assert!(sql.contains("GROUP BY \"tercero\""));
        assert!(sql.contains("LIMIT 20"));
    }

    #[test]
    fn aggregate_rejects_unknown_column() {
        let cols = vec!["saldo".to_string()];
        assert!(matches!(
            build_aggregate_sql(
                "cartera.saldos",
                &[MetricSpec {
                    func: "sum".into(),
                    column: Some("no_existe".into())
                }],
                &[],
                None,
                &cols,
                10,
            ),
            Err(PolicyError::UnknownColumn(_))
        ));
    }
}
