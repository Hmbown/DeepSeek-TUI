//! Hooks system for `Wagmii` CLI
//!
//! Provides lifecycle hooks that execute user-defined shell commands at:
//! - Session start/end
//! - Tool call before/after

#![allow(dead_code)]
//! - Mode changes
//! - Message submission
//! - Error events
//!
//! Configuration is done via `[[hooks.hooks]]` in config.toml.

// Note: anyhow is available if needed for future error handling
#[allow(unused_imports)]
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;

/// Events that can trigger hook execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// Triggered when a new session starts
    SessionStart,
    /// Triggered when a session ends (quit, Ctrl+C)
    SessionEnd,
    /// Triggered before a user message is sent to the LLM
    MessageSubmit,
    /// Triggered before a tool is executed
    ToolCallBefore,
    /// Triggered after a tool completes (success or failure)
    ToolCallAfter,
    /// Triggered when the user changes modes (Normal, Edit, Agent, Plan)
    ModeChange,
    /// Triggered when an error occurs
    OnError,
}

impl HookEvent {
    /// Get string representation for environment variable
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::SessionStart => "session_start",
            HookEvent::SessionEnd => "session_end",
            HookEvent::MessageSubmit => "message_submit",
            HookEvent::ToolCallBefore => "tool_call_before",
            HookEvent::ToolCallAfter => "tool_call_after",
            HookEvent::ModeChange => "mode_change",
            HookEvent::OnError => "on_error",
        }
    }
}

/// Condition for when a hook should run
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[derive(Default)]
pub enum HookCondition {
    /// Always run this hook
    #[default]
    Always,
    /// Only run for specific tool names
    ToolName {
        /// Tool name to match (e.g., "`exec_shell`", "`write_file`")
        name: String,
    },
    /// Only run for specific tool categories
    ToolCategory {
        /// Category: "safe", "`file_write`", "shell"
        category: String,
    },
    /// Only run in specific modes
    Mode {
        /// Mode: "plan", "agent", "yolo"
        mode: String,
    },
    /// Only run when exit code matches (for `ToolCallAfter`)
    ExitCode {
        /// Exit code to match
        code: i32,
    },
    /// Combine multiple conditions with AND
    All { conditions: Vec<HookCondition> },
    /// Combine multiple conditions with OR
    Any { conditions: Vec<HookCondition> },
}

/// A single hook definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    /// The event that triggers this hook
    pub event: HookEvent,

    /// Shell command to execute (platform shell: `sh -c` on Unix, `cmd /C` on Windows)
    pub command: String,

    /// Optional condition for when this hook should run
    #[serde(default)]
    pub condition: Option<HookCondition>,

    /// Timeout in seconds (default: 30)
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Run in background (don't wait for completion)
    #[serde(default)]
    pub background: bool,

    /// Continue if this hook fails (default: true)
    #[serde(default = "default_continue_on_error")]
    pub continue_on_error: bool,

    /// Optional name for logging/debugging
    #[serde(default)]
    pub name: Option<String>,
}

fn default_timeout() -> u64 {
    30
}
fn default_continue_on_error() -> bool {
    true
}

impl Hook {
    /// Create a new hook with minimal configuration
    pub fn new(event: HookEvent, command: &str) -> Self {
        Self {
            event,
            command: command.to_string(),
            condition: None,
            timeout_secs: 30,
            background: false,
            continue_on_error: true,
            name: None,
        }
    }

    /// Builder: set condition
    pub fn with_condition(mut self, condition: HookCondition) -> Self {
        self.condition = Some(condition);
        self
    }

    /// Builder: set timeout
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Builder: run in background
    pub fn background(mut self) -> Self {
        self.background = true;
        self
    }

    /// Builder: set name
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }
}

/// Configuration for hooks (loaded from config.toml)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {
    /// List of hooks to execute
    #[serde(default)]
    pub hooks: Vec<Hook>,

    /// Global enable/disable for all hooks
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Global timeout override (applies if hook doesn't specify one)
    #[serde(default)]
    pub default_timeout_secs: Option<u64>,

    /// Working directory for hook execution (default: workspace)
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

fn default_enabled() -> bool {
    true
}

impl HooksConfig {
    /// Get hooks for a specific event
    pub fn hooks_for_event(&self, event: HookEvent) -> Vec<&Hook> {
        if !self.enabled {
            return Vec::new();
        }
        self.hooks.iter().filter(|h| h.event == event).collect()
    }

    /// Check if hooks are configured and enabled
    pub fn has_hooks(&self) -> bool {
        self.enabled && !self.hooks.is_empty()
    }
}

/// Context passed to hooks via environment variables
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    /// Tool name (for ToolCallBefore/After)
    pub tool_name: Option<String>,
    /// Tool arguments as JSON string
    pub tool_args: Option<String>,
    /// Tool result output (truncated)
    pub tool_result: Option<String>,
    /// Tool exit code if applicable
    pub tool_exit_code: Option<i32>,
    /// Whether tool succeeded
    pub tool_success: Option<bool>,
    /// Current mode
    pub mode: Option<String>,
    /// Previous mode (for `ModeChange`)
    pub previous_mode: Option<String>,
    /// Session ID
    pub session_id: Option<String>,
    /// User message content
    pub message: Option<String>,
    /// Error message (for `OnError`)
    pub error_message: Option<String>,
    /// Workspace path
    pub workspace: Option<PathBuf>,
    /// Current model name
    pub model: Option<String>,
    /// Total tokens used
    pub total_tokens: Option<u32>,
    /// Session cost in USD
    pub session_cost: Option<f64>,
}

impl HookContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tool_name(mut self, name: &str) -> Self {
        self.tool_name = Some(name.to_string());
        self
    }

    pub fn with_tool_args(mut self, args: &serde_json::Value) -> Self {
        self.tool_args = Some(args.to_string());
        self
    }

    pub fn with_tool_result(mut self, result: &str, success: bool, exit_code: Option<i32>) -> Self {
        self.tool_result = Some(result.to_string());
        self.tool_success = Some(success);
        self.tool_exit_code = exit_code;
        self
    }

    pub fn with_mode(mut self, mode: &str) -> Self {
        self.mode = Some(mode.to_string());
        self
    }

    pub fn with_previous_mode(mut self, mode: &str) -> Self {
        self.previous_mode = Some(mode.to_string());
        self
    }

    pub fn with_workspace(mut self, path: PathBuf) -> Self {
        self.workspace = Some(path);
        self
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.model = Some(model.to_string());
        self
    }

    pub fn with_session_id(mut self, session_id: &str) -> Self {
        self.session_id = Some(session_id.to_string());
        self
    }

    pub fn with_message(mut self, message: &str) -> Self {
        self.message = Some(message.to_string());
        self
    }

    pub fn with_error(mut self, error: &str) -> Self {
        self.error_message = Some(error.to_string());
        self
    }

    pub fn with_tokens(mut self, tokens: u32) -> Self {
        self.total_tokens = Some(tokens);
        self
    }

    pub fn with_cost(mut self, cost: f64) -> Self {
        self.session_cost = Some(cost);
        self
    }

    /// Convert to environment variables
    pub fn to_env_vars(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();

        if let Some(ref name) = self.tool_name {
            env.insert("WAGMII_TOOL_NAME".to_string(), name.clone());
        }
        if let Some(ref args) = self.tool_args {
            env.insert("WAGMII_TOOL_ARGS".to_string(), args.clone());
        }
        if let Some(ref result) = self.tool_result {
            // Truncate result to 10KB to avoid environment variable size limits
            let truncated = if result.len() > 10000 {
                let safe_end = result
                    .char_indices()
                    .take_while(|(i, _)| *i < 10000)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!("{}...[truncated]", &result[..safe_end])
            } else {
                result.clone()
            };
            env.insert("WAGMII_TOOL_RESULT".to_string(), truncated);
        }
        if let Some(code) = self.tool_exit_code {
            env.insert("WAGMII_TOOL_EXIT_CODE".to_string(), code.to_string());
        }
        if let Some(success) = self.tool_success {
            env.insert("WAGMII_TOOL_SUCCESS".to_string(), success.to_string());
        }
        if let Some(ref mode) = self.mode {
            env.insert("WAGMII_MODE".to_string(), mode.clone());
        }
        if let Some(ref prev) = self.previous_mode {
            env.insert("WAGMII_PREVIOUS_MODE".to_string(), prev.clone());
        }
        if let Some(ref session_id) = self.session_id {
            env.insert("WAGMII_SESSION_ID".to_string(), session_id.clone());
        }
        if let Some(ref message) = self.message {
            // Truncate message to prevent env var issues
            let truncated = if message.len() > 5000 {
                let safe_end = message
                    .char_indices()
                    .take_while(|(i, _)| *i < 5000)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                format!("{}...[truncated]", &message[..safe_end])
            } else {
                message.clone()
            };
            env.insert("WAGMII_MESSAGE".to_string(), truncated);
        }
        if let Some(ref error) = self.error_message {
            env.insert("WAGMII_ERROR".to_string(), error.clone());
        }
        if let Some(ref ws) = self.workspace {
            env.insert("WAGMII_WORKSPACE".to_string(), ws.display().to_string());
        }
        if let Some(ref model) = self.model {
            env.insert("WAGMII_MODEL".to_string(), model.clone());
        }
        if let Some(tokens) = self.total_tokens {
            env.insert("WAGMII_TOTAL_TOKENS".to_string(), tokens.to_string());
        }
        if let Some(cost) = self.session_cost {
            env.insert("WAGMII_SESSION_COST".to_string(), format!("{cost:.6}"));
        }

        env
    }
}

/// Result of a hook execution
#[derive(Debug, Clone)]
pub struct HookResult {
    /// Hook name (if specified)
    pub name: Option<String>,
    /// Whether the hook succeeded
    pub success: bool,
    /// Exit code from the hook command
    pub exit_code: Option<i32>,
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Time taken to execute
    pub duration: Duration,
    /// Error message if execution failed
    pub error: Option<String>,
}

/// Executor for running hooks
#[derive(Debug, Clone)]
pub struct HookExecutor {
    config: HooksConfig,
    default_working_dir: PathBuf,
    session_id: String,
}

impl HookExecutor {
    fn build_shell_command(command: &str) -> Command {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(command);
            cmd
        }
        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(command);
            cmd
        }
    }

    /// Create a new `HookExecutor` with configuration
    pub fn new(config: HooksConfig, default_working_dir: PathBuf) -> Self {
        // Generate a session ID
        let session_id = format!("sess_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        Self {
            config,
            default_working_dir,
            session_id,
        }
    }

    /// Create a disabled `HookExecutor` (no hooks will run)
    pub fn disabled() -> Self {
        Self {
            config: HooksConfig {
                enabled: false,
                ..Default::default()
            },
            default_working_dir: PathBuf::from("."),
            session_id: String::new(),
        }
    }

    /// Check if hooks are enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Execute all hooks for an event
    pub fn execute(&self, event: HookEvent, context: &HookContext) -> Vec<HookResult> {
        if !self.config.enabled {
            return Vec::new();
        }

        let hooks = self.config.hooks_for_event(event);
        let env_vars = context.to_env_vars();
        let mut results = Vec::new();

        for hook in hooks {
            if !self.matches_condition(hook, context) {
                continue;
            }

            let result = if hook.background {
                self.execute_background(hook, &env_vars)
            } else {
                self.execute_sync(hook, &env_vars)
            };

            let should_continue = result.success || hook.continue_on_error;
            results.push(result);

            if !should_continue {
                break;
            }
        }

        results
    }

    /// Check if a hook's condition matches the context
    #[allow(clippy::only_used_in_recursion)]
    fn matches_condition(&self, hook: &Hook, context: &HookContext) -> bool {
        match &hook.condition {
            None | Some(HookCondition::Always) => true,
            Some(HookCondition::ToolName { name }) => {
                context.tool_name.as_ref().is_some_and(|n| n == name)
            }
            Some(HookCondition::ToolCategory { category }) => {
                // Map tool names to categories
                let tool_category = context.tool_name.as_ref().map(|name| match name.as_str() {
                    "exec_shell" => "shell",
                    "write_file" | "edit_file" | "apply_patch" => "file_write",
                    "read_file" | "list_dir" | "grep_files" => "safe",
                    _ => "other",
                });
                tool_category.is_some_and(|c| c == category.as_str())
            }
            Some(HookCondition::Mode { mode }) => context
                .mode
                .as_ref()
                .is_some_and(|m| m.to_lowercase() == mode.to_lowercase()),
            Some(HookCondition::ExitCode { code }) => context.tool_exit_code == Some(*code),
            Some(HookCondition::All { conditions }) => conditions.iter().all(|c| {
                self.matches_condition(
                    &Hook {
                        condition: Some(c.clone()),
                        ..hook.clone()
                    },
                    context,
                )
            }),
            Some(HookCondition::Any { conditions }) => conditions.iter().any(|c| {
                self.matches_condition(
                    &Hook {
                        condition: Some(c.clone()),
                        ..hook.clone()
                    },
                    context,
                )
            }),
        }
    }

    /// Execute a hook synchronously
    fn execute_sync(&self, hook: &Hook, env_vars: &HashMap<String, String>) -> HookResult {
        let started = Instant::now();
        let working_dir = self
            .config
            .working_dir
            .clone()
            .unwrap_or_else(|| self.default_working_dir.clone());

        let timeout_secs = self
            .config
            .default_timeout_secs
            .unwrap_or(hook.timeout_secs);
        let timeout = Duration::from_secs(timeout_secs);

        let mut child = match Self::build_shell_command(&hook.command)
            .current_dir(&working_dir)
            .envs(env_vars)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                return HookResult {
                    name: hook.name.clone(),
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: started.elapsed(),
                    error: Some(format!("Failed to spawn hook: {e}")),
                };
            }
        };

        fn read_pipe(mut pipe: impl Read) -> String {
            let mut buf = String::new();
            let _ = pipe.read_to_string(&mut buf);
            buf
        }

        match child.wait_timeout(timeout) {
            Ok(Some(status)) => HookResult {
                name: hook.name.clone(),
                success: status.success(),
                exit_code: status.code(),
                stdout: child.stdout.take().map(read_pipe).unwrap_or_default(),
                stderr: child.stderr.take().map(read_pipe).unwrap_or_default(),
                duration: started.elapsed(),
                error: None,
            },
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                HookResult {
                    name: hook.name.clone(),
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration: started.elapsed(),
                    error: Some(format!("Hook timed out after {}s", timeout_secs)),
                }
            }
            Err(e) => HookResult {
                name: hook.name.clone(),
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                duration: started.elapsed(),
                error: Some(format!("Failed to wait for hook: {e}")),
            },
        }
    }

    /// Execute a hook in the background (non-blocking)
    fn execute_background(&self, hook: &Hook, env_vars: &HashMap<String, String>) -> HookResult {
        let started = Instant::now();
        let working_dir = self
            .config
            .working_dir
            .clone()
            .unwrap_or_else(|| self.default_working_dir.clone());

        let cmd = hook.command.clone();
        let env = env_vars.clone();
        let wd = working_dir.clone();

        // Spawn in a detached thread
        std::thread::spawn(move || {
            let _ = HookExecutor::build_shell_command(&cmd)
                .current_dir(&wd)
                .envs(&env)
                .output();
        });

        // Return immediately with success (background execution is fire-and-forget)
        HookResult {
            name: hook.name.clone(),
            success: true,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration: started.elapsed(),
            error: None,
        }
    }
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn test_hook_event_as_str() {
        assert_eq!(HookEvent::SessionStart.as_str(), "session_start");
        assert_eq!(HookEvent::ToolCallAfter.as_str(), "tool_call_after");
        assert_eq!(HookEvent::ModeChange.as_str(), "mode_change");
    }

    #[test]
    fn test_hook_context_to_env_vars() {
        let ctx = HookContext::new()
            .with_tool_name("exec_shell")
            .with_mode("agent")
            .with_workspace(PathBuf::from("/tmp"));

        let env = ctx.to_env_vars();

        assert_eq!(
            env.get("WAGMII_TOOL_NAME"),
            Some(&"exec_shell".to_string())
        );
        assert_eq!(env.get("WAGMII_MODE"), Some(&"agent".to_string()));
        assert_eq!(env.get("WAGMII_WORKSPACE"), Some(&"/tmp".to_string()));
    }

    #[test]
    fn test_hook_condition_always() {
        let hook = Hook::new(HookEvent::SessionStart, "echo test");
        let executor = HookExecutor::disabled();
        let context = HookContext::new();

        assert!(executor.matches_condition(&hook, &context));
    }

    #[test]
    fn test_hook_condition_tool_name() {
        let hook = Hook::new(HookEvent::ToolCallBefore, "echo test").with_condition(
            HookCondition::ToolName {
                name: "exec_shell".to_string(),
            },
        );

        let executor = HookExecutor::disabled();

        let context_match = HookContext::new().with_tool_name("exec_shell");
        let context_no_match = HookContext::new().with_tool_name("write_file");

        assert!(executor.matches_condition(&hook, &context_match));
        assert!(!executor.matches_condition(&hook, &context_no_match));
    }

    #[test]
    fn test_hook_condition_mode() {
        let hook =
            Hook::new(HookEvent::ModeChange, "echo test").with_condition(HookCondition::Mode {
                mode: "agent".to_string(),
            });

        let executor = HookExecutor::disabled();

        let context_match = HookContext::new().with_mode("AGENT"); // Case insensitive
        let context_no_match = HookContext::new().with_mode("normal");

        assert!(executor.matches_condition(&hook, &context_match));
        assert!(!executor.matches_condition(&hook, &context_no_match));
    }

    #[test]
    fn test_hooks_config_for_event() {
        let config = HooksConfig {
            enabled: true,
            hooks: vec![
                Hook::new(HookEvent::SessionStart, "echo start"),
                Hook::new(HookEvent::SessionEnd, "echo end"),
                Hook::new(HookEvent::SessionStart, "echo start2"),
            ],
            ..Default::default()
        };

        let start_hooks = config.hooks_for_event(HookEvent::SessionStart);
        assert_eq!(start_hooks.len(), 2);

        let end_hooks = config.hooks_for_event(HookEvent::SessionEnd);
        assert_eq!(end_hooks.len(), 1);
    }

    #[test]
    fn test_hooks_config_disabled() {
        let config = HooksConfig {
            enabled: false,
            hooks: vec![Hook::new(HookEvent::SessionStart, "echo start")],
            ..Default::default()
        };

        let hooks = config.hooks_for_event(HookEvent::SessionStart);
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_hook_builder() {
        let hook = Hook::new(HookEvent::ToolCallAfter, "notify.sh")
            .with_name("notify_tool")
            .with_timeout(60)
            .background()
            .with_condition(HookCondition::ToolCategory {
                category: "shell".to_string(),
            });

        assert_eq!(hook.name, Some("notify_tool".to_string()));
        assert_eq!(hook.timeout_secs, 60);
        assert!(hook.background);
        assert!(matches!(
            hook.condition,
            Some(HookCondition::ToolCategory { .. })
        ));
    }

    #[test]
    fn test_hook_timeout_enforced() {
        let command = if cfg!(windows) {
            "ping -n 3 127.0.0.1 > nul"
        } else {
            "sleep 2"
        };
        let hook = Hook::new(HookEvent::SessionStart, command).with_timeout(1);
        let executor = HookExecutor::new(HooksConfig::default(), PathBuf::from("."));
        let env_vars = HashMap::new();

        let result = executor.execute_sync(&hook, &env_vars);
        assert!(!result.success);
        assert!(
            result
                .error
                .as_ref()
                .is_some_and(|e| e.contains("timed out"))
        );
    }

    #[test]
    fn test_executor_session_id() {
        let executor = HookExecutor::new(HooksConfig::default(), PathBuf::from("."));

        assert!(executor.session_id().starts_with("sess_"));
        assert_eq!(executor.session_id().len(), 13); // "sess_" + 8 chars
    }
}
