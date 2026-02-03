//! Tool specification traits for the deepseek-cli agent system.
//!
//! This module defines the core abstractions for tools:
//! - `ToolSpec`: The main trait that all tools must implement
//! - `ToolContext`: Execution context passed to tools
//! - `ToolResult`: Unified result type for tool execution
//! - `ToolCapability`: Capabilities and requirements of tools

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::tools::shell::{new_shared_shell_manager, SharedShellManager};

/// Capabilities that a tool may have or require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCapability {
    /// Tool only reads data, never modifies state
    ReadOnly,
    /// Tool writes to the filesystem
    WritesFiles,
    /// Tool executes arbitrary shell commands
    ExecutesCode,
    /// Tool makes network requests
    Network,
    /// Tool can be run in a sandbox
    Sandboxable,
    /// Tool requires user approval before execution
    RequiresApproval,
}

/// Approval requirement for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApprovalRequirement {
    /// Never needs approval - safe read-only operations
    #[default]
    Auto,
    /// Suggest approval but allow user to skip
    Suggest,
    /// Always require explicit user approval
    Required,
}

/// Errors that can occur during tool execution.
#[derive(Debug, Clone, Error)]
pub enum ToolError {
    #[error("Failed to validate input: {message}")]
    InvalidInput { message: String },

    #[error("Failed to validate input: missing required field '{field}'")]
    MissingField { field: String },

    #[error("Failed to resolve path '{path}': path escapes workspace")]
    PathEscape { path: PathBuf },

    #[error("Failed to execute tool: {message}")]
    ExecutionFailed { message: String },

    #[error("Failed to execute tool: operation timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("Failed to locate tool: {message}")]
    NotAvailable { message: String },

    #[error("Failed to authorize tool execution: {message}")]
    PermissionDenied { message: String },
}

impl ToolError {
    #[must_use]
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingField {
            field: field.into(),
        }
    }

    #[must_use]
    pub fn execution_failed(msg: impl Into<String>) -> Self {
        Self::ExecutionFailed {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn path_escape(path: impl Into<PathBuf>) -> Self {
        Self::PathEscape { path: path.into() }
    }

    #[must_use]
    pub fn not_available(msg: impl Into<String>) -> Self {
        Self::NotAvailable {
            message: msg.into(),
        }
    }

    #[must_use]
    pub fn permission_denied(msg: impl Into<String>) -> Self {
        Self::PermissionDenied {
            message: msg.into(),
        }
    }
}

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// The output content (may be JSON or plain text)
    pub content: String,
    /// Whether the execution was successful
    pub success: bool,
    /// Optional structured metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl ToolResult {
    /// Create a successful result with content.
    #[must_use]
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            success: true,
            metadata: None,
        }
    }

    /// Create an error result with message.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: message.into(),
            success: false,
            metadata: None,
        }
    }

    /// Create a successful result from JSON.
    pub fn json<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        Ok(Self {
            content: serde_json::to_string_pretty(value)?,
            success: true,
            metadata: None,
        })
    }

    /// Add metadata to the result.
    #[must_use]
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Sandbox policy for command execution.
#[derive(Debug, Clone, Default)]
pub enum SandboxPolicy {
    /// No sandboxing (dangerous but sometimes needed)
    #[default]
    None,
    /// Standard sandbox with workspace write access
    Standard {
        writable_roots: Vec<PathBuf>,
        allow_network: bool,
    },
}

/// Context passed to tools during execution.
#[derive(Clone)]
pub struct ToolContext {
    /// The workspace root directory
    pub workspace: PathBuf,
    /// Shared shell manager for background tasks and streaming IO.
    pub shell_manager: SharedShellManager,
    /// Whether to allow paths outside workspace
    pub trust_mode: bool,
    /// Current sandbox policy
    pub sandbox_policy: SandboxPolicy,
    /// Path for notes file
    pub notes_path: PathBuf,
    /// MCP configuration path
    pub mcp_config_path: PathBuf,
    /// Elevated sandbox policy override (used when retrying after sandbox denial).
    /// This overrides the default sandbox behavior for shell commands.
    pub elevated_sandbox_policy: Option<crate::sandbox::SandboxPolicy>,
    /// Whether tools should auto-approve without safety checks (YOLO mode).
    /// When true, command safety analysis is skipped for shell execution.
    pub auto_approve: bool,
}

impl ToolContext {
    /// Create a new `ToolContext` with default settings.
    #[must_use]
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        let shell_manager = new_shared_shell_manager(workspace.clone());
        let notes_path = workspace.join(".deepseek").join("notes.md");
        let mcp_config_path = workspace.join(".deepseek").join("mcp.json");
        Self {
            workspace,
            shell_manager,
            trust_mode: false,
            sandbox_policy: SandboxPolicy::None,
            notes_path,
            mcp_config_path,
            elevated_sandbox_policy: None,
            auto_approve: false,
        }
    }

    /// Create a `ToolContext` with all settings specified.
    pub fn with_options(
        workspace: impl Into<PathBuf>,
        trust_mode: bool,
        notes_path: impl Into<PathBuf>,
        mcp_config_path: impl Into<PathBuf>,
    ) -> Self {
        let workspace = workspace.into();
        let shell_manager = new_shared_shell_manager(workspace.clone());
        Self {
            workspace,
            shell_manager,
            trust_mode,
            sandbox_policy: SandboxPolicy::None,
            notes_path: notes_path.into(),
            mcp_config_path: mcp_config_path.into(),
            elevated_sandbox_policy: None,
            auto_approve: false,
        }
    }

    /// Create a `ToolContext` with auto-approve mode (YOLO).
    pub fn with_auto_approve(
        workspace: impl Into<PathBuf>,
        trust_mode: bool,
        notes_path: impl Into<PathBuf>,
        mcp_config_path: impl Into<PathBuf>,
        auto_approve: bool,
    ) -> Self {
        let workspace = workspace.into();
        let shell_manager = new_shared_shell_manager(workspace.clone());
        Self {
            workspace,
            shell_manager,
            trust_mode,
            sandbox_policy: SandboxPolicy::None,
            notes_path: notes_path.into(),
            mcp_config_path: mcp_config_path.into(),
            elevated_sandbox_policy: None,
            auto_approve,
        }
    }

    /// Resolve a path relative to workspace, validating it doesn't escape.
    ///
    /// This handles both existing files (using canonicalize) and non-existent files
    /// (for write operations) by canonicalizing the parent directory and appending
    /// the filename.
    /// Resolve a path relative to workspace, validating it doesn't escape.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use crate::tools::spec::ToolContext;
    /// let ctx = ToolContext::new(".");
    /// let path = ctx.resolve_path("README.md")?;
    /// # Ok::<(), crate::tools::spec::ToolError>(())
    /// ```
    pub fn resolve_path(&self, raw: &str) -> Result<PathBuf, ToolError> {
        let candidate = if std::path::Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.workspace.join(raw)
        };

        // In trust mode, allow any path without validation
        if self.trust_mode {
            // Still try to canonicalize for consistency, but don't require it
            return Ok(candidate.canonicalize().unwrap_or(candidate));
        }

        // Try to canonicalize the workspace
        let workspace_canonical = self
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| self.workspace.clone());

        // For the initial check, also try to canonicalize the candidate if possible
        // This handles symlinks like /var -> /private/var on macOS
        let candidate_canonical = candidate
            .canonicalize()
            .unwrap_or_else(|_| normalize_path(&candidate));
        let workspace_normalized = normalize_path(&workspace_canonical);

        // Check if the candidate is under the workspace (comparing canonical paths)
        if !candidate_canonical.starts_with(&workspace_normalized) {
            // Also try with non-canonical workspace for cases where workspace itself
            // hasn't been canonicalized yet
            let workspace_plain = normalize_path(&self.workspace);
            let candidate_normalized = normalize_path(&candidate);
            if !candidate_normalized.starts_with(&workspace_plain) {
                return Err(ToolError::PathEscape {
                    path: candidate_canonical,
                });
            }
        }

        // For existing paths, use canonicalize directly
        if candidate.exists() {
            let canonical = candidate.canonicalize().map_err(|e| {
                ToolError::execution_failed(format!(
                    "Failed to canonicalize {}: {}",
                    candidate.display(),
                    e
                ))
            })?;

            if !canonical.starts_with(&workspace_canonical) {
                return Err(ToolError::PathEscape { path: canonical });
            }

            return Ok(canonical);
        }

        // For non-existent paths (e.g., files to be created), validate via parent
        // Find the deepest existing ancestor and canonicalize it
        let mut existing_ancestor = candidate.clone();
        let mut suffix_parts: Vec<std::ffi::OsString> = Vec::new();

        while !existing_ancestor.exists() {
            if let Some(file_name) = existing_ancestor.file_name() {
                suffix_parts.push(file_name.to_owned());
            }
            match existing_ancestor.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => {
                    existing_ancestor = parent.to_path_buf();
                }
                _ => {
                    // No existing parent found; fall back to simple check
                    break;
                }
            }
        }

        let canonical_ancestor = if existing_ancestor.exists() {
            existing_ancestor
                .canonicalize()
                .unwrap_or(existing_ancestor)
        } else {
            existing_ancestor
        };

        // Rebuild the full path from canonicalized ancestor
        let mut canonical = canonical_ancestor;
        for part in suffix_parts.into_iter().rev() {
            canonical.push(part);
        }
        let canonical = normalize_path(&canonical);

        // Validate it's under workspace
        if !canonical.starts_with(&workspace_canonical)
            && !canonical.starts_with(&workspace_normalized)
        {
            return Err(ToolError::PathEscape { path: canonical });
        }

        Ok(canonical)
    }

    /// Set the trust mode.
    pub fn with_trust_mode(mut self, trust: bool) -> Self {
        self.trust_mode = trust;
        self
    }

    /// Set the sandbox policy.
    pub fn with_sandbox_policy(mut self, policy: SandboxPolicy) -> Self {
        self.sandbox_policy = policy;
        self
    }

    /// Override the shared shell manager.
    pub fn with_shell_manager(mut self, shell_manager: SharedShellManager) -> Self {
        self.shell_manager = shell_manager;
        self
    }

    /// Set the elevated sandbox policy override.
    ///
    /// This is used when retrying a tool after a sandbox denial, to run
    /// with elevated permissions.
    pub fn with_elevated_sandbox_policy(mut self, policy: crate::sandbox::SandboxPolicy) -> Self {
        self.elevated_sandbox_policy = Some(policy);
        self
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut prefix: Option<std::ffi::OsString> = None;
    let mut is_root = false;
    let mut stack: Vec<std::ffi::OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix_component) => {
                prefix = Some(prefix_component.as_os_str().to_owned());
            }
            Component::RootDir => {
                is_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                let parent = Component::ParentDir.as_os_str();
                if let Some(last) = stack.pop() {
                    if last == parent {
                        stack.push(last);
                        stack.push(parent.to_owned());
                    }
                } else if !is_root {
                    stack.push(parent.to_owned());
                }
            }
            Component::Normal(part) => {
                stack.push(part.to_owned());
            }
        }
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    if is_root {
        normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR));
    }
    for part in stack {
        normalized.push(part);
    }
    normalized
}

/// The core trait that all tools must implement.
#[async_trait]
pub trait ToolSpec: Send + Sync {
    /// Returns the unique name of this tool (used in API calls).
    fn name(&self) -> &str;

    /// Returns a human-readable description of what this tool does.
    fn description(&self) -> &str;

    /// Returns the JSON Schema for the tool's input parameters.
    fn input_schema(&self) -> Value;

    /// Returns the capabilities this tool has.
    fn capabilities(&self) -> Vec<ToolCapability>;

    /// Returns the approval requirement for this tool.
    fn approval_requirement(&self) -> ApprovalRequirement {
        let caps = self.capabilities();
        if caps.contains(&ToolCapability::ExecutesCode) {
            ApprovalRequirement::Required
        } else if caps.contains(&ToolCapability::WritesFiles) {
            ApprovalRequirement::Suggest
        } else {
            ApprovalRequirement::Auto
        }
    }

    /// Returns whether this tool is sandboxable.
    fn is_sandboxable(&self) -> bool {
        self.capabilities().contains(&ToolCapability::Sandboxable)
    }

    /// Returns whether this tool is read-only.
    fn is_read_only(&self) -> bool {
        let caps = self.capabilities();
        caps.contains(&ToolCapability::ReadOnly)
            && !caps.contains(&ToolCapability::WritesFiles)
            && !caps.contains(&ToolCapability::ExecutesCode)
    }

    /// Returns whether this tool can be executed in parallel with others.
    fn supports_parallel(&self) -> bool {
        false
    }

    /// Execute the tool with the given input and context.
    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError>;
}

// === Helper functions for extracting values from JSON input ===

/// Helper to extract required string field from JSON input.
pub fn required_str<'a>(input: &'a Value, field: &str) -> Result<&'a str, ToolError> {
    input
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::missing_field(field))
}

/// Helper to extract optional string field from JSON input.
pub fn optional_str<'a>(input: &'a Value, field: &str) -> Option<&'a str> {
    input.get(field).and_then(|v| v.as_str())
}

/// Helper to extract required u64 field from JSON input.
pub fn required_u64(input: &Value, field: &str) -> Result<u64, ToolError> {
    input
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ToolError::missing_field(field))
}

/// Helper to extract optional u64 field with default.
pub fn optional_u64(input: &Value, field: &str, default: u64) -> u64 {
    input
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(default)
}

/// Helper to extract optional bool field with default.
pub fn optional_bool(input: &Value, field: &str, default: bool) -> bool {
    input
        .get(field)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(default)
}

/// Helper to extract required i64 field from JSON input.
pub fn required_i64(input: &Value, field: &str) -> Result<i64, ToolError> {
    input
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| ToolError::missing_field(field))
}

/// Helper to extract optional i64 field with default.
pub fn optional_i64(input: &Value, field: &str, default: i64) -> i64 {
    input
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(default)
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("hello");
        assert!(result.success);
        assert_eq!(result.content, "hello");
        assert!(result.metadata.is_none());
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("something failed");
        assert!(!result.success);
        assert_eq!(result.content, "something failed");
    }

    #[test]
    fn test_tool_result_json() {
        let data = json!({"key": "value"});
        let result = ToolResult::json(&data).unwrap();
        assert!(result.success);
        assert!(result.content.contains("key"));
    }

    #[test]
    fn test_tool_result_with_metadata() {
        let result = ToolResult::success("content").with_metadata(json!({"extra": true}));
        assert!(result.metadata.is_some());
    }

    #[test]
    fn test_tool_context_resolve_path_relative() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Create a test file
        let test_file = tmp.path().join("test.txt");
        std::fs::write(&test_file, "test").expect("write");

        let resolved = ctx.resolve_path("test.txt").expect("resolve");
        assert!(resolved.ends_with("test.txt"));
    }

    #[test]
    fn test_tool_context_resolve_path_escape() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        // Try to escape workspace
        let result = ctx.resolve_path("/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_context_resolve_path_parent_traversal() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let result = ctx.resolve_path("../escape.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_context_resolve_path_normalizes_parent() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());

        let result = ctx.resolve_path("new/../safe.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn test_tool_context_trust_mode() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf()).with_trust_mode(true);

        // In trust mode, absolute paths should work
        let result = ctx.resolve_path("/tmp");
        assert!(result.is_ok());
    }

    #[test]
    fn test_required_str() {
        let input = json!({"name": "test", "count": 42});
        assert_eq!(required_str(&input, "name").unwrap(), "test");
        assert!(required_str(&input, "missing").is_err());
        assert!(required_str(&input, "count").is_err()); // not a string
    }

    #[test]
    fn test_optional_str() {
        let input = json!({"name": "test"});
        assert_eq!(optional_str(&input, "name"), Some("test"));
        assert_eq!(optional_str(&input, "missing"), None);
    }

    #[test]
    fn test_required_u64() {
        let input = json!({"count": 42});
        assert_eq!(required_u64(&input, "count").unwrap(), 42);
        assert!(required_u64(&input, "missing").is_err());
    }

    #[test]
    fn test_optional_u64() {
        let input = json!({"count": 42});
        assert_eq!(optional_u64(&input, "count", 0), 42);
        assert_eq!(optional_u64(&input, "missing", 100), 100);
    }

    #[test]
    fn test_optional_bool() {
        let input = json!({"flag": true});
        assert!(optional_bool(&input, "flag", false));
        assert!(!optional_bool(&input, "missing", false));
    }

    #[test]
    fn test_tool_error_display() {
        let err = ToolError::missing_field("path");
        assert_eq!(
            format!("{err}"),
            "Failed to validate input: missing required field 'path'"
        );

        let err = ToolError::execution_failed("boom");
        assert_eq!(format!("{err}"), "Failed to execute tool: boom");
    }

    #[test]
    fn test_approval_requirement_default() {
        let level = ApprovalRequirement::default();
        assert_eq!(level, ApprovalRequirement::Auto);
    }
}
