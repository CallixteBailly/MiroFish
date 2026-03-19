//! Route registration — assembles all API sub-routers under `/api`.

pub mod graph;
pub mod report;
pub mod simulation;

use axum::Router;

use crate::state::AppState;

/// Build the combined API router.
///
/// All routes are nested under `/api`:
/// - `/api/graph/*`
/// - `/api/simulation/*`
/// - `/api/report/*`
pub fn api_router() -> Router<AppState> {
    Router::new()
        .nest("/api/graph", graph::router())
        .nest("/api/simulation", simulation::router())
        .nest("/api/report", report::router())
}
