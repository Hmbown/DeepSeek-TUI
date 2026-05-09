//! Automatic memory extraction (#538).
//!
//! After each turn, if `auto_extract_memory` is enabled in the config, this
//! module scans the assistant's final response for decision-worthy sentences
//! and appends a 1-line note to the user's notes file.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::models::{ContentBlock, Message};

/// Scan the last assistant message for a notable memory-worthy sentence.
///
/// Heuristic: look for sentences containing trigger phrases like
/// "I will", "decided", "implemented". Returns the first such sentence
/// found, up to ~200 chars.
///
/// Returns `None` when no message matches.
pub fn extract_memory_from_response(messages: &[Message]) -> Option<String> {
    // Walk backwards: the last assistant message is most relevant.
    for msg in messages.iter().rev() {
        if msg.role != "assistant" {
            continue;
        }
        for block in &msg.content {
            if let ContentBlock::Text { text, .. } = block {
                if let Some(sentence) = scan_sentences(text) {
                    return Some(sentence);
                }
            }
        }
    }
    None
}

/// Scan through sentences in `text` looking for trigger phrases.
fn scan_sentences(text: &str) -> Option<String> {
    // Sentence boundaries: period, exclamation, question mark followed by
    // space or end-of-string. Also split on newlines.
    let sentences = split_sentences(text);
    for s in &sentences {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lowered = trimmed.to_ascii_lowercase();
        let has_trigger = lowered.contains("i will")
            || lowered.contains("i've decided")
            || lowered.contains("i decided")
            || lowered.contains("we decided")
            || lowered.contains("implemented")
            || lowered.contains("refactored")
            || lowered.contains("added ")
            || lowered.contains("created ")
            || lowered.contains("fixed ")
            || lowered.contains("changed ")
            || lowered.starts_with("decided");
        if has_trigger {
            // Cap at ~200 chars for a compact note.
            let note = if trimmed.len() > 200 {
                let end = trimmed
                    .char_indices()
                    .take(200)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(200);
                format!("{}…", &trimmed[..end])
            } else {
                trimmed.to_string()
            };
            return Some(note);
        }
    }
    None
}

/// Split text into sentences.
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?') {
            // Check if this is really a sentence end or an abbreviation.
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                sentences.push(current.trim().to_string());
                current.clear();
            }
        } else if ch == '\n' {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                sentences.push(trimmed.to_string());
                current.clear();
            }
        }
    }

    let remaining = current.trim();
    if !remaining.is_empty() {
        sentences.push(remaining.to_string());
    }

    sentences
}

/// Append a note to the notes file at `path`.
///
/// Follows the same pattern as `commands::note` — writes a `---` separator
/// and the note content, creating parent directories if needed.
pub fn append_notes_entry(notes_path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = notes_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create notes dir: {e}"))?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(notes_path)
        .map_err(|e| format!("Failed to open notes file: {e}"))?;

    writeln!(file, "\n---\n(auto) {}", content)
        .map_err(|e| format!("Failed to write note: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_decision() {
        let msg = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "I will refactor the config module first.".to_string(),
                cache_control: None,
            }],
        };
        let result = extract_memory_from_response(&[msg]);
        assert!(result.is_some());
        assert!(result.unwrap().contains("refactor the config module"));
    }

    #[test]
    fn test_extract_implemented() {
        let msg = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "Great question! I implemented the caching layer in src/cache.rs."
                    .to_string(),
                cache_control: None,
            }],
        };
        let result = extract_memory_from_response(&[msg]);
        assert!(result.is_some());
        assert!(result.unwrap().contains("implemented the caching layer"));
    }

    #[test]
    fn test_extract_decided() {
        let msg = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "After reviewing the code, I decided to use an enum-based approach."
                    .to_string(),
                cache_control: None,
            }],
        };
        let result = extract_memory_from_response(&[msg]);
        assert!(result.is_some());
        assert!(result.unwrap().contains("enum-based approach"));
    }

    #[test]
    fn test_no_triggers_returns_none() {
        let msg = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "Here is some general information about Rust.".to_string(),
                cache_control: None,
            }],
        };
        let result = extract_memory_from_response(&[msg]);
        assert!(result.is_none());
    }

    #[test]
    fn test_only_user_messages_returns_none() {
        let msg = Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "I will do the refactoring myself.".to_string(),
                cache_control: None,
            }],
        };
        let result = extract_memory_from_response(&[msg]);
        assert!(result.is_none());
    }

    #[test]
    fn test_append_notes_entry_creates_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(".deepseek").join("notes.md");
        assert!(!path.exists());

        append_notes_entry(&path, "Test memory note").unwrap();
        assert!(path.exists());

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("Test memory note"));
    }
}
