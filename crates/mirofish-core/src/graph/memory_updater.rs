//! Graph memory update service.
//!
//! Dynamically updates agent activities from simulations into the Zep graph.
//! Activities are queued and sent in batches.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

use super::zep::ZepClient;

/// An agent activity record to be sent to the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivity {
    pub platform: String,
    pub agent_id: u64,
    pub agent_name: String,
    pub action_type: String,
    pub action_args: Value,
    pub round_num: u64,
    pub timestamp: String,
}

impl AgentActivity {
    /// Convert the activity to a natural-language episode text for Zep ingestion.
    pub fn to_episode_text(&self) -> String {
        let desc = match self.action_type.as_str() {
            "CREATE_POST" => {
                let content = self.action_args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.is_empty() {
                    "published a post".to_string()
                } else {
                    format!("posted: \"{}\"", content)
                }
            }
            "LIKE_POST" => {
                let author = self.action_args.get("post_author_name").and_then(|v| v.as_str()).unwrap_or("");
                let content = self.action_args.get("post_content").and_then(|v| v.as_str()).unwrap_or("");
                match (author.is_empty(), content.is_empty()) {
                    (false, false) => format!("liked {}'s post: \"{}\"", author, content),
                    (false, true) => format!("liked a post by {}", author),
                    (true, false) => format!("liked a post: \"{}\"", content),
                    _ => "liked a post".to_string(),
                }
            }
            "REPOST" => {
                let content = self.action_args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.is_empty() { "reposted a post".to_string() } else { format!("reposted: \"{}\"", content) }
            }
            "QUOTE_POST" => {
                let content = self.action_args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.is_empty() { "quote-posted".to_string() } else { format!("quote-posted: \"{}\"", content) }
            }
            "FOLLOW" => {
                let target = self.action_args.get("target_name").and_then(|v| v.as_str()).unwrap_or("someone");
                format!("followed {}", target)
            }
            "CREATE_COMMENT" => {
                let content = self.action_args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if content.is_empty() { "commented on a post".to_string() } else { format!("commented: \"{}\"", content) }
            }
            "DISLIKE_POST" => "disliked a post".to_string(),
            "LIKE_COMMENT" => "liked a comment".to_string(),
            "DISLIKE_COMMENT" => "disliked a comment".to_string(),
            "MUTE" => {
                let target = self.action_args.get("target_name").and_then(|v| v.as_str()).unwrap_or("someone");
                format!("muted {}", target)
            }
            _ => format!("performed action: {}", self.action_type),
        };

        format!("{}: {}", self.agent_name, desc)
    }
}

/// Handle for sending activities to a background updater task.
#[derive(Clone)]
pub struct MemoryUpdaterHandle {
    tx: mpsc::UnboundedSender<AgentActivity>,
}

impl MemoryUpdaterHandle {
    /// Queue an activity for graph update.
    pub fn add_activity(&self, activity: AgentActivity) {
        let _ = self.tx.send(activity);
    }

    /// Parse an activity from a raw JSON dict (as emitted by the simulation action log).
    pub fn add_activity_from_dict(&self, data: &Value, platform: &str) {
        let activity = AgentActivity {
            platform: data.get("platform").and_then(|v| v.as_str()).unwrap_or(platform).to_string(),
            agent_id: data.get("agent_id").and_then(|v| v.as_u64()).unwrap_or(0),
            agent_name: data.get("agent_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            action_type: data.get("action_type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            action_args: data.get("action_args").cloned().unwrap_or(Value::Object(Default::default())),
            round_num: data.get("round").and_then(|v| v.as_u64()).unwrap_or(0),
            timestamp: data.get("timestamp").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        };
        self.add_activity(activity);
    }
}

/// Manager for graph memory updaters across multiple simulations.
pub struct GraphMemoryManager {
    updaters: Arc<RwLock<HashMap<String, MemoryUpdaterHandle>>>,
}

impl GraphMemoryManager {
    /// Create a new manager.
    pub fn new() -> Self {
        Self {
            updaters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create and start a background updater for a simulation.
    /// Activities sent via the returned handle will be batched and sent to Zep.
    pub fn create_updater(&self, simulation_id: &str, graph_id: &str, api_key: &str) -> MemoryUpdaterHandle {
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentActivity>();
        let handle = MemoryUpdaterHandle { tx };

        let client = ZepClient::new(api_key);
        let gid = graph_id.to_string();
        let sid = simulation_id.to_string();

        // Spawn a background task to batch-send activities
        tokio::spawn(async move {
            let mut batch: Vec<String> = Vec::new();
            let batch_size = 5;
            let flush_interval = tokio::time::Duration::from_secs(10);

            loop {
                tokio::select! {
                    activity = rx.recv() => {
                        match activity {
                            Some(a) => {
                                let text = a.to_episode_text();
                                if !text.is_empty() {
                                    batch.push(text);
                                }
                                if batch.len() >= batch_size {
                                    if let Err(e) = client.add_batch(&gid, &batch).await {
                                        tracing::warn!("Failed to send activity batch to Zep for {}: {}", sid, e);
                                    }
                                    batch.clear();
                                }
                            }
                            None => {
                                // Channel closed — flush remaining
                                if !batch.is_empty() {
                                    if let Err(e) = client.add_batch(&gid, &batch).await {
                                        tracing::warn!("Failed to flush final batch to Zep for {}: {}", sid, e);
                                    }
                                }
                                tracing::info!("Memory updater stopped for simulation {}", sid);
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep(flush_interval) => {
                        if !batch.is_empty() {
                            if let Err(e) = client.add_batch(&gid, &batch).await {
                                tracing::warn!("Failed to flush batch to Zep for {}: {}", sid, e);
                            }
                            batch.clear();
                        }
                    }
                }
            }
        });

        // Store handle
        let mut updaters = self.updaters.write().expect("updaters lock");
        updaters.insert(simulation_id.to_string(), handle.clone());

        tracing::info!("Created memory updater for simulation {}, graph {}", simulation_id, graph_id);
        handle
    }

    /// Get an existing updater handle.
    pub fn get_updater(&self, simulation_id: &str) -> Option<MemoryUpdaterHandle> {
        let updaters = self.updaters.read().expect("updaters lock");
        updaters.get(simulation_id).cloned()
    }

    /// Stop an updater by dropping the handle (closing the channel).
    pub fn stop_updater(&self, simulation_id: &str) {
        let mut updaters = self.updaters.write().expect("updaters lock");
        updaters.remove(simulation_id);
    }

    /// Stop all updaters.
    pub fn stop_all(&self) {
        let mut updaters = self.updaters.write().expect("updaters lock");
        updaters.clear();
        tracing::info!("All memory updaters stopped");
    }
}

impl Default for GraphMemoryManager {
    fn default() -> Self {
        Self::new()
    }
}
