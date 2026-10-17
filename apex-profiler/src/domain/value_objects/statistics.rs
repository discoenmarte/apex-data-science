//! Value Object: estadísticas de columna tomadas de `pg_stats`.
//! Solo metadatos estadísticos — NUNCA valores de datos (privacidad: DB clínica).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ColumnStatistics {
    /// Fracción de nulos [0.0, 1.0].
    pub null_frac: Option<f32>,
    /// Postgres semantics: >= 0 valor absoluto de distintos; < 0 => -(ratio sobre filas).
    pub n_distinct: Option<f32>,
    /// Ancho promedio en bytes.
    pub avg_width: Option<i32>,
    /// Correlación con el orden físico [-1.0, 1.0].
    pub correlation: Option<f32>,
}

impl ColumnStatistics {
    pub fn is_empty(&self) -> bool {
        self.null_frac.is_none()
            && self.n_distinct.is_none()
            && self.avg_width.is_none()
            && self.correlation.is_none()
    }

    /// Estimación absoluta de valores distintos dada la estimación de filas.
    pub fn distinct_estimate(&self, row_estimate: i64) -> Option<i64> {
        self.n_distinct.map(|n| {
            if n >= 0.0 {
                n as i64
            } else {
                ((-n as f64) * row_estimate.max(0) as f64) as i64
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_n_distinct_is_a_ratio() {
        let s = ColumnStatistics {
            n_distinct: Some(-0.5),
            ..Default::default()
        };
        assert_eq!(s.distinct_estimate(1000), Some(500));
    }

    #[test]
    fn positive_n_distinct_is_absolute() {
        let s = ColumnStatistics {
            n_distinct: Some(42.0),
            ..Default::default()
        };
        assert_eq!(s.distinct_estimate(1_000_000), Some(42));
    }
}
