//! `/api/simulation/*` route handlers.
//!
//! Covers simulation lifecycle management: creation, preparation, execution,
//! monitoring, agent interviews, and history retrieval.

use axum::{
    Router,
    extract::{Path, Query, State},
    routing::{get, post},
    Json,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::info;
use uuid::Uuid;

use crate::error::AppError;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Interview prompt optimisation prefix.
// Adding this prefix prevents the Agent from calling tools and forces a direct
// text reply.
// ---------------------------------------------------------------------------

const INTERVIEW_PROMPT_PREFIX: &str =
    "Based on your persona, all past memories and actions, reply to me directly in text without calling any tools: ";

fn optimize_interview_prompt(prompt: &str) -> String {
    if prompt.is_empty() || prompt.starts_with(INTERVIEW_PROMPT_PREFIX) {
        return prompt.to_string();
    }
    format!("{INTERVIEW_PROMPT_PREFIX}{prompt}")
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the `/api/simulation` sub-router.
pub fn router() -> Router<AppState> {
    Router::new()
        // Simulation CRUD
        .route("/create", post(create_simulation))
        .route("/prepare", post(prepare_simulation))
        .route("/prepare/status", post(get_prepare_status))
        .route("/list", get(list_simulations))
        .route("/history", get(get_simulation_history))
        // Run control
        .route("/start", post(start_simulation))
        .route("/stop", post(stop_simulation))
        // Environment
        .route("/close-env", post(close_simulation_env))
        .route("/env-status", post(get_env_status))
        // Interviews
        .route("/interview/batch", post(interview_agents_batch))
        // Entity endpoints (Zep)
        .route("/entities/{graph_id}", get(get_graph_entities))
        .route("/entities/{graph_id}/{entity_uuid}", get(get_entity_detail))
        .route(
            "/entities/{graph_id}/by-type/{entity_type}",
            get(get_entities_by_type),
        )
        // Profile generation (standalone)
        .route("/generate-profiles", post(generate_profiles))
        // Parameterised simulation endpoints (must come after fixed paths)
        .route("/{simulation_id}", get(get_simulation))
        .route("/{simulation_id}/profiles", get(get_simulation_profiles))
        .route(
            "/{simulation_id}/profiles/realtime",
            get(get_simulation_profiles_realtime),
        )
        .route("/{simulation_id}/config", get(get_simulation_config))
        .route(
            "/{simulation_id}/config/realtime",
            get(get_simulation_config_realtime),
        )
        .route("/{simulation_id}/run-status", get(get_run_status))
        .route(
            "/{simulation_id}/run-status/detail",
            get(get_run_status_detail),
        )
        .route("/{simulation_id}/posts", get(get_simulation_posts))
        .route("/{simulation_id}/timeline", get(get_simulation_timeline))
        .route("/{simulation_id}/agent-stats", get(get_agent_stats))
        .route("/{simulation_id}/actions", get(get_simulation_actions))
}

// ---------------------------------------------------------------------------
// Query / body types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListSimulationsQuery {
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ProfilesQuery {
    pub platform: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PostsQuery {
    pub platform: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct TimelineQuery {
    pub start_round: Option<i64>,
    pub end_round: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ActionsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub platform: Option<String>,
    pub agent_id: Option<i64>,
    pub round_num: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RunStatusDetailQuery {
    pub platform: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EntitiesQuery {
    pub entity_types: Option<String>,
    pub enrich: Option<String>,
}

// --- Request bodies --------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateSimulationRequest {
    pub project_id: String,
    pub graph_id: Option<String>,
    pub enable_twitter: Option<bool>,
    pub enable_reddit: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct PrepareSimulationRequest {
    pub simulation_id: String,
    pub entity_types: Option<Vec<String>>,
    pub use_llm_for_profiles: Option<bool>,
    pub parallel_profile_count: Option<usize>,
    pub force_regenerate: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct PrepareStatusRequest {
    pub task_id: Option<String>,
    pub simulation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StartSimulationRequest {
    pub simulation_id: String,
    pub platform: Option<String>,
    pub max_rounds: Option<i64>,
    pub enable_graph_memory_update: Option<bool>,
    pub force: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct StopSimulationRequest {
    pub simulation_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CloseEnvRequest {
    pub simulation_id: String,
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct EnvStatusRequest {
    pub simulation_id: String,
}

#[derive(Debug, Deserialize)]
pub struct InterviewItem {
    pub agent_id: i64,
    pub prompt: String,
    pub platform: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BatchInterviewRequest {
    pub simulation_id: String,
    pub interviews: Vec<InterviewItem>,
    pub platform: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct GenerateProfilesRequest {
    pub graph_id: String,
    pub entity_types: Option<Vec<String>>,
    pub use_llm: Option<bool>,
    pub platform: Option<String>,
}

// ---------------------------------------------------------------------------
// Entity endpoints
// ---------------------------------------------------------------------------

/// `GET /api/simulation/entities/:graph_id` — get all graph entities (filtered).
async fn get_graph_entities(
    State(state): State<AppState>,
    Path(graph_id): Path<String>,
    Query(params): Query<EntitiesQuery>,
) -> Result<Json<Value>, AppError> {
    if !state.config.is_zep_available() {
        return Err(AppError::ServiceUnavailable(
            "ZEP_API_KEY is not configured. This feature requires Zep Cloud.".into(),
        ));
    }

    let _entity_types: Option<Vec<String>> = params.entity_types.map(|s| {
        s.split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    });
    let _enrich = params
        .enrich
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(true);

    info!("Fetching graph entities: graph_id={}", graph_id);

    // TODO: integrate with ZepEntityReader
    Ok(Json(json!({
        "success": true,
        "data": {
            "graph_id": graph_id,
            "filtered_count": 0,
            "entity_types": [],
            "entities": [],
        }
    })))
}

/// `GET /api/simulation/entities/:graph_id/:entity_uuid` — single entity detail.
async fn get_entity_detail(
    State(state): State<AppState>,
    Path((graph_id, entity_uuid)): Path<(String, String)>,
) -> Result<Json<Value>, AppError> {
    if !state.config.is_zep_available() {
        return Err(AppError::ServiceUnavailable(
            "ZEP_API_KEY is not configured. This feature requires Zep Cloud.".into(),
        ));
    }

    info!(
        "Fetching entity detail: graph_id={}, entity_uuid={}",
        graph_id, entity_uuid
    );

    // TODO: integrate with ZepEntityReader
    Err(AppError::NotFound(format!(
        "Entity does not exist: {entity_uuid}"
    )))
}

/// `GET /api/simulation/entities/:graph_id/by-type/:entity_type` — entities by type.
async fn get_entities_by_type(
    State(state): State<AppState>,
    Path((graph_id, entity_type)): Path<(String, String)>,
    Query(params): Query<EntitiesQuery>,
) -> Result<Json<Value>, AppError> {
    if !state.config.is_zep_available() {
        return Err(AppError::ServiceUnavailable(
            "ZEP_API_KEY is not configured. This feature requires Zep Cloud.".into(),
        ));
    }

    let _enrich = params
        .enrich
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(true);

    info!(
        "Fetching entities by type: graph_id={}, entity_type={}",
        graph_id, entity_type
    );

    // TODO: integrate with ZepEntityReader
    Ok(Json(json!({
        "success": true,
        "data": {
            "entity_type": entity_type,
            "count": 0,
            "entities": [],
        }
    })))
}

// ---------------------------------------------------------------------------
// Simulation lifecycle
// ---------------------------------------------------------------------------

/// `POST /api/simulation/create` — create a new simulation.
async fn create_simulation(
    State(state): State<AppState>,
    Json(body): Json<CreateSimulationRequest>,
) -> Result<Json<Value>, AppError> {
    info!("Creating simulation for project: {}", body.project_id);

    let enable_twitter = body.enable_twitter.unwrap_or(true);
    let enable_reddit = body.enable_reddit.unwrap_or(true);
    let simulation_id = format!("sim_{}", &Uuid::new_v4().to_string()[..12]);

    let graph_id = body.graph_id.unwrap_or_else(|| {
        if state.config.lite_mode {
            "lite_mode".into()
        } else {
            String::new()
        }
    });

    if graph_id.is_empty() {
        return Err(AppError::BadRequest(
            "The project has not built a knowledge graph yet. Please call /api/graph/build first."
                .into(),
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();

    // TODO: integrate with SimulationManager from mirofish-core
    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "project_id": body.project_id,
            "graph_id": graph_id,
            "status": "created",
            "enable_twitter": enable_twitter,
            "enable_reddit": enable_reddit,
            "created_at": now,
        }
    })))
}

/// `POST /api/simulation/prepare` — prepare the simulation environment (async).
async fn prepare_simulation(
    State(_state): State<AppState>,
    Json(body): Json<PrepareSimulationRequest>,
) -> Result<Json<Value>, AppError> {
    let simulation_id = &body.simulation_id;
    let force_regenerate = body.force_regenerate.unwrap_or(false);

    info!(
        "Processing /prepare request: simulation_id={}, force_regenerate={}",
        simulation_id, force_regenerate
    );

    let task_id = format!("task_{}", &Uuid::new_v4().to_string()[..12]);

    // Spawn background preparation task.
    let sim_id = simulation_id.clone();
    let tid = task_id.clone();

    tokio::spawn(async move {
        info!("[{}] Background prepare started for {}", tid, sim_id);
        // TODO: integrate with SimulationManager.prepare_simulation
        info!("[{}] Prepare task placeholder complete", tid);
    });

    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "task_id": task_id,
            "status": "preparing",
            "message": "Preparation task started; use /api/simulation/prepare/status to poll for progress",
            "already_prepared": false,
            "expected_entities_count": 0,
            "entity_types": [],
        }
    })))
}

/// `POST /api/simulation/prepare/status` — query preparation task progress.
async fn get_prepare_status(
    State(_state): State<AppState>,
    Json(body): Json<PrepareStatusRequest>,
) -> Result<Json<Value>, AppError> {
    let task_id = &body.task_id;
    let simulation_id = &body.simulation_id;

    if task_id.is_none() && simulation_id.is_none() {
        return Err(AppError::BadRequest(
            "Please provide a task_id or simulation_id".into(),
        ));
    }

    // TODO: integrate with TaskManager and _check_simulation_prepared
    if let Some(sim_id) = simulation_id {
        return Ok(Json(json!({
            "success": true,
            "data": {
                "simulation_id": sim_id,
                "status": "not_started",
                "progress": 0,
                "message": "Preparation has not started yet. Please call /api/simulation/prepare to begin.",
                "already_prepared": false,
            }
        })));
    }

    Err(AppError::NotFound(format!(
        "Task does not exist: {}",
        task_id.as_deref().unwrap_or("unknown")
    )))
}

/// `GET /api/simulation/:simulation_id` — get simulation status.
async fn get_simulation(
    State(_state): State<AppState>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Getting simulation: {}", simulation_id);

    // TODO: integrate with SimulationManager
    Err(AppError::NotFound(format!(
        "Simulation does not exist: {simulation_id}"
    )))
}

/// `GET /api/simulation/:simulation_id/profiles` — get agent profiles.
async fn get_simulation_profiles(
    State(_state): State<AppState>,
    Path(simulation_id): Path<String>,
    Query(params): Query<ProfilesQuery>,
) -> Result<Json<Value>, AppError> {
    let platform = params.platform.unwrap_or_else(|| "reddit".into());
    info!(
        "Getting profiles: simulation_id={}, platform={}",
        simulation_id, platform
    );

    // TODO: integrate with SimulationManager.get_profiles
    Ok(Json(json!({
        "success": true,
        "data": {
            "platform": platform,
            "count": 0,
            "profiles": [],
        }
    })))
}

/// `GET /api/simulation/:simulation_id/profiles/realtime` — real-time profile data.
async fn get_simulation_profiles_realtime(
    State(state): State<AppState>,
    Path(simulation_id): Path<String>,
    Query(params): Query<ProfilesQuery>,
) -> Result<Json<Value>, AppError> {
    let platform = params.platform.unwrap_or_else(|| "reddit".into());

    let sim_dir = state
        .config
        .oasis_simulation_data_dir
        .join(&simulation_id);

    let profiles_file = if platform == "reddit" {
        sim_dir.join("reddit_profiles.json")
    } else {
        sim_dir.join("twitter_profiles.csv")
    };

    let file_exists = profiles_file.exists();
    let mut profiles = Vec::<Value>::new();
    let mut file_modified_at: Option<String> = None;

    if file_exists {
        if let Ok(meta) = tokio::fs::metadata(&profiles_file).await {
            if let Ok(mtime) = meta.modified() {
                let dt: chrono::DateTime<chrono::Utc> = mtime.into();
                file_modified_at = Some(dt.to_rfc3339());
            }
        }

        if platform == "reddit" {
            if let Ok(data) = tokio::fs::read_to_string(&profiles_file).await {
                if let Ok(parsed) = serde_json::from_str::<Vec<Value>>(&data) {
                    profiles = parsed;
                }
            }
        }
        // TODO: CSV parsing for twitter profiles
    }

    // Check state.json for generation status.
    let mut is_generating = false;
    let mut total_expected: Option<i64> = None;
    let state_file = sim_dir.join("state.json");
    if let Ok(data) = tokio::fs::read_to_string(&state_file).await {
        if let Ok(state_data) = serde_json::from_str::<Value>(&data) {
            is_generating = state_data.get("status").and_then(|s| s.as_str()) == Some("preparing");
            total_expected = state_data
                .get("entities_count")
                .and_then(|v| v.as_i64());
        }
    }

    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "platform": platform,
            "count": profiles.len(),
            "total_expected": total_expected,
            "is_generating": is_generating,
            "file_exists": file_exists,
            "file_modified_at": file_modified_at,
            "profiles": profiles,
        }
    })))
}

/// `GET /api/simulation/:simulation_id/config` — get simulation config.
async fn get_simulation_config(
    State(_state): State<AppState>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    info!("Getting simulation config: {}", simulation_id);

    // TODO: integrate with SimulationManager.get_simulation_config
    Err(AppError::NotFound(
        "Simulation config does not exist. Please call /prepare first.".into(),
    ))
}

/// `GET /api/simulation/:simulation_id/config/realtime` — real-time config data.
async fn get_simulation_config_realtime(
    State(state): State<AppState>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let sim_dir = state
        .config
        .oasis_simulation_data_dir
        .join(&simulation_id);

    if !sim_dir.exists() {
        return Err(AppError::NotFound(format!(
            "Simulation does not exist: {simulation_id}"
        )));
    }

    let config_file = sim_dir.join("simulation_config.json");
    let file_exists = config_file.exists();
    let mut config: Option<Value> = None;
    let mut file_modified_at: Option<String> = None;

    if file_exists {
        if let Ok(meta) = tokio::fs::metadata(&config_file).await {
            if let Ok(mtime) = meta.modified() {
                let dt: chrono::DateTime<chrono::Utc> = mtime.into();
                file_modified_at = Some(dt.to_rfc3339());
            }
        }
        if let Ok(data) = tokio::fs::read_to_string(&config_file).await {
            config = serde_json::from_str(&data).ok();
        }
    }

    // Check state.json.
    let mut is_generating = false;
    let mut generation_stage: Option<String> = None;
    let mut config_generated = false;

    let state_file = sim_dir.join("state.json");
    if let Ok(data) = tokio::fs::read_to_string(&state_file).await {
        if let Ok(state_data) = serde_json::from_str::<Value>(&data) {
            let status = state_data
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            is_generating = status == "preparing";
            config_generated = state_data
                .get("config_generated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if is_generating {
                let profiles_gen = state_data
                    .get("profiles_generated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                generation_stage = Some(if profiles_gen {
                    "generating_config".into()
                } else {
                    "generating_profiles".into()
                });
            } else if status == "ready" {
                generation_stage = Some("completed".into());
            }
        }
    }

    let mut response_data = json!({
        "simulation_id": simulation_id,
        "file_exists": file_exists,
        "file_modified_at": file_modified_at,
        "is_generating": is_generating,
        "generation_stage": generation_stage,
        "config_generated": config_generated,
        "config": config,
    });

    // Add summary if config is present.
    if let Some(ref cfg) = config {
        response_data["summary"] = json!({
            "total_agents": cfg.get("agent_configs").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0),
            "simulation_hours": cfg.pointer("/time_config/total_simulation_hours"),
            "initial_posts_count": cfg.pointer("/event_config/initial_posts").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0),
            "hot_topics_count": cfg.pointer("/event_config/hot_topics").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0),
            "has_twitter_config": cfg.get("twitter_config").is_some(),
            "has_reddit_config": cfg.get("reddit_config").is_some(),
            "generated_at": cfg.get("generated_at"),
            "llm_model": cfg.get("llm_model"),
        });
    }

    Ok(Json(json!({
        "success": true,
        "data": response_data,
    })))
}

/// `GET /api/simulation/list` — list all simulations.
async fn list_simulations(
    State(_state): State<AppState>,
    Query(params): Query<ListSimulationsQuery>,
) -> Result<Json<Value>, AppError> {
    info!("Listing simulations, project_id={:?}", params.project_id);

    // TODO: integrate with SimulationManager.list_simulations
    Ok(Json(json!({
        "success": true,
        "data": [],
        "count": 0,
    })))
}

/// `GET /api/simulation/history` — recent simulation history with enriched data.
async fn get_simulation_history(
    State(_state): State<AppState>,
    Query(params): Query<HistoryQuery>,
) -> Result<Json<Value>, AppError> {
    let limit = params.limit.unwrap_or(20);
    info!("Getting simulation history (limit={})", limit);

    // TODO: integrate with SimulationManager + ProjectManager enrichment
    Ok(Json(json!({
        "success": true,
        "data": [],
        "count": 0,
    })))
}

// ---------------------------------------------------------------------------
// Run control
// ---------------------------------------------------------------------------

/// `POST /api/simulation/start` — start running a simulation.
async fn start_simulation(
    State(_state): State<AppState>,
    Json(body): Json<StartSimulationRequest>,
) -> Result<Json<Value>, AppError> {
    let simulation_id = &body.simulation_id;
    let platform = body.platform.as_deref().unwrap_or("parallel");
    let enable_graph_memory_update = body.enable_graph_memory_update.unwrap_or(false);
    let force = body.force.unwrap_or(false);

    info!(
        "Starting simulation: id={}, platform={}, force={}",
        simulation_id, platform, force
    );

    // Validate platform.
    if !["twitter", "reddit", "parallel"].contains(&platform) {
        return Err(AppError::BadRequest(format!(
            "Invalid platform type: {platform}, options: twitter/reddit/parallel"
        )));
    }

    // Validate max_rounds.
    if let Some(max_rounds) = body.max_rounds {
        if max_rounds <= 0 {
            return Err(AppError::BadRequest(
                "max_rounds must be a positive integer".into(),
            ));
        }
    }

    // TODO: integrate with SimulationManager + SimulationRunner
    let now = chrono::Utc::now().to_rfc3339();

    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "runner_status": "running",
            "platform": platform,
            "started_at": now,
            "graph_memory_update_enabled": enable_graph_memory_update,
            "force_restarted": force,
        }
    })))
}

/// `POST /api/simulation/stop` — stop a simulation.
async fn stop_simulation(
    State(_state): State<AppState>,
    Json(body): Json<StopSimulationRequest>,
) -> Result<Json<Value>, AppError> {
    let simulation_id = &body.simulation_id;
    info!("Stopping simulation: {}", simulation_id);

    // TODO: integrate with SimulationRunner.stop_simulation
    let now = chrono::Utc::now().to_rfc3339();

    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "runner_status": "stopped",
            "completed_at": now,
        }
    })))
}

// ---------------------------------------------------------------------------
// Real-time status monitoring
// ---------------------------------------------------------------------------

/// `GET /api/simulation/:simulation_id/run-status` — current run status.
async fn get_run_status(
    State(_state): State<AppState>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    // TODO: integrate with SimulationRunner.get_run_state
    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "runner_status": "idle",
            "current_round": 0,
            "total_rounds": 0,
            "progress_percent": 0,
            "twitter_actions_count": 0,
            "reddit_actions_count": 0,
            "total_actions_count": 0,
        }
    })))
}

/// `GET /api/simulation/:simulation_id/run-status/detail` — detailed run status.
async fn get_run_status_detail(
    State(_state): State<AppState>,
    Path(simulation_id): Path<String>,
    Query(_params): Query<RunStatusDetailQuery>,
) -> Result<Json<Value>, AppError> {
    // TODO: integrate with SimulationRunner
    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "runner_status": "idle",
            "all_actions": [],
            "twitter_actions": [],
            "reddit_actions": [],
        }
    })))
}

/// `GET /api/simulation/:simulation_id/actions` — agent action history.
async fn get_simulation_actions(
    State(_state): State<AppState>,
    Path(simulation_id): Path<String>,
    Query(params): Query<ActionsQuery>,
) -> Result<Json<Value>, AppError> {
    let _limit = params.limit.unwrap_or(100);
    let _offset = params.offset.unwrap_or(0);

    // TODO: integrate with SimulationRunner.get_actions
    Ok(Json(json!({
        "success": true,
        "data": {
            "count": 0,
            "actions": [],
        }
    })))
}

/// `GET /api/simulation/:simulation_id/timeline` — round-by-round timeline.
async fn get_simulation_timeline(
    State(_state): State<AppState>,
    Path(simulation_id): Path<String>,
    Query(params): Query<TimelineQuery>,
) -> Result<Json<Value>, AppError> {
    let _start = params.start_round.unwrap_or(0);

    // TODO: integrate with SimulationRunner.get_timeline
    Ok(Json(json!({
        "success": true,
        "data": {
            "rounds_count": 0,
            "timeline": [],
        }
    })))
}

/// `GET /api/simulation/:simulation_id/agent-stats` — per-agent statistics.
async fn get_agent_stats(
    State(_state): State<AppState>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    // TODO: integrate with SimulationRunner.get_agent_stats
    Ok(Json(json!({
        "success": true,
        "data": {
            "agents_count": 0,
            "stats": [],
        }
    })))
}

/// `GET /api/simulation/:simulation_id/posts` — get posts from the simulation DB.
async fn get_simulation_posts(
    State(state): State<AppState>,
    Path(simulation_id): Path<String>,
    Query(params): Query<PostsQuery>,
) -> Result<Json<Value>, AppError> {
    let platform = params.platform.as_deref().unwrap_or("reddit");
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    let sim_dir = state
        .config
        .oasis_simulation_data_dir
        .join(&simulation_id);

    let db_file = format!("{platform}_simulation.db");
    let db_path = sim_dir.join(&db_file);

    if !db_path.exists() {
        return Ok(Json(json!({
            "success": true,
            "data": {
                "platform": platform,
                "count": 0,
                "posts": [],
                "message": "Database does not exist; the simulation may not have run yet",
            }
        })));
    }

    // Query SQLite (blocking — run on blocking thread).
    let db_path_str = db_path.to_string_lossy().to_string();
    let result = tokio::task::spawn_blocking(move || -> Result<(Vec<Value>, i64), String> {
        let conn =
            rusqlite::Connection::open(&db_path_str).map_err(|e| format!("DB open error: {e}"))?;

        let mut stmt = conn
            .prepare("SELECT * FROM post ORDER BY created_at DESC LIMIT ?1 OFFSET ?2")
            .map_err(|e| format!("SQL error: {e}"))?;

        let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();

        let rows: Vec<Value> = stmt
            .query_map(rusqlite::params![limit, offset], |row| {
                let mut map = serde_json::Map::new();
                for (i, col) in columns.iter().enumerate() {
                    let val: rusqlite::Result<String> = row.get(i);
                    match val {
                        Ok(s) => {
                            map.insert(col.clone(), Value::String(s));
                        }
                        Err(_) => {
                            // Try as integer, then as null.
                            if let Ok(n) = row.get::<_, i64>(i) {
                                map.insert(col.clone(), json!(n));
                            } else {
                                map.insert(col.clone(), Value::Null);
                            }
                        }
                    }
                }
                Ok(Value::Object(map))
            })
            .map_err(|e| format!("Query error: {e}"))?
            .filter_map(|r| r.ok())
            .collect();

        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM post", [], |row| row.get(0))
            .unwrap_or(0);

        Ok((rows, total))
    })
    .await
    .map_err(|e| AppError::Internal(format!("Task join error: {e}")))?
    .map_err(|e| AppError::Internal(e))?;

    Ok(Json(json!({
        "success": true,
        "data": {
            "platform": platform,
            "total": result.1,
            "count": result.0.len(),
            "posts": result.0,
        }
    })))
}

// ---------------------------------------------------------------------------
// Environment management
// ---------------------------------------------------------------------------

/// `POST /api/simulation/close-env` — gracefully close the simulation environment.
async fn close_simulation_env(
    State(_state): State<AppState>,
    Json(body): Json<CloseEnvRequest>,
) -> Result<Json<Value>, AppError> {
    let simulation_id = &body.simulation_id;
    let _timeout = body.timeout.unwrap_or(30);
    info!("Closing simulation env: {}", simulation_id);

    // TODO: integrate with SimulationRunner.close_simulation_env
    let now = chrono::Utc::now().to_rfc3339();

    Ok(Json(json!({
        "success": true,
        "data": {
            "message": "Environment close command sent",
            "simulation_id": simulation_id,
            "timestamp": now,
        }
    })))
}

/// `POST /api/simulation/env-status` — check if environment is alive.
async fn get_env_status(
    State(_state): State<AppState>,
    Json(body): Json<EnvStatusRequest>,
) -> Result<Json<Value>, AppError> {
    let simulation_id = &body.simulation_id;
    info!("Checking env status: {}", simulation_id);

    // TODO: integrate with SimulationRunner.check_env_alive
    Ok(Json(json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "env_alive": false,
            "twitter_available": false,
            "reddit_available": false,
            "message": "Environment is not running or has been closed",
        }
    })))
}

// ---------------------------------------------------------------------------
// Interviews
// ---------------------------------------------------------------------------

/// `POST /api/simulation/interview/batch` — interview multiple agents.
async fn interview_agents_batch(
    State(_state): State<AppState>,
    Json(body): Json<BatchInterviewRequest>,
) -> Result<Json<Value>, AppError> {
    let simulation_id = &body.simulation_id;
    let default_platform = body.platform.as_deref();

    info!(
        "Batch interview: simulation_id={}, count={}",
        simulation_id,
        body.interviews.len()
    );

    // Validate each interview item.
    for (i, item) in body.interviews.iter().enumerate() {
        if item.prompt.is_empty() {
            return Err(AppError::BadRequest(format!(
                "Interview list item {} is missing prompt",
                i + 1
            )));
        }
        if let Some(ref p) = item.platform {
            if p != "twitter" && p != "reddit" {
                return Err(AppError::BadRequest(format!(
                    "The platform for interview list item {} must be either 'twitter' or 'reddit'",
                    i + 1
                )));
            }
        }
    }

    // Optimise prompts.
    let _optimized: Vec<_> = body
        .interviews
        .iter()
        .map(|item| {
            (
                item.agent_id,
                optimize_interview_prompt(&item.prompt),
                item.platform.as_deref().or(default_platform),
            )
        })
        .collect();

    // TODO: integrate with SimulationRunner.interview_agents_batch
    let now = chrono::Utc::now().to_rfc3339();

    Ok(Json(json!({
        "success": true,
        "data": {
            "interviews_count": body.interviews.len(),
            "result": {
                "interviews_count": 0,
                "results": {},
            },
            "timestamp": now,
        }
    })))
}

// ---------------------------------------------------------------------------
// Profile generation (standalone)
// ---------------------------------------------------------------------------

/// `POST /api/simulation/generate-profiles` — generate profiles from a graph.
async fn generate_profiles(
    State(state): State<AppState>,
    Json(body): Json<GenerateProfilesRequest>,
) -> Result<Json<Value>, AppError> {
    let graph_id = &body.graph_id;
    let platform = body.platform.as_deref().unwrap_or("reddit");

    if !state.config.is_zep_available() {
        return Err(AppError::ServiceUnavailable(
            "ZEP_API_KEY is not configured. This feature requires Zep Cloud.".into(),
        ));
    }

    info!(
        "Generating profiles: graph_id={}, platform={}",
        graph_id, platform
    );

    // TODO: integrate with ZepEntityReader + OasisProfileGenerator
    Ok(Json(json!({
        "success": true,
        "data": {
            "platform": platform,
            "entity_types": [],
            "count": 0,
            "profiles": [],
        }
    })))
}
