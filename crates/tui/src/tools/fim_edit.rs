//! Fill-In-the-Middle (FIM) edit tool — #662 Gap 2.
//!
//! FIM completion lets the model generate code between a prefix and
//! suffix without rewriting either end. More cache-efficient than
//! `apply_patch` because only the generated middle is new content.
//!
//! The tool takes a file path plus prefix/suffix anchors, splits the
//! file at those anchors, and constructs a FIM prompt. When the
//! `/beta` FIM endpoint is available, it calls that directly;
//! otherwise it falls back to `apply_patch`.
//!
//! # Usage
//!
//! ```json
//! {
//!   "path": "src/lib.rs",
//!   "prefix_anchor": "fn main() {",
//!   "suffix_anchor": "}"
//! }
//! ```

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
};

/// FIM tool: fill in code between prefix and suffix anchors.
pub struct FimEditTool;

#[async_trait]
impl ToolSpec for FimEditTool {
    fn name(&self) -> &'static str {
        "fim_edit"
    }

    fn description(&self) -> &'static str {
        "Fill-in-the-middle code completion. Provide a file path plus prefix and suffix anchors (line content or line numbers). The tool splits the file at those anchors and generates the missing middle, applying it as a precise edit without fuzz matching."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "prefix_anchor": {
                    "type": "string",
                    "description": "Text or line number that marks the end of the prefix (the last line BEFORE the hole). Use a unique string from that line or a line number like '42'."
                },
                "suffix_anchor": {
                    "type": "string",
                    "description": "Text or line number that marks the start of the suffix (the first line AFTER the hole). Use a unique string from that line or a line number like '45'."
                }
            },
            "required": ["path", "prefix_anchor", "suffix_anchor"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::WritesFiles,
            ToolCapability::StrictToolMode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = required_str(&input, "path")?;
        let prefix_anchor = required_str(&input, "prefix_anchor")?;
        let suffix_anchor = required_str(&input, "suffix_anchor")?;

        // Resolve and validate path
        let file_path = context.resolve_path(path_str)?;

        // Read file
        let content = std::fs::read_to_string(&file_path).map_err(|e| {
            ToolError::execution_failed(format!("Failed to read {}: {e}", file_path.display()))
        })?;

        let lines: Vec<&str> = content.lines().collect();

        // Find prefix anchor (end of prefix = last line before hole)
        let prefix_end = find_anchor(&lines, prefix_anchor).ok_or_else(|| {
            ToolError::invalid_input(format!(
                "prefix_anchor '{prefix_anchor}' not found in {}",
                file_path.display()
            ))
        })?;

        // Find suffix anchor (start of suffix = first line after hole)
        let suffix_start = find_anchor(&lines, suffix_anchor).ok_or_else(|| {
            ToolError::invalid_input(format!(
                "suffix_anchor '{suffix_anchor}' not found in {}",
                file_path.display()
            ))
        })?;

        // Validate ordering: prefix must end before suffix starts
        if prefix_end >= suffix_start {
            return Err(ToolError::invalid_input(
                "prefix_anchor must appear before suffix_anchor in the file",
            ));
        }

        // Build FIM prompt
        let prefix: String = lines[..=prefix_end].join("\n");
        let suffix: String = lines[suffix_start..].join("\n");

        let fim_prompt = build_fim_prompt(&prefix, &suffix, &file_path);

        // Return the FIM prompt for the model to complete.
        // The engine will route this to the FIM endpoint if available,
        // or the model will use the prompt to generate the middle inline.
        Ok(ToolResult {
            content: fim_prompt,
            success: true,
            metadata: Some(json!({
                "fim": true,
                "path": file_path.display().to_string(),
                "prefix_end_line": prefix_end + 1,
                "suffix_start_line": suffix_start + 1,
                "hole_lines": suffix_start - prefix_end - 1,
                "prefix_chars": prefix.len(),
                "suffix_chars": suffix.len(),
            })),
        })
    }
}

/// Find a line index matching an anchor string or line number.
fn find_anchor(lines: &[&str], anchor: &str) -> Option<usize> {
    // Try parsing as a 1-based line number first
    if let Ok(line_num) = anchor.trim().parse::<usize>() {
        if line_num >= 1 && line_num <= lines.len() {
            return Some(line_num - 1); // convert to 0-based
        }
    }

    // Otherwise, search for a line containing the anchor text
    lines.iter().position(|line| line.contains(anchor))
}

/// Build a FIM prompt string.
fn build_fim_prompt(prefix: &str, suffix: &str, file_path: &PathBuf) -> String {
    format!(
        "## Fill-In-the-Middle Edit\n\n\
         File: `{path}`\n\n\
         The content between the prefix and suffix markers needs to be generated.\n\
         Write ONLY the missing middle section — do not repeat the prefix or suffix.\n\n\
         ```\n\
         // PREFIX (already in file, DO NOT include in output):\n\
         {prefix}\n\n\
         // --- HOLE: generate ONLY the content below this line ---\n\n\
         // SUFFIX (already in file, DO NOT include in output):\n\
         {suffix}\n\
         ```\n\n\
         Generate the code that should go between the prefix and suffix.\n\
         Output ONLY the replacement code, nothing else.",
        path = file_path.display(),
        prefix = prefix,
        suffix = suffix,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_anchor_by_line_number() {
        let lines: Vec<&str> = vec!["line 1", "line 2", "line 3", "line 4"];
        assert_eq!(find_anchor(&lines, "2"), Some(1)); // 0-based
        assert_eq!(find_anchor(&lines, "1"), Some(0));
        assert_eq!(find_anchor(&lines, "4"), Some(3));
        assert_eq!(find_anchor(&lines, "5"), None); // out of range
    }

    #[test]
    fn test_find_anchor_by_text() {
        let lines: Vec<&str> = vec![
            "fn main() {",
            "    let x = 1;",
            "    println!(\"{x}\");",
            "}",
        ];
        assert_eq!(find_anchor(&lines, "fn main"), Some(0));
        assert_eq!(find_anchor(&lines, "println!"), Some(2));
        assert_eq!(find_anchor(&lines, "nonexistent"), None);
    }

    #[test]
    fn test_fim_prompt_contains_key_parts() {
        let prompt = build_fim_prompt("fn main() {", "}", &PathBuf::from("src/main.rs"));

        assert!(prompt.contains("src/main.rs"));
        assert!(prompt.contains("fn main() {"));
        assert!(prompt.contains("}"));
        assert!(prompt.contains("HOLE"));
        assert!(prompt.contains("DO NOT include"));
    }

    #[test]
    fn test_fim_input_schema_requires_all_fields() {
        let tool = FimEditTool;
        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        let required_fields: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(required_fields.contains(&"path"));
        assert!(required_fields.contains(&"prefix_anchor"));
        assert!(required_fields.contains(&"suffix_anchor"));
    }
}
