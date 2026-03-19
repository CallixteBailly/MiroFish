//! Simulation state management.
//!
//! Tracks multiple concurrent simulations with thread-safe shared state.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Simulation status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationStatus {
    Created,
    Preparing,
    Ready,
    Running,
    Paused,
    Stopped,
    Completed,
    Failed,
}

/// Platform type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformType {
    Twitter,
    Reddit,
}

/// Simulation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationState {
    pub simulation_id: String,
    pub project_id: String,
    pub graph_id: String,
    pub enable_twitter: bool,
    pub enable_reddit: bool,
    pub status: SimulationStatus,
    pub entities_count: usize,
    pub profiles_count: usize,
    pub entity_types: Vec<String>,
    pub config_generated: bool,
    pub config_reasoning: String,
    pub current_round: u64,
    pub twitter_status: String,
    pub reddit_status: String,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<String>,
}

impl SimulationState {
    /// Create a new simulation state.
    pub fn new(simulation_id: &str, project_id: &str, graph_id: &str) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            simulation_id: simulation_id.to_string(),
            project_id: project_id.to_string(),
            graph_id: graph_id.to_string(),
            enable_twitter: true,
            enable_reddit: true,
            status: SimulationStatus::Created,
            entities_count: 0,
            profiles_count: 0,
            entity_types: Vec::new(),
            config_generated: false,
            config_reasoning: String::new(),
            current_round: 0,
            twitter_status: "not_started".to_string(),
            reddit_status: "not_started".to_string(),
            created_at: now.clone(),
            updated_at: now,
            error: None,
        }
    }
}

/// Simulation manager — tracks multiple concurrent simulations.
pub struct SimulationManager {
    data_dir: PathBuf,
    simulations: Arc<RwLock<HashMap<String, SimulationState>>>,
}

impl SimulationManager {
    /// Create a new manager with the given data directory.
    pub fn new(data_dir: &Path) -> Self {
        fs::create_dir_all(data_dir).ok();
        Self {
            data_dir: data_dir.to_path_buf(),
            simulations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create from global config.
    pub fn from_global_config() -> Self {
        let cfg = crate::config::Config::global();
        Self::new(&cfg.oasis_simulation_data_dir)
    }

    /// Get the directory for a simulation.
    fn sim_dir(&self, simulation_id: &str) -> PathBuf {
        let dir = self.data_dir.join(simulation_id);
        fs::create_dir_all(&dir).ok();
        dir
    }

    /// Save simulation state to file.
    pub fn save_state(&self, state: &SimulationState) -> anyhow::Result<()> {
        let mut state = state.clone();
        state.updated_at = Utc::now().to_rfc3339();

        let dir = self.sim_dir(&state.simulation_id);
        let state_file = dir.join("state.json");
        let json = serde_json::to_string_pretty(&state)?;
        fs::write(state_file, json)?;

        let mut sims = self.simulations.write().map_err(|e| anyhow::anyhow!("lock: {}", e))?;
        sims.insert(state.simulation_id.clone(), state);
        Ok(())
    }

    /// Load simulation state from file or cache.
    pub fn load_state(&self, simulation_id: &str) -> anyhow::Result<Option<SimulationState>> {
        // Check cache first
        {
            let sims = self.simulations.read().map_err(|e| anyhow::anyhow!("lock: {}", e))?;
            if let Some(state) = sims.get(simulation_id) {
                return Ok(Some(state.clone()));
            }
        }

        // Load from file
        let state_file = self.sim_dir(simulation_id).join("state.json");
        if !state_file.exists() {
            return Ok(None);
        }

        let data = fs::read_to_string(state_file)?;
        let state: SimulationState = serde_json::from_str(&data)?;

        let mut sims = self.simulations.write().map_err(|e| anyhow::anyhow!("lock: {}", e))?;
        sims.insert(simulation_id.to_string(), state.clone());
        Ok(Some(state))
    }

    /// Create a new simulation.
    pub fn create_simulation(
        &self,
        project_id: &str,
        graph_id: &str,
        enable_twitter: bool,
        enable_reddit: bool,
    ) -> anyhow::Result<SimulationState> {
        let sim_id = format!("sim_{}", &Uuid::new_v4().to_string().replace('-', "")[..12]);
        let mut state = SimulationState::new(&sim_id, project_id, graph_id);
        state.enable_twitter = enable_twitter;
        state.enable_reddit = enable_reddit;
        self.save_state(&state)?;
        tracing::info!("Simulation created: {}, project={}, graph={}", sim_id, project_id, graph_id);
        Ok(state)
    }

    /// Get simulation state.
    pub fn get_simulation(&self, simulation_id: &str) -> anyhow::Result<Option<SimulationState>> {
        self.load_state(simulation_id)
    }

    /// List all simulations, optionally filtered by project.
    pub fn list_simulations(&self, project_id: Option<&str>) -> anyhow::Result<Vec<SimulationState>> {
        let mut simulations = Vec::new();

        if self.data_dir.exists() {
            for entry in fs::read_dir(&self.data_dir)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                if let Ok(Some(state)) = self.load_state(&name) {
                    if project_id.map_or(true, |pid| state.project_id == pid) {
                        simulations.push(state);
                    }
                }
            }
        }

        Ok(simulations)
    }

    /// Get the simulation data directory.
    pub fn get_sim_dir(&self, simulation_id: &str) -> PathBuf {
        self.sim_dir(simulation_id)
    }

    /// Get the simulation configuration.
    pub fn get_simulation_config(&self, simulation_id: &str) -> anyhow::Result<Option<Value>> {
        let config_path = self.sim_dir(simulation_id).join("simulation_config.json");
        if !config_path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(config_path)?;
        let config: Value = serde_json::from_str(&data)?;
        Ok(Some(config))
    }

    /// Get agent profiles for a simulation.
    pub fn get_profiles(&self, simulation_id: &str, platform: &str) -> anyhow::Result<Vec<Value>> {
        let profile_path = self.sim_dir(simulation_id).join(format!("{}_profiles.json", platform));
        if !profile_path.exists() {
            return Ok(Vec::new());
        }
        let data = fs::read_to_string(profile_path)?;
        let profiles: Vec<Value> = serde_json::from_str(&data)?;
        Ok(profiles)
    }
}
