//! Utility helpers shared across the `DeepSeek` CLI.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::{ContentBlock, Message};
use anyhow::{Context, Result};
use serde_json::Value;

// === Filesystem Helpers ===

#[allow(dead_code)]
pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("Failed to create directory: {}", path.display()))
}

#[allow(dead_code)]
pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(path, bytes).with_context(|| format!("Failed to write {}", path.display()))
}

/// Create a timestamped filename for generated assets.
#[must_use]
#[allow(dead_code)]
pub fn timestamped_filename(prefix: &str, extension: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{prefix}_{now}.{extension}")
}

/// Render JSON with pretty formatting, falling back to a compact string on error.
#[must_use]
#[allow(dead_code)]
pub fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// Extract a lowercase file extension from a URL, if present.
#[must_use]
#[allow(dead_code)]
pub fn extension_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let ext = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_lowercase);
    ext.filter(|e| !e.is_empty())
}

/// Build an output path within the given directory.
#[must_use]
#[allow(dead_code)]
pub fn output_path(output_dir: &Path, filename: &str) -> PathBuf {
    output_dir.join(filename)
}

/// Truncate a string to a maximum length, adding an ellipsis if truncated
#[must_use]
pub fn truncate_with_ellipsis(s: &str, max_len: usize, ellipsis: &str) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncate_at = max_len.saturating_sub(ellipsis.len());
        format!("{}{}", &s[..truncate_at], ellipsis)
    }
}

/// Estimate the total character count across message content blocks.
#[must_use]
pub fn estimate_message_chars(messages: &[Message]) -> usize {
    let mut total = 0;
    for msg in messages {
        for block in &msg.content {
            match block {
                ContentBlock::Text { text, .. } => total += text.len(),
                ContentBlock::Thinking { thinking } => total += thinking.len(),
                ContentBlock::ToolUse { input, .. } => total += input.to_string().len(),
                ContentBlock::ToolResult { content, .. } => total += content.len(),
            }
        }
    }
    total
}
