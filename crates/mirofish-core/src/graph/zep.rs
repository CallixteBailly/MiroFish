//! Zep Cloud HTTP client for graph operations.

use reqwest::Client;
use serde_json::Value;
use uuid::Uuid;

/// Zep Cloud REST API client.
pub struct ZepClient {
    api_key: String,
    base_url: String,
    http: Client,
}

impl ZepClient {
    /// Create a new Zep client.
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            base_url: "https://api.getzep.com/api/v2".to_string(),
            http: Client::new(),
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

    fn auth_header(&self) -> String {
        format!("Api-Key {}", self.api_key)
    }

    /// Create a new graph in Zep.
    pub async fn create_graph(&self, name: &str) -> anyhow::Result<String> {
        let graph_id = format!("mirofish_{}", &Uuid::new_v4().to_string().replace('-', "")[..16]);

        let body = serde_json::json!({
            "graph_id": graph_id,
            "name": name,
            "description": "MiroFish Social Simulation Graph",
        });

        let resp = self
            .http
            .post(format!("{}/graph", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zep create_graph error ({}): {}", status, body);
        }

        tracing::info!("Created Zep graph: {}", graph_id);
        Ok(graph_id)
    }

    /// Add text data to a graph as an episode.
    pub async fn add_data(&self, graph_id: &str, text: &str) -> anyhow::Result<Option<String>> {
        let body = serde_json::json!({
            "data": text,
            "type": "text",
        });

        let resp = self
            .http
            .post(format!("{}/graph/{}/episode", self.base_url, graph_id))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zep add_data error ({}): {}", status, body_text);
        }

        let result: Value = resp.json().await?;
        let uuid = result.get("uuid_")
            .or(result.get("uuid"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(uuid)
    }

    /// Add a batch of text episodes.
    pub async fn add_batch(&self, graph_id: &str, texts: &[String]) -> anyhow::Result<Vec<String>> {
        let episodes: Vec<Value> = texts
            .iter()
            .map(|t| serde_json::json!({"data": t, "type": "text"}))
            .collect();

        let body = serde_json::json!({ "episodes": episodes });

        let resp = self
            .http
            .post(format!("{}/graph/{}/episode/batch", self.base_url, graph_id))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zep add_batch error ({}): {}", status, body_text);
        }

        let result: Value = resp.json().await?;
        let mut uuids = Vec::new();
        if let Some(arr) = result.as_array() {
            for item in arr {
                if let Some(uuid) = item.get("uuid_").or(item.get("uuid")).and_then(|v| v.as_str()) {
                    uuids.push(uuid.to_string());
                }
            }
        }

        Ok(uuids)
    }

    /// Set ontology for a graph.
    pub async fn set_ontology(&self, graph_id: &str, ontology: &Value) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "graph_ids": [graph_id],
            "ontology": ontology,
        });

        let resp = self
            .http
            .post(format!("{}/graph/ontology", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zep set_ontology error ({}): {}", status, body_text);
        }

        Ok(())
    }

    /// Get graph data (nodes).
    pub async fn get_nodes(&self, graph_id: &str) -> anyhow::Result<Vec<Value>> {
        let resp = self
            .http
            .get(format!("{}/graph/{}/node", self.base_url, graph_id))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zep get_nodes error ({}): {}", status, body);
        }

        let nodes: Vec<Value> = resp.json().await?;
        Ok(nodes)
    }

    /// Get graph edges.
    pub async fn get_edges(&self, graph_id: &str) -> anyhow::Result<Vec<Value>> {
        let resp = self
            .http
            .get(format!("{}/graph/{}/edge", self.base_url, graph_id))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zep get_edges error ({}): {}", status, body);
        }

        let edges: Vec<Value> = resp.json().await?;
        Ok(edges)
    }

    /// Search the graph with a query.
    pub async fn search(&self, graph_id: &str, query: &str) -> anyhow::Result<Value> {
        let body = serde_json::json!({
            "query": query,
        });

        let resp = self
            .http
            .post(format!("{}/graph/{}/search", self.base_url, graph_id))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zep search error ({}): {}", status, body_text);
        }

        let result: Value = resp.json().await?;
        Ok(result)
    }

    /// Check if an episode has been processed.
    pub async fn is_episode_processed(&self, episode_uuid: &str) -> anyhow::Result<bool> {
        let resp = self
            .http
            .get(format!("{}/graph/episode/{}", self.base_url, episode_uuid))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(false);
        }

        let result: Value = resp.json().await?;
        Ok(result.get("processed").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    /// Delete a graph.
    pub async fn delete_graph(&self, graph_id: &str) -> anyhow::Result<()> {
        let resp = self
            .http
            .delete(format!("{}/graph/{}", self.base_url, graph_id))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zep delete_graph error ({}): {}", status, body);
        }

        tracing::info!("Deleted Zep graph: {}", graph_id);
        Ok(())
    }
}
