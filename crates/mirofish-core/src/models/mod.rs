//! Data models — projects and tasks.

pub mod project;
pub mod task;

pub use project::{Project, ProjectManager, ProjectStatus};
pub use task::{Task, TaskManager, TaskStatus};
