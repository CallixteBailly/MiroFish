//! API error types and their conversion into HTTP responses.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Application-level errors that map to HTTP status codes.
#[derive(Debug)]
pub enum AppError {
    /// 404 — requested resource was not found.
    NotFound(String),
    /// 400 — the request was malformed or missing required fields.
    BadRequest(String),
    /// 500 — an unexpected internal error occurred.
    Internal(String),
    /// 503 — a required external service is unavailable.
    ServiceUnavailable(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::NotFound(msg) => write!(f, "Not Found: {msg}"),
            AppError::BadRequest(msg) => write!(f, "Bad Request: {msg}"),
            AppError::Internal(msg) => write!(f, "Internal Error: {msg}"),
            AppError::ServiceUnavailable(msg) => write!(f, "Service Unavailable: {msg}"),
        }
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            AppError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
        };

        let body = json!({
            "success": false,
            "error": message,
        });

        (status, Json(body)).into_response()
    }
}

/// Allow `anyhow::Error` to be converted into `AppError::Internal`.
impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Internal(format!("{err:#}"))
    }
}

/// Allow `serde_json::Error` to be converted into `AppError::BadRequest`.
impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::BadRequest(format!("JSON error: {err}"))
    }
}
