//! Agent profile generation via LLM.
//!
//! Converts entity nodes from the knowledge graph into detailed agent profiles
//! suitable for the simulation platforms.

use serde_json::Value;

use crate::graph::entity_reader::EntityNode;
use crate::llm::client::{ChatMessage, LlmClient, LlmError};

use super::agent::AgentProfile;

/// Profile generator — creates simulation agent profiles from graph entities.
pub struct ProfileGenerator {
    llm: LlmClient,
}

impl ProfileGenerator {
    /// Create a new generator.
    pub fn new(llm: LlmClient) -> Self {
        Self { llm }
    }

    /// Generate profiles for a list of entities.
    ///
    /// If `use_llm` is true, each profile is enriched with LLM-generated persona text.
    /// The `progress_callback` is called with `(current, total, message)`.
    pub async fn generate_profiles(
        &self,
        entities: &[EntityNode],
        use_llm: bool,
        mut progress_callback: Option<Box<dyn FnMut(usize, usize, &str) + Send>>,
    ) -> Result<Vec<AgentProfile>, LlmError> {
        let total = entities.len();
        let mut profiles = Vec::with_capacity(total);

        for (i, entity) in entities.iter().enumerate() {
            if let Some(ref mut cb) = progress_callback {
                cb(i, total, &format!("Generating profile for {}", entity.name));
            }

            let profile = if use_llm {
                self.generate_llm_profile(entity, i as u64).await?
            } else {
                Self::generate_basic_profile(entity, i as u64)
            };

            profiles.push(profile);
        }

        if let Some(ref mut cb) = progress_callback {
            cb(total, total, &format!("Complete, {} profiles generated", total));
        }

        Ok(profiles)
    }

    /// Generate a basic profile without LLM enrichment.
    pub fn generate_basic_profile(entity: &EntityNode, user_id: u64) -> AgentProfile {
        let entity_type = entity.get_entity_type().unwrap_or("Person");
        let username = entity
            .name
            .to_lowercase()
            .replace(' ', "_")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .take(20)
            .collect::<String>();

        let bio = if entity.summary.is_empty() {
            format!("A {} participating in the simulation.", entity_type)
        } else {
            entity.summary.chars().take(200).collect()
        };

        let persona = format!(
            "This is {} (type: {}). {}",
            entity.name, entity_type, bio
        );

        AgentProfile {
            user_id,
            username,
            name: entity.name.clone(),
            bio,
            persona,
            karma: 1000,
            friend_count: 100,
            follower_count: 150,
            statuses_count: 500,
            age: None,
            gender: None,
            mbti: None,
            country: None,
            profession: None,
            interested_topics: Vec::new(),
            source_entity_uuid: Some(entity.uuid.clone()),
            source_entity_type: Some(entity_type.to_string()),
        }
    }

    /// Generate an LLM-enriched profile with detailed persona.
    async fn generate_llm_profile(
        &self,
        entity: &EntityNode,
        user_id: u64,
    ) -> Result<AgentProfile, LlmError> {
        let entity_type = entity.get_entity_type().unwrap_or("Person");

        // Collect related edge descriptions
        let relations: Vec<String> = entity
            .related_edges
            .iter()
            .filter_map(|e| {
                let fact = e.get("fact").and_then(|v| v.as_str()).unwrap_or("");
                let source = e.get("source").and_then(|v| v.as_str()).unwrap_or("");
                let target = e.get("target").and_then(|v| v.as_str()).unwrap_or("");
                if fact.is_empty() {
                    None
                } else {
                    Some(format!("{} -> {}: {}", source, target, fact))
                }
            })
            .take(10)
            .collect();

        let relations_text = if relations.is_empty() {
            "No known relationships.".to_string()
        } else {
            relations.join("\n")
        };

        let prompt = format!(
            "Generate a detailed social media persona for this entity.\n\
             \n\
             Entity name: {name}\n\
             Entity type: {etype}\n\
             Summary: {summary}\n\
             Known relationships:\n{relations}\n\
             \n\
             Generate a JSON object with:\n\
             {{\n\
               \"bio\": \"a short bio (max 160 chars)\",\n\
               \"persona\": \"detailed persona paragraph describing personality, background, interests, social media behavior style (200-500 chars)\",\n\
               \"age\": <number or null>,\n\
               \"gender\": \"<string or null>\",\n\
               \"mbti\": \"<4-letter code or null>\",\n\
               \"country\": \"<string or null>\",\n\
               \"profession\": \"<string or null>\",\n\
               \"interested_topics\": [\"topic1\", \"topic2\"]\n\
             }}",
            name = entity.name,
            etype = entity_type,
            summary = entity.summary,
            relations = relations_text,
        );

        let messages = vec![ChatMessage::user(prompt)];
        let result = self.llm.chat_json(&messages, 0.7, Some(2048)).await?;

        let bio = result
            .get("bio")
            .and_then(|v| v.as_str())
            .unwrap_or(&entity.summary)
            .to_string();

        let persona = result
            .get("persona")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let age = result.get("age").and_then(|v| v.as_u64()).map(|a| a as u32);
        let gender = result.get("gender").and_then(|v| v.as_str()).map(|s| s.to_string());
        let mbti = result.get("mbti").and_then(|v| v.as_str()).map(|s| s.to_string());
        let country = result.get("country").and_then(|v| v.as_str()).map(|s| s.to_string());
        let profession = result.get("profession").and_then(|v| v.as_str()).map(|s| s.to_string());

        let topics: Vec<String> = result
            .get("interested_topics")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let username = entity
            .name
            .to_lowercase()
            .replace(' ', "_")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .take(20)
            .collect::<String>();

        Ok(AgentProfile {
            user_id,
            username,
            name: entity.name.clone(),
            bio,
            persona,
            karma: 1000,
            friend_count: 100,
            follower_count: 150,
            statuses_count: 500,
            age,
            gender,
            mbti,
            country,
            profession,
            interested_topics: topics,
            source_entity_uuid: Some(entity.uuid.clone()),
            source_entity_type: Some(entity_type.to_string()),
        })
    }

    /// Save profiles to a JSON file (Reddit format).
    pub fn save_profiles_json(profiles: &[AgentProfile], path: &str) -> anyhow::Result<()> {
        let data: Vec<Value> = profiles
            .iter()
            .map(|p| {
                let mut obj = serde_json::to_value(p).unwrap_or_default();
                // Rename user_name -> username for OASIS compatibility
                if let Some(map) = obj.as_object_mut() {
                    if let Some(un) = map.remove("username") {
                        map.insert("username".to_string(), un);
                    }
                }
                obj
            })
            .collect();

        let json = serde_json::to_string_pretty(&data)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}
