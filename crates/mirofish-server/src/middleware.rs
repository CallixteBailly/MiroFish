//! Middleware setup: CORS and request/response tracing.

use axum::Router;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::Level;

/// Apply CORS and trace middleware to the router.
pub fn apply_middleware(router: Router) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any);

    let trace = TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<_>| {
            tracing::span!(
                Level::INFO,
                "http_request",
                method = %request.method(),
                uri = %request.uri(),
            )
        })
        .on_response(
            |response: &axum::http::Response<_>,
             latency: std::time::Duration,
             _span: &tracing::Span| {
                tracing::info!(
                    status = %response.status(),
                    latency_ms = latency.as_millis(),
                    "response"
                );
            },
        );

    router.layer(cors).layer(trace)
}
