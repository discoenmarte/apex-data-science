//! Capa MCP: expone los casos de uso como tools tipados (rmcp).
//! Este módulo es la frontera de aplicación: orquesta puertos del dominio,
//! aplica la política y serializa resultados. Aquí no vive lógica de negocio.

use crate::domain::{policy, DataSource, MetricSpec, Ontology};
use crate::infrastructure::config::AppConfig;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use std::sync::Arc;
use tracing::info;

#[derive(Clone)]
pub struct ApexMcp {
    ontology: Arc<dyn Ontology>,
    source: Arc<dyn DataSource>,
    cfg: Arc<AppConfig>,
}

// ---------- Parámetros de tools (el JSON Schema sale de estos structs) ----------

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    /// Término de negocio a buscar, ej. "cartera", "facturación", "saldo"
    pub term: String,
    /// Máximo de tablas a retornar (default 10)
    pub top_k: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FqnParams {
    /// Nombre calificado de la tabla: "esquema.tabla", ej. "cartera.saldos"
    pub fqn: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct JoinPathParams {
    /// Tabla origen ("esquema.tabla")
    pub from_fqn: String,
    /// Tabla destino ("esquema.tabla")
    pub to_fqn: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RunQueryParams {
    /// Consulta SQL de SOLO LECTURA (SELECT/WITH). Un solo statement.
    pub sql: String,
    /// Tope de filas (se aplica el mínimo entre esto y el límite del servidor)
    pub max_rows: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MetricParam {
    /// Función: count | sum | avg | min | max
    pub func: String,
    /// Columna a agregar (opcional solo para count)
    pub column: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AggregateParams {
    /// Tabla objetivo: "esquema.tabla"
    pub table_fqn: String,
    /// Métricas a calcular
    pub metrics: Vec<MetricParam>,
    /// Columnas de agrupación (opcional)
    pub group_by: Option<Vec<String>>,
    /// Filtro SQL opcional para el WHERE (solo la expresión, sin "WHERE")
    pub filter_sql: Option<String>,
    /// Máximo de grupos/filas a retornar (default 20)
    pub limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SampleParams {
    /// Tabla objetivo: "esquema.tabla"
    pub table_fqn: String,
    /// Número de filas de muestra (máx 50)
    pub n: Option<usize>,
}

// ---------- Helpers ----------

fn ok_json<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

fn err_msg(context: &str, e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(format!("{context}: {e}"), None)
}

// ---------- Tools ----------

#[tool_router]
impl ApexMcp {
    pub async fn new(cfg: AppConfig) -> anyhow::Result<Self> {
        let ontology = Arc::new(
            crate::infrastructure::neo4j_ontology::Neo4jOntology::connect(
                &cfg.neo4j_uri,
                &cfg.neo4j_user,
                &cfg.neo4j_password,
            )
            .await?,
        );
        ontology.ensure_indexes().await?;
        info!("ontología conectada e índices fulltext asegurados");

        let pool = cfg.pg_pool()?;
        let source = Arc::new(crate::infrastructure::postgres_source::PostgresSource::new(
            pool,
            cfg.source_max_concurrency,
            cfg.query_timeout_ms,
        ));

        Ok(Self {
            ontology,
            source,
            cfg: Arc::new(cfg),
        })
    }

    #[tool(
        description = "Vista general del dominio: esquemas, tablas más grandes y tablas más conectadas (centralidad por FKs). Empieza aquí si no sabes qué existe. Local y rápido (no usa la VPN)."
    )]
    async fn domain_overview(&self) -> Result<CallToolResult, McpError> {
        let overview = self
            .ontology
            .overview()
            .await
            .map_err(|e| err_msg("consultando la ontología", e))?;
        ok_json(&overview)
    }

    #[tool(
        description = "Busca tablas relevantes para un término de negocio (ej. 'cartera') entre 4,542 tablas usando índices fulltext sobre nombres, comentarios y columnas, con ranking por centralidad FK. Local y rápido (no usa la VPN)."
    )]
    async fn search_tables(
        &self,
        Parameters(SearchParams { term, top_k }): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let hits = self
            .ontology
            .search_tables(&term, top_k.unwrap_or(10).clamp(1, 50))
            .await
            .map_err(|e| err_msg("buscando en la ontología", e))?;
        ok_json(&hits)
    }

    #[tool(
        description = "Describe una tabla: columnas con tipos y estadísticas (null_frac, cardinalidad), constraints, y FKs entrantes/salientes con sus pares de columnas. Local y rápido."
    )]
    async fn describe_table(
        &self,
        Parameters(FqnParams { fqn }): Parameters<FqnParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .ontology
            .describe_table(&fqn)
            .await
            .map_err(|e| err_msg("describiendo tabla", e))?
        {
            Some(detail) => ok_json(&detail),
            None => Err(McpError::invalid_params(
                format!("tabla no encontrada en la ontología: {fqn} (usa search_tables primero)"),
                None,
            )),
        }
    }

    #[tool(
        description = "Camino de JOINs más corto entre dos tablas siguiendo las foreign keys del grafo, con los pares de columnas exactos de cada salto. Úsalo antes de escribir SQL multi-tabla."
    )]
    async fn join_path(
        &self,
        Parameters(JoinPathParams { from_fqn, to_fqn }): Parameters<JoinPathParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .ontology
            .join_path(&from_fqn, &to_fqn)
            .await
            .map_err(|e| err_msg("calculando join path", e))?
        {
            Some(path) => ok_json(&path),
            None => ok_json(&serde_json::json!({
                "path_found": false,
                "hint": "no hay camino de FKs (≤6 saltos) entre esas tablas; puede requerir un join implícito por convención de nombres"
            })),
        }
    }

    #[tool(
        description = "Tablas vecinas de una tabla (FKs entrantes y salientes con sus columnas). Contexto inmediato de una entidad. Local y rápido."
    )]
    async fn table_neighborhood(
        &self,
        Parameters(FqnParams { fqn }): Parameters<FqnParams>,
    ) -> Result<CallToolResult, McpError> {
        let fks = self
            .ontology
            .neighborhood(&fqn)
            .await
            .map_err(|e| err_msg("consultando vecindario", e))?;
        ok_json(&fks)
    }

    #[tool(
        description = "Agregado seguro sobre la fuente real (vía VPN): count/sum/avg/min/max con group by, generado por plantilla y validado contra las columnas reales. Camino preferido para insights de negocio: números, no registros."
    )]
    async fn aggregate_table(
        &self,
        Parameters(AggregateParams {
            table_fqn,
            metrics,
            group_by,
            filter_sql,
            limit,
        }): Parameters<AggregateParams>,
    ) -> Result<CallToolResult, McpError> {
        // Validar columnas contra la ontología (no confiamos en el llamador)
        let detail = self
            .ontology
            .describe_table(&table_fqn)
            .await
            .map_err(|e| err_msg("validando tabla", e))?
            .ok_or_else(|| {
                McpError::invalid_params(
                    format!("tabla no encontrada en la ontología: {table_fqn}"),
                    None,
                )
            })?;
        let known: Vec<String> = detail.columns.iter().map(|c| c.name.clone()).collect();
        let specs: Vec<MetricSpec> = metrics
            .into_iter()
            .map(|m| MetricSpec {
                func: m.func,
                column: m.column,
            })
            .collect();

        let sql = policy::build_aggregate_sql(
            &table_fqn,
            &specs,
            &group_by.unwrap_or_default(),
            filter_sql.as_deref(),
            &known,
            limit.unwrap_or(20).clamp(1, self.cfg.max_rows),
        )
        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        let result = self
            .source
            .run_select(&sql, self.cfg.max_rows)
            .await
            .map_err(|e| err_msg("ejecutando agregado en la fuente", e))?;
        ok_json(&result)
    }

    #[tool(
        description = "Ejecuta un SELECT de solo lectura sobre la fuente real (vía VPN) con guardrails: validación por AST, transacción READ ONLY, timeout de 15s y tope de filas. Para cuando aggregate_table no alcanza."
    )]
    async fn run_read_query(
        &self,
        Parameters(RunQueryParams { sql, max_rows }): Parameters<RunQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let cleaned = policy::validate_read_only(&sql)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let cap = max_rows
            .unwrap_or(self.cfg.max_rows)
            .clamp(1, self.cfg.max_rows);
        let result = self
            .source
            .run_select(&cleaned, cap)
            .await
            .map_err(|e| err_msg("ejecutando en la fuente", e))?;
        ok_json(&result)
    }

    #[tool(
        description = "Muestra de filas crudas de una tabla (máx 50). DESHABILITADO por defecto porque la fuente contiene datos clínicos; requiere ALLOW_ROW_SAMPLING=true."
    )]
    async fn sample_rows(
        &self,
        Parameters(SampleParams { table_fqn, n }): Parameters<SampleParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.cfg.allow_row_sampling {
            return Err(McpError::invalid_params(
                "sample_rows está deshabilitado (datos clínicos). Usa aggregate_table, o habilita ALLOW_ROW_SAMPLING=true bajo tu responsabilidad.".to_string(),
                None,
            ));
        }
        let table = policy::quote_fqn(&table_fqn)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let n = n.unwrap_or(10).clamp(1, 50);
        let result = self
            .source
            .run_select(&format!("SELECT * FROM {table}"), n)
            .await
            .map_err(|e| err_msg("muestreando la fuente", e))?;
        ok_json(&result)
    }

    #[tool(
        description = "Estado de la fuente de datos: ¿el túnel VPN/port-forward responde? Latencia del ping. Llama esto si una consulta a la fuente falla."
    )]
    async fn source_status(&self) -> Result<CallToolResult, McpError> {
        match self.source.ping().await {
            Ok(ms) => ok_json(&serde_json::json!({
                "reachable": true, "latency_ms": ms,
                "hint": "fuente disponible; las consultas pagarán ~esta latencia por round-trip"
            })),
            Err(e) => ok_json(&serde_json::json!({
                "reachable": false, "error": e.to_string(),
                "hint": "levanta la VPN (make vpn-connect + make vpn-forward en apex-profiler). Los tools de ontología siguen funcionando."
            })),
        }
    }
}

#[tool_handler]
impl ServerHandler for ApexMcp {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "apex-mcp: acceso a la ontología del cliente (grafo Neo4j de 4,542 tablas) y \
             consultas puntuales de solo lectura a la fuente real. Flujo recomendado: \
             search_tables → describe_table/join_path → aggregate_table. \
             Los tools de ontología son locales y rápidos; los de datos pagan la VPN."
                .to_string(),
        );
        info
    }
}
