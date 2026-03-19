//! Task model — in-memory storage with Mutex for thread safety.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use uuid::Uuid;

/// Task status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

/// Task data model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub task_type: String,
    pub status: TaskStatus,
    pub created_at: String,
    pub updated_at: String,
    pub progress: u8,
    pub message: String,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub metadata: Value,
    pub progress_detail: Value,
}

impl Task {
    /// Create a new task.
    pub fn new(task_type: &str, metadata: Option<Value>) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            task_id: Uuid::new_v4().to_string(),
            task_type: task_type.to_string(),
            status: TaskStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            progress: 0,
            message: String::new(),
            result: None,
            error: None,
            metadata: metadata.unwrap_or(Value::Object(Default::default())),
            progress_detail: Value::Object(Default::default()),
        }
    }
}

/// Thread-safe task manager (singleton).
#[derive(Debug, Clone)]
pub struct TaskManager {
    tasks: Arc<Mutex<HashMap<String, Task>>>,
}

/// Global singleton.
static TASK_MANAGER: OnceLock<TaskManager> = OnceLock::new();

impl TaskManager {
    /// Get the global task manager instance.
    pub fn global() -> &'static TaskManager {
        TASK_MANAGER.get_or_init(|| TaskManager {
            tasks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Create a new instance (for testing or isolated use).
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new task and return its ID.
    pub fn create_task(&self, task_type: &str, metadata: Option<Value>) -> String {
        let task = Task::new(task_type, metadata);
        let task_id = task.task_id.clone();
        let mut tasks = self.tasks.lock().expect("task lock poisoned");
        tasks.insert(task_id.clone(), task);
        task_id
    }

    /// Get a task by ID.
    pub fn get_task(&self, task_id: &str) -> Option<Task> {
        let tasks = self.tasks.lock().expect("task lock poisoned");
        tasks.get(task_id).cloned()
    }

    /// Update a task's fields.
    pub fn update_task(
        &self,
        task_id: &str,
        status: Option<TaskStatus>,
        progress: Option<u8>,
        message: Option<String>,
        result: Option<Value>,
        error: Option<String>,
        progress_detail: Option<Value>,
    ) {
        let mut tasks = self.tasks.lock().expect("task lock poisoned");
        if let Some(task) = tasks.get_mut(task_id) {
            task.updated_at = Utc::now().to_rfc3339();
            if let Some(s) = status {
                task.status = s;
            }
            if let Some(p) = progress {
                task.progress = p;
            }
            if let Some(m) = message {
                task.message = m;
            }
            if let Some(r) = result {
                task.result = Some(r);
            }
            if let Some(e) = error {
                task.error = Some(e);
            }
            if let Some(pd) = progress_detail {
                task.progress_detail = pd;
            }
        }
    }

    /// Mark a task as completed.
    pub fn complete_task(&self, task_id: &str, result: Value) {
        self.update_task(
            task_id,
            Some(TaskStatus::Completed),
            Some(100),
            Some("Task completed".to_string()),
            Some(result),
            None,
            None,
        );
    }

    /// Mark a task as failed.
    pub fn fail_task(&self, task_id: &str, error: &str) {
        self.update_task(
            task_id,
            Some(TaskStatus::Failed),
            None,
            Some("Task failed".to_string()),
            None,
            Some(error.to_string()),
            None,
        );
    }

    /// List tasks, optionally filtered by type.
    pub fn list_tasks(&self, task_type: Option<&str>) -> Vec<Task> {
        let tasks = self.tasks.lock().expect("task lock poisoned");
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| task_type.map_or(true, |tt| t.task_type == tt))
            .cloned()
            .collect();
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        result
    }

    /// Remove completed/failed tasks older than `max_age_hours`.
    pub fn cleanup_old_tasks(&self, max_age_hours: i64) {
        let cutoff = Utc::now() - chrono::Duration::hours(max_age_hours);
        let cutoff_str = cutoff.to_rfc3339();
        let mut tasks = self.tasks.lock().expect("task lock poisoned");
        tasks.retain(|_, task| {
            if task.status == TaskStatus::Completed || task.status == TaskStatus::Failed {
                task.created_at > cutoff_str
            } else {
                true
            }
        });
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}
