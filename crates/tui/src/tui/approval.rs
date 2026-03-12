//! Tool approval system for `DeepSeek` CLI
//!
//! Provides types and overlay widget for requesting user approval before
//! executing tools that may have costs or side effects.

use crate::sandbox::SandboxPolicy;
use crate::tui::views::{ModalKind, ModalView, ViewAction, ViewEvent};
use crate::tui::widgets::{ApprovalWidget, ElevationWidget, Renderable};
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Determines when tool executions require user approval
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApprovalMode {
    /// Auto-approve all tools (YOLO mode / --yolo flag)
    Auto,
    /// Suggest approval for non-safe tools (non-YOLO modes)
    #[default]
    Suggest,
    /// Never execute tools requiring approval
    Never,
}

impl ApprovalMode {
    pub fn label(self) -> &'static str {
        match self {
            ApprovalMode::Auto => "AUTO",
            ApprovalMode::Suggest => "SUGGEST",
            ApprovalMode::Never => "NEVER",
        }
    }
}

/// User's decision for a pending approval
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewDecision {
    /// Execute this tool once
    Approved,
    /// Approve and don't ask again for this tool type this session
    ApprovedForSession,
    /// Reject the tool execution
    Denied,
    /// Abort the entire turn
    Abort,
}

/// Categorizes tools by cost/risk level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// Free, read-only operations (`list_dir`, `read_file`, todo_*)
    Safe,
    /// File modifications (`write_file`, `edit_file`)
    FileWrite,
    /// Shell execution (`exec_shell`)
    Shell,
    /// Network-oriented built-in tools
    Network,
    /// Read-only MCP discovery and resource access
    McpRead,
    /// MCP actions that may change remote state
    McpAction,
    /// Unknown or unclassified tool surface
    Unknown,
}

/// Request for user approval of a tool execution
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    /// Unique ID for this tool use
    pub id: String,
    /// Tool being executed
    pub tool_name: String,
    /// Human-readable tool description from the engine
    pub description: String,
    /// Tool category
    pub category: ToolCategory,
    /// Derived impact summary for the approval prompt
    pub impacts: Vec<String>,
    /// Tool parameters (for display)
    pub params: Value,
}

impl ApprovalRequest {
    pub fn new(id: &str, tool_name: &str, description: &str, params: &Value) -> Self {
        let category = get_tool_category(tool_name);

        Self {
            id: id.to_string(),
            tool_name: tool_name.to_string(),
            description: description.to_string(),
            category,
            impacts: build_impact_summary(tool_name, category, params),
            params: params.clone(),
        }
    }

    /// Format parameters for display (truncated)
    pub fn params_display(&self) -> String {
        let truncated = truncate_params_value(&self.params, 200);
        serde_json::to_string(&truncated).unwrap_or_else(|_| truncated.to_string())
    }
}

/// Get the category for a tool by name
pub fn get_tool_category(name: &str) -> ToolCategory {
    if matches!(name, "write_file" | "edit_file" | "apply_patch") {
        ToolCategory::FileWrite
    } else if matches!(name, "web_run" | "web_search" | "fetch_url") {
        ToolCategory::Network
    } else if name == "exec_shell" {
        ToolCategory::Shell
    } else if name.starts_with("list_mcp_")
        || name.starts_with("read_mcp_")
        || name.starts_with("get_mcp_")
    {
        ToolCategory::McpRead
    } else if name.starts_with("mcp_") {
        ToolCategory::McpAction
    } else if matches!(
        name,
        "read_file"
            | "list_dir"
            | "todo_write"
            | "todo_read"
            | "note"
            | "update_plan"
            | "search"
            | "file_search"
            | "project"
            | "diagnostics"
    ) || name.starts_with("read_")
        || name.starts_with("list_")
        || name.starts_with("get_")
    {
        ToolCategory::Safe
    } else {
        ToolCategory::Unknown
    }
}

fn param_preview(params: &Value, keys: &[&str], max_len: usize) -> Option<String> {
    let Value::Object(map) = params else {
        return None;
    };

    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        match value {
            Value::String(text) => return Some(truncate_string_value(text, max_len)),
            Value::Number(number) => return Some(number.to_string()),
            Value::Bool(flag) => return Some(flag.to_string()),
            Value::Array(items) if !items.is_empty() => {
                let preview = items
                    .iter()
                    .take(3)
                    .map(|item| match item {
                        Value::String(text) => truncate_string_value(text, max_len / 2),
                        other => truncate_string_value(&other.to_string(), max_len / 2),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                return Some(truncate_string_value(&preview, max_len));
            }
            other => return Some(truncate_string_value(&other.to_string(), max_len)),
        }
    }

    None
}

fn mcp_server_hint(tool_name: &str) -> Option<String> {
    let remainder = tool_name.strip_prefix("mcp_")?;
    let (server, _) = remainder.split_once('_')?;
    if server.is_empty() {
        None
    } else {
        Some(server.to_string())
    }
}

fn build_impact_summary(tool_name: &str, category: ToolCategory, params: &Value) -> Vec<String> {
    match category {
        ToolCategory::Safe => {
            let mut impacts = vec!["Read-only operation.".to_string()];
            if let Some(path) = param_preview(params, &["path", "ref_id", "uri"], 72) {
                impacts.push(format!("Reads: {path}"));
            }
            impacts
        }
        ToolCategory::FileWrite => {
            let mut impacts =
                vec!["Writes files in the workspace or an approved write scope.".to_string()];
            if let Some(path) = param_preview(params, &["path", "target", "destination"], 72) {
                impacts.push(format!("Writes: {path}"));
            }
            impacts
        }
        ToolCategory::Shell => {
            let mut impacts = vec!["Executes a shell command.".to_string()];
            if let Some(command) = param_preview(params, &["cmd", "command"], 96) {
                impacts.push(format!("Command: {command}"));
            }
            if let Some(workdir) = param_preview(params, &["workdir", "cwd"], 72) {
                impacts.push(format!("Working dir: {workdir}"));
            }
            impacts
        }
        ToolCategory::Network => {
            let mut impacts = vec!["May reach network services or remote content.".to_string()];
            if let Some(target) =
                param_preview(params, &["url", "q", "query", "location", "repo"], 96)
            {
                impacts.push(format!("Target: {target}"));
            }
            impacts
        }
        ToolCategory::McpRead => {
            let mut impacts =
                vec!["Reads from an MCP server without an obvious local write.".to_string()];
            if let Some(server) = mcp_server_hint(tool_name) {
                impacts.push(format!("Server: {server}"));
            }
            impacts
        }
        ToolCategory::McpAction => {
            let mut impacts =
                vec!["Calls an MCP server action that may have side effects.".to_string()];
            if let Some(server) = mcp_server_hint(tool_name) {
                impacts.push(format!("Server: {server}"));
            }
            impacts
        }
        ToolCategory::Unknown => {
            let mut impacts = vec![
                "Tool is not classified. Review params carefully before approving.".to_string(),
            ];
            if let Some(target) = param_preview(
                params,
                &["path", "cmd", "command", "url", "q", "query", "ref_id"],
                96,
            ) {
                impacts.push(format!("Primary input: {target}"));
            }
            impacts
        }
    }
}

/// Approval overlay state managed by the modal view stack
#[derive(Debug, Clone)]
pub struct ApprovalView {
    request: ApprovalRequest,
    selected: usize,
    timeout: Option<Duration>,
    requested_at: Instant,
}

impl ApprovalView {
    pub fn new(request: ApprovalRequest) -> Self {
        Self {
            request,
            selected: 0,
            timeout: None,
            requested_at: Instant::now(),
        }
    }

    fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn select_next(&mut self) {
        self.selected = (self.selected + 1).min(3);
    }

    fn current_decision(&self) -> ReviewDecision {
        match self.selected {
            0 => ReviewDecision::Approved,
            1 => ReviewDecision::ApprovedForSession,
            2 => ReviewDecision::Denied,
            _ => ReviewDecision::Abort,
        }
    }

    fn emit_decision(&self, decision: ReviewDecision, timed_out: bool) -> ViewAction {
        ViewAction::EmitAndClose(ViewEvent::ApprovalDecision {
            tool_id: self.request.id.clone(),
            tool_name: self.request.tool_name.clone(),
            decision,
            timed_out,
        })
    }

    fn emit_params_pager(&self) -> ViewAction {
        let content = serde_json::to_string_pretty(&self.request.params)
            .unwrap_or_else(|_| self.request.params.to_string());
        ViewAction::Emit(ViewEvent::OpenTextPager {
            title: format!("Tool Params: {}", self.request.tool_name),
            content,
        })
    }

    fn is_timed_out(&self) -> bool {
        match self.timeout {
            Some(timeout) => self.requested_at.elapsed() >= timeout,
            None => false,
        }
    }
}

impl ModalView for ApprovalView {
    fn kind(&self) -> ModalKind {
        ModalKind::Approval
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_prev();
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                ViewAction::None
            }
            KeyCode::Enter => self.emit_decision(self.current_decision(), false),
            KeyCode::Char('y') => self.emit_decision(ReviewDecision::Approved, false),
            KeyCode::Char('a') => self.emit_decision(ReviewDecision::ApprovedForSession, false),
            KeyCode::Char('n') => self.emit_decision(ReviewDecision::Denied, false),
            KeyCode::Char('v') | KeyCode::Char('V') => self.emit_params_pager(),
            KeyCode::Esc => self.emit_decision(ReviewDecision::Abort, false),
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) {
        let approval_widget = ApprovalWidget::new(&self.request, self.selected);
        approval_widget.render(area, buf);
    }

    fn tick(&mut self) -> ViewAction {
        if self.is_timed_out() {
            return self.emit_decision(ReviewDecision::Denied, true);
        }
        ViewAction::None
    }
}

fn truncate_params_value(value: &Value, max_len: usize) -> Value {
    match value {
        Value::Object(map) => {
            let truncated = map
                .iter()
                .map(|(key, val)| (key.clone(), truncate_params_value(val, max_len)))
                .collect();
            Value::Object(truncated)
        }
        Value::Array(items) => {
            let truncated_items = items
                .iter()
                .map(|val| truncate_params_value(val, max_len))
                .collect();
            Value::Array(truncated_items)
        }
        Value::String(text) => Value::String(truncate_string_value(text, max_len)),
        other => {
            let rendered = other.to_string();
            if rendered.chars().count() > max_len {
                Value::String(truncate_string_value(&rendered, max_len))
            } else {
                other.clone()
            }
        }
    }
}

fn truncate_string_value(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        return value.to_string();
    }
    let truncated: String = value.chars().take(max_len).collect();
    format!("{truncated}...")
}

// ============================================================================
// Sandbox Elevation Flow
// ============================================================================

/// Options for elevating sandbox permissions after a denial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevationOption {
    /// Add network access to the sandbox policy.
    WithNetwork,
    /// Add write access to specific paths.
    WithWriteAccess(Vec<PathBuf>),
    /// Remove sandbox restrictions entirely (dangerous).
    FullAccess,
    /// Abort the tool execution.
    Abort,
}

impl ElevationOption {
    /// Get the display label for this option.
    pub fn label(&self) -> &'static str {
        match self {
            ElevationOption::WithNetwork => "Allow outbound network",
            ElevationOption::WithWriteAccess(_) => "Allow extra write access",
            ElevationOption::FullAccess => "Full access (filesystem + network)",
            ElevationOption::Abort => "Abort",
        }
    }

    /// Get a short description.
    pub fn description(&self) -> &'static str {
        match self {
            ElevationOption::WithNetwork => {
                "Retry this tool call with outbound network access for downloads and HTTP requests"
            }
            ElevationOption::WithWriteAccess(_) => {
                "Retry this tool call with additional writable filesystem scope"
            }
            ElevationOption::FullAccess => {
                "Retry without sandbox limits; grants unrestricted filesystem and network access"
            }
            ElevationOption::Abort => "Cancel this tool execution",
        }
    }

    /// Convert to a sandbox policy.
    pub fn to_policy(&self, base_cwd: &Path) -> SandboxPolicy {
        match self {
            ElevationOption::WithNetwork => SandboxPolicy::workspace_with_network(),
            ElevationOption::WithWriteAccess(paths) => {
                let mut roots = paths.clone();
                roots.push(base_cwd.to_path_buf());
                SandboxPolicy::workspace_with_roots(roots, false)
            }
            ElevationOption::FullAccess => SandboxPolicy::DangerFullAccess,
            ElevationOption::Abort => SandboxPolicy::default(), // Won't be used
        }
    }
}

/// Request for user decision after a sandbox denial.
#[derive(Debug, Clone)]
pub struct ElevationRequest {
    /// The tool ID that was blocked.
    pub tool_id: String,
    /// The tool name.
    pub tool_name: String,
    /// The command that was blocked (if shell).
    pub command: Option<String>,
    /// The reason for denial (from sandbox).
    pub denial_reason: String,
    /// Available elevation options.
    pub options: Vec<ElevationOption>,
}

impl ElevationRequest {
    /// Create a new elevation request for a shell command.
    pub fn for_shell(
        tool_id: &str,
        command: &str,
        denial_reason: &str,
        blocked_network: bool,
        blocked_write: bool,
    ) -> Self {
        let mut options = Vec::new();

        if blocked_network {
            options.push(ElevationOption::WithNetwork);
        }
        if blocked_write {
            options.push(ElevationOption::WithWriteAccess(vec![]));
        }
        options.push(ElevationOption::FullAccess);
        options.push(ElevationOption::Abort);

        Self {
            tool_id: tool_id.to_string(),
            tool_name: "exec_shell".to_string(),
            command: Some(command.to_string()),
            denial_reason: denial_reason.to_string(),
            options,
        }
    }

    /// Create a generic elevation request.
    #[allow(dead_code)]
    pub fn generic(tool_id: &str, tool_name: &str, denial_reason: &str) -> Self {
        Self {
            tool_id: tool_id.to_string(),
            tool_name: tool_name.to_string(),
            command: None,
            denial_reason: denial_reason.to_string(),
            options: vec![
                ElevationOption::WithNetwork,
                ElevationOption::FullAccess,
                ElevationOption::Abort,
            ],
        }
    }
}

/// Elevation overlay state managed by the modal view stack.
#[derive(Debug, Clone)]
pub struct ElevationView {
    request: ElevationRequest,
    selected: usize,
}

impl ElevationView {
    pub fn new(request: ElevationRequest) -> Self {
        Self {
            request,
            selected: 0,
        }
    }

    fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn select_next(&mut self) {
        let max = self.request.options.len().saturating_sub(1);
        self.selected = (self.selected + 1).min(max);
    }

    fn current_option(&self) -> &ElevationOption {
        &self.request.options[self.selected]
    }

    fn emit_decision(&self, option: ElevationOption) -> ViewAction {
        ViewAction::EmitAndClose(ViewEvent::ElevationDecision {
            tool_id: self.request.tool_id.clone(),
            tool_name: self.request.tool_name.clone(),
            option,
        })
    }

    /// Get the request for rendering.
    #[allow(dead_code)]
    pub fn request(&self) -> &ElevationRequest {
        &self.request
    }

    /// Get the currently selected index.
    #[allow(dead_code)]
    pub fn selected(&self) -> usize {
        self.selected
    }
}

impl ModalView for ElevationView {
    fn kind(&self) -> ModalKind {
        ModalKind::Elevation
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_prev();
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                ViewAction::None
            }
            KeyCode::Enter => self.emit_decision(self.current_option().clone()),
            KeyCode::Char('n') => self.emit_decision(ElevationOption::WithNetwork),
            KeyCode::Char('w') => {
                // Find the write access option if available
                for opt in &self.request.options {
                    if matches!(opt, ElevationOption::WithWriteAccess(_)) {
                        return self.emit_decision(opt.clone());
                    }
                }
                ViewAction::None
            }
            KeyCode::Char('f') => self.emit_decision(ElevationOption::FullAccess),
            KeyCode::Esc | KeyCode::Char('a') => self.emit_decision(ElevationOption::Abort),
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) {
        let elevation_widget = ElevationWidget::new(&self.request, self.selected);
        elevation_widget.render(area, buf);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use serde_json::json;

    fn create_key_event(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    // ========================================================================
    // Tool Category Tests
    // ========================================================================

    #[test]
    fn test_get_tool_category_safe_tools() {
        // Read-only operations should be Safe
        assert_eq!(get_tool_category("read_file"), ToolCategory::Safe);
        assert_eq!(get_tool_category("list_dir"), ToolCategory::Safe);
        assert_eq!(get_tool_category("todo_write"), ToolCategory::Safe);
        assert_eq!(get_tool_category("todo_read"), ToolCategory::Safe);
        assert_eq!(get_tool_category("note"), ToolCategory::Safe);
        assert_eq!(get_tool_category("update_plan"), ToolCategory::Safe);
    }

    #[test]
    fn test_get_tool_category_file_write_tools() {
        // File modification tools should be FileWrite
        assert_eq!(get_tool_category("write_file"), ToolCategory::FileWrite);
        assert_eq!(get_tool_category("edit_file"), ToolCategory::FileWrite);
        assert_eq!(get_tool_category("apply_patch"), ToolCategory::FileWrite);
    }

    #[test]
    fn test_get_tool_category_shell_tools() {
        // Shell execution tools should be Shell
        assert_eq!(get_tool_category("exec_shell"), ToolCategory::Shell);
        assert_eq!(
            get_tool_category("mcp_linear_save_issue"),
            ToolCategory::McpAction
        );
        assert_eq!(get_tool_category("list_mcp_tools"), ToolCategory::McpRead);
    }

    #[test]
    fn test_get_tool_category_unknown_tools_need_review() {
        assert_eq!(get_tool_category("unknown_tool"), ToolCategory::Unknown);
    }

    // ========================================================================
    // ApprovalRequest Tests
    // ========================================================================

    #[test]
    fn test_approval_request_new() {
        let params = json!({"path": "src/main.rs", "content": "test"});
        let request =
            ApprovalRequest::new("test-id", "write_file", "Write a file to disk", &params);

        assert_eq!(request.id, "test-id");
        assert_eq!(request.tool_name, "write_file");
        assert_eq!(request.category, ToolCategory::FileWrite);
        assert_eq!(request.params, params);
    }

    #[test]
    fn test_approval_request_params_display_truncates() {
        // Create params with a very long string
        let long_content = "x".repeat(300);
        let params = json!({"path": "src/main.rs", "content": long_content});
        let request =
            ApprovalRequest::new("test-id", "write_file", "Write a file to disk", &params);

        let display = request.params_display();
        // Should be truncated to around 200 chars
        assert!(display.len() < 250);
        assert!(display.contains("src/main.rs"));
    }

    #[test]
    fn test_approval_request_params_display_short() {
        let params = json!({"path": "src/main.rs"});
        let request =
            ApprovalRequest::new("test-id", "read_file", "Read a file from disk", &params);

        let display = request.params_display();
        assert!(display.contains("src/main.rs"));
    }

    #[test]
    fn test_approval_request_derives_impact_summary() {
        let params = json!({"cmd": "cargo test", "workdir": "/tmp/project"});
        let request = ApprovalRequest::new("test-id", "exec_shell", "Run a shell command", &params);

        assert_eq!(request.category, ToolCategory::Shell);
        assert!(
            request
                .impacts
                .iter()
                .any(|line| line.contains("Executes a shell command"))
        );
        assert!(
            request
                .impacts
                .iter()
                .any(|line| line.contains("cargo test"))
        );
    }

    // ========================================================================
    // ApprovalView Tests
    // ========================================================================

    #[test]
    fn test_approval_view_initial_state() {
        let params = json!({"path": "src/main.rs"});
        let request =
            ApprovalRequest::new("test-id", "read_file", "Read a file from disk", &params);
        let view = ApprovalView::new(request.clone());

        assert_eq!(view.selected, 0);
        assert!(view.timeout.is_none());
    }

    #[test]
    fn test_approval_view_navigation() {
        let params = json!({"path": "src/main.rs"});
        let request =
            ApprovalRequest::new("test-id", "read_file", "Read a file from disk", &params);
        let mut view = ApprovalView::new(request);

        // Initially at 0
        assert_eq!(view.selected, 0);

        // Navigate down
        view.select_next();
        assert_eq!(view.selected, 1);

        view.select_next();
        assert_eq!(view.selected, 2);

        view.select_next();
        assert_eq!(view.selected, 3);

        // Should clamp at 3
        view.select_next();
        assert_eq!(view.selected, 3);

        // Navigate up
        view.select_prev();
        assert_eq!(view.selected, 2);
    }

    #[test]
    fn test_approval_view_keybindings_decisions() {
        let params = json!({"path": "src/main.rs"});
        let request =
            ApprovalRequest::new("test-id", "read_file", "Read a file from disk", &params);
        let mut view = ApprovalView::new(request.clone());

        // Test 'y' -> Approved
        let action = view.handle_key(create_key_event(KeyCode::Char('y')));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ApprovalDecision {
                decision: ReviewDecision::Approved,
                ..
            })
        ));

        // Test 'n' -> Denied
        let mut view = ApprovalView::new(request.clone());
        let action = view.handle_key(create_key_event(KeyCode::Char('n')));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ApprovalDecision {
                decision: ReviewDecision::Denied,
                ..
            })
        ));

        // Test 'a' -> ApprovedForSession
        let mut view = ApprovalView::new(request.clone());
        let action = view.handle_key(create_key_event(KeyCode::Char('a')));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ApprovalDecision {
                decision: ReviewDecision::ApprovedForSession,
                ..
            })
        ));

        // Test Esc -> Abort
        let mut view = ApprovalView::new(request);
        let action = view.handle_key(create_key_event(KeyCode::Esc));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ApprovalDecision {
                decision: ReviewDecision::Abort,
                ..
            })
        ));
    }

    #[test]
    fn test_approval_view_enter_uses_selected_option() {
        let params = json!({"path": "src/main.rs"});
        let request =
            ApprovalRequest::new("test-id", "read_file", "Read a file from disk", &params);
        let mut view = ApprovalView::new(request);

        // Navigate to index 2 (Denied)
        view.select_next();
        view.select_next();
        assert_eq!(view.selected, 2);

        // Press Enter - should use current selection
        let action = view.handle_key(create_key_event(KeyCode::Enter));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ApprovalDecision {
                decision: ReviewDecision::Denied,
                ..
            })
        ));
    }

    #[test]
    fn test_approval_view_navigation_keys() {
        let params = json!({"path": "src/main.rs"});
        let request =
            ApprovalRequest::new("test-id", "read_file", "Read a file from disk", &params);
        let mut view = ApprovalView::new(request);

        // Test Up arrow
        view.handle_key(create_key_event(KeyCode::Up));
        assert_eq!(view.selected, 0); // Should clamp at 0

        // Test Down arrow
        view.handle_key(create_key_event(KeyCode::Down));
        assert_eq!(view.selected, 1);

        // Test 'j' for down
        view.handle_key(create_key_event(KeyCode::Char('j')));
        assert_eq!(view.selected, 2);

        // Test 'k' for up
        view.handle_key(create_key_event(KeyCode::Char('k')));
        assert_eq!(view.selected, 1);
    }

    #[test]
    fn test_approval_view_view_params() {
        let params = json!({"path": "src/main.rs", "content": "test"});
        let request =
            ApprovalRequest::new("test-id", "read_file", "Read a file from disk", &params);
        let mut view = ApprovalView::new(request.clone());

        // Test 'v' to view params
        let action = view.handle_key(create_key_event(KeyCode::Char('v')));
        assert!(matches!(
            action,
            ViewAction::Emit(ViewEvent::OpenTextPager { .. })
        ));

        // Test 'V' (uppercase) also works
        let mut view = ApprovalView::new(request.clone());
        let action = view.handle_key(create_key_event(KeyCode::Char('V')));
        assert!(matches!(
            action,
            ViewAction::Emit(ViewEvent::OpenTextPager { .. })
        ));
    }

    #[test]
    fn test_approval_view_current_decision_mapping() {
        let params = json!({"path": "src/main.rs"});
        let request =
            ApprovalRequest::new("test-id", "read_file", "Read a file from disk", &params);
        let mut view = ApprovalView::new(request);

        // Index 0 -> Approved
        view.selected = 0;
        assert_eq!(view.current_decision(), ReviewDecision::Approved);

        // Index 1 -> ApprovedForSession
        view.selected = 1;
        assert_eq!(view.current_decision(), ReviewDecision::ApprovedForSession);

        // Index 2 -> Denied
        view.selected = 2;
        assert_eq!(view.current_decision(), ReviewDecision::Denied);

        // Index 3 -> Abort
        view.selected = 3;
        assert_eq!(view.current_decision(), ReviewDecision::Abort);
    }

    // ========================================================================
    // ElevationView Tests
    // ========================================================================

    #[test]
    fn test_elevation_view_initial_state() {
        let request =
            ElevationRequest::for_shell("test-id", "cargo build", "network blocked", true, false);
        let view = ElevationView::new(request);

        assert_eq!(view.selected, 0);
    }

    #[test]
    fn test_elevation_view_keybindings() {
        let request =
            ElevationRequest::for_shell("test-id", "cargo test", "write blocked", false, true);
        let mut view = ElevationView::new(request);

        // Test 'n' -> WithNetwork
        let action = view.handle_key(create_key_event(KeyCode::Char('n')));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ElevationDecision {
                option: ElevationOption::WithNetwork,
                ..
            })
        ));

        // Test 'w' -> WithWriteAccess
        let request =
            ElevationRequest::for_shell("test-id", "cargo build", "write blocked", false, true);
        let mut view = ElevationView::new(request);
        let action = view.handle_key(create_key_event(KeyCode::Char('w')));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ElevationDecision {
                option: ElevationOption::WithWriteAccess(_),
                ..
            })
        ));

        // Test 'f' -> FullAccess
        let request =
            ElevationRequest::for_shell("test-id", "cargo build", "blocked", false, false);
        let mut view = ElevationView::new(request);
        let action = view.handle_key(create_key_event(KeyCode::Char('f')));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ElevationDecision {
                option: ElevationOption::FullAccess,
                ..
            })
        ));

        // Test Esc -> Abort
        let request =
            ElevationRequest::for_shell("test-id", "cargo build", "blocked", false, false);
        let mut view = ElevationView::new(request);
        let action = view.handle_key(create_key_event(KeyCode::Esc));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ElevationDecision {
                option: ElevationOption::Abort,
                ..
            })
        ));

        // Test 'a' -> Abort (alternative)
        let request =
            ElevationRequest::for_shell("test-id", "cargo build", "blocked", false, false);
        let mut view = ElevationView::new(request);
        let action = view.handle_key(create_key_event(KeyCode::Char('a')));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ElevationDecision {
                option: ElevationOption::Abort,
                ..
            })
        ));
    }

    #[test]
    fn test_elevation_view_navigation() {
        let request = ElevationRequest::for_shell("test-id", "cargo build", "blocked", true, false);
        let mut view = ElevationView::new(request);

        // Initially at 0
        assert_eq!(view.selected, 0);

        // Navigate down
        view.handle_key(create_key_event(KeyCode::Down));
        assert_eq!(view.selected, 1);

        // Navigate up
        view.handle_key(create_key_event(KeyCode::Up));
        assert_eq!(view.selected, 0);

        // Test 'j' and 'k'
        view.handle_key(create_key_event(KeyCode::Char('j')));
        assert_eq!(view.selected, 1);

        view.handle_key(create_key_event(KeyCode::Char('k')));
        assert_eq!(view.selected, 0);
    }

    #[test]
    fn test_elevation_view_enter_uses_selected_option() {
        let request = ElevationRequest::for_shell("test-id", "cargo build", "blocked", true, false);
        let mut view = ElevationView::new(request);

        // Navigate to index 1
        view.handle_key(create_key_event(KeyCode::Down));
        assert_eq!(view.selected, 1);

        // Press Enter
        let action = view.handle_key(create_key_event(KeyCode::Enter));
        assert!(matches!(
            action,
            ViewAction::EmitAndClose(ViewEvent::ElevationDecision {
                option: ElevationOption::FullAccess,
                ..
            })
        ));
    }

    // ========================================================================
    // ElevationOption Tests
    // ========================================================================

    #[test]
    fn test_elevation_option_labels() {
        assert_eq!(
            ElevationOption::WithNetwork.label(),
            "Allow outbound network"
        );
        assert_eq!(
            ElevationOption::FullAccess.label(),
            "Full access (filesystem + network)"
        );
        assert!(
            ElevationOption::WithWriteAccess(vec![])
                .label()
                .contains("write")
        );
        assert_eq!(ElevationOption::Abort.label(), "Abort");
    }

    #[test]
    fn test_elevation_option_descriptions() {
        assert!(
            ElevationOption::WithNetwork
                .description()
                .contains("network")
        );
        assert!(
            ElevationOption::FullAccess
                .description()
                .contains("filesystem and network access")
        );
        assert!(ElevationOption::Abort.description().contains("Cancel"));
    }

    #[test]
    fn test_elevation_option_to_policy() {
        let cwd = PathBuf::from("/tmp/test");

        let policy = ElevationOption::WithNetwork.to_policy(&cwd);
        assert!(matches!(
            policy,
            SandboxPolicy::WorkspaceWrite {
                network_access: true,
                ..
            }
        ));

        let policy = ElevationOption::FullAccess.to_policy(&cwd);
        assert!(matches!(policy, SandboxPolicy::DangerFullAccess));

        let paths = vec![PathBuf::from("/tmp/test/src")];
        let policy = ElevationOption::WithWriteAccess(paths).to_policy(&cwd);
        assert!(matches!(policy, SandboxPolicy::WorkspaceWrite { .. }));
    }

    // ========================================================================
    // ElevationRequest Tests
    // ========================================================================

    #[test]
    fn test_elevation_request_for_shell_with_network_block() {
        let request = ElevationRequest::for_shell(
            "test-id",
            "curl example.com",
            "network blocked",
            true,
            false,
        );

        assert_eq!(request.tool_id, "test-id");
        assert_eq!(request.tool_name, "exec_shell");
        assert!(request.command.is_some());
        assert!(request.denial_reason.contains("network"));
        assert!(
            request
                .options
                .iter()
                .any(|o| matches!(o, ElevationOption::WithNetwork))
        );
    }

    #[test]
    fn test_elevation_request_for_shell_with_write_block() {
        let request =
            ElevationRequest::for_shell("test-id", "rm -rf /tmp", "write blocked", false, true);

        assert_eq!(request.tool_id, "test-id");
        assert!(
            request
                .options
                .iter()
                .any(|o| matches!(o, ElevationOption::WithWriteAccess(_)))
        );
    }

    #[test]
    fn test_elevation_request_generic() {
        let request = ElevationRequest::generic("test-id", "some_tool", "permission denied");

        assert_eq!(request.tool_id, "test-id");
        assert_eq!(request.tool_name, "some_tool");
        assert!(request.command.is_none());
        assert!(
            request
                .options
                .iter()
                .any(|o| matches!(o, ElevationOption::WithNetwork))
        );
        assert!(
            request
                .options
                .iter()
                .any(|o| matches!(o, ElevationOption::FullAccess))
        );
        assert!(
            request
                .options
                .iter()
                .any(|o| matches!(o, ElevationOption::Abort))
        );
    }

    // ========================================================================
    // ApprovalMode Tests
    // ========================================================================

    #[test]
    fn test_approval_mode_labels() {
        assert_eq!(ApprovalMode::Auto.label(), "AUTO");
        assert_eq!(ApprovalMode::Suggest.label(), "SUGGEST");
        assert_eq!(ApprovalMode::Never.label(), "NEVER");
    }
}
