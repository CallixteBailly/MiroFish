//! Zep retrieval tools for the report agent.
//!
//! Wraps graph search, node reading, and edge querying into structured results
//! that the report agent can consume.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::zep::ZepClient;

/// Search result from the Zep graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub facts: Vec<String>,
    pub edges: Vec<Value>,
    pub nodes: Vec<Value>,
    pub query: String,
    pub total_count: usize,
}

impl SearchResult {
    /// Convert to a text representation for LLM consumption.
    pub fn to_text(&self) -> String {
        let mut parts = vec![
            format!("Search query: {}", self.query),
            format!("Found {} relevant results", self.total_count),
        ];

        if !self.facts.is_empty() {
            parts.push("\n### Related facts:".to_string());
            for (i, fact) in self.facts.iter().enumerate() {
                parts.push(format!("{}. {}", i + 1, fact));
            }
        }

        parts.join("\n")
    }
}

/// Panorama search result (includes valid and expired facts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanoramaResult {
    pub valid_facts: Vec<String>,
    pub expired_facts: Vec<String>,
    pub entities: Vec<Value>,
    pub query: String,
}

impl PanoramaResult {
    pub fn to_text(&self) -> String {
        let mut parts = vec![format!("Panorama search: {}", self.query)];

        if !self.valid_facts.is_empty() {
            parts.push("\n### Currently valid facts:".to_string());
            for (i, f) in self.valid_facts.iter().enumerate() {
                parts.push(format!("{}. {}", i + 1, f));
            }
        }
        if !self.expired_facts.is_empty() {
            parts.push("\n### Historical/expired facts:".to_string());
            for (i, f) in self.expired_facts.iter().enumerate() {
                parts.push(format!("{}. {}", i + 1, f));
            }
        }

        parts.join("\n")
    }
}

/// Insight forge result (deep retrieval with sub-queries).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightForgeResult {
    pub sub_queries: Vec<String>,
    pub combined_facts: Vec<String>,
    pub entity_insights: Vec<String>,
    pub query: String,
}

impl InsightForgeResult {
    pub fn to_text(&self) -> String {
        let mut parts = vec![
            format!("Deep insight query: {}", self.query),
            format!("Sub-queries generated: {}", self.sub_queries.len()),
        ];

        if !self.combined_facts.is_empty() {
            parts.push("\n### Combined facts:".to_string());
            for (i, f) in self.combined_facts.iter().enumerate() {
                parts.push(format!("{}. {}", i + 1, f));
            }
        }
        if !self.entity_insights.is_empty() {
            parts.push("\n### Entity insights:".to_string());
            for insight in &self.entity_insights {
                parts.push(format!("- {}", insight));
            }
        }

        parts.join("\n")
    }
}

/// Zep tools service for report agent use.
pub struct ZepToolsService {
    client: ZepClient,
    graph_id: String,
}

impl ZepToolsService {
    /// Create a new tools service for a specific graph.
    pub fn new(client: ZepClient, graph_id: &str) -> Self {
        Self {
            client,
            graph_id: graph_id.to_string(),
        }
    }

    /// Quick search — simple fact retrieval.
    pub async fn quick_search(&self, query: &str) -> anyhow::Result<SearchResult> {
        let result = self.client.search(&self.graph_id, query).await?;

        let mut facts = Vec::new();
        let mut edges = Vec::new();
        let mut nodes = Vec::new();

        // Extract facts from search result
        if let Some(edge_arr) = result.get("edges").and_then(|v| v.as_array()) {
            for edge in edge_arr {
                if let Some(fact) = edge.get("fact").and_then(|v| v.as_str()) {
                    if !fact.is_empty() {
                        facts.push(fact.to_string());
                    }
                }
                edges.push(edge.clone());
            }
        }

        if let Some(node_arr) = result.get("nodes").and_then(|v| v.as_array()) {
            for node in node_arr {
                nodes.push(node.clone());
            }
        }

        let total_count = facts.len();

        Ok(SearchResult {
            facts,
            edges,
            nodes,
            query: query.to_string(),
            total_count,
        })
    }

    /// Panorama search — includes both valid and expired/historical facts.
    pub async fn panorama_search(&self, query: &str) -> anyhow::Result<PanoramaResult> {
        let result = self.client.search(&self.graph_id, query).await?;

        let mut valid_facts = Vec::new();
        let mut expired_facts = Vec::new();
        let mut entities = Vec::new();

        if let Some(edge_arr) = result.get("edges").and_then(|v| v.as_array()) {
            for edge in edge_arr {
                let fact = edge.get("fact").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if fact.is_empty() {
                    continue;
                }
                let is_expired = edge.get("expired_at").and_then(|v| v.as_str()).is_some()
                    || edge.get("invalid_at").and_then(|v| v.as_str()).is_some();

                if is_expired {
                    expired_facts.push(fact);
                } else {
                    valid_facts.push(fact);
                }
            }
        }

        if let Some(node_arr) = result.get("nodes").and_then(|v| v.as_array()) {
            for node in node_arr {
                entities.push(node.clone());
            }
        }

        Ok(PanoramaResult {
            valid_facts,
            expired_facts,
            entities,
            query: query.to_string(),
        })
    }

    /// Insight forge — deep retrieval with sub-query decomposition.
    /// Uses the LLM to decompose the query, then performs multiple searches.
    pub async fn insight_forge(
        &self,
        query: &str,
        llm: &crate::llm::LlmClient,
    ) -> anyhow::Result<InsightForgeResult> {
        use crate::llm::client::ChatMessage;

        // Ask LLM to decompose the query into sub-queries
        let decompose_prompt = format!(
            "Please decompose the following query into 2-4 specific sub-queries for graph search.\n\
             Output a JSON array of strings only.\n\nQuery: {}",
            query
        );

        let sub_queries = match llm
            .chat_json(
                &[ChatMessage::user(decompose_prompt)],
                0.3,
                Some(512),
            )
            .await
        {
            Ok(val) => {
                if let Some(arr) = val.as_array() {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                } else if let Some(queries) = val.get("queries").and_then(|v| v.as_array()) {
                    queries.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                } else {
                    vec![query.to_string()]
                }
            }
            Err(_) => vec![query.to_string()],
        };

        // Search with each sub-query
        let mut combined_facts = Vec::new();
        let mut entity_insights = Vec::new();
        let mut seen_facts = std::collections::HashSet::new();

        for sq in &sub_queries {
            if let Ok(result) = self.client.search(&self.graph_id, sq).await {
                if let Some(edge_arr) = result.get("edges").and_then(|v| v.as_array()) {
                    for edge in edge_arr {
                        if let Some(fact) = edge.get("fact").and_then(|v| v.as_str()) {
                            if !fact.is_empty() && seen_facts.insert(fact.to_string()) {
                                combined_facts.push(fact.to_string());
                            }
                        }
                    }
                }
                if let Some(node_arr) = result.get("nodes").and_then(|v| v.as_array()) {
                    for node in node_arr {
                        let name = node.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let summary = node.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                        if !name.is_empty() && !summary.is_empty() {
                            let insight = format!("{}: {}", name, summary);
                            if seen_facts.insert(insight.clone()) {
                                entity_insights.push(insight);
                            }
                        }
                    }
                }
            }
        }

        Ok(InsightForgeResult {
            sub_queries,
            combined_facts,
            entity_insights,
            query: query.to_string(),
        })
    }

    /// Get all nodes in the graph.
    pub async fn get_all_nodes(&self) -> anyhow::Result<Vec<Value>> {
        self.client.get_nodes(&self.graph_id).await
    }

    /// Get all edges in the graph.
    pub async fn get_all_edges(&self) -> anyhow::Result<Vec<Value>> {
        self.client.get_edges(&self.graph_id).await
    }
}
