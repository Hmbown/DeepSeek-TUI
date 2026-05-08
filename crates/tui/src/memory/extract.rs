//! Automatic memory extraction from conversations.
//!
//! NOTE: Not yet wired — kept for follow-up integration.
#![allow(dead_code)]
//!
//! Provides the prompt template and parsing logic for extracting structured
//! memories from recent conversation messages. The extraction prompt asks the
//! model to identify user preferences, project conventions, architectural
//! decisions, and known issues.

use super::store::{Memory, MemoryConfidence, new_memory};
use serde::{Deserialize, Serialize};

/// A single extracted memory item as the model returns it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtractedMemory {
    pub content: String,
    pub confidence: String,
    pub tags: Vec<String>,
}

/// Expected model output format.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtractOutput {
    pub memories: Vec<ExtractedMemory>,
}

/// Build the extraction prompt for the model.
///
/// Takes the last N messages as a string (already formatted). Returns a
/// prompt that asks the model to identify durable facts worth remembering.
#[must_use]
pub fn extraction_prompt(conversation_text: &str) -> String {
    format!(
        r#"You are a memory curator. Review the conversation below and identify durable facts worth remembering across sessions.

Look for:
- **User preferences**: "prefers X", "uses Y workflow", "likes Z"
- **Project conventions**: "this project uses X", "always run Y before Z", "tests are in dir T"
- **Architectural decisions**: "decided to use X", "chose Y because Z", "avoid pattern A"
- **Known issues / workarounds**: "X is broken", "Y doesn't work on macOS", "use Z as fallback"

Output ONLY a JSON object in this exact format:
{{
  "memories": [
    {{
      "content": "one-line description of the fact",
      "confidence": "high|medium|low",
      "tags": ["tag1", "tag2"]
    }}
  ]
}}

Confidence guide:
- "high": explicit user statement ("I always want 4-space indents")
- "medium": observed convention (the project's Cargo.toml uses edition 2024)
- "low": inference from context (the user seems to prefer short variable names)

Limit to at most 10 memories. Skip transient facts (current bug, one-time task). Only extract what another instance of you would benefit from knowing in a future session.

Conversation:
{conversation_text}
"#
    )
}

/// Parse the model's output into structured Memory objects.
pub fn parse_extracted(
    raw_json: &str,
    source: &str,
    project_hash: Option<String>,
) -> Result<Vec<Memory>, String> {
    // Try to extract JSON from model output (may be wrapped in markdown).
    let json_str = extract_json_block(raw_json).unwrap_or(raw_json);

    let output: ExtractOutput = serde_json::from_str(json_str)
        .map_err(|e| format!("failed to parse extraction output: {e}"))?;

    output
        .memories
        .into_iter()
        .filter_map(|em| {
            let confidence: MemoryConfidence = em.confidence.parse().ok()?;
            if em.content.trim().is_empty() {
                return None;
            }
            Some(new_memory(
                em.content,
                source.to_string(),
                confidence,
                em.tags,
                project_hash.clone(),
            ))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .fold(Ok(Vec::new()), |acc: Result<Vec<Memory>, String>, m| {
            let mut vec = acc?;
            vec.push(m);
            Ok(vec)
        })
}

/// Try to extract the first JSON object/array from text that may be wrapped
/// in markdown code fences.
fn extract_json_block(text: &str) -> Option<&str> {
    // Strip markdown code fences.
    let stripped = text
        .trim()
        .strip_prefix("```json")
        .or_else(|| text.trim().strip_prefix("```"))
        .map(|s| s.strip_suffix("```").unwrap_or(s))
        .unwrap_or(text);

    // Find the outermost `{...}` or `[...]`.
    let start_brace = stripped.find('{');
    let start_bracket = stripped.find('[');

    let start = match (start_brace, start_bracket) {
        (Some(b), Some(k)) => b.min(k),
        (Some(b), None) => b,
        (None, Some(k)) => k,
        (None, None) => return None,
    };

    let _end_char = if stripped.as_bytes()[start] == b'{' {
        '}'
    } else {
        ']'
    };

    let mut depth = 0i32;
    let mut end = start;
    for (i, ch) in stripped[start..].char_indices() {
        match ch {
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + ch.len_utf8();
                    break;
                }
            }
            _ => {}
        }
    }

    Some(&stripped[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_prompt_includes_keywords() {
        let prompt = extraction_prompt("user: I like Rust\nassistant: me too");
        assert!(prompt.contains("prefers X"));
        assert!(prompt.contains("this project uses X"));
        assert!(prompt.contains("decided to use X"));
        assert!(prompt.contains("high|medium|low"));
        assert!(prompt.contains("memories"));
    }

    #[test]
    fn parse_valid_json_output() {
        let json = r#"{"memories": [{"content": "prefers 4-space indentation", "confidence": "high", "tags": ["style"]}]}"#;
        let result = parse_extracted(json, "session-1", Some("hash".into())).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "prefers 4-space indentation");
        assert_eq!(result[0].confidence, MemoryConfidence::High);
        assert_eq!(result[0].tags, vec!["style"]);
    }

    #[test]
    fn parse_json_in_markdown_fence() {
        let json = "```json\n{\"memories\": [{\"content\": \"uses cargo fmt\", \"confidence\": \"medium\", \"tags\": [\"rust\"]}]}\n```";
        let result = parse_extracted(json, "session-1", None).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "uses cargo fmt");
    }

    #[test]
    fn parse_skips_empty_content() {
        let json = r#"{"memories": [{"content": "", "confidence": "low", "tags": []}, {"content": "valid", "confidence": "high", "tags": []}]}"#;
        let result = parse_extracted(json, "session-1", None).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "valid");
    }

    #[test]
    fn parse_rejects_invalid_json() {
        let result = parse_extracted("not json", "session-1", None);
        assert!(result.is_err());
    }

    #[test]
    fn extract_json_handles_fences() {
        let text = "```json\n{\"key\": \"value\"}\n```";
        let extracted = extract_json_block(text).unwrap();
        assert_eq!(extracted.trim(), "{\"key\": \"value\"}");
    }

    #[test]
    fn extract_json_handles_nested() {
        let text = r#"some text {"outer": {"inner": [1,2,3]}} more text"#;
        let extracted = extract_json_block(text).unwrap();
        assert_eq!(extracted, r#"{"outer": {"inner": [1,2,3]}}"#);
    }
}
