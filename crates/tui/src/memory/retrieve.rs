//! Memory retrieval and session-start injection.
//!
//! Loads memories from disk at session start and provides functions for
//! injecting a concise memory block into the system prompt.

use std::path::Path;

use super::store::{self, Memory};

/// Maximum number of project memories to auto-inject at session start.
const MAX_INJECT_MEMORIES: usize = 10;

/// Load and rank memories for injection into the session start prompt.
///
/// Returns the top N project memories (most recent first), optionally
/// including global memories.
pub fn load_for_injection(
    workspace: &Path,
    include_global: bool,
    max: Option<usize>,
) -> Vec<Memory> {
    let hash = store::project_hash(workspace);
    let max = max.unwrap_or(MAX_INJECT_MEMORIES).min(MAX_INJECT_MEMORIES);

    match store::load_memories(Some(&hash), include_global) {
        Ok(memories) => memories.into_iter().take(max).collect(),
        Err(_) => Vec::new(),
    }
}

/// Format a list of memories into a `<user_memories>` XML block for
/// injection into the system prompt.
#[must_use]
pub fn format_injection_block(memories: &[Memory]) -> Option<String> {
    if memories.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = Vec::with_capacity(memories.len() + 2);
    lines.push("<user_memories>".to_string());

    for mem in memories {
        let tag_str = if mem.tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", mem.tags.join(", "))
        };
        let confidence = mem.confidence.to_string();
        lines.push(format!(
            "- [{confidence}]{tag_str} {}",
            mem.content
        ));
    }

    lines.push("</user_memories>".to_string());
    Some(lines.join("\n"))
}

/// Compose the memory injection block for session start.
///
/// Returns `None` when there are no memories to inject (no files exist,
/// no project memories, or feature is disabled).
#[must_use]
pub fn compose_injection_block(
    enabled: bool,
    workspace: &Path,
    include_global: bool,
    max: Option<usize>,
) -> Option<String> {
    if !enabled {
        return None;
    }
    let memories = load_for_injection(workspace, include_global, max);
    format_injection_block(&memories)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::store::{MemoryConfidence, new_memory};

    #[test]
    fn empty_memories_returns_none() {
        assert!(format_injection_block(&[]).is_none());
    }

    #[test]
    fn formats_memories_as_xml() {
        let mem = new_memory(
            "prefers Rust".into(),
            "test".into(),
            MemoryConfidence::High,
            vec!["language".into()],
            None,
        );
        let block = format_injection_block(&[mem]).unwrap();
        assert!(block.contains("<user_memories>"));
        assert!(block.contains("</user_memories>"));
        assert!(block.contains("prefers Rust"));
        assert!(block.contains("[high]"));
        assert!(block.contains("[language]"));
    }

    #[test]
    fn compose_block_disabled_returns_none() {
        let result = compose_injection_block(false, Path::new("/tmp"), false, None);
        assert!(result.is_none());
    }
}
