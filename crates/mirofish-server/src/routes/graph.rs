//! `/api/graph/*` route handlers.
//!
//! Covers project management, ontology generation, graph building,
//! task tracking, and graph data retrieval/deletion.

use axum::{
    Router,
    extract::{Multipart, Path, Query, State},
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

/// Build the `/api/graph` sub-router.
pub fn router() -> Router<AppState> {
    Router::new()
        // Project management
        .route("/project/list", get(list_projects))
        .route("/project/{project_id}", get(get_project).delete(delete_project))
        .route("/project/{project_id}/reset", post(reset_project))
        // Ontology
        .route("/ontology/generate", post(generate_ontology))
        // Graph build
        .route("/build", post(build_graph))
        // Task tracking
        .route("/task/{task_id}", get(get_task))
        .route("/tasks", get(list_tasks))
        // Graph data
        .route("/data/{graph_id}", get(get_graph_data))
        .route("/delete/{graph_id}", delete(delete_graph))
}

// ---------------------------------------------------------------------------
// Query / Body types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListProjectsQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct BuildGraphRequest {
    pub project_id: String,
    pub graph_name: Option<String>,
    pub chunk_size: Option<usize>,
    pub chunk_overlap: Option<usize>,
    pub force: Option<bool>,
}

// ---------------------------------------------------------------------------
// Project management handlers
// ---------------------------------------------------------------------------

/// `GET /api/graph/project/:project_id` — get project details.
async fn get_project(
    State(_state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Getting project: {}", project_id);

    // TODO: integrate with ProjectManager from mirofish-core once implemented
    Err(AppError::NotFound(format!("Project not found: {project_id}")))
}

/// `GET /api/graph/project/list` — list all projects.
async fn list_projects(
    State(_state): State<AppState>,
    Query(params): Query<ListProjectsQuery>,
) -> Result<Json<Value>, AppError> {
    let limit = params.limit.unwrap_or(50);
    info!("Listing projects (limit={})", limit);

    // TODO: integrate with ProjectManager
    Ok(Json(json!({
        "success": true,
        "data": [],
        "count": 0
    })))
}

/// `DELETE /api/graph/project/:project_id` — delete a project.
async fn delete_project(
    State(_state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Deleting project: {}", project_id);

    // TODO: integrate with ProjectManager
    Err(AppError::NotFound(format!(
        "Project not found or deletion failed: {project_id}"
    )))
}

/// `POST /api/graph/project/:project_id/reset` — reset project status.
async fn reset_project(
    State(_state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Resetting project: {}", project_id);

    // TODO: integrate with ProjectManager — reset to ONTOLOGY_GENERATED or CREATED
    Err(AppError::NotFound(format!("Project not found: {project_id}")))
}

// ---------------------------------------------------------------------------
// Ontology generation
// ---------------------------------------------------------------------------

/// `POST /api/graph/ontology/generate` — upload files and generate ontology.
///
/// Accepts `multipart/form-data` with fields:
/// - `files` (one or more uploaded documents)
/// - `simulation_requirement` (required text)
/// - `project_name` (optional)
/// - `additional_context` (optional)
async fn generate_ontology(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    info!("=== Starting ontology generation ===");

    let mut simulation_requirement = String::new();
    let mut project_name = String::from("Unnamed Project");
    let mut additional_context = String::new();
    let mut files_data: Vec<(String, Vec<u8>)> = Vec::new();

    // Parse multipart fields.
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "simulation_requirement" => {
                simulation_requirement = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("Failed to read field: {e}")))?;
            }
            "project_name" => {
                project_name = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("Failed to read field: {e}")))?;
            }
            "additional_context" => {
                additional_context = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("Failed to read field: {e}")))?;
            }
            "files" => {
                let filename = field
                    .file_name()
                    .unwrap_or("unknown")
                    .to_string();
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("Failed to read file: {e}")))?;
                files_data.push((filename, data.to_vec()));
            }
            _ => {
                // Ignore unknown fields.
            }
        }
    }

    if simulation_requirement.is_empty() {
        return Err(AppError::BadRequest(
            "Please provide simulation_requirement".into(),
        ));
    }

    if files_data.is_empty() {
        return Err(AppError::BadRequest(
            "Please upload at least one document file".into(),
        ));
    }

    // Validate file extensions.
    let allowed = &state.config.allowed_extensions;
    let valid_files: Vec<_> = files_data
        .into_iter()
        .filter(|(name, _)| {
            name.rsplit('.')
                .next()
                .map(|ext| allowed.contains(&ext.to_lowercase()))
                .unwrap_or(false)
        })
        .collect();

    if valid_files.is_empty() {
        return Err(AppError::BadRequest(
            "No documents were successfully processed, please check the file formats".into(),
        ));
    }

    // Generate project ID.
    let project_id = format!("proj_{}", &Uuid::new_v4().to_string()[..12]);

    // Ensure upload directory exists.
    let project_dir = state.config.upload_folder.join(&project_id);
    tokio::fs::create_dir_all(&project_dir)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create project dir: {e}")))?;

    // Save files and collect metadata.
    let mut file_infos: Vec<Value> = Vec::new();
    let mut all_text = String::new();

    for (filename, data) in &valid_files {
        let dest = project_dir.join(filename);
        tokio::fs::write(&dest, data)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to save file: {e}")))?;

        file_infos.push(json!({
            "filename": filename,
            "size": data.len(),
        }));

        // Extract text — for now just treat all files as UTF-8 text.
        // TODO: integrate FileParser / TextProcessor from mirofish-core
        let text = String::from_utf8_lossy(data);
        all_text.push_str(&format!("\n\n=== {filename} ===\n{text}"));
    }

    let total_text_length = all_text.len();
    info!(
        "Text extraction complete, {} characters total",
        total_text_length
    );

    // Save extracted text.
    let text_path = project_dir.join("extracted_text.txt");
    tokio::fs::write(&text_path, &all_text)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to save extracted text: {e}")))?;

    // TODO: call OntologyGenerator from mirofish-core to produce real ontology.
    // For now return a placeholder.
    let ontology = json!({
        "entity_types": [],
        "edge_types": [],
    });
    let analysis_summary = "";

    info!("=== Ontology generation complete === Project ID: {}", project_id);

    Ok(Json(json!({
        "success": true,
        "data": {
            "project_id": project_id,
            "project_name": project_name,
            "ontology": ontology,
            "analysis_summary": analysis_summary,
            "files": file_infos,
            "total_text_length": total_text_length,
        }
    })))
}

// ---------------------------------------------------------------------------
// Graph build
// ---------------------------------------------------------------------------

/// `POST /api/graph/build` — start an async graph-build task.
async fn build_graph(
    State(state): State<AppState>,
    Json(body): Json<BuildGraphRequest>,
) -> Result<Json<Value>, AppError> {
    info!("=== Starting graph build ===");

    let project_id = &body.project_id;
    let graph_mode = state.config.graph_mode();

    // In NONE mode, skip graph building entirely.
    if graph_mode == mirofish_core::config::GraphMode::None {
        let task_id = format!("task_{}", &Uuid::new_v4().to_string()[..12]);
        info!("NONE mode: graph build skipped, project={}", project_id);
        return Ok(Json(json!({
            "success": true,
            "data": {
                "task_id": task_id,
                "project_id": project_id,
                "graph_id": "lite_mode",
            }
        })));
    }

    let graph_name = body
        .graph_name
        .clone()
        .unwrap_or_else(|| "MiroFish Graph".into());
    let chunk_size = body.chunk_size.unwrap_or(state.config.default_chunk_size);
    let chunk_overlap = body.chunk_overlap.unwrap_or(state.config.default_chunk_overlap);

    let task_id = format!("task_{}", &Uuid::new_v4().to_string()[..12]);

    info!(
        "Created graph build task: task_id={}, project_id={}, mode={}",
        task_id, project_id, graph_mode
    );

    // Spawn the long-running build in the background.
    let task_id_clone = task_id.clone();
    let project_id_clone = project_id.clone();
    let config = state.config.clone();

    tokio::spawn(async move {
        info!(
            "[{}] Background graph build started for project {}",
            task_id_clone, project_id_clone
        );
        // TODO: implement actual graph build via mirofish-core (local or Zep).
        // For now, just log that the task was spawned.
        info!("[{}] Graph build task placeholder complete", task_id_clone);
    });

    Ok(Json(json!({
        "success": true,
        "data": {
            "project_id": project_id,
            "task_id": task_id,
            "message": format!("Graph build task started, check progress via /task/{task_id}"),
        }
    })))
}

// ---------------------------------------------------------------------------
// Task tracking
// ---------------------------------------------------------------------------

/// `GET /api/graph/task/:task_id` — query task status.
async fn get_task(
    State(_state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Getting task: {}", task_id);

    // TODO: integrate with TaskManager from mirofish-core
    Err(AppError::NotFound(format!("Task not found: {task_id}")))
}

/// `GET /api/graph/tasks` — list all tasks.
async fn list_tasks(
    State(_state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    info!("Listing tasks");

    // TODO: integrate with TaskManager
    Ok(Json(json!({
        "success": true,
        "data": [],
        "count": 0
    })))
}

// ---------------------------------------------------------------------------
// Graph data
// ---------------------------------------------------------------------------

/// `GET /api/graph/data/:graph_id` — get graph data (nodes and edges).
async fn get_graph_data(
    State(state): State<AppState>,
    Path(graph_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Getting graph data: {}", graph_id);

    // Lite-mode placeholder graph.
    if graph_id == "lite_mode" {
        return Ok(Json(json!({
            "success": true,
            "data": {
                "graph_id": "lite_mode",
                "node_count": 0,
                "edge_count": 0,
                "nodes": [],
                "edges": [],
            }
        })));
    }

    // Local graph (prefix "local_").
    if graph_id.starts_with("local_") {
        // TODO: integrate with LocalGraphService
        return Ok(Json(json!({
            "success": true,
            "data": {
                "graph_id": graph_id,
                "node_count": 0,
                "edge_count": 0,
                "nodes": [],
                "edges": [],
            }
        })));
    }

    // Zep graph — requires API key.
    if !state.config.is_zep_available() {
        return Err(AppError::ServiceUnavailable(
            "Zep Cloud is not configured.".into(),
        ));
    }

    // TODO: integrate with GraphBuilderService (Zep)
    Ok(Json(json!({
        "success": true,
        "data": {
            "graph_id": graph_id,
            "node_count": 0,
            "edge_count": 0,
            "nodes": [],
            "edges": [],
        }
    })))
}

/// `DELETE /api/graph/delete/:graph_id` — delete a graph.
async fn delete_graph(
    State(state): State<AppState>,
    Path(graph_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Deleting graph: {}", graph_id);

    if !state.config.is_zep_available() {
        return Err(AppError::ServiceUnavailable(
            "ZEP_API_KEY is not configured. This feature requires Zep Cloud.".into(),
        ));
    }

    // TODO: integrate with GraphBuilderService (Zep)
    Ok(Json(json!({
        "success": true,
        "message": format!("Graph deleted: {graph_id}"),
    })))
}
