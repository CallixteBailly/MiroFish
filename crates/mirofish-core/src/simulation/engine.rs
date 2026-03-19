//! Simulation engine — manages the simulation loop.
//!
//! For each round, each active agent observes the platform state,
//! calls LLM to decide an action, and executes it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

use crate::llm::client::LlmClient;

use super::actions::ActionRecord;
use super::agent::{AgentProfile, SimulatedAgent};
use super::config_generator::SimulationParameters;
use super::platforms::reddit::RedditPlatform;
use super::platforms::twitter::TwitterPlatform;

/// Status of a running simulation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineStatus {
    Idle,
    Running,
    Paused,
    Completed,
    Failed,
}

/// Round summary statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundSummary {
    pub round_num: u64,
    pub simulated_hour: u32,
    pub twitter_actions: usize,
    pub reddit_actions: usize,
    pub active_agents: Vec<u64>,
}

/// The main simulation engine.
pub struct SimulationEngine {
    pub status: EngineStatus,
    pub config: SimulationParameters,
    pub twitter: Option<TwitterPlatform>,
    pub reddit: Option<RedditPlatform>,
    pub agents: Vec<SimulatedAgent>,
    pub current_round: u64,
    pub total_rounds: u64,
    pub action_log: Vec<ActionRecord>,
    pub round_summaries: Vec<RoundSummary>,
    llm: LlmClient,
}

impl SimulationEngine {
    /// Create a new engine from configuration.
    pub fn new(config: SimulationParameters, llm: LlmClient) -> Self {
        let total_hours = config.time_config.total_simulation_hours;
        let mins_per_round = config.time_config.minutes_per_round;
        let total_rounds = (total_hours as u64 * 60) / mins_per_round as u64;

        // Initialize platforms
        let has_twitter = config.platform_configs.iter().any(|p| p.platform == "twitter");
        let has_reddit = config.platform_configs.iter().any(|p| p.platform == "reddit");

        let twitter = if has_twitter { Some(TwitterPlatform::new()) } else { None };
        let reddit = if has_reddit { Some(RedditPlatform::new()) } else { None };

        Self {
            status: EngineStatus::Idle,
            config,
            twitter,
            reddit,
            agents: Vec::new(),
            current_round: 0,
            total_rounds,
            action_log: Vec::new(),
            round_summaries: Vec::new(),
            llm,
        }
    }

    /// Load agent profiles and register them on the platforms.
    pub fn load_profiles(&mut self, profiles: Vec<AgentProfile>) {
        for profile in &profiles {
            if let Some(ref mut tw) = self.twitter {
                tw.add_user(profile.user_id, &profile.username, &profile.name, &profile.bio);
            }
            if let Some(ref mut rd) = self.reddit {
                rd.add_user(
                    profile.user_id,
                    &profile.username,
                    &profile.name,
                    &profile.bio,
                    profile.karma,
                );
            }
        }

        self.agents = profiles.into_iter().map(SimulatedAgent::new).collect();
    }

    /// Inject initial posts from the event config.
    pub fn inject_initial_events(&mut self) {
        for (i, post) in self.config.event_config.initial_posts.iter().enumerate() {
            let content = post.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if content.is_empty() {
                continue;
            }

            // Find an agent to post the initial event, or use the first agent
            let agent_id = if !self.agents.is_empty() {
                self.agents[i % self.agents.len()].profile.user_id
            } else {
                continue;
            };

            if let Some(ref mut tw) = self.twitter {
                tw.create_post(agent_id, content);
            }
            if let Some(ref mut rd) = self.reddit {
                rd.create_post(agent_id, "general", content, content);
            }
        }
    }

    /// Get the simulated time for a given round.
    fn simulated_time(&self, round: u64) -> String {
        let minutes_elapsed = round * self.config.time_config.minutes_per_round as u64;
        let hours = minutes_elapsed / 60;
        let mins = minutes_elapsed % 60;
        format!("Day {} {:02}:{:02}", hours / 24 + 1, hours % 24, mins)
    }

    /// Get the simulated hour of day for a round.
    fn simulated_hour(&self, round: u64) -> u32 {
        let minutes_elapsed = round * self.config.time_config.minutes_per_round as u64;
        ((minutes_elapsed / 60) % 24) as u32
    }

    /// Determine which agents are active this round based on time-of-day and config.
    fn active_agents_for_round(&self, round: u64) -> Vec<usize> {
        let hour = self.simulated_hour(round);

        // Determine activity multiplier for the hour
        let tc = &self.config.time_config;
        let multiplier = if tc.off_peak_hours.contains(&hour) {
            tc.off_peak_activity_multiplier
        } else if tc.peak_hours.contains(&hour) {
            tc.peak_activity_multiplier
        } else {
            0.7 // default work/morning hours
        };

        let mut active = Vec::new();
        let mut rng = rand::rng();

        for (i, agent_cfg) in self.config.agent_configs.iter().enumerate() {
            let agent_active_hours: Vec<u32> = agent_cfg.active_hours.clone();
            if !agent_active_hours.contains(&hour) {
                continue;
            }

            // Probability based on activity level * time multiplier
            let prob = agent_cfg.activity_level * multiplier;
            let roll: f64 = rand::Rng::random(&mut rng);
            if roll < prob {
                active.push(i);
            }
        }

        active
    }

    /// Run a single round of the simulation.
    pub async fn run_round(&mut self) -> anyhow::Result<RoundSummary> {
        self.current_round += 1;
        let round = self.current_round;
        let sim_time = self.simulated_time(round);
        let sim_hour = self.simulated_hour(round);

        tracing::info!("Round {}/{} — simulated time: {}", round, self.total_rounds, sim_time);

        let active_indices = self.active_agents_for_round(round);
        let mut twitter_actions = 0usize;
        let mut reddit_actions = 0usize;
        let mut active_agent_ids = Vec::new();

        for agent_idx in active_indices {
            if agent_idx >= self.agents.len() {
                continue;
            }

            // Clone the profile to avoid holding a borrow on self.agents
            // while we need &mut self for execute_*_action.
            let profile = self.agents[agent_idx].profile.clone();
            let agent_id = profile.user_id;
            let agent_name = profile.name.clone();
            active_agent_ids.push(agent_id);

            // Twitter action
            if self.twitter.is_some() {
                let feed_text = self.twitter.as_ref().unwrap().format_feed_for_prompt(agent_id, 10);
                let available: Vec<&str> = vec![
                    "CREATE_POST", "LIKE_POST", "REPOST", "QUOTE_POST", "FOLLOW", "DO_NOTHING",
                ];

                let standalone = SimulatedAgent::new(profile.clone());
                match standalone.decide_action(&self.llm, "Twitter", &feed_text, &available, &sim_time, round).await {
                    Ok(decision) => {
                        let record = self.execute_twitter_action(
                            agent_id, &agent_name, &decision.action_type, &decision.action_args, round, &sim_time,
                        );
                        if record.action_type != "DO_NOTHING" {
                            twitter_actions += 1;
                        }
                        self.action_log.push(record);
                    }
                    Err(e) => {
                        tracing::warn!("Agent {} Twitter decision failed: {}", agent_name, e);
                    }
                }
            }

            // Reddit action
            if self.reddit.is_some() {
                let feed_text = self.reddit.as_ref().unwrap().format_feed_for_prompt(agent_id, 10);
                let available: Vec<&str> = vec![
                    "CREATE_POST", "CREATE_COMMENT", "LIKE_POST", "DISLIKE_POST",
                    "LIKE_COMMENT", "DISLIKE_COMMENT", "SEARCH_POSTS", "FOLLOW",
                    "MUTE", "DO_NOTHING",
                ];

                let standalone = SimulatedAgent::new(profile.clone());
                match standalone.decide_action(&self.llm, "Reddit", &feed_text, &available, &sim_time, round).await {
                    Ok(decision) => {
                        let record = self.execute_reddit_action(
                            agent_id, &agent_name, &decision.action_type, &decision.action_args, round, &sim_time,
                        );
                        if record.action_type != "DO_NOTHING" {
                            reddit_actions += 1;
                        }
                        self.action_log.push(record);
                    }
                    Err(e) => {
                        tracing::warn!("Agent {} Reddit decision failed: {}", agent_name, e);
                    }
                }
            }
        }

        let summary = RoundSummary {
            round_num: round,
            simulated_hour: sim_hour,
            twitter_actions,
            reddit_actions,
            active_agents: active_agent_ids,
        };

        self.round_summaries.push(summary.clone());
        Ok(summary)
    }

    /// Execute a Twitter action.
    fn execute_twitter_action(
        &mut self,
        agent_id: u64,
        agent_name: &str,
        action_type: &str,
        args: &Value,
        round: u64,
        sim_time: &str,
    ) -> ActionRecord {
        let tw = match self.twitter.as_mut() {
            Some(t) => t,
            None => {
                return ActionRecord {
                    round_num: round,
                    timestamp: sim_time.to_string(),
                    platform: "twitter".to_string(),
                    agent_id,
                    agent_name: agent_name.to_string(),
                    action_type: "DO_NOTHING".to_string(),
                    action_args: Value::Object(Default::default()),
                    result: Some("Platform not available".to_string()),
                    success: false,
                };
            }
        };

        let (success, result_msg) = match action_type {
            "CREATE_POST" => {
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                match tw.create_post(agent_id, content) {
                    Some(pid) => (true, format!("Created post #{}", pid)),
                    None => (false, "Failed to create post".to_string()),
                }
            }
            "LIKE_POST" => {
                let pid = args.get("post_id").and_then(|v| v.as_u64()).unwrap_or(0);
                (tw.like_post(agent_id, pid), format!("Liked post #{}", pid))
            }
            "REPOST" => {
                let pid = args.get("post_id").and_then(|v| v.as_u64()).unwrap_or(0);
                match tw.repost(agent_id, pid) {
                    Some(new_id) => (true, format!("Reposted #{} as #{}", pid, new_id)),
                    None => (false, format!("Failed to repost #{}", pid)),
                }
            }
            "QUOTE_POST" => {
                let pid = args.get("post_id").and_then(|v| v.as_u64()).unwrap_or(0);
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                match tw.quote_post(agent_id, pid, content) {
                    Some(new_id) => (true, format!("Quote-posted #{} as #{}", pid, new_id)),
                    None => (false, format!("Failed to quote post #{}", pid)),
                }
            }
            "FOLLOW" => {
                let target = args.get("target_id").and_then(|v| v.as_u64()).unwrap_or(0);
                (tw.follow(agent_id, target), format!("Followed user #{}", target))
            }
            _ => (true, "No action taken".to_string()),
        };

        ActionRecord {
            round_num: round,
            timestamp: sim_time.to_string(),
            platform: "twitter".to_string(),
            agent_id,
            agent_name: agent_name.to_string(),
            action_type: action_type.to_string(),
            action_args: args.clone(),
            result: Some(result_msg),
            success,
        }
    }

    /// Execute a Reddit action.
    fn execute_reddit_action(
        &mut self,
        agent_id: u64,
        agent_name: &str,
        action_type: &str,
        args: &Value,
        round: u64,
        sim_time: &str,
    ) -> ActionRecord {
        let rd = match self.reddit.as_mut() {
            Some(r) => r,
            None => {
                return ActionRecord {
                    round_num: round,
                    timestamp: sim_time.to_string(),
                    platform: "reddit".to_string(),
                    agent_id,
                    agent_name: agent_name.to_string(),
                    action_type: "DO_NOTHING".to_string(),
                    action_args: Value::Object(Default::default()),
                    result: Some("Platform not available".to_string()),
                    success: false,
                };
            }
        };

        let (success, result_msg) = match action_type {
            "CREATE_POST" => {
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let subreddit = args.get("subreddit").and_then(|v| v.as_str()).unwrap_or("general");
                let title = args.get("title").and_then(|v| v.as_str()).unwrap_or(content);
                match rd.create_post(agent_id, subreddit, title, content) {
                    Some(pid) => (true, format!("Created post #{}", pid)),
                    None => (false, "Failed to create post".to_string()),
                }
            }
            "CREATE_COMMENT" => {
                let pid = args.get("post_id").and_then(|v| v.as_u64()).unwrap_or(0);
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                match rd.create_comment(agent_id, pid, content, None) {
                    Some(cid) => (true, format!("Created comment #{} on post #{}", cid, pid)),
                    None => (false, format!("Failed to comment on post #{}", pid)),
                }
            }
            "LIKE_POST" => {
                let pid = args.get("post_id").and_then(|v| v.as_u64()).unwrap_or(0);
                (rd.like_post(agent_id, pid), format!("Upvoted post #{}", pid))
            }
            "DISLIKE_POST" => {
                let pid = args.get("post_id").and_then(|v| v.as_u64()).unwrap_or(0);
                (rd.dislike_post(agent_id, pid), format!("Downvoted post #{}", pid))
            }
            "LIKE_COMMENT" => {
                let cid = args.get("comment_id").and_then(|v| v.as_u64()).unwrap_or(0);
                (rd.like_comment(agent_id, cid), format!("Upvoted comment #{}", cid))
            }
            "DISLIKE_COMMENT" => {
                let cid = args.get("comment_id").and_then(|v| v.as_u64()).unwrap_or(0);
                (rd.dislike_comment(agent_id, cid), format!("Downvoted comment #{}", cid))
            }
            "FOLLOW" => {
                let target = args.get("target_id").and_then(|v| v.as_u64()).unwrap_or(0);
                (rd.follow(agent_id, target), format!("Followed user #{}", target))
            }
            "MUTE" => {
                let target = args.get("target_id").and_then(|v| v.as_u64()).unwrap_or(0);
                (rd.mute(agent_id, target), format!("Muted user #{}", target))
            }
            "SEARCH_POSTS" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let results = rd.search_posts(query, 5);
                (true, format!("Search '{}': found {} results", query, results.len()))
            }
            _ => (true, "No action taken".to_string()),
        };

        ActionRecord {
            round_num: round,
            timestamp: sim_time.to_string(),
            platform: "reddit".to_string(),
            agent_id,
            agent_name: agent_name.to_string(),
            action_type: action_type.to_string(),
            action_args: args.clone(),
            result: Some(result_msg),
            success,
        }
    }

    /// Run the full simulation for all rounds.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        self.status = EngineStatus::Running;
        self.inject_initial_events();

        tracing::info!(
            "Starting simulation: {} rounds, {} agents",
            self.total_rounds,
            self.agents.len()
        );

        while self.current_round < self.total_rounds {
            if self.status != EngineStatus::Running {
                break;
            }
            self.run_round().await?;
        }

        if self.status == EngineStatus::Running {
            self.status = EngineStatus::Completed;
            tracing::info!(
                "Simulation completed: {} rounds, {} total actions",
                self.current_round,
                self.action_log.len()
            );
        }

        Ok(())
    }

    /// Stop the simulation.
    pub fn stop(&mut self) {
        self.status = EngineStatus::Paused;
    }

    /// Save the action log to a JSONL file.
    pub fn save_action_log(&self, path: &Path) -> anyhow::Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(path)?;
        for record in &self.action_log {
            let line = serde_json::to_string(record)?;
            writeln!(file, "{}", line)?;
        }
        Ok(())
    }

    /// Get simulation statistics.
    pub fn get_stats(&self) -> Value {
        let twitter_actions = self.action_log.iter().filter(|a| a.platform == "twitter").count();
        let reddit_actions = self.action_log.iter().filter(|a| a.platform == "reddit").count();

        serde_json::json!({
            "status": format!("{:?}", self.status),
            "current_round": self.current_round,
            "total_rounds": self.total_rounds,
            "total_actions": self.action_log.len(),
            "twitter_actions": twitter_actions,
            "reddit_actions": reddit_actions,
            "agents_count": self.agents.len(),
            "twitter_posts": self.twitter.as_ref().map(|t| t.post_count()).unwrap_or(0),
            "reddit_posts": self.reddit.as_ref().map(|r| r.post_count()).unwrap_or(0),
        })
    }
}
