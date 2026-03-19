//! Simulated agent with LLM-powered decision-making.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::client::{ChatMessage, LlmClient, LlmError};

/// Agent profile — personality and demographic information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub user_id: u64,
    pub username: String,
    pub name: String,
    pub bio: String,
    pub persona: String,
    #[serde(default = "default_karma")]
    pub karma: i64,
    #[serde(default = "default_friend_count")]
    pub friend_count: u64,
    #[serde(default = "default_follower_count")]
    pub follower_count: u64,
    #[serde(default = "default_statuses_count")]
    pub statuses_count: u64,
    pub age: Option<u32>,
    pub gender: Option<String>,
    pub mbti: Option<String>,
    pub country: Option<String>,
    pub profession: Option<String>,
    #[serde(default)]
    pub interested_topics: Vec<String>,
    pub source_entity_uuid: Option<String>,
    pub source_entity_type: Option<String>,
}

fn default_karma() -> i64 { 1000 }
fn default_friend_count() -> u64 { 100 }
fn default_follower_count() -> u64 { 150 }
fn default_statuses_count() -> u64 { 500 }

/// A simulated agent that can observe the platform and decide on actions.
pub struct SimulatedAgent {
    pub profile: AgentProfile,
}

/// The decision made by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDecision {
    pub action_type: String,
    #[serde(default)]
    pub action_args: Value,
    pub reasoning: Option<String>,
}

impl SimulatedAgent {
    /// Create a new agent from a profile.
    pub fn new(profile: AgentProfile) -> Self {
        Self { profile }
    }

    /// Have the agent observe the current platform state and decide on an action.
    ///
    /// The LLM is given the agent's profile, current feed, and available actions,
    /// then asked to choose and return a JSON decision.
    pub async fn decide_action(
        &self,
        llm: &LlmClient,
        platform_name: &str,
        feed_text: &str,
        available_actions: &[&str],
        simulated_time: &str,
        round_num: u64,
    ) -> Result<AgentDecision, LlmError> {
        let actions_str = available_actions.join(", ");

        let system_prompt = format!(
            "You are simulating a social media user on {platform}. \
             You must act according to your persona and make realistic decisions.\n\
             \n\
             Your Profile:\n\
             - Name: {name}\n\
             - Bio: {bio}\n\
             - Persona: {persona}\n\
             \n\
             Current simulated time: {time}\n\
             Simulation round: {round}\n\
             \n\
             Available actions: [{actions}]\n\
             \n\
             You MUST respond with a JSON object:\n\
             {{\n\
               \"action_type\": \"ONE_OF_THE_AVAILABLE_ACTIONS\",\n\
               \"action_args\": {{}},\n\
               \"reasoning\": \"brief explanation\"\n\
             }}\n\
             \n\
             Action argument formats:\n\
             - CREATE_POST: {{\"content\": \"your post text\"}}\n\
             - LIKE_POST: {{\"post_id\": <number>}}\n\
             - REPOST: {{\"post_id\": <number>}}\n\
             - QUOTE_POST: {{\"post_id\": <number>, \"content\": \"your quote\"}}\n\
             - FOLLOW: {{\"target_id\": <number>}}\n\
             - CREATE_COMMENT: {{\"post_id\": <number>, \"content\": \"your comment\"}}\n\
             - DISLIKE_POST: {{\"post_id\": <number>}}\n\
             - LIKE_COMMENT: {{\"comment_id\": <number>}}\n\
             - DISLIKE_COMMENT: {{\"comment_id\": <number>}}\n\
             - SEARCH_POSTS: {{\"query\": \"search terms\"}}\n\
             - SEARCH_USER: {{\"query\": \"username\"}}\n\
             - MUTE: {{\"target_id\": <number>}}\n\
             - DO_NOTHING: {{}}\n\
             - TREND: {{}}\n\
             - REFRESH: {{}}",
            platform = platform_name,
            name = self.profile.name,
            bio = self.profile.bio,
            persona = self.profile.persona,
            time = simulated_time,
            round = round_num,
            actions = actions_str,
        );

        let user_prompt = format!(
            "Here is your current timeline/feed:\n\n{}\n\n\
             Based on your persona and the current content, what action do you want to take? \
             Respond with JSON only.",
            feed_text,
        );

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ];

        let result = llm.chat_json(&messages, 0.7, Some(1024)).await?;

        let action_type = result
            .get("action_type")
            .and_then(|v| v.as_str())
            .unwrap_or("DO_NOTHING")
            .to_string();

        let action_args = result
            .get("action_args")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        let reasoning = result
            .get("reasoning")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(AgentDecision {
            action_type,
            action_args,
            reasoning,
        })
    }

    /// Interview the agent — ask a question and get a response in character.
    pub async fn interview(
        &self,
        llm: &LlmClient,
        prompt: &str,
        platform_name: &str,
    ) -> Result<String, LlmError> {
        let system_prompt = format!(
            "You are role-playing as a social media user on {}.\n\
             Your profile:\n\
             - Name: {}\n\
             - Bio: {}\n\
             - Persona: {}\n\
             \n\
             Answer the following question in character. Be authentic and consistent with your persona.",
            platform_name,
            self.profile.name,
            self.profile.bio,
            self.profile.persona,
        );

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(prompt),
        ];

        llm.chat(&messages, 0.7, Some(1024), None).await
    }
}
