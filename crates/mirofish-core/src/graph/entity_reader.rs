//! Entity reading and filtering — reads nodes from a graph and filters by entity type.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Entity node data structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityNode {
    pub uuid: String,
    pub name: String,
    pub labels: Vec<String>,
    pub summary: String,
    pub attributes: serde_json::Value,
    /// Related edge information.
    #[serde(default)]
    pub related_edges: Vec<serde_json::Value>,
    /// Related node information.
    #[serde(default)]
    pub related_nodes: Vec<serde_json::Value>,
}

impl EntityNode {
    /// Get the entity type label (excluding "Entity" and "Node").
    pub fn get_entity_type(&self) -> Option<&str> {
        self.labels
            .iter()
            .find(|l| *l != "Entity" && *l != "Node")
            .map(|s| s.as_str())
    }
}

/// Collection of filtered entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteredEntities {
    pub entities: Vec<EntityNode>,
    pub entity_types: HashSet<String>,
    pub total_count: usize,
    pub filtered_count: usize,
}

/// Entity reader for Zep graphs (HTTP-based).
pub struct ZepEntityReader {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

impl ZepEntityReader {
    /// Create a new reader with an API key.
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            base_url: "https://api.getzep.com/api/v2".to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Create from global config.
    pub fn from_global_config() -> anyhow::Result<Self> {
        let cfg = crate::config::Config::global();
        let key = cfg
            .zep_api_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("ZEP_API_KEY is not configured"))?;
        Ok(Self::new(key))
    }

    /// Read all nodes from a graph and filter by defined entity types.
    ///
    /// If `defined_entity_types` is `None`, returns all non-default-label nodes.
    pub async fn filter_defined_entities(
        &self,
        graph_id: &str,
        defined_entity_types: Option<&[String]>,
        enrich_with_edges: bool,
    ) -> anyhow::Result<FilteredEntities> {
        // Fetch nodes
        let nodes_url = format!("{}/graph/{}/node", self.base_url, graph_id);
        let resp = self
            .http
            .get(&nodes_url)
            .header("Authorization", format!("Api-Key {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zep API error ({}): {}", status, body);
        }

        let nodes_data: Vec<serde_json::Value> = resp.json().await?;

        // Build entity nodes
        let mut entities = Vec::new();
        let mut entity_types_set = HashSet::new();
        let total_count = nodes_data.len();

        let type_filter: Option<HashSet<&str>> = defined_entity_types.map(|types| {
            types.iter().map(|s| s.as_str()).collect()
        });

        for node in &nodes_data {
            let labels: Vec<String> = node
                .get("labels")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            // Get entity type (non-default labels)
            let entity_type = labels
                .iter()
                .find(|l| *l != "Entity" && *l != "Node");

            let entity_type_str = match entity_type {
                Some(t) => t.as_str(),
                None => continue, // Skip nodes with only default labels
            };

            // Filter by type if requested
            if let Some(ref filter) = type_filter {
                if !filter.contains(entity_type_str) {
                    continue;
                }
            }

            entity_types_set.insert(entity_type_str.to_string());

            let entity = EntityNode {
                uuid: node.get("uuid_").or(node.get("uuid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                name: node.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                labels,
                summary: node.get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                attributes: node.get("attributes")
                    .cloned()
                    .unwrap_or(serde_json::Value::Object(Default::default())),
                related_edges: Vec::new(),
                related_nodes: Vec::new(),
            };

            entities.push(entity);
        }

        // Optionally enrich with edges
        if enrich_with_edges && !entities.is_empty() {
            if let Ok(edges) = self.fetch_edges(graph_id).await {
                let node_names: std::collections::HashMap<String, String> = entities
                    .iter()
                    .map(|e| (e.uuid.clone(), e.name.clone()))
                    .collect();

                for entity in entities.iter_mut() {
                    for edge in &edges {
                        let src = edge.get("source_node_uuid").and_then(|v| v.as_str()).unwrap_or("");
                        let tgt = edge.get("target_node_uuid").and_then(|v| v.as_str()).unwrap_or("");
                        if src == entity.uuid || tgt == entity.uuid {
                            let empty = String::new();
                            let edge_info = serde_json::json!({
                                "relation_type": edge.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                                "fact": edge.get("fact").and_then(|v| v.as_str()).unwrap_or(""),
                                "source": node_names.get(src).unwrap_or(&empty),
                                "target": node_names.get(tgt).unwrap_or(&empty),
                            });
                            entity.related_edges.push(edge_info);
                        }
                    }
                }
            }
        }

        let filtered_count = entities.len();

        Ok(FilteredEntities {
            entities,
            entity_types: entity_types_set,
            total_count,
            filtered_count,
        })
    }

    /// Fetch all edges from a graph.
    async fn fetch_edges(&self, graph_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let url = format!("{}/graph/{}/edge", self.base_url, graph_id);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Api-Key {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Failed to fetch edges: {}", resp.status());
        }

        let edges: Vec<serde_json::Value> = resp.json().await?;
        Ok(edges)
    }
}
