use serde_json::Value;
use std::sync::LazyLock;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

/// Maximum number of concurrent claude CLI invocations.
static SEMAPHORE: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(4));

/// Map OpenAI / shorthand model names to Claude CLI model identifiers.
pub fn resolve_model(model: &str) -> &str {
    match model {
        "gpt-4o" | "gpt-4o-mini" => "sonnet",
        "gpt-4" | "gpt-4-turbo" => "opus",
        "gpt-3.5-turbo" => "haiku",
        "sonnet" | "opus" | "haiku" => model,
        other if other.contains("claude") => other,
        _ => "sonnet",
    }
}

/// Run a chat completion through the `claude` CLI.
///
/// * `messages` – OpenAI-style message array (objects with `role` and `content`).
/// * `model` – Model name (will be resolved via [`resolve_model`]).
/// * `json_mode` – When `true`, instruct the model to respond with valid JSON only.
///
/// Returns the assistant's text response.
pub async fn chat_completion(
    messages: &[Value],
    model: &str,
    json_mode: bool,
) -> Result<String, String> {
    let resolved_model = resolve_model(model);

    // Split messages into system prompt and user/assistant conversation.
    let mut system_parts: Vec<String> = Vec::new();
    let mut conversation_parts: Vec<String> = Vec::new();

    for msg in messages {
        let role = msg["role"].as_str().unwrap_or("user");
        let content = msg["content"].as_str().unwrap_or("");
        match role {
            "system" => {
                system_parts.push(content.to_string());
            }
            "assistant" => {
                conversation_parts.push(format!("[assistant]: {content}"));
            }
            _ => {
                // "user" and anything else
                conversation_parts.push(content.to_string());
            }
        }
    }

    let mut prompt = conversation_parts.join("\n");

    if json_mode {
        prompt.push_str("\n\nIMPORTANT: You must respond with valid JSON only.");
    }

    // Build the CLI command.
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg(&prompt)
        .arg("--model")
        .arg(resolved_model)
        .arg("--output-format")
        .arg("json")
        .arg("--tools")
        .arg("");

    if !system_parts.is_empty() {
        let system_prompt = system_parts.join("\n");
        cmd.arg("-s").arg(&system_prompt);
    }

    info!(
        model = resolved_model,
        prompt_len = prompt.len(),
        "Invoking claude CLI"
    );

    // Acquire a permit to limit concurrency.
    let _permit = SEMAPHORE
        .acquire()
        .await
        .map_err(|e| format!("Semaphore error: {e}"))?;

    let output = cmd.output().await.map_err(|e| {
        error!(error = %e, "Failed to spawn claude CLI process");
        format!("Failed to execute claude CLI: {e}")
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(
            status = ?output.status,
            stderr = %stderr,
            "claude CLI exited with non-zero status"
        );
        return Err(format!(
            "claude CLI failed (status {:?}): {}",
            output.status.code(),
            stderr
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    debug!(stdout_len = stdout.len(), "claude CLI raw output received");

    // The CLI returns JSON with a "result" field containing the response text.
    let parsed: Value = serde_json::from_str(&stdout).map_err(|e| {
        warn!(
            error = %e,
            raw = %stdout,
            "Failed to parse claude CLI JSON output, using raw stdout"
        );
        format!("Failed to parse claude CLI output as JSON: {e}")
    })?;

    let result_text = parsed["result"]
        .as_str()
        .unwrap_or_else(|| {
            warn!("No 'result' field in claude CLI JSON output, falling back to raw stdout");
            stdout.as_ref()
        })
        .to_string();

    // Strip markdown code block markers if present.
    let cleaned = strip_code_block_markers(&result_text);
    debug!(result_len = cleaned.len(), "claude CLI result cleaned");

    Ok(cleaned)
}

/// Remove leading/trailing markdown code block fences (``` or ```json etc.).
fn strip_code_block_markers(text: &str) -> String {
    let trimmed = text.trim();

    // Check for ```...``` wrapping.
    if trimmed.starts_with("```") && trimmed.ends_with("```") {
        // Remove opening fence (first line) and closing fence.
        let without_opening = if let Some(pos) = trimmed.find('\n') {
            &trimmed[pos + 1..]
        } else {
            // Single-line code block – unusual but handle it.
            trimmed.trim_start_matches("```")
        };

        let without_closing = without_opening
            .trim_end()
            .strip_suffix("```")
            .unwrap_or(without_opening);

        return without_closing.trim().to_string();
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_model_gpt_mappings() {
        assert_eq!(resolve_model("gpt-4o"), "sonnet");
        assert_eq!(resolve_model("gpt-4o-mini"), "sonnet");
        assert_eq!(resolve_model("gpt-4"), "opus");
        assert_eq!(resolve_model("gpt-4-turbo"), "opus");
        assert_eq!(resolve_model("gpt-3.5-turbo"), "haiku");
    }

    #[test]
    fn test_resolve_model_passthrough() {
        assert_eq!(resolve_model("sonnet"), "sonnet");
        assert_eq!(resolve_model("opus"), "opus");
        assert_eq!(resolve_model("haiku"), "haiku");
        assert_eq!(resolve_model("claude-sonnet-4-6"), "claude-sonnet-4-6");
        assert_eq!(resolve_model("claude-opus-4-6"), "claude-opus-4-6");
    }

    #[test]
    fn test_resolve_model_default() {
        assert_eq!(resolve_model("unknown-model"), "sonnet");
        assert_eq!(resolve_model(""), "sonnet");
    }

    #[test]
    fn test_strip_code_block_markers() {
        assert_eq!(
            strip_code_block_markers("```json\n{\"a\": 1}\n```"),
            "{\"a\": 1}"
        );
        assert_eq!(
            strip_code_block_markers("```\nhello\n```"),
            "hello"
        );
        assert_eq!(strip_code_block_markers("no blocks"), "no blocks");
        assert_eq!(strip_code_block_markers("  hello  "), "hello");
    }
}
