mod claude_runner;

use axum::{
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info};

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

const AVAILABLE_MODELS: &[&str] = &[
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-haiku-4-5-20251001",
    "sonnet",
    "opus",
    "haiku",
];

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Value>,
    #[serde(default)]
    #[allow(dead_code)]
    temperature: Option<f64>,
    #[serde(default)]
    #[allow(dead_code)]
    max_tokens: Option<u64>,
    #[serde(default)]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Deserialize)]
struct ResponseFormat {
    #[serde(default, rename = "type")]
    format_type: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: &'static str,
    created: i64,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Debug, Serialize)]
struct Choice {
    index: u32,
    message: Message,
    finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

#[derive(Debug, Serialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Serialize)]
struct ModelObject {
    id: String,
    object: &'static str,
    created: i64,
    owned_by: &'static str,
}

#[derive(Debug, Serialize)]
struct ModelList {
    object: &'static str,
    data: Vec<ModelObject>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn welcome() -> impl IntoResponse {
    Json(json!({
        "message": "MiroFish Claude Proxy - OpenAI-compatible proxy for the claude CLI",
        "endpoints": {
            "chat_completions": "POST /v1/chat/completions",
            "models": "GET /v1/models",
            "health": "GET /health"
        }
    }))
}

async fn list_models() -> impl IntoResponse {
    let now = chrono::Utc::now().timestamp();
    let data: Vec<ModelObject> = AVAILABLE_MODELS
        .iter()
        .map(|id| ModelObject {
            id: id.to_string(),
            object: "model",
            created: now,
            owned_by: "anthropic",
        })
        .collect();

    Json(ModelList {
        object: "list",
        data,
    })
}

async fn get_model(Path(model_id): Path<String>) -> impl IntoResponse {
    if AVAILABLE_MODELS.contains(&model_id.as_str()) {
        let now = chrono::Utc::now().timestamp();
        (
            StatusCode::OK,
            Json(json!(ModelObject {
                id: model_id,
                object: "model",
                created: now,
                owned_by: "anthropic",
            })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "message": format!("Model '{}' not found", model_id),
                    "type": "invalid_request_error",
                    "code": "model_not_found"
                }
            })),
        )
    }
}

async fn chat_completions(
    Json(payload): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let model = payload.model.clone();
    info!(model = %model, messages = payload.messages.len(), "Chat completion request");

    let json_mode = payload
        .response_format
        .as_ref()
        .and_then(|rf| rf.format_type.as_deref())
        .map(|t| t == "json_object")
        .unwrap_or(false);

    match claude_runner::chat_completion(&payload.messages, &model, json_mode).await {
        Ok(content) => {
            let response = ChatCompletionResponse {
                id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                object: "chat.completion",
                created: chrono::Utc::now().timestamp(),
                model: model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: Message {
                        role: "assistant",
                        content,
                    },
                    finish_reason: "stop",
                }],
                usage: Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            };

            info!(model = %model, "Chat completion succeeded");
            (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
        }
        Err(err) => {
            error!(model = %model, error = %err, "Chat completion failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "message": err,
                        "type": "server_error",
                        "code": "claude_cli_error"
                    }
                })),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Load environment.
    let port: u16 = std::env::var("CLAUDE_PROXY_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8082);

    let log_level = std::env::var("CLAUDE_PROXY_LOG_LEVEL").unwrap_or_else(|_| "info".to_string());

    // Setup tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(&log_level)
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!(port = port, "Starting MiroFish Claude Proxy");

    // Build router.
    let app = Router::new()
        .route("/", get(welcome))
        .route("/health", get(health))
        .route("/v1/models", get(list_models))
        .route("/v1/models/{model_id}", get(get_model))
        .route("/v1/chat/completions", post(chat_completions))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!(%addr, "Listening");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app)
        .await
        .expect("Server error");
}
