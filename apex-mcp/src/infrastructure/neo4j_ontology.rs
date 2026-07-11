//! Adapter del puerto Ontology contra Neo4j (el grafo del paso 1).
//! Escala: índices fulltext (Lucene) para búsqueda O(log n), shortestPath
//! acotado para join planning; nunca escaneos lineales sobre el grafo.

use crate::domain::{
    ColumnInfo, DomainOverview, ForeignKeyInfo, JoinPath, Ontology, SchemaSummary, TableDetail,
    TableHit,
};
use async_trait::async_trait;
use neo4rs::{query, Graph};
use std::collections::HashMap;

pub struct Neo4jOntology {
    graph: Graph,
}

impl Neo4jOntology {
    pub async fn connect(uri: &str, user: &str, password: &str) -> anyhow::Result<Self> {
        let graph = Graph::new(uri, user, password).await?;
        Ok(Self { graph })
    }

    /// Escapa caracteres especiales de Lucene y arma una consulta tolerante:
    /// coincidencia exacta O prefijo, por término.
    fn lucene_query(term: &str) -> String {
        let sanitized: String = term
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c.is_whitespace() {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        sanitized
            .split_whitespace()
            .map(|w| format!("{w} {w}*"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[async_trait]
impl Ontology for Neo4jOntology {
    async fn ensure_indexes(&self) -> anyhow::Result<()> {
        let stmts = [
            "CREATE FULLTEXT INDEX apexTableSearch IF NOT EXISTS \
             FOR (t:Table) ON EACH [t.name, t.fqn, t.comment]",
            "CREATE FULLTEXT INDEX apexColumnSearch IF NOT EXISTS \
             FOR (c:Column) ON EACH [c.name, c.comment]",
        ];
        for s in stmts {
            self.graph.run(query(s)).await?;
        }
        Ok(())
    }

    async fn overview(&self) -> anyhow::Result<DomainOverview> {
        let mut database = String::new();
        let mut result = self
            .graph
            .execute(query("MATCH (d:Database) RETURN d.name AS name LIMIT 1"))
            .await?;
        if let Ok(Some(row)) = result.next().await {
            database = row.get::<String>("name").unwrap_or_default();
        }

        let mut schemas = Vec::new();
        let mut result = self
            .graph
            .execute(query(
                "MATCH (s:Schema) OPTIONAL MATCH (s)-[:HAS_TABLE]->(t:Table) \
                 RETURN s.name AS schema, count(t) AS tables, \
                        sum(coalesce(t.size_bytes, 0)) AS bytes \
                 ORDER BY bytes DESC",
            ))
            .await?;
        while let Ok(Some(row)) = result.next().await {
            if row.get::<String>("schema").is_err() {
                continue;
            }
            schemas.push(SchemaSummary {
                schema: row.get::<String>("schema").unwrap_or_default(),
                tables: row.get::<i64>("tables").unwrap_or(0),
                size_bytes: row.get::<i64>("bytes").unwrap_or(0),
            });
        }

        let top_query = |order: &str| {
            format!(
                "MATCH (t:Table) OPTIONAL MATCH (t)-[r:REFERENCES]-() \
                 WITH t, count(r) AS deg \
                 RETURN t.fqn AS fqn, t.schema AS schema, t.name AS name, \
                        t.comment AS comment, coalesce(t.row_estimate,0) AS rows, \
                        coalesce(t.size_bytes,0) AS bytes, deg \
                 ORDER BY {order} DESC LIMIT 10"
            )
        };
        let mut biggest = Vec::new();
        let mut result = self.graph.execute(query(&top_query("bytes"))).await?;
        while let Ok(Some(row)) = result.next().await {
            biggest.push(row_to_hit(&row, 0.0));
        }
        let mut connected = Vec::new();
        let mut result = self.graph.execute(query(&top_query("deg"))).await?;
        while let Ok(Some(row)) = result.next().await {
            connected.push(row_to_hit(&row, 0.0));
        }

        Ok(DomainOverview {
            database,
            schemas,
            biggest_tables: biggest,
            most_connected_tables: connected,
        })
    }

    async fn search_tables(&self, term: &str, top_k: usize) -> anyhow::Result<Vec<TableHit>> {
        let q = Self::lucene_query(term);
        let k = top_k as i64;
        let mut hits: HashMap<String, TableHit> = HashMap::new();

        // 1. Match directo sobre tablas
        let mut result = self
            .graph
            .execute(
                query(
                    "CALL db.index.fulltext.queryNodes('apexTableSearch', $q) \
                     YIELD node, score WITH node, score LIMIT $k \
                     OPTIONAL MATCH (node)-[r:REFERENCES]-() \
                     RETURN node.fqn AS fqn, node.schema AS schema, node.name AS name, \
                            node.comment AS comment, coalesce(node.row_estimate,0) AS rows, \
                            coalesce(node.size_bytes,0) AS bytes, count(r) AS deg, \
                            max(score) AS score",
                )
                .param("q", q.clone())
                .param("k", k),
            )
            .await?;
        while let Ok(Some(row)) = result.next().await {
            let hit = row_to_hit(&row, row.get::<f64>("score").unwrap_or(0.0));
            hits.insert(hit.fqn.clone(), hit);
        }

        // 2. Match por columnas (ej. buscar "saldo" encuentra la tabla dueña)
        let mut result = self
            .graph
            .execute(
                query(
                    "CALL db.index.fulltext.queryNodes('apexColumnSearch', $q) \
                     YIELD node, score \
                     WITH node.table_fqn AS fqn, collect(DISTINCT node.name)[..5] AS cols, \
                          max(score) AS cscore LIMIT $k \
                     MATCH (t:Table {fqn: fqn}) \
                     OPTIONAL MATCH (t)-[r:REFERENCES]-() \
                     RETURN t.fqn AS fqn, t.schema AS schema, t.name AS name, \
                            t.comment AS comment, coalesce(t.row_estimate,0) AS rows, \
                            coalesce(t.size_bytes,0) AS bytes, count(r) AS deg, \
                            max(cscore) AS score, cols",
                )
                .param("q", q)
                .param("k", k),
            )
            .await?;
        while let Ok(Some(row)) = result.next().await {
            let cols: Vec<String> = row.get::<Vec<String>>("cols").unwrap_or_default();
            // Peso menor: el match fue por columna, no por la tabla misma
            let mut hit = row_to_hit(&row, row.get::<f64>("score").unwrap_or(0.0) * 0.6);
            hit.matched_columns = cols;
            hits.entry(hit.fqn.clone())
                .and_modify(|h| {
                    h.matched_columns = hit.matched_columns.clone();
                    h.score += hit.score * 0.3;
                })
                .or_insert(hit);
        }

        let mut out: Vec<TableHit> = hits.into_values().collect();
        // Desempate por centralidad: tablas más conectadas suelen ser las de negocio
        out.sort_by(|a, b| {
            (b.score, b.fk_degree)
                .partial_cmp(&(a.score, a.fk_degree))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(top_k);
        Ok(out)
    }

    async fn describe_table(&self, fqn: &str) -> anyhow::Result<Option<TableDetail>> {
        let mut result = self
            .graph
            .execute(
                query(
                    "MATCH (t:Table {fqn: $fqn}) \
                     RETURN t.comment AS comment, coalesce(t.row_estimate,0) AS rows, \
                            coalesce(t.size_bytes,0) AS bytes, \
                            coalesce(t.constraints, []) AS constraints",
                )
                .param("fqn", fqn.to_string()),
            )
            .await?;
        let head = match result.next().await {
            Ok(Some(row)) => row,
            _ => return Ok(None),
        };

        let mut detail = TableDetail {
            fqn: fqn.to_string(),
            comment: head.get::<String>("comment").ok().filter(|s| !s.is_empty()),
            row_estimate: head.get::<i64>("rows").unwrap_or(0),
            size_bytes: head.get::<i64>("bytes").unwrap_or(0),
            columns: Vec::new(),
            constraints: head.get::<Vec<String>>("constraints").unwrap_or_default(),
            foreign_keys: Vec::new(),
        };

        let mut result = self
            .graph
            .execute(
                query(
                    "MATCH (t:Table {fqn: $fqn})-[:HAS_COLUMN]->(c:Column) \
                     RETURN c.name AS name, coalesce(c.position,0) AS position, \
                            coalesce(c.data_type,'') AS data_type, \
                            coalesce(c.nullable,true) AS nullable, \
                            coalesce(c.is_primary_key,false) AS pk, \
                            c.null_frac AS null_frac, c.distinct_estimate AS nd, \
                            c.comment AS comment \
                     ORDER BY position",
                )
                .param("fqn", fqn.to_string()),
            )
            .await?;
        while let Ok(Some(row)) = result.next().await {
            detail.columns.push(ColumnInfo {
                name: row.get::<String>("name").unwrap_or_default(),
                position: row.get::<i64>("position").unwrap_or(0),
                data_type: row.get::<String>("data_type").unwrap_or_default(),
                nullable: row.get::<bool>("nullable").unwrap_or(true),
                is_primary_key: row.get::<bool>("pk").unwrap_or(false),
                null_frac: row.get::<f64>("null_frac").ok(),
                distinct_estimate: row.get::<f64>("nd").ok(),
                comment: row.get::<String>("comment").ok().filter(|s| !s.is_empty()),
            });
        }

        detail.foreign_keys = self.neighborhood(fqn).await?;
        Ok(Some(detail))
    }

    async fn join_path(&self, from_fqn: &str, to_fqn: &str) -> anyhow::Result<Option<JoinPath>> {
        let mut result = self
            .graph
            .execute(
                query(
                    "MATCH (a:Table {fqn: $from}), (b:Table {fqn: $to}) \
                     MATCH p = shortestPath((a)-[:REFERENCES*..6]-(b)) \
                     RETURN [n IN nodes(p) | n.fqn] AS tables, \
                            [r IN relationships(p) | startNode(r).fqn + ' -[' + \
                              coalesce(r.constraint,'fk') + ']-> ' + endNode(r).fqn] AS hops, \
                            [r IN relationships(p) | \
                              reduce(s = '', i IN range(0, size(coalesce(r.from_columns,[]))-1) | \
                                s + (CASE WHEN i > 0 THEN ', ' ELSE '' END) + \
                                r.from_columns[i] + ' = ' + r.to_columns[i])] AS pairs \
                     LIMIT 1",
                )
                .param("from", from_fqn.to_string())
                .param("to", to_fqn.to_string()),
            )
            .await?;
        match result.next().await {
            Ok(Some(row)) => Ok(Some(JoinPath {
                tables: row.get::<Vec<String>>("tables").unwrap_or_default(),
                hops: row.get::<Vec<String>>("hops").unwrap_or_default(),
                column_pairs: row.get::<Vec<String>>("pairs").unwrap_or_default(),
            })),
            _ => Ok(None),
        }
    }

    async fn neighborhood(&self, fqn: &str) -> anyhow::Result<Vec<ForeignKeyInfo>> {
        let mut fks = Vec::new();
        let mut result = self
            .graph
            .execute(
                query(
                    "MATCH (t:Table {fqn: $fqn})-[r:REFERENCES]->(o:Table) \
                     RETURN 'out' AS dir, coalesce(r.constraint,'fk') AS constraint, \
                            o.fqn AS other, \
                            reduce(s = '', i IN range(0, size(coalesce(r.from_columns,[]))-1) | \
                              s + (CASE WHEN i > 0 THEN ', ' ELSE '' END) + \
                              r.from_columns[i] + ' = ' + r.to_columns[i]) AS pairs \
                     UNION ALL \
                     MATCH (t:Table {fqn: $fqn})<-[r:REFERENCES]-(o:Table) \
                     RETURN 'in' AS dir, coalesce(r.constraint,'fk') AS constraint, \
                            o.fqn AS other, \
                            reduce(s = '', i IN range(0, size(coalesce(r.from_columns,[]))-1) | \
                              s + (CASE WHEN i > 0 THEN ', ' ELSE '' END) + \
                              r.from_columns[i] + ' = ' + r.to_columns[i]) AS pairs",
                )
                .param("fqn", fqn.to_string()),
            )
            .await?;
        while let Ok(Some(row)) = result.next().await {
            fks.push(ForeignKeyInfo {
                direction: row.get::<String>("dir").unwrap_or_default(),
                constraint: row.get::<String>("constraint").unwrap_or_default(),
                other_table: row.get::<String>("other").unwrap_or_default(),
                column_pairs: row.get::<String>("pairs").unwrap_or_default(),
            });
        }
        Ok(fks)
    }
}

fn row_to_hit(row: &neo4rs::Row, score: f64) -> TableHit {
    TableHit {
        fqn: row.get::<String>("fqn").unwrap_or_default(),
        schema: row.get::<String>("schema").unwrap_or_default(),
        name: row.get::<String>("name").unwrap_or_default(),
        comment: row.get::<String>("comment").ok().filter(|s| !s.is_empty()),
        row_estimate: row.get::<i64>("rows").unwrap_or(0),
        size_bytes: row.get::<i64>("bytes").unwrap_or(0),
        fk_degree: row.get::<i64>("deg").unwrap_or(0),
        score,
        matched_columns: Vec::new(),
    }
}
