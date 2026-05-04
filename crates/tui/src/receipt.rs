//! Session receipt ingestion (#545).
//!
//! At session end, when `ingest_session_receipts` is enabled in config, this
//! module extracts key metadata from the transcript and appends a structured
//! receipt note (tagged `session_receipt`) to the notes file.
//!
//! The receipt is metadata-driven: session id, model, message count, token
//! usage, tool-call frequency, and workspace. No LLM call — the extraction
//! is purely structural so session-end teardown stays fast and reliable.

use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::Utc;

use crate::models::{ContentBlock, Message};

/// Build a structured session receipt from the conversation metadata and
/// messages, then append it to the notes file at `notes_path`.
///
/// The receipt is formatted as a Markdown block under a `## session_receipt`
/// heading, which serves as the stable tag for downstream aggregation.
pub fn write_session_receipt(
    notes_path: &Path,
    session_id: &str,
    model: &str,
    messages: &[Message],
    total_tokens: u64,
) {
    let receipt = build_receipt(session_id, model, messages, total_tokens);
    if let Err(e) = append_to_notes(notes_path, &receipt) {
        tracing::warn!(
            target: "receipt",
            ?e,
            session = session_id,
            "failed to write session receipt"
        );
    }
}

/// Build the receipt text (no I/O).
fn build_receipt(
    session_id: &str,
    model: &str,
    messages: &[Message],
    total_tokens: u64,
) -> String {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let short_id = &session_id[..session_id.len().min(8)];
    let msg_count = messages.len();

    // Count tool calls by name.
    let mut tool_counts: Vec<(&str, usize)> = Vec::new();
    for msg in messages {
        if msg.role == "assistant" {
            for block in &msg.content {
                if let ContentBlock::ToolUse { name, .. } = block {
                    if let Some(entry) = tool_counts.iter_mut().find(|(n, _)| *n == name.as_str()) {
                        entry.1 += 1;
                    } else {
                        tool_counts.push((name.as_str(), 1));
                    }
                }
            }
        }
    }
    tool_counts.sort_by(|a, b| b.1.cmp(&a.1));

    // Extract session title from first user message.
    let title = messages
        .iter()
        .find(|m| m.role == "user")
        .and_then(|m| {
            m.content.iter().find_map(|block| match block {
                ContentBlock::Text { text, .. } => {
                    let line = text.lines().next().unwrap_or(text);
                    let trimmed = line.trim();
                    if trimmed.len() > 60 {
                        let mut s: String = trimmed.chars().take(57).collect();
                        s.push_str("...");
                        Some(s)
                    } else {
                        Some(trimmed.to_string())
                    }
                }
                _ => None,
            })
        })
        .unwrap_or_default();

    let mut receipt = String::new();
    receipt.push_str("## session_receipt\n\n");
    receipt.push_str(&format!("- **{now}** | `{short_id}` | `{model}`\n"));
    if !title.is_empty() {
        receipt.push_str(&format!("  - summary: {title}\n"));
    }
    receipt.push_str(&format!("  - msgs: {msg_count} | tokens: {total_tokens}\n"));
    if !tool_counts.is_empty() {
        let tools_str: Vec<String> = tool_counts
            .iter()
            .map(|(name, count)| format!("{name}({count})"))
            .collect();
        receipt.push_str(&format!("  - tools: {}\n", tools_str.join(", ")));
    }

    receipt
}

/// Append a receipt block to the notes file, creating the parent directory
/// and file if they don't exist. Uses the same `---` separator convention
/// as the `/note` command and `NoteTool`.
fn append_to_notes(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;

    writeln!(file, "\n---\n{}", content.trim())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ContentBlock;
    use tempfile::tempdir;

    fn text_msg(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
        }
    }

    fn tool_use_msg(tool_name: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: tool_name.to_string(),
                input: serde_json::json!({}),
                caller: None,
            }],
        }
    }

    #[test]
    fn build_receipt_includes_session_id_and_model() {
        // Build a receipt with minimum data so the output is
        // deterministic enough to check for key strings.
        let msgs = vec![text_msg("user", "hello")];
        let receipt = build_receipt("abc-123-test", "deepseek-v4-flash", &msgs, 42);
        assert!(receipt.contains("abc-123"));
        assert!(receipt.contains("deepseek-v4-flash"));
        assert!(receipt.contains("42"));
        assert!(receipt.contains("msgs: 1"));
    }

    #[test]
    fn build_receipt_counts_tool_calls() {
        let msgs = vec![
            text_msg("user", "do stuff"),
            tool_use_msg("exec_shell"),
            tool_use_msg("read_file"),
            tool_use_msg("exec_shell"),
        ];
        let receipt = build_receipt("test-id", "m", &msgs, 0);
        assert!(receipt.contains("exec_shell(2)"));
        assert!(receipt.contains("read_file(1)"));
    }

    #[test]
    fn build_receipt_extracts_title() {
        let msgs = vec![text_msg("user", "Refactor the auth module")];
        let receipt = build_receipt("x", "m", &msgs, 0);
        assert!(receipt.contains("Refactor the auth module"));
    }

    #[test]
    fn append_to_notes_creates_file_when_missing() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("notes.md");
        assert!(!path.exists());
        append_to_notes(&path, "## session_receipt\n- test receipt").unwrap();
        assert!(path.exists());
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("test receipt"));
    }

    #[test]
    fn append_to_notes_appends_multiple_receipts() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("notes.md");
        append_to_notes(&path, "first").unwrap();
        append_to_notes(&path, "second").unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("first"));
        assert!(contents.contains("second"));
    }

    #[test]
    fn write_session_receipt_integration() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("notes.md");
        let msgs = vec![
            text_msg("user", "Add CI pipeline"),
            tool_use_msg("write_file"),
            tool_use_msg("exec_shell"),
        ];
        write_session_receipt(&path, "sess-001", "deepseek-v4-pro", &msgs, 128);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("## session_receipt"));
        assert!(contents.contains("sess-001"));
        assert!(contents.contains("deepseek-v4-pro"));
        assert!(contents.contains("128"));
        assert!(contents.contains("msgs: 3"));
        assert!(contents.contains("write_file(1)"));
        assert!(contents.contains("exec_shell(1)"));
    }
}
