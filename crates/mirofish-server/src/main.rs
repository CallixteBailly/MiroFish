//! MiroFish Server — Axum web server exposing the REST API.

mod config;
mod error;
mod middleware;
mod routes;
pub mod state;

use std::sync::Arc;

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::{Config, server_host, server_port};
use crate::state::AppState;

/// Application entry point.
#[tokio::main]
async fn main() {
    // Load .env from the project root (two levels up from crates/mirofish-server/).
    dotenvy::dotenv().ok();

    // Initialise tracing (console output with env-filter).
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug")),
        )
        .init();

    // Load and validate configuration.
    let config = Config::from_env();
    let errors = config.validate();
    if !errors.is_empty() {
        for e in &errors {
            warn!("Config warning: {}", e);
        }
    }

    // Print startup banner.
    info!("========================================");
    info!("  MiroFish Backend (Rust)");
    info!("========================================");
    info!("LLM model : {}", config.llm_model_name);
    info!("Graph mode: {}", config.graph_mode());
    if config.lite_mode {
        info!("LITE MODE enabled — Zep Cloud features disabled");
    }
    if config.is_zep_available() {
        info!("Zep Cloud : connected");
    }

    let host = server_host();
    let port = server_port();

    // Build shared application state.
    let app_state = AppState {
        config: Arc::new(config),
    };

    // Build the router.
    let app = Router::new()
        .route("/health", get(health_check))
        .merge(routes::api_router())
        .with_state(app_state.clone());

    // Apply middleware (CORS + trace).
    let app = middleware::apply_middleware(app);

    let addr = format!("{host}:{port}");
    info!("Listening on http://{}", addr);

    let listener = TcpListener::bind(&addr)
        .await
        .expect("failed to bind address");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

/// Health check handler.
async fn health_check(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "MiroFish Backend",
        "lite_mode": state.config.lite_mode,
        "zep_available": state.config.is_zep_available(),
    }))
}

/// Wait for SIGINT or SIGTERM for graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received, stopping server...");
}
