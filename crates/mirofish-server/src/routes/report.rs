//! `/api/report/*` route handlers.
//!
//! Covers report generation, retrieval, progress tracking, agent/console logs,
//! chat with the Report Agent, and debug tool endpoints.

use axum::{
    Router,
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the `/api/report` sub-router.
pub fn router() -> Router<AppState> {
    Router::new()
        // Generation
        .route("/generate", post(generate_report))
        .route("/generate/status", post(get_generate_status))
        // Retrieval
        .route("/list", get(list_reports))
        .route("/by-simulation/{simulation_id}", get(get_report_by_simulation))
        .route("/check/{simulation_id}", get(check_report_status))
        // Chat
        .route("/chat", post(chat_with_report_agent))
        // Debug tools
        .route("/tools/search", post(search_graph_tool))
        .route("/tools/statistics", post(get_graph_statistics_tool))
        // Parameterised endpoints (must come after fixed paths)
        .route("/{report_id}", get(get_report).delete(delete_report))
        .route("/{report_id}/progress", get(get_report_progress))
        .route("/{report_id}/sections", get(get_report_sections))
        .route(
            "/{report_id}/section/{section_index}",
            get(get_single_section),
        )
        .route("/{report_id}/agent-log", get(get_agent_log))
        .route("/{report_id}/agent-log/stream", get(stream_agent_log))
        .route("/{report_id}/console-log", get(get_console_log))
        .route("/{report_id}/console-log/stream", get(stream_console_log))
        .route("/{report_id}/download", get(download_report))
}

// ---------------------------------------------------------------------------
// Query / body types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct GenerateReportRequest {
    pub simulation_id: String,
    pub force_regenerate: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct GenerateStatusRequest {
    pub task_id: Option<String>,
    pub simulation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListReportsQuery {
    pub simulation_id: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct LogQuery {
    pub from_line: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub simulation_id: String,
    pub message: String,
    pub chat_history: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
pub struct SearchGraphRequest {
    pub graph_id: String,
    pub query: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct GraphStatsRequest {
    pub graph_id: String,
}

// ---------------------------------------------------------------------------
// Report generation
// ---------------------------------------------------------------------------

/// `POST /api/report/generate` — start report generation (async task).
async fn generate_report(
    State(state): State<AppState>,
    Json(body): Json<GenerateReportRequest>,
) -> Result<Json<Value>, AppError> {
    let simulation_id = &body.simulation_id;
    let force_regenerate = body.force_regenerate.unwrap_or(false);

    info!(
        "Generating report: simulation_id={}, force={}",
        simulation_id, force_regenerate
    );

    // TODO: check if report already exists via ReportManager

    let report_id = format!("report_{}", &Uuid::new_v4().to_string()[..12]);
    let task_id = format!("task_{}", &Uuid::new_v4().to_string()[..12]);

    // Spawn background generation task.
    let rid = report_id.clone();
    let sid = simulation_id.clone();
    let tid = task_id.clone();

    tokio::spawn(async move {
        info!("[{}] Background report generation started for simulation {}", tid, sid);
        // TODO: integrate with ReportAgent.generate_report
        info!("[{}] Report generation placeholder complete: {}", tid, rid);
    });

    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "report_id": report_id,
            "task_id": task_id,
            "status": "generating",
            "message": "Report generation task started, check progress via /api/report/generate/status",
            "already_generated": false,
        }
    })))
}

/// `POST /api/report/generate/status` — query report generation progress.
async fn get_generate_status(
    State(_state): State<AppState>,
    Json(body): Json<GenerateStatusRequest>,
) -> Result<Json<Value>, AppError> {
    // TODO: check ReportManager for completed report, then TaskManager for in-progress task

    if let Some(ref sim_id) = body.simulation_id {
        // TODO: check ReportManager.get_report_by_simulation
        info!("Checking report status for simulation: {}", sim_id);
    }

    if body.task_id.is_none() && body.simulation_id.is_none() {
        return Err(AppError::BadRequest(
            "Please provide task_id or simulation_id".into(),
        ));
    }

    if let Some(ref task_id) = body.task_id {
        // TODO: integrate with TaskManager
        return Err(AppError::NotFound(format!("Task not found: {task_id}")));
    }

    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": body.simulation_id,
            "status": "not_started",
            "progress": 0,
            "message": "No report generation in progress",
        }
    })))
}

// ---------------------------------------------------------------------------
// Report retrieval
// ---------------------------------------------------------------------------

/// `GET /api/report/:report_id` — get report details.
async fn get_report(
    State(_state): State<AppState>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Getting report: {}", report_id);

    // TODO: integrate with ReportManager.get_report
    Err(AppError::NotFound(format!("Report not found: {report_id}")))
}

/// `GET /api/report/by-simulation/:simulation_id` — get report by simulation ID.
async fn get_report_by_simulation(
    State(_state): State<AppState>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Getting report for simulation: {}", simulation_id);

    // TODO: integrate with ReportManager.get_report_by_simulation
    Err(AppError::NotFound(format!(
        "No report found for this simulation: {simulation_id}"
    )))
}

/// `GET /api/report/list` — list all reports.
async fn list_reports(
    State(_state): State<AppState>,
    Query(params): Query<ListReportsQuery>,
) -> Result<Json<Value>, AppError> {
    let _limit = params.limit.unwrap_or(50);
    info!("Listing reports, simulation_id={:?}", params.simulation_id);

    // TODO: integrate with ReportManager.list_reports
    Ok(Json(json!({
        "success": true,
        "data": [],
        "count": 0,
    })))
}

/// `DELETE /api/report/:report_id` — delete a report.
async fn delete_report(
    State(_state): State<AppState>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Deleting report: {}", report_id);

    // TODO: integrate with ReportManager.delete_report
    Err(AppError::NotFound(format!("Report not found: {report_id}")))
}

/// `GET /api/report/:report_id/download` — download report as Markdown.
async fn download_report(
    State(_state): State<AppState>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Downloading report: {}", report_id);

    // TODO: integrate with ReportManager, return file response
    // For now, return an error. A real implementation would return
    // axum::body::Body with proper content-disposition headers.
    Err(AppError::NotFound(format!("Report not found: {report_id}")))
}

/// `GET /api/report/check/:simulation_id` — check report availability.
async fn check_report_status(
    State(_state): State<AppState>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Checking report status for simulation: {}", simulation_id);

    // TODO: integrate with ReportManager.get_report_by_simulation
    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "has_report": false,
            "report_status": null,
            "report_id": null,
            "interview_unlocked": false,
        }
    })))
}

// ---------------------------------------------------------------------------
// Report progress and sections
// ---------------------------------------------------------------------------

/// `GET /api/report/:report_id/progress` — real-time report generation progress.
async fn get_report_progress(
    State(_state): State<AppState>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Getting report progress: {}", report_id);

    // TODO: integrate with ReportManager.get_progress
    Err(AppError::NotFound(format!(
        "Report not found or progress information unavailable: {report_id}"
    )))
}

/// `GET /api/report/:report_id/sections` — list generated sections.
async fn get_report_sections(
    State(_state): State<AppState>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Getting report sections: {}", report_id);

    // TODO: integrate with ReportManager.get_generated_sections
    Ok(Json(json!({
        "success": true,
        "data": {
            "report_id": report_id,
            "sections": [],
            "total_sections": 0,
            "is_complete": false,
        }
    })))
}

/// `GET /api/report/:report_id/section/:section_index` — get a single section.
async fn get_single_section(
    State(_state): State<AppState>,
    Path((report_id, section_index)): Path<(String, u32)>,
) -> Result<Json<Value>, AppError> {
    info!(
        "Getting section: report_id={}, section_index={}",
        report_id, section_index
    );

    // TODO: integrate with ReportManager
    Err(AppError::NotFound(format!(
        "Section not found: section_{:02}.md",
        section_index
    )))
}

// ---------------------------------------------------------------------------
// Agent / console logs
// ---------------------------------------------------------------------------

/// `GET /api/report/:report_id/agent-log` — incremental agent action log.
async fn get_agent_log(
    State(_state): State<AppState>,
    Path(report_id): Path<String>,
    Query(params): Query<LogQuery>,
) -> Result<Json<Value>, AppError> {
    let from_line = params.from_line.unwrap_or(0);
    info!(
        "Getting agent log: report_id={}, from_line={}",
        report_id, from_line
    );

    // TODO: integrate with ReportManager.get_agent_log
    Ok(Json(json!({
        "success": true,
        "data": {
            "logs": [],
            "total_lines": 0,
            "from_line": from_line,
            "has_more": false,
        }
    })))
}

/// `GET /api/report/:report_id/agent-log/stream` — complete agent log.
async fn stream_agent_log(
    State(_state): State<AppState>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Streaming agent log: {}", report_id);

    // TODO: integrate with ReportManager.get_agent_log_stream
    Ok(Json(json!({
        "success": true,
        "data": {
            "logs": [],
            "count": 0,
        }
    })))
}

/// `GET /api/report/:report_id/console-log` — incremental console log.
async fn get_console_log(
    State(_state): State<AppState>,
    Path(report_id): Path<String>,
    Query(params): Query<LogQuery>,
) -> Result<Json<Value>, AppError> {
    let from_line = params.from_line.unwrap_or(0);
    info!(
        "Getting console log: report_id={}, from_line={}",
        report_id, from_line
    );

    // TODO: integrate with ReportManager.get_console_log
    Ok(Json(json!({
        "success": true,
        "data": {
            "logs": [],
            "total_lines": 0,
            "from_line": from_line,
            "has_more": false,
        }
    })))
}

/// `GET /api/report/:report_id/console-log/stream` — complete console log.
async fn stream_console_log(
    State(_state): State<AppState>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Streaming console log: {}", report_id);

    // TODO: integrate with ReportManager.get_console_log_stream
    Ok(Json(json!({
        "success": true,
        "data": {
            "logs": [],
            "count": 0,
        }
    })))
}

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

/// `POST /api/report/chat` — chat with the Report Agent.
async fn chat_with_report_agent(
    State(_state): State<AppState>,
    Json(body): Json<ChatRequest>,
) -> Result<Json<Value>, AppError> {
    let simulation_id = &body.simulation_id;
    let message = &body.message;

    if message.is_empty() {
        return Err(AppError::BadRequest("Please provide message".into()));
    }

    info!(
        "Chat with report agent: simulation_id={}, message_len={}",
        simulation_id,
        message.len()
    );

    // TODO: integrate with ReportAgent.chat
    Ok(Json(json!({
        "success": true,
        "data": {
            "response": "Report Agent chat is not yet implemented in the Rust backend.",
            "tool_calls": [],
            "sources": [],
        }
    })))
}

// ---------------------------------------------------------------------------
// Debug tool endpoints
// ---------------------------------------------------------------------------

/// `POST /api/report/tools/search` — graph search tool (debugging).
async fn search_graph_tool(
    State(state): State<AppState>,
    Json(body): Json<SearchGraphRequest>,
) -> Result<Json<Value>, AppError> {
    if body.graph_id.is_empty() || body.query.is_empty() {
        return Err(AppError::BadRequest(
            "Please provide graph_id and query".into(),
        ));
    }

    if !state.config.is_zep_available() {
        return Err(AppError::ServiceUnavailable(
            "ZEP_API_KEY is not configured.".into(),
        ));
    }

    info!(
        "Graph search: graph_id={}, query={}",
        body.graph_id, body.query
    );

    // TODO: integrate with ZepToolsService.search_graph
    Ok(Json(json!({
        "success": true,
        "data": {
            "graph_id": body.graph_id,
            "query": body.query,
            "results": [],
        }
    })))
}

/// `POST /api/report/tools/statistics` — graph statistics tool (debugging).
async fn get_graph_statistics_tool(
    State(state): State<AppState>,
    Json(body): Json<GraphStatsRequest>,
) -> Result<Json<Value>, AppError> {
    if body.graph_id.is_empty() {
        return Err(AppError::BadRequest("Please provide graph_id".into()));
    }

    if !state.config.is_zep_available() {
        return Err(AppError::ServiceUnavailable(
            "ZEP_API_KEY is not configured.".into(),
        ));
    }

    info!("Graph statistics: graph_id={}", body.graph_id);

    // TODO: integrate with ZepToolsService.get_graph_statistics
    Ok(Json(json!({
        "success": true,
        "data": {
            "graph_id": body.graph_id,
            "node_count": 0,
            "edge_count": 0,
        }
    })))
}
