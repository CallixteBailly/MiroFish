//! Social media simulation engine.
//!
//! Pure-Rust reimplementation of the OASIS social media simulation,
//! supporting Twitter and Reddit platform models.

pub mod actions;
pub mod agent;
pub mod config_generator;
pub mod engine;
pub mod manager;
pub mod platforms;
pub mod profile_generator;

pub use actions::{RedditAction, TwitterAction};
pub use agent::SimulatedAgent;
pub use engine::SimulationEngine;
pub use manager::{SimulationManager, SimulationState, SimulationStatus};
