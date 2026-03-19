//! File parsing utilities.
//! Supports text extraction from PDF, Markdown, and TXT files.

use std::collections::HashSet;
use std::path::Path;

/// Errors produced by the file parser.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("File not found: {0}")]
    NotFound(String),
    #[error("Unsupported file format: {0}")]
    UnsupportedFormat(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("PDF extraction error: {0}")]
    Pdf(String),
}

/// Supported file extensions.
fn supported_extensions() -> HashSet<&'static str> {
    [".pdf", ".md", ".markdown", ".txt"].iter().copied().collect()
}

/// File parser that extracts text from PDF, Markdown, and plain-text files.
pub struct FileParser;

impl FileParser {
    /// Extract text from a single file.
    pub fn extract_text(file_path: &str) -> Result<String, ParseError> {
        let path = Path::new(file_path);

        if !path.exists() {
            return Err(ParseError::NotFound(file_path.to_string()));
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();

        if !supported_extensions().contains(ext.as_str()) {
            return Err(ParseError::UnsupportedFormat(ext));
        }

        match ext.as_str() {
            ".pdf" => Self::extract_from_pdf(file_path),
            ".md" | ".markdown" => Self::extract_from_text_file(file_path),
            ".txt" => Self::extract_from_text_file(file_path),
            _ => Err(ParseError::UnsupportedFormat(ext)),
        }
    }

    /// Extract text from a PDF file using pdf-extract.
    fn extract_from_pdf(file_path: &str) -> Result<String, ParseError> {
        let bytes = std::fs::read(file_path)?;
        pdf_extract::extract_text_from_mem(&bytes)
            .map_err(|e| ParseError::Pdf(e.to_string()))
    }

    /// Read a text file with encoding fallback.
    /// Tries UTF-8 first, then falls back to lossy decoding.
    fn extract_from_text_file(file_path: &str) -> Result<String, ParseError> {
        let data = std::fs::read(file_path)?;

        // Try UTF-8 first
        match String::from_utf8(data.clone()) {
            Ok(text) => Ok(text),
            Err(_) => {
                // Fallback: lossy UTF-8 decoding
                Ok(String::from_utf8_lossy(&data).into_owned())
            }
        }
    }

    /// Extract and merge text from multiple files.
    pub fn extract_from_multiple(file_paths: &[String]) -> String {
        let mut all_texts = Vec::new();

        for (i, file_path) in file_paths.iter().enumerate() {
            let doc_num = i + 1;
            let filename = Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path);

            match Self::extract_text(file_path) {
                Ok(text) => {
                    all_texts.push(format!("=== Document {}: {} ===\n{}", doc_num, filename, text));
                }
                Err(e) => {
                    all_texts.push(format!(
                        "=== Document {}: {} (extraction failed: {}) ===",
                        doc_num, file_path, e
                    ));
                }
            }
        }

        all_texts.join("\n\n")
    }
}

/// Split text into smaller chunks with overlap.
///
/// Tries to split at sentence boundaries (Chinese and English punctuation, newlines).
pub fn split_text_into_chunks(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();

    if total <= chunk_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    // Sentence boundary separators, ordered by preference
    let separators: &[&str] = &[
        "\u{3002}", // 。
        "\u{ff01}",   // ！
        "\u{ff1f}",   // ？
        ".\n",
        "!\n",
        "?\n",
        "\n\n",
        ". ",
        "! ",
        "? ",
    ];

    while start < total {
        let mut end = (start + chunk_size).min(total);

        // Try to split at sentence boundaries
        if end < total {
            let chunk_str: String = chars[start..end].iter().collect();
            let min_boundary = (chunk_size as f64 * 0.3) as usize;

            for sep in separators {
                if let Some(last_pos) = chunk_str.rfind(*sep) {
                    // Measure position in chars, not bytes
                    let char_pos = chunk_str[..last_pos].chars().count();
                    if char_pos > min_boundary {
                        end = start + char_pos + (*sep).chars().count();
                        break;
                    }
                }
            }
        }

        let chunk: String = chars[start..end].iter().collect();
        let trimmed = chunk.trim().to_string();
        if !trimmed.is_empty() {
            chunks.push(trimmed);
        }

        // Next chunk starts at the overlap position
        if end < total {
            start = if end > overlap { end - overlap } else { end };
        } else {
            break;
        }
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_short_text() {
        let chunks = split_text_into_chunks("Hello world", 100, 10);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world");
    }

    #[test]
    fn test_split_empty_text() {
        let chunks = split_text_into_chunks("", 100, 10);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_split_produces_chunks() {
        let text = "A".repeat(1000);
        let chunks = split_text_into_chunks(&text, 200, 20);
        assert!(chunks.len() > 1);
    }
}
