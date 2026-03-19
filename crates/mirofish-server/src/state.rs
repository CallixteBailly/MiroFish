//! Shared application state, passed to all handlers via Axum's `State` extractor.

use std::sync::Arc;
use mirofish_core::config::Config;

/// Application state shared across all request handlers.
#[derive(Clone)]
pub struct AppState {
    /// Global configuration (loaded once at startup).
    pub config: Arc<Config>,
}
