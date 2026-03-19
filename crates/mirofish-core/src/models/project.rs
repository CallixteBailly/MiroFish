//! Project model with JSON file-based storage.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::config::Config;

/// Project status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Created,
    OntologyGenerated,
    GraphBuilding,
    GraphCompleted,
    Failed,
}

/// Project data model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    pub name: String,
    pub status: ProjectStatus,
    pub created_at: String,
    pub updated_at: String,

    // File information: [{original_filename, saved_filename, path, size}]
    #[serde(default)]
    pub files: Vec<Value>,
    #[serde(default)]
    pub total_text_length: usize,

    // Ontology information (populated after generation)
    pub ontology: Option<Value>,
    pub analysis_summary: Option<String>,

    // Graph information (populated after graph build)
    pub graph_id: Option<String>,
    pub graph_build_task_id: Option<String>,

    // Configuration
    pub simulation_requirement: Option<String>,
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,
    #[serde(default = "default_chunk_overlap")]
    pub chunk_overlap: usize,

    // Error information
    pub error: Option<String>,
}

fn default_chunk_size() -> usize { 500 }
fn default_chunk_overlap() -> usize { 50 }

impl Project {
    /// Create a new project with default values.
    pub fn new(name: &str) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            project_id: format!("proj_{}", &Uuid::new_v4().to_string().replace('-', "")[..12]),
            name: name.to_string(),
            status: ProjectStatus::Created,
            created_at: now.clone(),
            updated_at: now,
            files: Vec::new(),
            total_text_length: 0,
            ontology: None,
            analysis_summary: None,
            graph_id: None,
            graph_build_task_id: None,
            simulation_requirement: None,
            chunk_size: 500,
            chunk_overlap: 50,
            error: None,
        }
    }
}

/// Project manager — responsible for project persistence and retrieval.
/// Uses JSON files stored in `uploads/projects/<project_id>/project.json`.
pub struct ProjectManager {
    projects_dir: PathBuf,
}

impl ProjectManager {
    /// Create a new ProjectManager using a specific base directory.
    pub fn new(upload_folder: &Path) -> Self {
        let projects_dir = upload_folder.join("projects");
        fs::create_dir_all(&projects_dir).ok();
        Self { projects_dir }
    }

    /// Create from the global config.
    pub fn from_global_config() -> Self {
        Self::new(&Config::global().upload_folder)
    }

    fn project_dir(&self, project_id: &str) -> PathBuf {
        self.projects_dir.join(project_id)
    }

    fn meta_path(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("project.json")
    }

    fn files_dir(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("files")
    }

    fn text_path(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("extracted_text.txt")
    }

    /// Create a new project.
    pub fn create_project(&self, name: &str) -> anyhow::Result<Project> {
        let project = Project::new(name);
        let project_dir = self.project_dir(&project.project_id);
        let files_dir = self.files_dir(&project.project_id);
        fs::create_dir_all(&project_dir)?;
        fs::create_dir_all(&files_dir)?;
        self.save_project(&project)?;
        Ok(project)
    }

    /// Save project metadata to disk.
    pub fn save_project(&self, project: &Project) -> anyhow::Result<()> {
        let meta_path = self.meta_path(&project.project_id);
        let json = serde_json::to_string_pretty(project)?;
        fs::write(meta_path, json)?;
        Ok(())
    }

    /// Load a project by ID.
    pub fn get_project(&self, project_id: &str) -> anyhow::Result<Option<Project>> {
        let meta_path = self.meta_path(project_id);
        if !meta_path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(meta_path)?;
        let project: Project = serde_json::from_str(&data)?;
        Ok(Some(project))
    }

    /// List all projects, sorted by creation time descending.
    pub fn list_projects(&self, limit: usize) -> anyhow::Result<Vec<Project>> {
        fs::create_dir_all(&self.projects_dir)?;

        let mut projects = Vec::new();
        for entry in fs::read_dir(&self.projects_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let project_id = entry.file_name().to_string_lossy().to_string();
                if let Ok(Some(p)) = self.get_project(&project_id) {
                    projects.push(p);
                }
            }
        }

        // Sort by created_at descending
        projects.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        projects.truncate(limit);
        Ok(projects)
    }

    /// Delete a project and all its files.
    pub fn delete_project(&self, project_id: &str) -> anyhow::Result<bool> {
        let dir = self.project_dir(project_id);
        if !dir.exists() {
            return Ok(false);
        }
        fs::remove_dir_all(dir)?;
        Ok(true)
    }

    /// Save extracted text for a project.
    pub fn save_extracted_text(&self, project_id: &str, text: &str) -> anyhow::Result<()> {
        let path = self.text_path(project_id);
        fs::write(path, text)?;
        Ok(())
    }

    /// Get extracted text for a project.
    pub fn get_extracted_text(&self, project_id: &str) -> anyhow::Result<Option<String>> {
        let path = self.text_path(project_id);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(fs::read_to_string(path)?))
    }

    /// Get all file paths for a project.
    pub fn get_project_files(&self, project_id: &str) -> anyhow::Result<Vec<PathBuf>> {
        let dir = self.files_dir(project_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                paths.push(entry.path());
            }
        }
        Ok(paths)
    }

    /// Get the files directory for uploading.
    pub fn get_files_dir(&self, project_id: &str) -> PathBuf {
        self.files_dir(project_id)
    }
}
