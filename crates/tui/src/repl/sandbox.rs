//! REPL fence-extraction utilities.
//!
//! The agent's main loop scans assistant text for ` ```repl ` fenced blocks
//! and feeds them to a [`crate::repl::runtime::PythonRuntime`]. Capturing
//! `FINAL(...)` and routing sub-LLM RPCs are handled inside the runtime via
//! a stdin/stdout protocol — no scraping required here.

/// Check if a string contains a `` ```repl `` fenced code block.
pub fn has_repl_block(text: &str) -> bool {
    text.contains("```repl")
}

/// Extract every `` ```repl `` block from `text` with byte offsets.
pub fn extract_repl_blocks(text: &str) -> Vec<ReplBlock> {
    let mut blocks = Vec::new();
    let mut rest = text;

    while let Some(start_idx) = rest.find("```repl") {
        let after_fence = &rest[start_idx..];
        let code_start = after_fence.find('\n').unwrap_or(after_fence.len());
        let code_region = &after_fence[code_start..];
        let Some(end_offset) = code_region.find("\n```") else {
            break;
        };
        let code = code_region[..end_offset].to_string();
        let global_start = text.len() - rest.len() + start_idx;
        let global_end = global_start + code_start + end_offset + 3;
        blocks.push(ReplBlock {
            code,
            start_offset: global_start,
            end_offset: global_end,
        });
        rest = &after_fence[code_start + end_offset + 4..];
    }

    blocks
}

/// A `` ```repl `` code block with byte-offset position info.
#[derive(Debug, Clone)]
pub struct ReplBlock {
    pub code: String,
    pub start_offset: usize,
    pub end_offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_repl_block_detects_fence() {
        assert!(has_repl_block("some text ```repl\ncode\n``` more"));
        assert!(!has_repl_block("no repl here ```python\ncode\n```"));
        assert!(!has_repl_block("just text"));
    }

    #[test]
    fn extract_repl_blocks_single() {
        let text = "before\n```repl\nprint('hello')\n```\nafter";
        let blocks = extract_repl_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].code.trim(), "print('hello')");
    }

    #[test]
    fn extract_repl_blocks_multiple() {
        let text = "```repl\ncode1\n```\nmid\n```repl\ncode2\n```\nend";
        let blocks = extract_repl_blocks(text);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].code.trim(), "code1");
        assert_eq!(blocks[1].code.trim(), "code2");
    }

    #[test]
    fn extract_repl_blocks_empty_when_none() {
        let blocks = extract_repl_blocks("no blocks here");
        assert!(blocks.is_empty());
    }
}
