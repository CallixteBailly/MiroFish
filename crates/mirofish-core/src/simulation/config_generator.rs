//! Simulation configuration generation via LLM.
//!
//! Generates time config, event config, agent activity configs, and platform
//! configs based on simulation requirements and document context.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::graph::entity_reader::EntityNode;
use crate::llm::client::{ChatMessage, LlmClient, LlmError};

/// Activity configuration for a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivityConfig {
    pub agent_id: u64,
    pub entity_uuid: String,
    pub entity_name: String,
    pub entity_type: String,
    pub activity_level: f64,
    pub posts_per_hour: f64,
    pub comments_per_hour: f64,
    pub active_hours: Vec<u32>,
    pub response_delay_min: u32,
    pub response_delay_max: u32,
    pub sentiment_bias: f64,
    pub stance: String,
    pub influence_weight: f64,
}

/// Time simulation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSimulationConfig {
    pub total_simulation_hours: u32,
    pub minutes_per_round: u32,
    pub agents_per_hour_min: u32,
    pub agents_per_hour_max: u32,
    pub peak_hours: Vec<u32>,
    pub peak_activity_multiplier: f64,
    pub off_peak_hours: Vec<u32>,
    pub off_peak_activity_multiplier: f64,
}

impl Default for TimeSimulationConfig {
    fn default() -> Self {
        Self {
            total_simulation_hours: 72,
            minutes_per_round: 60,
            agents_per_hour_min: 5,
            agents_per_hour_max: 20,
            peak_hours: vec![19, 20, 21, 22],
            peak_activity_multiplier: 1.5,
            off_peak_hours: vec![0, 1, 2, 3, 4, 5],
            off_peak_activity_multiplier: 0.05,
        }
    }
}

/// Event configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventConfig {
    #[serde(default)]
    pub initial_posts: Vec<Value>,
    #[serde(default)]
    pub scheduled_events: Vec<Value>,
    #[serde(default)]
    pub hot_topics: Vec<String>,
    #[serde(default)]
    pub narrative_direction: String,
}

impl Default for EventConfig {
    fn default() -> Self {
        Self {
            initial_posts: Vec::new(),
            scheduled_events: Vec::new(),
            hot_topics: Vec::new(),
            narrative_direction: String::new(),
        }
    }
}

/// Platform-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformConfig {
    pub platform: String,
    pub recency_weight: f64,
    pub popularity_weight: f64,
    pub relevance_weight: f64,
    pub viral_threshold: u32,
    pub echo_chamber_strength: f64,
}

/// Complete simulation parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationParameters {
    pub simulation_id: String,
    pub project_id: String,
    pub graph_id: String,
    pub simulation_requirement: String,
    pub time_config: TimeSimulationConfig,
    pub event_config: EventConfig,
    pub agent_configs: Vec<AgentActivityConfig>,
    pub platform_configs: Vec<PlatformConfig>,
    pub generation_reasoning: String,
}

impl SimulationParameters {
    /// Serialize to JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Simulation config generator.
pub struct SimulationConfigGenerator {
    llm: LlmClient,
}

impl SimulationConfigGenerator {
    /// Create a new generator.
    pub fn new(llm: LlmClient) -> Self {
        Self { llm }
    }

    /// Generate complete simulation configuration.
    pub async fn generate_config(
        &self,
        simulation_id: &str,
        project_id: &str,
        graph_id: &str,
        simulation_requirement: &str,
        document_text: &str,
        entities: &[EntityNode],
        enable_twitter: bool,
        enable_reddit: bool,
    ) -> Result<SimulationParameters, LlmError> {
        // Generate time config via LLM
        let time_config = self.generate_time_config(simulation_requirement, entities.len()).await?;

        // Generate event config
        let event_config = self.generate_event_config(simulation_requirement, document_text).await?;

        // Generate agent configs
        let agent_configs = self.generate_agent_configs(entities, simulation_requirement).await?;

        // Build platform configs
        let mut platform_configs = Vec::new();
        if enable_twitter {
            platform_configs.push(PlatformConfig {
                platform: "twitter".to_string(),
                recency_weight: 0.4,
                popularity_weight: 0.3,
                relevance_weight: 0.3,
                viral_threshold: 10,
                echo_chamber_strength: 0.5,
            });
        }
        if enable_reddit {
            platform_configs.push(PlatformConfig {
                platform: "reddit".to_string(),
                recency_weight: 0.3,
                popularity_weight: 0.4,
                relevance_weight: 0.3,
                viral_threshold: 15,
                echo_chamber_strength: 0.4,
            });
        }

        Ok(SimulationParameters {
            simulation_id: simulation_id.to_string(),
            project_id: project_id.to_string(),
            graph_id: graph_id.to_string(),
            simulation_requirement: simulation_requirement.to_string(),
            time_config,
            event_config,
            agent_configs,
            platform_configs,
            generation_reasoning: "Configuration generated via LLM analysis of simulation requirements and entity profiles.".to_string(),
        })
    }

    /// Generate time configuration via LLM.
    async fn generate_time_config(
        &self,
        simulation_requirement: &str,
        num_entities: usize,
    ) -> Result<TimeSimulationConfig, LlmError> {
        let prompt = format!(
            "Based on the following simulation requirement, generate time configuration parameters.\n\
             \n\
             Simulation requirement: {}\n\
             Number of agents: {}\n\
             \n\
             Output JSON:\n\
             {{\n\
               \"total_simulation_hours\": <int, 24-168>,\n\
               \"minutes_per_round\": <int, 30 or 60>,\n\
               \"agents_per_hour_min\": <int>,\n\
               \"agents_per_hour_max\": <int>\n\
             }}",
            simulation_requirement, num_entities,
        );

        let messages = vec![ChatMessage::user(prompt)];
        let result = self.llm.chat_json(&messages, 0.3, Some(512)).await?;

        Ok(TimeSimulationConfig {
            total_simulation_hours: result.get("total_simulation_hours")
                .and_then(|v| v.as_u64()).unwrap_or(72) as u32,
            minutes_per_round: result.get("minutes_per_round")
                .and_then(|v| v.as_u64()).unwrap_or(60) as u32,
            agents_per_hour_min: result.get("agents_per_hour_min")
                .and_then(|v| v.as_u64()).unwrap_or(5) as u32,
            agents_per_hour_max: result.get("agents_per_hour_max")
                .and_then(|v| v.as_u64()).unwrap_or(20) as u32,
            ..Default::default()
        })
    }

    /// Generate event configuration via LLM.
    async fn generate_event_config(
        &self,
        simulation_requirement: &str,
        document_text: &str,
    ) -> Result<EventConfig, LlmError> {
        // Truncate document text for the prompt
        let doc_preview: String = document_text.chars().take(5000).collect();

        let prompt = format!(
            "Based on the simulation requirement and document, generate initial events and hot topics.\n\
             \n\
             Simulation requirement: {}\n\
             Document preview: {}\n\
             \n\
             Output JSON:\n\
             {{\n\
               \"initial_posts\": [\n\
                 {{\"author_type\": \"entity_type\", \"content\": \"initial post content\"}}\n\
               ],\n\
               \"hot_topics\": [\"topic1\", \"topic2\"],\n\
               \"narrative_direction\": \"brief description of how events should unfold\"\n\
             }}",
            simulation_requirement, doc_preview,
        );

        let messages = vec![ChatMessage::user(prompt)];
        let result = self.llm.chat_json(&messages, 0.5, Some(2048)).await?;

        Ok(EventConfig {
            initial_posts: result.get("initial_posts")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
            scheduled_events: Vec::new(),
            hot_topics: result.get("hot_topics")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default(),
            narrative_direction: result.get("narrative_direction")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }

    /// Generate agent activity configurations.
    async fn generate_agent_configs(
        &self,
        entities: &[EntityNode],
        simulation_requirement: &str,
    ) -> Result<Vec<AgentActivityConfig>, LlmError> {
        let mut configs = Vec::new();

        // Build a summary of all entities for context
        let entity_summary: Vec<String> = entities
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let etype = e.get_entity_type().unwrap_or("Unknown");
                format!("{}: {} ({})", i, e.name, etype)
            })
            .collect();

        let prompt = format!(
            "For the following simulation agents, generate activity level configurations.\n\
             \n\
             Simulation requirement: {}\n\
             Agents:\n{}\n\
             \n\
             For each agent, output a JSON array where each element has:\n\
             {{\n\
               \"agent_id\": <index>,\n\
               \"activity_level\": <0.0-1.0>,\n\
               \"posts_per_hour\": <float>,\n\
               \"sentiment_bias\": <-1.0 to 1.0>,\n\
               \"stance\": \"supportive|opposing|neutral|observer\",\n\
               \"influence_weight\": <0.1-3.0>\n\
             }}\n\
             \n\
             Output only the JSON array.",
            simulation_requirement,
            entity_summary.join("\n"),
        );

        let messages = vec![ChatMessage::user(prompt)];
        let result = self.llm.chat_json(&messages, 0.3, Some(4096)).await;

        // Parse LLM result or fall back to defaults
        let agent_data: Vec<Value> = match result {
            Ok(val) => {
                if let Some(arr) = val.as_array() {
                    arr.clone()
                } else if let Some(arr) = val.get("agents").and_then(|v| v.as_array()) {
                    arr.clone()
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new(),
        };

        for (i, entity) in entities.iter().enumerate() {
            let etype = entity.get_entity_type().unwrap_or("Person").to_string();

            let llm_cfg = agent_data.iter().find(|v| {
                v.get("agent_id").and_then(|id| id.as_u64()) == Some(i as u64)
            });

            let activity_level = llm_cfg
                .and_then(|v| v.get("activity_level").and_then(|a| a.as_f64()))
                .unwrap_or(0.5);
            let posts_per_hour = llm_cfg
                .and_then(|v| v.get("posts_per_hour").and_then(|a| a.as_f64()))
                .unwrap_or(1.0);
            let sentiment_bias = llm_cfg
                .and_then(|v| v.get("sentiment_bias").and_then(|a| a.as_f64()))
                .unwrap_or(0.0);
            let stance = llm_cfg
                .and_then(|v| v.get("stance").and_then(|a| a.as_str()))
                .unwrap_or("neutral")
                .to_string();
            let influence_weight = llm_cfg
                .and_then(|v| v.get("influence_weight").and_then(|a| a.as_f64()))
                .unwrap_or(1.0);

            configs.push(AgentActivityConfig {
                agent_id: i as u64,
                entity_uuid: entity.uuid.clone(),
                entity_name: entity.name.clone(),
                entity_type: etype,
                activity_level,
                posts_per_hour,
                comments_per_hour: posts_per_hour * 2.0,
                active_hours: (8..23).collect(),
                response_delay_min: 5,
                response_delay_max: 60,
                sentiment_bias,
                stance,
                influence_weight,
            });
        }

        Ok(configs)
    }
}
