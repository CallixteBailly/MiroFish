//! Server-specific configuration that extends `mirofish_core::Config`.

pub use mirofish_core::config::{Config, GraphMode};

/// Server bind address (defaults to 0.0.0.0).
pub const DEFAULT_HOST: &str = "0.0.0.0";

/// Server port (defaults to 5001).
pub const DEFAULT_PORT: u16 = 5001;

/// Return the host string from env or the default.
pub fn server_host() -> String {
    std::env::var("SERVER_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string())
}

/// Return the port from env or the default.
pub fn server_port() -> u16 {
    std::env::var("SERVER_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}
