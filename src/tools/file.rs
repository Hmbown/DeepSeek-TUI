//! File system tools: `read_file`, `write_file`, `edit_file`, `list_dir`
//!
//! These tools provide safe file system operations within the workspace,
//! with path validation to prevent escaping the workspace boundary.

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_str, required_str,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::fs;

// === ReadFileTool ===

/// Tool for reading UTF-8 files from the workspace.
pub struct ReadFileTool;

#[async_trait]
impl ToolSpec for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read a UTF-8 file from the workspace."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (relative to workspace or absolute)"
                }
            },
            "required": ["path"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = required_str(&input, "path")?;
        let file_path = context.resolve_path(path_str)?;

        let contents = fs::read_to_string(&file_path).map_err(|e| {
            ToolError::execution_failed(format!("Failed to read {}: {}", file_path.display(), e))
        })?;

        Ok(ToolResult::success(contents))
    }
}

// === WriteFileTool ===

/// Tool for writing UTF-8 files to the workspace.
pub struct WriteFileTool;

#[async_trait]
impl ToolSpec for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        "Write content to a UTF-8 file in the workspace."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                }
            },
            "required": ["path", "content"]
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
        let file_content = required_str(&input, "content")?;

        let file_path = context.resolve_path(path_str)?;

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

        fs::write(&file_path, file_content).map_err(|e| {
            ToolError::execution_failed(format!("Failed to write {}: {}", file_path.display(), e))
        })?;

        Ok(ToolResult::success(format!(
            "Wrote {} bytes to {}",
            file_content.len(),
            file_path.display()
        )))
    }
}

// === EditFileTool ===

/// Tool for search/replace editing of files.
pub struct EditFileTool;

#[async_trait]
impl ToolSpec for EditFileTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Replace text in a file using search/replace."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file"
                },
                "search": {
                    "type": "string",
                    "description": "Text to search for"
                },
                "replace": {
                    "type": "string",
                    "description": "Text to replace with"
                }
            },
            "required": ["path", "search", "replace"]
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
        let search = required_str(&input, "search")?;
        let replace = required_str(&input, "replace")?;

        let file_path = context.resolve_path(path_str)?;

        let contents = fs::read_to_string(&file_path).map_err(|e| {
            ToolError::execution_failed(format!("Failed to read {}: {}", file_path.display(), e))
        })?;

        let count = contents.matches(search).count();
        if count == 0 {
            return Err(ToolError::execution_failed(format!(
                "Search string not found in {}",
                file_path.display()
            )));
        }

        let updated = contents.replace(search, replace);

        fs::write(&file_path, &updated).map_err(|e| {
            ToolError::execution_failed(format!("Failed to write {}: {}", file_path.display(), e))
        })?;

        Ok(ToolResult::success(format!(
            "Replaced {} occurrence(s) in {}",
            count,
            file_path.display()
        )))
    }
}

// === ListDirTool ===

/// Tool for listing directory contents.
pub struct ListDirTool;

#[async_trait]
impl ToolSpec for ListDirTool {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "List entries in a directory relative to the workspace."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path (default: .)"
                }
            },
            "required": []
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = optional_str(&input, "path").unwrap_or(".");
        let dir_path = context.resolve_path(path_str)?;

        let mut entries = Vec::new();

        for entry in fs::read_dir(&dir_path).map_err(|e| {
            ToolError::execution_failed(format!(
                "Failed to read directory {}: {}",
                dir_path.display(),
                e
            ))
        })? {
            let entry = entry.map_err(|e| ToolError::execution_failed(e.to_string()))?;
            let file_type = entry
                .file_type()
                .map_err(|e| ToolError::execution_failed(e.to_string()))?;

            entries.push(json!({
                "name": entry.file_name().to_string_lossy().to_string(),
                "is_dir": file_type.is_dir(),
            }));
        }

        ToolResult::json(&entries).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_read_file_tool() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Create a test file
        let test_file = tmp.path().join("test.txt");
        fs::write(&test_file, "hello world").expect("write");

        let tool = ReadFileTool;
        let result = tool
            .execute(json!({"path": "test.txt"}), &ctx)
            .await
            .expect("execute");

        assert!(result.success);
        assert_eq!(result.content, "hello world");
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let tool = ReadFileTool;
        let result = tool.execute(json!({"path": "nonexistent.txt"}), &ctx).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file_missing_path() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let tool = ReadFileTool;
        let result = tool.execute(json!({}), &ctx).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to validate input: missing required field 'path'")
        );
    }

    #[tokio::test]
    async fn test_write_file_tool() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let tool = WriteFileTool;
        let result = tool
            .execute(
                json!({"path": "output.txt", "content": "test content"}),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.success);
        assert!(result.content.contains("Wrote"));

        // Verify file was written
        let written = fs::read_to_string(tmp.path().join("output.txt")).expect("read");
        assert_eq!(written, "test content");
    }

    #[tokio::test]
    async fn test_write_file_creates_dirs() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let tool = WriteFileTool;
        let result = tool
            .execute(
                json!({"path": "subdir/nested/file.txt", "content": "nested content"}),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.success);

        // Verify nested file was created
        let written = fs::read_to_string(tmp.path().join("subdir/nested/file.txt")).expect("read");
        assert_eq!(written, "nested content");
    }

    #[tokio::test]
    async fn test_edit_file_tool() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Create a file to edit
        let test_file = tmp.path().join("edit_me.txt");
        fs::write(&test_file, "hello world hello").expect("write");

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({"path": "edit_me.txt", "search": "hello", "replace": "hi"}),
                &ctx,
            )
            .await
            .expect("execute");

        assert!(result.success);
        assert!(result.content.contains("2 occurrence(s)"));

        // Verify edit was applied
        let edited = fs::read_to_string(&test_file).expect("read");
        assert_eq!(edited, "hi world hi");
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Create a file without the search string
        let test_file = tmp.path().join("no_match.txt");
        fs::write(&test_file, "foo bar baz").expect("write");

        let tool = EditFileTool;
        let result = tool
            .execute(
                json!({"path": "no_match.txt", "search": "hello", "replace": "hi"}),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_list_dir_tool() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Create some files and directories
        fs::write(tmp.path().join("file1.txt"), "").expect("write");
        fs::write(tmp.path().join("file2.txt"), "").expect("write");
        fs::create_dir(tmp.path().join("subdir")).expect("mkdir");

        let tool = ListDirTool;
        let result = tool.execute(json!({}), &ctx).await.expect("execute");

        assert!(result.success);
        assert!(result.content.contains("file1.txt"));
        assert!(result.content.contains("file2.txt"));
        assert!(result.content.contains("subdir"));
        assert!(result.content.contains("\"is_dir\": true"));
    }

    #[tokio::test]
    async fn test_list_dir_with_path() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Create a subdirectory with files
        let subdir = tmp.path().join("mydir");
        fs::create_dir(&subdir).expect("mkdir");
        fs::write(subdir.join("nested.txt"), "").expect("write");

        let tool = ListDirTool;
        let result = tool
            .execute(json!({"path": "mydir"}), &ctx)
            .await
            .expect("execute");

        assert!(result.success);
        assert!(result.content.contains("nested.txt"));
    }

    #[test]
    fn test_read_file_tool_properties() {
        let tool = ReadFileTool;
        assert_eq!(tool.name(), "read_file");
        assert!(tool.is_read_only());
        assert!(tool.is_sandboxable());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);
    }

    #[test]
    fn test_write_file_tool_properties() {
        let tool = WriteFileTool;
        assert_eq!(tool.name(), "write_file");
        assert!(!tool.is_read_only());
        assert!(tool.is_sandboxable());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Suggest);
    }

    #[test]
    fn test_edit_file_tool_properties() {
        let tool = EditFileTool;
        assert_eq!(tool.name(), "edit_file");
        assert!(!tool.is_read_only());
        assert!(tool.is_sandboxable());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Suggest);
    }

    #[test]
    fn test_list_dir_tool_properties() {
        let tool = ListDirTool;
        assert_eq!(tool.name(), "list_dir");
        assert!(tool.is_read_only());
        assert!(tool.is_sandboxable());
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Auto);
    }

    #[test]
    fn test_parallel_support_flags() {
        let read_tool = ReadFileTool;
        let list_tool = ListDirTool;
        let write_tool = WriteFileTool;

        assert!(read_tool.supports_parallel());
        assert!(list_tool.supports_parallel());
        assert!(!write_tool.supports_parallel());
    }

    #[test]
    fn test_input_schemas() {
        // Verify all tools have valid JSON schemas
        let read_schema = ReadFileTool.input_schema();
        assert!(read_schema.get("type").is_some());
        assert!(read_schema.get("properties").is_some());

        let write_schema = WriteFileTool.input_schema();
        let required = write_schema
            .get("required")
            .and_then(|value| value.as_array())
            .expect("write schema should include required array");
        assert!(required.iter().any(|v| v.as_str() == Some("path")));
        assert!(required.iter().any(|v| v.as_str() == Some("content")));

        let edit_schema = EditFileTool.input_schema();
        let required = edit_schema
            .get("required")
            .and_then(|value| value.as_array())
            .expect("edit schema should include required array");
        assert_eq!(required.len(), 3);

        let list_schema = ListDirTool.input_schema();
        let required = list_schema
            .get("required")
            .and_then(|value| value.as_array())
            .expect("list schema should include required array");
        assert!(required.is_empty()); // path is optional
    }
}
