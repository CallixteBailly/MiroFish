//! Text processing service — chunking, extraction, and preprocessing.

use regex::Regex;
use std::sync::OnceLock;

use super::file_parser::{FileParser, split_text_into_chunks};

/// Text processor providing extraction, splitting, and preprocessing.
pub struct TextProcessor;

/// Compiled regex for collapsing multiple blank lines.
fn blank_lines_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\n{3,}").expect("invalid regex"))
}

impl TextProcessor {
    /// Extract text from multiple files and merge them.
    pub fn extract_from_files(file_paths: &[String]) -> String {
        FileParser::extract_from_multiple(file_paths)
    }

    /// Split text into chunks with overlap.
    pub fn split_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
        split_text_into_chunks(text, chunk_size, overlap)
    }

    /// Preprocess text:
    /// - Normalize line endings
    /// - Remove excessive blank lines
    /// - Strip leading/trailing whitespace from each line
    pub fn preprocess_text(text: &str) -> String {
        // Normalize line endings
        let text = text.replace("\r\n", "\n").replace('\r', "\n");

        // Remove consecutive blank lines (keep at most two newlines)
        let text = blank_lines_re().replace_all(&text, "\n\n").to_string();

        // Strip leading/trailing whitespace from each line
        let lines: Vec<&str> = text.lines().map(|l| l.trim()).collect();
        lines.join("\n").trim().to_string()
    }

    /// Get text statistics.
    pub fn get_text_stats(text: &str) -> TextStats {
        TextStats {
            total_chars: text.chars().count(),
            total_lines: text.lines().count(),
            total_words: text.split_whitespace().count(),
        }
    }
}

/// Simple text statistics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TextStats {
    pub total_chars: usize,
    pub total_lines: usize,
    pub total_words: usize,
}
