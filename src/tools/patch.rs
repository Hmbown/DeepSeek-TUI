//! Patch tools: `apply_patch` for unified diff patching
//!
//! This tool provides precise file modifications using unified diff format,
//! supporting multi-hunk patches and fuzzy matching.

use std::fs;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_bool, optional_u64, required_str,
};

/// Maximum lines of context for fuzzy matching (increased for better tolerance)
const MAX_FUZZ: usize = 50;

// === Types ===

/// Result of applying a patch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchResult {
    pub success: bool,
    pub hunks_applied: usize,
    pub hunks_total: usize,
    pub fuzz_used: usize,
    pub message: String,
}

/// A single hunk in a unified diff
#[derive(Debug, Clone)]
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<HunkLine>,
}

/// A line in a hunk
#[derive(Debug, Clone)]
pub enum HunkLine {
    Context(String),
    Add(String),
    Remove(String),
}

/// Tool for applying unified diff patches to files
pub struct ApplyPatchTool;

// === Errors ===

#[derive(Debug, Error)]
enum ApplyHunkError {
    #[error(
        "Failed to find matching location for hunk (expected at line {expected_line}, adjusted to {adjusted_line} with offset {offset:+})"
    )]
    NoMatch {
        expected_line: usize,
        adjusted_line: usize,
        offset: isize,
    },
}

#[async_trait]
impl ToolSpec for ApplyPatchTool {
    fn name(&self) -> &'static str {
        "apply_patch"
    }

    fn description(&self) -> &'static str {
        "Apply a unified diff patch to a file. Supports multi-hunk patches with fuzzy matching."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to patch (relative to workspace)"
                },
                "patch": {
                    "type": "string",
                    "description": "Unified diff patch content"
                },
                "fuzz": {
                    "type": "integer",
                    "description": "Maximum fuzz factor for fuzzy matching (default: 3)"
                },
                "create_if_missing": {
                    "type": "boolean",
                    "description": "Create the file if it doesn't exist (for new file patches)"
                }
            },
            "required": ["path", "patch"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::WritesFiles,
            ToolCapability::Sandboxable,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Suggest
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = required_str(&input, "path")?;
        let patch_text = required_str(&input, "patch")?;
        let fuzz = optional_u64(&input, "fuzz", MAX_FUZZ as u64).min(MAX_FUZZ as u64);
        let fuzz = usize::try_from(fuzz).unwrap_or(MAX_FUZZ);
        let create_if_missing = optional_bool(&input, "create_if_missing", false);

        let file_path = context.resolve_path(path_str)?;

        // Read existing file content (or empty for new files)
        let original_content = if file_path.exists() {
            fs::read_to_string(&file_path).map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to read {}: {}",
                    file_path.display(),
                    e
                ))
            })?
        } else if create_if_missing {
            String::new()
        } else {
            return Err(ToolError::execution_failed(format!(
                "File {} does not exist. Set create_if_missing=true for new files.",
                file_path.display()
            )));
        };

        // Parse the patch
        let hunks = parse_unified_diff(patch_text)?;
        if hunks.is_empty() {
            return Err(ToolError::invalid_input("No valid hunks found in patch"));
        }

        // Apply hunks
        let mut lines: Vec<String> = original_content.lines().map(String::from).collect();
        let mut total_fuzz = 0;
        let mut hunks_applied = 0;
        let mut cumulative_offset: isize = 0; // Track line drift across hunks

        for hunk in &hunks {
            match apply_hunk(&mut lines, hunk, fuzz, &mut cumulative_offset) {
                Ok(fuzz_used) => {
                    total_fuzz += fuzz_used;
                    hunks_applied += 1;
                }
                Err(e) => {
                    return Err(ToolError::execution_failed(format!(
                        "Failed to apply hunk at line {}: {}",
                        hunk.old_start, e
                    )));
                }
            }
        }

        // Write the patched file
        let new_content = lines.join("\n");

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to create directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        fs::write(&file_path, &new_content).map_err(|e| {
            ToolError::execution_failed(format!("Failed to write {}: {}", file_path.display(), e))
        })?;

        let result = PatchResult {
            success: true,
            hunks_applied,
            hunks_total: hunks.len(),
            fuzz_used: total_fuzz,
            message: format!(
                "Applied {}/{} hunks to {} (fuzz: {})",
                hunks_applied,
                hunks.len(),
                file_path.display(),
                total_fuzz
            ),
        };

        ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Parse a unified diff into hunks
fn parse_unified_diff(patch: &str) -> Result<Vec<Hunk>, ToolError> {
    let mut hunks = Vec::new();
    let mut lines = patch.lines().peekable();

    // Skip header lines (---, +++ etc)
    while let Some(line) = lines.peek() {
        if line.starts_with("@@") {
            break;
        }
        lines.next();
    }

    // Parse hunks
    while let Some(line) = lines.next() {
        if line.starts_with("@@") {
            let hunk = parse_hunk_header(line, &mut lines)?;
            hunks.push(hunk);
        }
    }

    Ok(hunks)
}

/// Parse a hunk header and its content
fn parse_hunk_header<'a, I>(
    header: &str,
    lines: &mut std::iter::Peekable<I>,
) -> Result<Hunk, ToolError>
where
    I: Iterator<Item = &'a str>,
{
    // Parse @@ -old_start,old_count +new_start,new_count @@
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 3 {
        return Err(ToolError::invalid_input(format!(
            "Invalid hunk header: {header}"
        )));
    }

    let old_range = parts[1].trim_start_matches('-');
    let new_range = parts[2].trim_start_matches('+');

    let (old_start, old_count) = parse_range(old_range)?;
    let (new_start, new_count) = parse_range(new_range)?;

    // Parse hunk lines
    let mut hunk_lines = Vec::new();
    let expected_lines = old_count.max(new_count) + old_count.min(new_count);

    for _ in 0..expected_lines * 2 {
        // Allow for more lines than expected
        match lines.peek() {
            Some(line) if line.starts_with("@@") => break,
            Some(line) if line.starts_with('-') => {
                hunk_lines.push(HunkLine::Remove(line[1..].to_string()));
                lines.next();
            }
            Some(line) if line.starts_with('+') => {
                hunk_lines.push(HunkLine::Add(line[1..].to_string()));
                lines.next();
            }
            Some(line) if line.starts_with(' ') || line.is_empty() => {
                let content = if line.is_empty() { "" } else { &line[1..] };
                hunk_lines.push(HunkLine::Context(content.to_string()));
                lines.next();
            }
            Some(line) if !line.starts_with('\\') => {
                // Treat as context line without leading space
                hunk_lines.push(HunkLine::Context((*line).to_string()));
                lines.next();
            }
            Some(_) => {
                lines.next(); // Skip "\ No newline at end of file" etc
            }
            None => break,
        }
    }

    Ok(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: hunk_lines,
    })
}

/// Parse a range like "10,5" or "10" into (start, count)
fn parse_range(range: &str) -> Result<(usize, usize), ToolError> {
    let parts: Vec<&str> = range.split(',').collect();
    let start = parts[0]
        .parse::<usize>()
        .map_err(|_| ToolError::invalid_input(format!("Invalid line number: {}", parts[0])))?;
    let count = if parts.len() > 1 {
        parts[1]
            .parse::<usize>()
            .map_err(|_| ToolError::invalid_input(format!("Invalid count: {}", parts[1])))?
    } else {
        1
    };
    Ok((start, count))
}

/// Apply a hunk to the file content with fuzzy matching
fn apply_hunk(
    lines: &mut Vec<String>,
    hunk: &Hunk,
    max_fuzz: usize,
    cumulative_offset: &mut isize,
) -> Result<usize, ApplyHunkError> {
    // Build expected old lines from hunk
    let old_lines: Vec<&str> = hunk
        .lines
        .iter()
        .filter_map(|line| match line {
            HunkLine::Context(s) | HunkLine::Remove(s) => Some(s.as_str()),
            HunkLine::Add(_) => None,
        })
        .collect();

    // Build new lines from hunk
    let new_lines: Vec<String> = hunk
        .lines
        .iter()
        .filter_map(|line| match line {
            HunkLine::Context(s) | HunkLine::Add(s) => Some(s.clone()),
            HunkLine::Remove(_) => None,
        })
        .collect();

    // Try to find the location with fuzzy matching
    // Apply cumulative offset from previous hunks
    let base_idx = if hunk.old_start > 0 {
        hunk.old_start - 1
    } else {
        0
    };
    let start_idx = ((base_idx as isize) + *cumulative_offset).max(0) as usize;

    for fuzz in 0..=max_fuzz {
        // Try at exact position first, then nearby
        let search_range = if fuzz == 0 {
            vec![start_idx]
        } else {
            let min = start_idx.saturating_sub(fuzz);
            let max = (start_idx + fuzz).min(lines.len());
            (min..=max).collect()
        };

        for pos in search_range {
            if matches_at_position(lines, &old_lines, pos) {
                // Apply the hunk
                let end_pos = pos + old_lines.len();
                lines.splice(pos..end_pos, new_lines.clone());

                // Update cumulative offset: new lines added minus old lines removed
                let delta = new_lines.len() as isize - old_lines.len() as isize;
                *cumulative_offset += delta;

                return Ok(fuzz);
            }
        }
    }

    // Special case: adding to empty file or new hunk at end
    if old_lines.is_empty() && (lines.is_empty() || start_idx >= lines.len()) {
        let delta = new_lines.len() as isize;
        lines.extend(new_lines);
        *cumulative_offset += delta;
        return Ok(0);
    }

    Err(ApplyHunkError::NoMatch {
        expected_line: hunk.old_start,
        adjusted_line: start_idx + 1, // Convert back to 1-indexed
        offset: *cumulative_offset,
    })
}

/// Check if `old_lines` match at the given position
fn matches_at_position(lines: &[String], old_lines: &[&str], pos: usize) -> bool {
    if pos + old_lines.len() > lines.len() {
        return false;
    }

    for (i, old_line) in old_lines.iter().enumerate() {
        // Normalize whitespace for comparison
        let file_line = lines[pos + i].trim_end();
        let expected = old_line.trim_end();
        if file_line != expected {
            return false;
        }
    }

    true
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_range() {
        assert_eq!(parse_range("10,5").unwrap(), (10, 5));
        assert_eq!(parse_range("10").unwrap(), (10, 1));
        assert_eq!(parse_range("1,0").unwrap(), (1, 0));
    }

    #[test]
    fn test_parse_unified_diff() {
        let patch = r"--- a/test.txt
+++ b/test.txt
@@ -1,3 +1,3 @@
 line1
-line2
+modified line2
 line3
";

        let hunks = parse_unified_diff(patch).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].old_count, 3);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].new_count, 3);
    }

    #[test]
    fn test_apply_hunk_simple() {
        let mut lines = vec![
            "line1".to_string(),
            "line2".to_string(),
            "line3".to_string(),
        ];

        let hunk = Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                HunkLine::Context("line1".to_string()),
                HunkLine::Remove("line2".to_string()),
                HunkLine::Add("modified".to_string()),
                HunkLine::Context("line3".to_string()),
            ],
        };

        let mut offset: isize = 0;
        let fuzz = apply_hunk(&mut lines, &hunk, 0, &mut offset).unwrap();
        assert_eq!(fuzz, 0);
        assert_eq!(lines, vec!["line1", "modified", "line3"]);
    }

    #[test]
    fn test_apply_hunk_with_fuzz() {
        let mut lines = vec![
            "line0".to_string(),
            "line1".to_string(),
            "line2".to_string(),
            "line3".to_string(),
        ];

        // Hunk expects to start at line 1, but content is at line 2
        let hunk = Hunk {
            old_start: 1, // Wrong position
            old_count: 2,
            new_start: 1,
            new_count: 2,
            lines: vec![
                HunkLine::Remove("line1".to_string()),
                HunkLine::Add("modified".to_string()),
                HunkLine::Context("line2".to_string()),
            ],
        };

        let mut offset: isize = 0;
        let fuzz = apply_hunk(&mut lines, &hunk, 3, &mut offset).unwrap();
        assert!(fuzz > 0);
        assert_eq!(lines, vec!["line0", "modified", "line2", "line3"]);
    }

    #[test]
    fn test_apply_hunk_no_match_returns_error() {
        let mut lines = vec!["line1".to_string(), "line2".to_string()];
        let hunk = Hunk {
            old_start: 5,
            old_count: 1,
            new_start: 5,
            new_count: 1,
            lines: vec![
                HunkLine::Context("missing".to_string()),
                HunkLine::Add("new".to_string()),
            ],
        };

        let mut offset: isize = 0;
        let err = apply_hunk(&mut lines, &hunk, 0, &mut offset).unwrap_err();
        assert!(matches!(
            err,
            ApplyHunkError::NoMatch {
                expected_line: 5,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_apply_patch_tool() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Create a test file
        fs::write(tmp.path().join("test.txt"), "line1\nline2\nline3\n").expect("write");

        let patch = r"--- a/test.txt
+++ b/test.txt
@@ -1,3 +1,3 @@
 line1
-line2
+modified
 line3
";

        let tool = ApplyPatchTool;
        let result = tool
            .execute(json!({"path": "test.txt", "patch": patch}), &ctx)
            .await
            .expect("execute");

        assert!(result.success);

        // Verify the patch was applied
        let content = fs::read_to_string(tmp.path().join("test.txt")).expect("read");
        assert!(content.contains("modified"));
        assert!(!content.contains("line2"));
    }

    #[tokio::test]
    async fn test_apply_patch_add_lines() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        fs::write(tmp.path().join("test.txt"), "line1\nline3\n").expect("write");

        let patch = r"@@ -1,2 +1,3 @@
 line1
+line2
 line3
";

        let tool = ApplyPatchTool;
        let result = tool
            .execute(json!({"path": "test.txt", "patch": patch}), &ctx)
            .await
            .expect("execute");

        assert!(result.success);

        let content = fs::read_to_string(tmp.path().join("test.txt")).expect("read");
        assert!(content.contains("line2"));
    }

    #[tokio::test]
    async fn test_apply_patch_create_new_file() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let patch = r"@@ -0,0 +1,3 @@
+line1
+line2
+line3
";

        let tool = ApplyPatchTool;
        let result = tool
            .execute(
                json!({"path": "new_file.txt", "patch": patch, "create_if_missing": true}),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.success);
        assert!(tmp.path().join("new_file.txt").exists());
    }

    #[test]
    fn test_apply_patch_tool_properties() {
        let tool = ApplyPatchTool;
        assert_eq!(tool.name(), "apply_patch");
        assert!(!tool.is_read_only());
        assert!(tool.is_sandboxable());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Suggest);
    }

    #[test]
    fn test_multi_hunk_offset_tracking() {
        // File with 6 lines
        let mut lines: Vec<String> = vec![
            "line1".to_string(),
            "line2".to_string(),
            "line3".to_string(),
            "line4".to_string(),
            "line5".to_string(),
            "line6".to_string(),
        ];

        // Hunk 1: Add 2 lines after line1 (offset becomes +2)
        let hunk1 = Hunk {
            old_start: 1,
            old_count: 2,
            new_start: 1,
            new_count: 4,
            lines: vec![
                HunkLine::Context("line1".to_string()),
                HunkLine::Add("new_a".to_string()),
                HunkLine::Add("new_b".to_string()),
                HunkLine::Context("line2".to_string()),
            ],
        };

        // Hunk 2: Modify line5 (originally at position 5, now at position 7 due to +2 offset)
        let hunk2 = Hunk {
            old_start: 5, // Original position in the diff
            old_count: 1,
            new_start: 7,
            new_count: 1,
            lines: vec![
                HunkLine::Remove("line5".to_string()),
                HunkLine::Add("modified5".to_string()),
            ],
        };

        let mut offset: isize = 0;

        // Apply first hunk
        let fuzz1 = apply_hunk(&mut lines, &hunk1, 3, &mut offset).unwrap();
        assert_eq!(fuzz1, 0);
        assert_eq!(offset, 2); // Added 2 lines (4 new - 2 old)
        assert_eq!(
            lines,
            vec![
                "line1", "new_a", "new_b", "line2", "line3", "line4", "line5", "line6"
            ]
        );

        // Apply second hunk - this would fail without offset tracking!
        let fuzz2 = apply_hunk(&mut lines, &hunk2, 3, &mut offset).unwrap();
        assert_eq!(fuzz2, 0);
        assert!(lines.contains(&"modified5".to_string()));
        assert!(!lines.contains(&"line5".to_string()));
    }
}
