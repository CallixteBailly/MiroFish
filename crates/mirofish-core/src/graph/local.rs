//! Local knowledge graph service using petgraph + rusqlite.
//!
//! Provides an SQLite-backed graph that can replace Zep Cloud for development.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

use super::entity_reader::{EntityNode, FilteredEntities};

/// Graph metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphInfo {
    pub graph_id: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub entity_types: Vec<String>,
}

/// Local node record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalNode {
    pub uuid: String,
    pub graph_id: String,
    pub name: String,
    pub labels: Vec<String>,
    pub summary: String,
    pub attributes: serde_json::Value,
}

/// Local edge record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalEdge {
    pub uuid: String,
    pub graph_id: String,
    pub source_uuid: String,
    pub target_uuid: String,
    pub source_name: String,
    pub target_name: String,
    pub relation_type: String,
    pub fact: String,
}

/// Local graph service backed by SQLite.
pub struct LocalGraphService {
    conn: Mutex<Connection>,
}

impl LocalGraphService {
    /// Open (or create) the graph database at the given path.
    pub fn open(db_path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        let svc = Self { conn: Mutex::new(conn) };
        svc.init_db()?;
        Ok(svc)
    }

    /// Open using the default path derived from the global config upload folder.
    pub fn from_global_config() -> anyhow::Result<Self> {
        let cfg = crate::config::Config::global();
        let db_path = cfg.upload_folder.parent()
            .unwrap_or(Path::new("."))
            .join("local_graph.db");
        Self::open(&db_path)
    }

    /// Initialize the database schema.
    fn init_db(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {}", e))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS graphs (
                graph_id TEXT PRIMARY KEY,
                name     TEXT,
                ontology TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS nodes (
                uuid       TEXT PRIMARY KEY,
                graph_id   TEXT,
                name       TEXT,
                labels     TEXT,
                summary    TEXT,
                attributes TEXT DEFAULT '{}',
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS edges (
                uuid          TEXT PRIMARY KEY,
                graph_id      TEXT,
                source_uuid   TEXT,
                target_uuid   TEXT,
                source_name   TEXT,
                target_name   TEXT,
                relation_type TEXT,
                fact          TEXT,
                created_at    TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_nodes_graph ON nodes(graph_id);
            CREATE INDEX IF NOT EXISTS idx_edges_graph ON edges(graph_id);"
        )?;
        Ok(())
    }

    // ── Write operations ──

    /// Create a new graph and return its ID.
    pub fn create_graph(&self, name: &str) -> anyhow::Result<String> {
        let graph_id = format!("local_{}", &Uuid::new_v4().to_string().replace('-', "")[..12]);
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {}", e))?;
        conn.execute(
            "INSERT INTO graphs (graph_id, name) VALUES (?1, ?2)",
            params![graph_id, name],
        )?;
        tracing::info!("Created local graph: {} ({})", graph_id, name);
        Ok(graph_id)
    }

    /// Store ontology definition for a graph.
    pub fn set_ontology(&self, graph_id: &str, ontology: &serde_json::Value) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {}", e))?;
        let ontology_str = serde_json::to_string(ontology)?;
        conn.execute(
            "UPDATE graphs SET ontology = ?1 WHERE graph_id = ?2",
            params![ontology_str, graph_id],
        )?;
        Ok(())
    }

    /// Add a node to a graph. Returns the node UUID.
    /// If a node with the same name (case-insensitive) already exists, updates its summary.
    pub fn add_node(
        &self,
        graph_id: &str,
        name: &str,
        labels: &[String],
        summary: &str,
        attributes: Option<&serde_json::Value>,
    ) -> anyhow::Result<String> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {}", e))?;

        // Check for existing node
        let existing: Option<String> = conn
            .query_row(
                "SELECT uuid FROM nodes WHERE graph_id = ?1 AND LOWER(name) = LOWER(?2)",
                params![graph_id, name],
                |row| row.get(0),
            )
            .ok();

        if let Some(existing_uuid) = existing {
            if !summary.is_empty() {
                conn.execute(
                    "UPDATE nodes SET summary = ?1 WHERE uuid = ?2",
                    params![summary, existing_uuid],
                )?;
            }
            return Ok(existing_uuid);
        }

        let node_uuid = Uuid::new_v4().to_string();
        let labels_json = serde_json::to_string(labels)?;
        let attrs_json = attributes
            .map(|a| serde_json::to_string(a))
            .transpose()?
            .unwrap_or_else(|| "{}".to_string());

        conn.execute(
            "INSERT INTO nodes (uuid, graph_id, name, labels, summary, attributes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![node_uuid, graph_id, name, labels_json, summary, attrs_json],
        )?;

        Ok(node_uuid)
    }

    /// Add an edge between two nodes (looked up by name). Returns the edge UUID.
    pub fn add_edge(
        &self,
        graph_id: &str,
        source_name: &str,
        target_name: &str,
        relation_type: &str,
        fact: &str,
    ) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {}", e))?;

        let src_uuid: Option<String> = conn
            .query_row(
                "SELECT uuid FROM nodes WHERE graph_id = ?1 AND LOWER(name) = LOWER(?2)",
                params![graph_id, source_name],
                |row| row.get(0),
            )
            .ok();

        let tgt_uuid: Option<String> = conn
            .query_row(
                "SELECT uuid FROM nodes WHERE graph_id = ?1 AND LOWER(name) = LOWER(?2)",
                params![graph_id, target_name],
                |row| row.get(0),
            )
            .ok();

        let (src, tgt) = match (src_uuid, tgt_uuid) {
            (Some(s), Some(t)) => (s, t),
            _ => return Ok(None),
        };

        // Check for duplicate edge
        let dup: Option<String> = conn
            .query_row(
                "SELECT uuid FROM edges WHERE graph_id=?1 AND source_uuid=?2 AND target_uuid=?3 AND relation_type=?4",
                params![graph_id, src, tgt, relation_type],
                |row| row.get(0),
            )
            .ok();

        if let Some(existing) = dup {
            return Ok(Some(existing));
        }

        let edge_uuid = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO edges (uuid, graph_id, source_uuid, target_uuid, source_name, target_name, relation_type, fact) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![edge_uuid, graph_id, src, tgt, source_name, target_name, relation_type, fact],
        )?;

        Ok(Some(edge_uuid))
    }

    // ── Read operations ──

    /// Get graph data (nodes and edges).
    pub fn get_graph_data(&self, graph_id: &str) -> anyhow::Result<serde_json::Value> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {}", e))?;

        // Fetch nodes
        let mut stmt = conn.prepare(
            "SELECT uuid, name, labels, summary, attributes FROM nodes WHERE graph_id = ?1"
        )?;
        let nodes: Vec<serde_json::Value> = stmt
            .query_map(params![graph_id], |row| {
                let uuid: String = row.get(0)?;
                let name: String = row.get(1)?;
                let labels_str: String = row.get(2)?;
                let summary: String = row.get(3)?;
                let attrs_str: String = row.get(4)?;
                Ok(serde_json::json!({
                    "uuid": uuid,
                    "name": name,
                    "labels": serde_json::from_str::<serde_json::Value>(&labels_str).unwrap_or_default(),
                    "summary": summary,
                    "attributes": serde_json::from_str::<serde_json::Value>(&attrs_str).unwrap_or_default(),
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Fetch edges
        let mut stmt = conn.prepare(
            "SELECT uuid, source_uuid, target_uuid, source_name, target_name, relation_type, fact \
             FROM edges WHERE graph_id = ?1"
        )?;
        let edges: Vec<serde_json::Value> = stmt
            .query_map(params![graph_id], |row| {
                let uuid: String = row.get(0)?;
                let source_uuid: String = row.get(1)?;
                let target_uuid: String = row.get(2)?;
                let source_name: String = row.get(3)?;
                let target_name: String = row.get(4)?;
                let relation_type: String = row.get(5)?;
                let fact: String = row.get(6)?;
                Ok(serde_json::json!({
                    "uuid": uuid,
                    "source_node_uuid": source_uuid,
                    "target_node_uuid": target_uuid,
                    "source_node_name": source_name,
                    "target_node_name": target_name,
                    "name": relation_type,
                    "fact": fact,
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::json!({
            "graph_id": graph_id,
            "nodes": nodes,
            "edges": edges,
            "node_count": nodes.len(),
            "edge_count": edges.len(),
        }))
    }

    /// Get graph statistics.
    pub fn get_statistics(&self, graph_id: &str) -> anyhow::Result<GraphInfo> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {}", e))?;

        let node_count: usize = conn.query_row(
            "SELECT COUNT(*) FROM nodes WHERE graph_id = ?1",
            params![graph_id],
            |row| row.get(0),
        )?;

        let edge_count: usize = conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE graph_id = ?1",
            params![graph_id],
            |row| row.get(0),
        )?;

        // Collect entity types
        let mut stmt = conn.prepare("SELECT labels FROM nodes WHERE graph_id = ?1")?;
        let mut entity_types = HashSet::new();
        let rows = stmt.query_map(params![graph_id], |row| {
            let labels_str: String = row.get(0)?;
            Ok(labels_str)
        })?;

        for row in rows {
            if let Ok(labels_str) = row {
                if let Ok(labels) = serde_json::from_str::<Vec<String>>(&labels_str) {
                    for label in labels {
                        if label != "Entity" && label != "Node" {
                            entity_types.insert(label);
                        }
                    }
                }
            }
        }

        Ok(GraphInfo {
            graph_id: graph_id.to_string(),
            node_count,
            edge_count,
            entity_types: entity_types.into_iter().collect(),
        })
    }

    /// Delete a graph and all its nodes/edges.
    pub fn delete_graph(&self, graph_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {}", e))?;
        conn.execute("DELETE FROM edges WHERE graph_id = ?1", params![graph_id])?;
        conn.execute("DELETE FROM nodes WHERE graph_id = ?1", params![graph_id])?;
        conn.execute("DELETE FROM graphs WHERE graph_id = ?1", params![graph_id])?;
        tracing::info!("Deleted local graph: {}", graph_id);
        Ok(())
    }

    /// Get filtered entities from the local graph (for simulation use).
    pub fn get_filtered_entities(
        &self,
        graph_id: &str,
        defined_entity_types: Option<&[String]>,
    ) -> anyhow::Result<FilteredEntities> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {}", e))?;

        // Fetch all edges for this graph, keyed by node UUID
        let mut edge_stmt = conn.prepare(
            "SELECT source_uuid, target_uuid, source_name, target_name, relation_type, fact \
             FROM edges WHERE graph_id = ?1"
        )?;
        let mut edges_by_node: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
        let edge_rows = edge_stmt.query_map(params![graph_id], |row| {
            let source_uuid: String = row.get(0)?;
            let target_uuid: String = row.get(1)?;
            let source_name: String = row.get(2)?;
            let target_name: String = row.get(3)?;
            let relation_type: String = row.get(4)?;
            let fact: String = row.get(5)?;
            Ok((source_uuid, target_uuid, source_name, target_name, relation_type, fact))
        })?;

        for row in edge_rows {
            if let Ok((src_uuid, tgt_uuid, src_name, tgt_name, rel, fact)) = row {
                let info = serde_json::json!({
                    "relation_type": rel,
                    "fact": fact,
                    "source": src_name,
                    "target": tgt_name,
                });
                edges_by_node.entry(src_uuid.clone()).or_default().push(info.clone());
                edges_by_node.entry(tgt_uuid).or_default().push(info);
            }
        }

        // Fetch nodes
        let mut node_stmt = conn.prepare(
            "SELECT uuid, name, labels, summary, attributes FROM nodes WHERE graph_id = ?1"
        )?;
        let type_filter: Option<HashSet<&str>> = defined_entity_types.map(|t| {
            t.iter().map(|s| s.as_str()).collect()
        });

        let mut entities = Vec::new();
        let mut entity_types = HashSet::new();
        let mut total_count = 0usize;

        let node_rows = node_stmt.query_map(params![graph_id], |row| {
            let uuid: String = row.get(0)?;
            let name: String = row.get(1)?;
            let labels_str: String = row.get(2)?;
            let summary: String = row.get(3)?;
            let attrs_str: String = row.get(4)?;
            Ok((uuid, name, labels_str, summary, attrs_str))
        })?;

        for row in node_rows {
            if let Ok((uuid, name, labels_str, summary, attrs_str)) = row {
                total_count += 1;
                let labels: Vec<String> =
                    serde_json::from_str(&labels_str).unwrap_or_default();

                let entity_type = labels
                    .iter()
                    .find(|l| *l != "Entity" && *l != "Node");

                let et = match entity_type {
                    Some(t) => t.as_str(),
                    None => continue,
                };

                if let Some(ref filter) = type_filter {
                    if !filter.contains(et) {
                        continue;
                    }
                }

                entity_types.insert(et.to_string());

                let related = edges_by_node.get(&uuid).cloned().unwrap_or_default();
                let attrs: serde_json::Value =
                    serde_json::from_str(&attrs_str).unwrap_or_default();

                entities.push(EntityNode {
                    uuid,
                    name,
                    labels,
                    summary,
                    attributes: attrs,
                    related_edges: related,
                    related_nodes: Vec::new(),
                });
            }
        }

        let filtered_count = entities.len();
        Ok(FilteredEntities {
            entities,
            entity_types,
            total_count,
            filtered_count,
        })
    }

    /// Simple keyword search across nodes and edges (for report agent).
    pub fn search(&self, graph_id: &str, query: &str) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("lock: {}", e))?;
        let pattern = format!("%{}%", query);
        let mut facts = Vec::new();

        // Search node summaries
        let mut stmt = conn.prepare(
            "SELECT name, summary FROM nodes WHERE graph_id = ?1 AND (name LIKE ?2 OR summary LIKE ?2)"
        )?;
        let rows = stmt.query_map(params![graph_id, pattern], |row| {
            let name: String = row.get(0)?;
            let summary: String = row.get(1)?;
            Ok(format!("{}: {}", name, summary))
        })?;
        for row in rows {
            if let Ok(fact) = row {
                facts.push(fact);
            }
        }

        // Search edge facts
        let mut stmt = conn.prepare(
            "SELECT source_name, target_name, relation_type, fact FROM edges WHERE graph_id = ?1 AND (fact LIKE ?2 OR source_name LIKE ?2 OR target_name LIKE ?2)"
        )?;
        let rows = stmt.query_map(params![graph_id, pattern], |row| {
            let src: String = row.get(0)?;
            let tgt: String = row.get(1)?;
            let rel: String = row.get(2)?;
            let fact: String = row.get(3)?;
            Ok(format!("{} --[{}]--> {}: {}", src, rel, tgt, fact))
        })?;
        for row in rows {
            if let Ok(fact) = row {
                facts.push(fact);
            }
        }

        Ok(facts)
    }
}
