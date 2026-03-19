//! OpenAI-compatible LLM client using reqwest.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use regex::Regex;
use std::sync::OnceLock;

use crate::config::Config;

/// Errors produced by the LLM client.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("LLM_API_KEY is not configured")]
    MissingApiKey,
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("LLM API error (status {status}): {body}")]
    Api { status: u16, body: String },
    #[error("No response content from LLM")]
    EmptyResponse,
    #[error("LLM returned invalid JSON: {0}")]
    InvalidJson(String),
}

/// A single message in the chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into() }
    }
}

/// Request body sent to the OpenAI-compatible API.
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
}

/// A single choice in the API response.
#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

/// The top-level API response.
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

/// OpenAI-compatible LLM client.
#[derive(Debug, Clone)]
pub struct LlmClient {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    http: Client,
}

/// Compiled regexes, initialized once.
fn think_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<think>.*?</think>").expect("invalid regex"))
}

fn code_block_start_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^```(?:json)?\s*\n?").expect("invalid regex"))
}

fn code_block_end_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\n?```\s*$").expect("invalid regex"))
}

impl LlmClient {
    /// Create a new client with explicit parameters.
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>, model: impl Into<String>) -> Result<Self, LlmError> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(LlmError::MissingApiKey);
        }
        Ok(Self {
            api_key,
            base_url: base_url.into(),
            model: model.into(),
            http: Client::new(),
        })
    }

    /// Create a client from the global config.
    pub fn from_config(config: &Config) -> Result<Self, LlmError> {
        Self::new(&config.llm_api_key, &config.llm_base_url, &config.llm_model_name)
    }

    /// Create a client from the global singleton config.
    pub fn from_global_config() -> Result<Self, LlmError> {
        Self::from_config(Config::global())
    }

    /// Send a chat completion request and return the text content.
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        temperature: f64,
        max_tokens: Option<u32>,
        response_format: Option<Value>,
    ) -> Result<String, LlmError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let body = ChatRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            temperature,
            max_tokens,
            response_format,
        };

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status: status.as_u16(),
                body: body_text,
            });
        }

        let chat_resp: ChatResponse = resp.json().await?;
        let content = chat_resp
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or(LlmError::EmptyResponse)?;

        // Strip <think>...</think> tags (some models include reasoning)
        let content = think_re().replace_all(&content, "").trim().to_string();
        Ok(content)
    }

    /// Send a chat request with JSON response format and parse the result.
    pub async fn chat_json(
        &self,
        messages: &[ChatMessage],
        temperature: f64,
        max_tokens: Option<u32>,
    ) -> Result<Value, LlmError> {
        let response_format = serde_json::json!({"type": "json_object"});

        let response = self
            .chat(messages, temperature, max_tokens, Some(response_format))
            .await?;

        // Strip markdown code block markers
        let cleaned = code_block_start_re().replace(&response, "");
        let cleaned = code_block_end_re().replace(&cleaned, "");
        let cleaned = cleaned.trim();

        serde_json::from_str(cleaned)
            .map_err(|_| LlmError::InvalidJson(cleaned.chars().take(500).collect()))
    }
}
