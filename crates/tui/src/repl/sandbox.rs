//! REPL sandbox utilities: FINAL/FINAL_VAR parsing, llm_query injection,
//! and the ReplOutput type.

/// Output from a REPL execution round.
#[derive(Debug, Clone)]
pub struct ReplOutput {
    /// Cleaned stdout (protocol lines removed).
    pub stdout: String,
    /// Raw stdout including protocol lines.
    pub raw_stdout: String,
    /// Whether the round had an error.
    pub has_error: bool,
    /// If FINAL() or FINAL_VAR() was called, the value.
    pub final_value: Option<String>,
    /// Any llm_query() calls that were detected (prompt, model, max_tokens).
    pub llm_queries: Vec<LlmQueryRequest>,
}

/// A request from Python's `llm_query()` function.
#[derive(Debug, Clone)]
pub struct LlmQueryRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
}

/// Parse a stdout string into a ReplOutput, extracting FINAL markers
/// and cleaning protocol lines.
pub fn parse_final(raw_stdout: &str) -> (String, Option<String>) {
    let mut final_value: Option<String> = None;
    let mut cleaned = String::new();

    for line in raw_stdout.lines() {
        if let Some(val) = line.strip_prefix("__REPL_FINAL__::") {
            // Parse the JSON-encoded final value.
            if let Ok(parsed) = serde_json::from_str::<String>(val) {
                final_value = Some(parsed);
            } else {
                // Fallback: use the raw text after the prefix.
                final_value = Some(val.to_string());
            }
            continue;
        }
        // Skip other protocol lines.
        if line.starts_with("__REPL_LLM_QUERY__::")
            || line.starts_with("__REPL_DONE__")
            || line.starts_with("__REPL_READY__")
        {
            continue;
        }
        cleaned.push_str(line);
        cleaned.push('\n');
    }

    (cleaned.trim().to_string(), final_value)
}

/// Generate the Python code that injects `llm_query()` with a callback
/// mechanism. The function writes a JSON request to stdout, and the Rust
/// side reads it, dispatches the API call, and writes the result back.
///
/// In practice, the `llm_query()` stub in the bootstrap does this via
/// `print('__REPL_LLM_QUERY__::...')` and we handle the dispatch on the
/// Rust side. For a single round, we pre-compute all llm_query results
/// before executing the code.
pub fn inject_llm_query_fn(
    bootstrap: &str,
    queries: &[(usize, &str)], // (id, result)
) -> String {
    // Replace the stub llm_query with one that returns pre-computed results.
    let mock_results: Vec<String> = queries
        .iter()
        .map(|(id, result)| format!("    {id}: {result:?}"))
        .collect();
    let mock_dict = format!("{{\n{}\n}}", mock_results.join(",\n"));

    let override_fn = format!(
        r#"
_llm_query_results = {mock_dict}
_llm_query_idx = [0]
def llm_query(prompt, model=None, max_tokens=None):
    idx = _llm_query_idx[0]
    _llm_query_idx[0] += 1
    result = _llm_query_results.get(idx, f'[llm_query: idx {{idx}} not found]')
    return result
"#
    );

    bootstrap.replace(
        "def llm_query(prompt, model=None, max_tokens=None):\n    return f'[llm_query stub: {str(prompt)[:100]}...]'",
        &override_fn,
    )
}

/// Check if a string contains a ```repl fenced code block.
pub fn has_repl_block(text: &str) -> bool {
    text.contains("```repl")
}

/// Extract all ```repl code blocks from text.
/// Returns a list of (code, start_offset, end_offset).
pub fn extract_repl_blocks(text: &str) -> Vec<ReplBlock> {
    let mut blocks = Vec::new();
    let mut rest = text;

    while let Some(start_idx) = rest.find("```repl") {
        let after_fence = &rest[start_idx..];
        // Find the end of the opening fence line.
        let code_start = after_fence.find('\n').unwrap_or(after_fence.len());
        let code_region = &after_fence[code_start..];
        // Find the closing ```.
        let Some(end_offset) = code_region.find("\n```") else {
            break;
        };
        let code = code_region[..end_offset].to_string();
        let global_start = text.len() - rest.len() + start_idx;
        let global_end = global_start + code_start + end_offset + 3; // 3 for "```\n"
        blocks.push(ReplBlock {
            code,
            start_offset: global_start,
            end_offset: global_end,
        });
        rest = &after_fence[code_start + end_offset + 4..];
    }

    blocks
}

/// A ```repl code block with position info.
#[derive(Debug, Clone)]
pub struct ReplBlock {
    pub code: String,
    pub start_offset: usize,
    pub end_offset: usize,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_final_detects_value() {
        let raw = "hello\n__REPL_FINAL__::\"the answer\"\nworld";
        let (cleaned, final_val) = parse_final(raw);
        assert_eq!(final_val.as_deref(), Some("the answer"));
        assert!(cleaned.contains("hello"));
        assert!(!cleaned.contains("__REPL_FINAL__"));
    }

    #[test]
    fn parse_final_no_final_returns_none() {
        let raw = "just some output\nnothing special";
        let (cleaned, final_val) = parse_final(raw);
        assert_eq!(final_val, None);
        assert_eq!(cleaned, "just some output\nnothing special");
    }

    #[test]
    fn parse_final_handles_non_json_value() {
        let raw = "__REPL_FINAL__::plain text value";
        let (_, final_val) = parse_final(raw);
        assert_eq!(final_val.as_deref(), Some("plain text value"));
    }

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

    #[test]
    fn inject_llm_query_replaces_stub() {
        let bootstrap = "def llm_query(prompt, model=None, max_tokens=None):\n    return f'[llm_query stub: {str(prompt)[:100]}...]'";
        let result = inject_llm_query_fn(bootstrap, &[(0, "result0"), (1, "result1")]);
        assert!(!result.contains("llm_query stub"));
        assert!(result.contains("_llm_query_results"));
        assert!(result.contains("result0"));
        assert!(result.contains("result1"));
    }
}
