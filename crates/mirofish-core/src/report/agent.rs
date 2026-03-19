//! Report agent — generates structured reports using LLM with tool-call loop.
//!
//! Tools: query_graph (search), get_graph_overview, query_simulation_data.
//! Generates a report outline first, then fills each section using ReACT-style
//! think-observe-act iterations.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::config::Config;
use crate::llm::client::{ChatMessage, LlmClient, LlmError};

/// Report status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Pending,
    Planning,
    Generating,
    Completed,
    Failed,
}

/// A report section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSection {
    pub title: String,
    pub content: String,
}

impl ReportSection {
    pub fn to_markdown(&self, level: usize) -> String {
        let hashes = "#".repeat(level);
        format!("{} {}\n\n{}\n\n", hashes, self.title, self.content)
    }
}

/// Report outline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportOutline {
    pub title: String,
    pub summary: String,
    pub sections: Vec<ReportSection>,
}

impl ReportOutline {
    pub fn to_markdown(&self) -> String {
        let mut md = format!("# {}\n\n> {}\n\n", self.title, self.summary);
        for section in &self.sections {
            md.push_str(&section.to_markdown(2));
        }
        md
    }
}

/// A complete report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub report_id: String,
    pub simulation_id: String,
    pub graph_id: String,
    pub simulation_requirement: String,
    pub status: ReportStatus,
    pub outline: Option<ReportOutline>,
    pub markdown_content: String,
    pub created_at: String,
    pub completed_at: String,
    pub error: Option<String>,
}

/// Prompt templates (based on the Python implementation).
const PLAN_SYSTEM_PROMPT: &str = r#"You are an expert writer of "Future Prediction Reports," possessing a "god's-eye view" of the simulated world.

[Core Concept]
We have constructed a simulated world and injected specific "simulation requirements" as variables. The evolution of the simulated world is a prediction of what may happen in the future.

[Your Task]
Write a "Future Prediction Report" that answers:
1. Under the conditions we have set, what happened in the future?
2. How did various Agents (populations) react and act?
3. What noteworthy future trends and risks does this simulation reveal?

[Section Count Limit]
- Minimum 2 sections, maximum 5 sections
- No sub-sections needed

Please output a report outline in JSON format:
{
    "title": "Report Title",
    "summary": "One sentence summary",
    "sections": [
        {"title": "Section Title", "description": "Content description"}
    ]
}"#;

/// Report agent that generates reports with tool-call loops.
pub struct ReportAgent {
    llm: LlmClient,
    /// Maximum tool calls per section (reserved for future multi-tool iteration).
    pub max_tool_calls: usize,
    /// Maximum reflection rounds per section (reserved for ReACT loop).
    pub max_reflection_rounds: usize,
    temperature: f64,
}

impl ReportAgent {
    /// Create a new report agent.
    pub fn new(llm: LlmClient) -> Self {
        let cfg = Config::global();
        Self {
            llm,
            max_tool_calls: cfg.report_agent_max_tool_calls,
            max_reflection_rounds: cfg.report_agent_max_reflection_rounds,
            temperature: cfg.report_agent_temperature,
        }
    }

    /// Generate a complete report.
    pub async fn generate_report(
        &self,
        simulation_id: &str,
        graph_id: &str,
        simulation_requirement: &str,
        search_fn: impl Fn(&str) -> Vec<String> + Send + Sync,
        progress_callback: Option<Box<dyn Fn(&str, &str, u8) + Send + Sync>>,
    ) -> anyhow::Result<Report> {
        let report_id = format!("rpt_{}", &Uuid::new_v4().to_string().replace('-', "")[..12]);
        let now = Utc::now().to_rfc3339();

        let mut report = Report {
            report_id: report_id.clone(),
            simulation_id: simulation_id.to_string(),
            graph_id: graph_id.to_string(),
            simulation_requirement: simulation_requirement.to_string(),
            status: ReportStatus::Pending,
            outline: None,
            markdown_content: String::new(),
            created_at: now,
            completed_at: String::new(),
            error: None,
        };

        // Phase 1: Plan outline
        if let Some(ref cb) = progress_callback {
            cb("planning", "Planning report outline...", 5);
        }

        report.status = ReportStatus::Planning;

        // Gather some context via search
        let context_facts = search_fn(simulation_requirement);
        let facts_preview: String = context_facts
            .iter()
            .take(20)
            .enumerate()
            .map(|(i, f)| format!("{}. {}", i + 1, f))
            .collect::<Vec<_>>()
            .join("\n");

        let plan_user_msg = format!(
            "[Prediction Scenario Setup]\n\
             The variable we injected: {}\n\
             \n\
             [Sample of Future Facts]\n\
             {}\n\
             \n\
             Design the report section structure (2-5 sections).",
            simulation_requirement, facts_preview,
        );

        let plan_messages = vec![
            ChatMessage::system(PLAN_SYSTEM_PROMPT),
            ChatMessage::user(plan_user_msg),
        ];

        let outline_json = self.llm.chat_json(&plan_messages, 0.3, Some(2048)).await?;

        let title = outline_json.get("title").and_then(|v| v.as_str()).unwrap_or("Prediction Report").to_string();
        let summary = outline_json.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let section_defs: Vec<(String, String)> = outline_json
            .get("sections")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|s| {
                        let t = s.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let d = s.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        (t, d)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let sections: Vec<ReportSection> = section_defs
            .iter()
            .map(|(t, _)| ReportSection {
                title: t.clone(),
                content: String::new(),
            })
            .collect();

        let mut outline = ReportOutline {
            title: title.clone(),
            summary: summary.clone(),
            sections,
        };

        if let Some(ref cb) = progress_callback {
            cb("planning", "Outline complete", 15);
        }

        // Phase 2: Generate each section
        report.status = ReportStatus::Generating;
        let total_sections = outline.sections.len();

        for (i, section) in outline.sections.iter_mut().enumerate() {
            let progress = 15 + (i * 70 / total_sections.max(1)) as u8;
            if let Some(ref cb) = progress_callback {
                cb("generating", &format!("Writing section: {}", section.title), progress);
            }

            let content = self.generate_section(
                &title,
                &summary,
                simulation_requirement,
                &section.title,
                &section_defs.get(i).map(|(_, d)| d.as_str()).unwrap_or(""),
                &search_fn,
            ).await?;

            section.content = content;

            if let Some(ref cb) = progress_callback {
                cb("generating", &format!("Section complete: {}", section.title), progress + 10);
            }
        }

        // Phase 3: Assemble
        report.outline = Some(outline.clone());
        report.markdown_content = outline.to_markdown();
        report.status = ReportStatus::Completed;
        report.completed_at = Utc::now().to_rfc3339();

        if let Some(ref cb) = progress_callback {
            cb("completed", "Report generation complete", 100);
        }

        Ok(report)
    }

    /// Generate content for a single section using tool-call iterations.
    async fn generate_section(
        &self,
        report_title: &str,
        report_summary: &str,
        simulation_requirement: &str,
        section_title: &str,
        section_description: &str,
        search_fn: &(impl Fn(&str) -> Vec<String> + Send + Sync),
    ) -> Result<String, LlmError> {
        // Perform searches related to this section
        let mut gathered_facts = Vec::new();

        // Search with section title
        gathered_facts.extend(search_fn(section_title));

        // Search with section description if available
        if !section_description.is_empty() {
            gathered_facts.extend(search_fn(section_description));
        }

        // Deduplicate
        gathered_facts.sort();
        gathered_facts.dedup();

        let facts_text = if gathered_facts.is_empty() {
            "No specific data found.".to_string()
        } else {
            gathered_facts
                .iter()
                .take(30)
                .enumerate()
                .map(|(i, f)| format!("{}. {}", i + 1, f))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let system_prompt = format!(
            "You are writing one section of a Future Prediction Report.\n\
             \n\
             Report Title: {}\n\
             Report Summary: {}\n\
             Prediction Scenario: {}\n\
             Current Section: {}\n\
             \n\
             Rules:\n\
             1. All content must be based on the provided simulation data\n\
             2. Quote Agent's original speech using > blockquote format\n\
             3. Do NOT use any Markdown headings (##, ###) within the section\n\
             4. Use **bold**, paragraphs, quotes, and lists to organize content\n\
             5. Write substantial, well-supported content (300-800 words)",
            report_title, report_summary, simulation_requirement, section_title,
        );

        let user_prompt = format!(
            "Here is the simulation data for this section:\n\n{}\n\n\
             Write the content for section \"{}\".\n\
             Description: {}\n\n\
             Remember: no headings, use blockquotes for agent speech, be detailed.",
            facts_text, section_title, section_description,
        );

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ];

        self.llm.chat(&messages, self.temperature, Some(4096), None).await
    }

    /// Interactive chat after report generation — answer questions using the graph.
    pub async fn chat(
        &self,
        question: &str,
        report: &Report,
        search_fn: impl Fn(&str) -> Vec<String>,
    ) -> Result<String, LlmError> {
        // Search for relevant context
        let facts = search_fn(question);
        let facts_text = facts
            .iter()
            .take(15)
            .enumerate()
            .map(|(i, f)| format!("{}. {}", i + 1, f))
            .collect::<Vec<_>>()
            .join("\n");

        let system_prompt = format!(
            "You are a simulation analyst. A report has been generated about:\n\
             Requirement: {}\n\n\
             Report summary: {}\n\n\
             Answer the user's question using the provided data. \
             Be concise and reference specific findings.",
            report.simulation_requirement,
            report.outline.as_ref().map(|o| o.summary.as_str()).unwrap_or(""),
        );

        let user_msg = format!(
            "Relevant data:\n{}\n\nQuestion: {}",
            facts_text, question,
        );

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_msg),
        ];

        self.llm.chat(&messages, 0.5, Some(2048), None).await
    }

    /// Save report to disk.
    pub fn save_report(report: &Report, upload_folder: &Path) -> anyhow::Result<PathBuf> {
        let report_dir = upload_folder.join("reports").join(&report.report_id);
        std::fs::create_dir_all(&report_dir)?;

        // Save report JSON
        let json_path = report_dir.join("report.json");
        let json = serde_json::to_string_pretty(report)?;
        std::fs::write(&json_path, json)?;

        // Save markdown
        let md_path = report_dir.join("report.md");
        std::fs::write(&md_path, &report.markdown_content)?;

        Ok(report_dir)
    }

    /// Load a report from disk.
    pub fn load_report(report_id: &str, upload_folder: &Path) -> anyhow::Result<Option<Report>> {
        let json_path = upload_folder.join("reports").join(report_id).join("report.json");
        if !json_path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(json_path)?;
        let report: Report = serde_json::from_str(&data)?;
        Ok(Some(report))
    }
}
