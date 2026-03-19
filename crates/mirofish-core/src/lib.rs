//! MiroFish Core — collective intelligence prediction engine.
//!
//! This crate contains the core business logic for MiroFish:
//! LLM integration, knowledge graph operations, social media simulation,
//! report generation, text processing, and data models.

pub mod llm;
pub mod graph;
pub mod ontology;
pub mod simulation;
pub mod report;
pub mod text;
pub mod models;

/// Re-export commonly used error type.
pub use anyhow::Result;

pub mod config {
    //! Application configuration, loaded from environment / `.env` file.

    use std::collections::HashSet;
    use std::env;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    /// Global singleton config.
    static CONFIG: OnceLock<Config> = OnceLock::new();

    /// Graph backend mode.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum GraphMode {
        Zep,
        Local,
        None,
    }

    impl std::fmt::Display for GraphMode {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                GraphMode::Zep => write!(f, "zep"),
                GraphMode::Local => write!(f, "local"),
                GraphMode::None => write!(f, "none"),
            }
        }
    }

    /// Application configuration.
    #[derive(Debug, Clone)]
    pub struct Config {
        // Lite mode — runs without Zep Cloud
        pub lite_mode: bool,

        // LLM config
        pub llm_api_key: String,
        pub llm_base_url: String,
        pub llm_model_name: String,

        // Optional boost LLM
        pub llm_boost_api_key: Option<String>,
        pub llm_boost_base_url: Option<String>,
        pub llm_boost_model_name: Option<String>,

        // Zep config
        pub zep_api_key: Option<String>,

        // File upload config
        pub upload_folder: PathBuf,
        pub allowed_extensions: HashSet<String>,
        pub max_content_length: usize,

        // Text processing
        pub default_chunk_size: usize,
        pub default_chunk_overlap: usize,

        // OASIS simulation config
        pub oasis_default_max_rounds: usize,
        pub oasis_simulation_data_dir: PathBuf,
        pub oasis_twitter_actions: Vec<String>,
        pub oasis_reddit_actions: Vec<String>,

        // Report agent config
        pub report_agent_max_tool_calls: usize,
        pub report_agent_max_reflection_rounds: usize,
        pub report_agent_temperature: f64,
    }

    const ZEP_PLACEHOLDER: &str = "your_zep_api_key_here";

    impl Config {
        /// Load configuration from environment variables.
        /// Call `dotenvy::dotenv().ok()` before this if you want `.env` support.
        pub fn from_env() -> Self {
            let lite_mode = env::var("LITE_MODE")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(false);

            let upload_folder = env::var("UPLOAD_FOLDER")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("uploads"));

            let sim_data_dir = env::var("OASIS_SIMULATION_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| upload_folder.join("simulations"));

            Config {
                lite_mode,

                llm_api_key: env::var("LLM_API_KEY").unwrap_or_default(),
                llm_base_url: env::var("LLM_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
                llm_model_name: env::var("LLM_MODEL_NAME")
                    .unwrap_or_else(|_| "gpt-4o-mini".into()),

                llm_boost_api_key: env::var("LLM_BOOST_API_KEY").ok(),
                llm_boost_base_url: env::var("LLM_BOOST_BASE_URL").ok(),
                llm_boost_model_name: env::var("LLM_BOOST_MODEL_NAME").ok(),

                zep_api_key: env::var("ZEP_API_KEY").ok(),

                upload_folder: upload_folder.clone(),
                allowed_extensions: ["pdf", "md", "txt", "markdown"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                max_content_length: 50 * 1024 * 1024, // 50 MB

                default_chunk_size: env::var("DEFAULT_CHUNK_SIZE")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(500),
                default_chunk_overlap: env::var("DEFAULT_CHUNK_OVERLAP")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(50),

                oasis_default_max_rounds: env::var("OASIS_DEFAULT_MAX_ROUNDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10),
                oasis_simulation_data_dir: sim_data_dir,
                oasis_twitter_actions: vec![
                    "CREATE_POST".into(),
                    "LIKE_POST".into(),
                    "REPOST".into(),
                    "FOLLOW".into(),
                    "DO_NOTHING".into(),
                    "QUOTE_POST".into(),
                ],
                oasis_reddit_actions: vec![
                    "LIKE_POST".into(),
                    "DISLIKE_POST".into(),
                    "CREATE_POST".into(),
                    "CREATE_COMMENT".into(),
                    "LIKE_COMMENT".into(),
                    "DISLIKE_COMMENT".into(),
                    "SEARCH_POSTS".into(),
                    "SEARCH_USER".into(),
                    "TREND".into(),
                    "REFRESH".into(),
                    "DO_NOTHING".into(),
                    "FOLLOW".into(),
                    "MUTE".into(),
                ],

                report_agent_max_tool_calls: env::var("REPORT_AGENT_MAX_TOOL_CALLS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5),
                report_agent_max_reflection_rounds: env::var("REPORT_AGENT_MAX_REFLECTION_ROUNDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(2),
                report_agent_temperature: env::var("REPORT_AGENT_TEMPERATURE")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.5),
            }
        }

        /// Check whether a real (non-placeholder) Zep API key is configured.
        pub fn is_zep_available(&self) -> bool {
            match &self.zep_api_key {
                Some(key) => !key.is_empty() && key != ZEP_PLACEHOLDER,
                None => false,
            }
        }

        /// Returns the active graph backend.
        pub fn graph_mode(&self) -> GraphMode {
            if self.lite_mode {
                return GraphMode::None;
            }
            if self.is_zep_available() {
                return GraphMode::Zep;
            }
            GraphMode::Local
        }

        /// Validate required configuration. Returns a list of error messages.
        pub fn validate(&self) -> Vec<String> {
            let mut errors = Vec::new();
            if self.llm_api_key.is_empty() {
                errors.push("LLM_API_KEY is not configured".into());
            }
            if self.graph_mode() == GraphMode::Zep && !self.is_zep_available() {
                errors.push("ZEP_API_KEY is not configured".into());
            }
            errors
        }

        /// Get (or initialize) the global config singleton.
        pub fn global() -> &'static Config {
            CONFIG.get_or_init(|| {
                dotenvy::dotenv().ok();
                Config::from_env()
            })
        }
    }
}
