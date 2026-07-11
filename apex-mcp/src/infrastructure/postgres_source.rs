//! Adapter del puerto DataSource contra el Postgres origen (vía VPN).
//! Guardrails de ejecución (defensa en profundidad, la política ya validó el SQL):
//!   - Semáforo: máximo N consultas concurrentes (protege el OLTP clínico)
//!   - Transacción READ ONLY + statement_timeout por consulta
//!   - Envoltura row_to_json + LIMIT: tipado universal y tope de filas en el servidor
//!   - Pool de conexiones perezoso: la VPN puede no estar activa al arrancar

use crate::domain::{DataSource, QueryResult};
use async_trait::async_trait;
use deadpool_postgres::Pool;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

pub struct PostgresSource {
    pool: Pool,
    limiter: Arc<Semaphore>,
    timeout_ms: u64,
}

impl PostgresSource {
    pub fn new(pool: Pool, max_concurrency: usize, timeout_ms: u64) -> Self {
        Self {
            pool,
            limiter: Arc::new(Semaphore::new(max_concurrency.max(1))),
            timeout_ms,
        }
    }
}

#[async_trait]
impl DataSource for PostgresSource {
    async fn run_select(&self, sql: &str, max_rows: usize) -> anyhow::Result<QueryResult> {
        let _permit = self.limiter.acquire().await?;
        let started = Instant::now();

        let mut client = self.pool.get().await.map_err(|e| {
            anyhow::anyhow!("no hay conexión a la fuente (¿VPN/port-forward activos?): {e}")
        })?;

        // Pedimos max_rows + 1 para poder reportar truncamiento honesto
        let wrapped = format!(
            "SELECT row_to_json(apex_q)::text AS r FROM ({sql}) AS apex_q LIMIT {}",
            max_rows + 1
        );

        let tx = client.transaction().await?;
        tx.batch_execute(&format!(
            "SET TRANSACTION READ ONLY; SET LOCAL statement_timeout = {}",
            self.timeout_ms
        ))
        .await?;
        let rows = tx
            .query(&wrapped, &[])
            .await
            .map_err(|e| anyhow::anyhow!("la fuente rechazó la consulta: {e}"))?;
        tx.rollback().await.ok();

        let truncated = rows.len() > max_rows;
        let mut out = Vec::with_capacity(rows.len().min(max_rows));
        for row in rows.iter().take(max_rows) {
            let raw: &str = row.get("r");
            out.push(serde_json::from_str(raw).unwrap_or(serde_json::Value::Null));
        }

        Ok(QueryResult {
            row_count: out.len(),
            rows: out,
            truncated,
            elapsed_ms: started.elapsed().as_millis(),
            executed_sql: wrapped,
        })
    }

    async fn ping(&self) -> anyhow::Result<u128> {
        let started = Instant::now();
        let client = tokio::time::timeout(
            std::time::Duration::from_millis(self.timeout_ms.min(5000)),
            self.pool.get(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("timeout obteniendo conexión (¿VPN caída?)"))??;
        tokio::time::timeout(
            std::time::Duration::from_millis(self.timeout_ms.min(5000)),
            client.query_one("SELECT 1", &[]),
        )
        .await
        .map_err(|_| anyhow::anyhow!("timeout en SELECT 1 (¿túnel congelado?)"))??;
        Ok(started.elapsed().as_millis())
    }
}
