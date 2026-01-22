//! Advanced shell execution with background process support and sandboxing.
//!
//! Provides:
//! - Synchronous command execution with timeout
//! - Background process execution
//! - Process output retrieval
//! - Process termination
//! - Sandbox support (macOS Seatbelt)
//! - Streaming output (future)

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;
use wait_timeout::ChildExt;

use crate::sandbox::{
    CommandSpec,
    ExecEnv,
    SandboxManager,
    SandboxPolicy as ExecutionSandboxPolicy, // Rename to avoid conflict with spec::SandboxPolicy
    SandboxType,
};

/// Maximum output size before truncation (30KB like Claude Code)
const MAX_OUTPUT_SIZE: usize = 30_000;

/// Status of a shell process
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ShellStatus {
    Running,
    Completed,
    Failed,
    Killed,
    TimedOut,
}

/// Result from a shell command execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellResult {
    pub task_id: Option<String>,
    pub status: ShellStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    /// Whether the command was executed in a sandbox.
    #[serde(default)]
    pub sandboxed: bool,
    /// Type of sandbox used (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_type: Option<String>,
    /// Whether the command was blocked by sandbox restrictions.
    #[serde(default)]
    pub sandbox_denied: bool,
}

/// A background shell process being tracked
pub struct BackgroundShell {
    pub id: String,
    pub command: String,
    pub working_dir: PathBuf,
    pub status: ShellStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub started_at: Instant,
    pub sandbox_type: SandboxType,
    child: Option<Child>,
    stdout_thread: Option<std::thread::JoinHandle<Vec<u8>>>,
    stderr_thread: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl BackgroundShell {
    /// Check if the process has completed and update status
    fn poll(&mut self) -> bool {
        if self.status != ShellStatus::Running {
            return true;
        }

        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.exit_code = status.code();
                    self.status = if status.success() {
                        ShellStatus::Completed
                    } else {
                        ShellStatus::Failed
                    };
                    self.collect_output();
                    true
                }
                Ok(None) => false, // Still running
                Err(_) => {
                    self.status = ShellStatus::Failed;
                    true
                }
            }
        } else {
            true
        }
    }

    /// Collect output from the background threads
    fn collect_output(&mut self) {
        if let Some(handle) = self.stdout_thread.take()
            && let Ok(data) = handle.join()
        {
            self.stdout = String::from_utf8_lossy(&data).to_string();
        }
        if let Some(handle) = self.stderr_thread.take()
            && let Ok(data) = handle.join()
        {
            self.stderr = String::from_utf8_lossy(&data).to_string();
        }
    }

    /// Kill the process
    fn kill(&mut self) -> Result<()> {
        if let Some(ref mut child) = self.child {
            child.kill().context("Failed to kill process")?;
            let _ = child.wait(); // Reap the zombie
            self.status = ShellStatus::Killed;
            self.collect_output();
        }
        Ok(())
    }

    /// Get a snapshot of the current state
    pub fn snapshot(&self) -> ShellResult {
        let sandboxed = !matches!(self.sandbox_type, SandboxType::None);
        ShellResult {
            task_id: Some(self.id.clone()),
            status: self.status.clone(),
            exit_code: self.exit_code,
            stdout: truncate_output(&self.stdout),
            stderr: truncate_output(&self.stderr),
            duration_ms: u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
            sandboxed,
            sandbox_type: if sandboxed {
                Some(self.sandbox_type.to_string())
            } else {
                None
            },
            sandbox_denied: false, // Determined after completion
        }
    }
}

/// Manages background shell processes with optional sandboxing.
pub struct ShellManager {
    processes: HashMap<String, BackgroundShell>,
    default_workspace: PathBuf,
    sandbox_manager: SandboxManager,
    sandbox_policy: ExecutionSandboxPolicy,
}

impl ShellManager {
    /// Create a new `ShellManager` with default (no sandbox) policy.
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            processes: HashMap::new(),
            default_workspace: workspace,
            sandbox_manager: SandboxManager::new(),
            sandbox_policy: ExecutionSandboxPolicy::default(),
        }
    }

    /// Create a new `ShellManager` with a specific sandbox policy.
    pub fn with_sandbox(workspace: PathBuf, policy: ExecutionSandboxPolicy) -> Self {
        Self {
            processes: HashMap::new(),
            default_workspace: workspace,
            sandbox_manager: SandboxManager::new(),
            sandbox_policy: policy,
        }
    }

    /// Set the sandbox policy for future commands.
    pub fn set_sandbox_policy(&mut self, policy: ExecutionSandboxPolicy) {
        self.sandbox_policy = policy;
    }

    /// Get the current sandbox policy.
    pub fn sandbox_policy(&self) -> &ExecutionSandboxPolicy {
        &self.sandbox_policy
    }

    /// Check if sandboxing is available on this platform.
    pub fn is_sandbox_available(&mut self) -> bool {
        self.sandbox_manager.is_available()
    }

    /// Execute a shell command with the configured sandbox policy.
    pub fn execute(
        &mut self,
        command: &str,
        working_dir: Option<&str>,
        timeout_ms: u64,
        background: bool,
    ) -> Result<ShellResult> {
        self.execute_with_policy(command, working_dir, timeout_ms, background, None)
    }

    /// Execute a shell command with a specific sandbox policy (overrides default).
    pub fn execute_with_policy(
        &mut self,
        command: &str,
        working_dir: Option<&str>,
        timeout_ms: u64,
        background: bool,
        policy_override: Option<ExecutionSandboxPolicy>,
    ) -> Result<ShellResult> {
        let work_dir = working_dir.map_or_else(|| self.default_workspace.clone(), PathBuf::from);

        // Clamp timeout to max 10 minutes (600000ms)
        let timeout_ms = timeout_ms.clamp(1000, 600_000);

        // Use override policy if provided, otherwise use the manager's policy
        let policy = policy_override.unwrap_or_else(|| self.sandbox_policy.clone());

        // Create command spec and prepare sandboxed environment
        let spec = CommandSpec::shell(command, work_dir.clone(), Duration::from_millis(timeout_ms))
            .with_policy(policy);
        let exec_env = self.sandbox_manager.prepare(&spec);

        if background {
            self.spawn_background_sandboxed(command, &work_dir, &exec_env)
        } else {
            Self::execute_sync_sandboxed(command, &work_dir, timeout_ms, &exec_env)
        }
    }

    /// Execute a shell command interactively (stdin/stdout/stderr inherit from terminal).
    pub fn execute_interactive(
        &mut self,
        command: &str,
        working_dir: Option<&str>,
        timeout_ms: u64,
    ) -> Result<ShellResult> {
        self.execute_interactive_with_policy(command, working_dir, timeout_ms, None)
    }

    /// Execute a shell command interactively with a specific sandbox policy override.
    pub fn execute_interactive_with_policy(
        &mut self,
        command: &str,
        working_dir: Option<&str>,
        timeout_ms: u64,
        policy_override: Option<ExecutionSandboxPolicy>,
    ) -> Result<ShellResult> {
        let work_dir = working_dir.map_or_else(|| self.default_workspace.clone(), PathBuf::from);

        let timeout_ms = timeout_ms.clamp(1000, 600_000);
        let policy = policy_override.unwrap_or_else(|| self.sandbox_policy.clone());

        let spec = CommandSpec::shell(command, work_dir.clone(), Duration::from_millis(timeout_ms))
            .with_policy(policy);
        let exec_env = self.sandbox_manager.prepare(&spec);

        Self::execute_interactive_sandboxed(command, &work_dir, timeout_ms, &exec_env)
    }

    /// Execute command synchronously with timeout (sandboxed).
    fn execute_sync_sandboxed(
        original_command: &str,
        working_dir: &std::path::Path,
        timeout_ms: u64,
        exec_env: &ExecEnv,
    ) -> Result<ShellResult> {
        let started = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
        let sandbox_type = exec_env.sandbox_type;
        let sandboxed = exec_env.is_sandboxed();

        // Build the command from ExecEnv
        let program = exec_env.program();
        let args = exec_env.args();

        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set environment variables from exec_env
        for (key, value) in &exec_env.env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to execute: {original_command}"))?;

        let stdout_handle = child.stdout.take().context("Failed to capture stdout")?;
        let stderr_handle = child.stderr.take().context("Failed to capture stderr")?;

        // Spawn threads to read output
        let stdout_thread = std::thread::spawn(move || {
            let mut reader = stdout_handle;
            let mut buf = Vec::new();
            let _ = reader.read_to_end(&mut buf);
            buf
        });

        let stderr_thread = std::thread::spawn(move || {
            let mut reader = stderr_handle;
            let mut buf = Vec::new();
            let _ = reader.read_to_end(&mut buf);
            buf
        });

        // Wait with timeout
        if let Some(status) = child.wait_timeout(timeout)? {
            let stdout = stdout_thread.join().unwrap_or_default();
            let stderr = stderr_thread.join().unwrap_or_default();
            let stderr_str = String::from_utf8_lossy(&stderr);
            let exit_code = status.code().unwrap_or(-1);

            // Check if sandbox denied the operation
            let sandbox_denied = SandboxManager::was_denied(sandbox_type, exit_code, &stderr_str);

            Ok(ShellResult {
                task_id: None,
                status: if status.success() {
                    ShellStatus::Completed
                } else {
                    ShellStatus::Failed
                },
                exit_code: status.code(),
                stdout: truncate_output(&String::from_utf8_lossy(&stdout)),
                stderr: truncate_output(&stderr_str),
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                sandboxed,
                sandbox_type: if sandboxed {
                    Some(sandbox_type.to_string())
                } else {
                    None
                },
                sandbox_denied,
            })
        } else {
            // Timeout - kill the process
            let _ = child.kill();
            let status = child.wait().ok();
            let stdout = stdout_thread.join().unwrap_or_default();
            let stderr = stderr_thread.join().unwrap_or_default();

            Ok(ShellResult {
                task_id: None,
                status: ShellStatus::TimedOut,
                exit_code: status.and_then(|s| s.code()),
                stdout: truncate_output(&String::from_utf8_lossy(&stdout)),
                stderr: truncate_output(&String::from_utf8_lossy(&stderr)),
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                sandboxed,
                sandbox_type: if sandboxed {
                    Some(sandbox_type.to_string())
                } else {
                    None
                },
                sandbox_denied: false,
            })
        }
    }

    /// Execute command interactively with timeout (sandboxed).
    fn execute_interactive_sandboxed(
        original_command: &str,
        working_dir: &std::path::Path,
        timeout_ms: u64,
        exec_env: &ExecEnv,
    ) -> Result<ShellResult> {
        let started = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
        let sandbox_type = exec_env.sandbox_type;
        let sandboxed = exec_env.is_sandboxed();

        let program = exec_env.program();
        let args = exec_env.args();

        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(working_dir)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        for (key, value) in &exec_env.env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to execute: {original_command}"))?;

        if let Some(status) = child.wait_timeout(timeout)? {
            Ok(ShellResult {
                task_id: None,
                status: if status.success() {
                    ShellStatus::Completed
                } else {
                    ShellStatus::Failed
                },
                exit_code: status.code(),
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                sandboxed,
                sandbox_type: if sandboxed {
                    Some(sandbox_type.to_string())
                } else {
                    None
                },
                sandbox_denied: false,
            })
        } else {
            let _ = child.kill();
            let status = child.wait().ok();

            Ok(ShellResult {
                task_id: None,
                status: ShellStatus::TimedOut,
                exit_code: status.and_then(|s| s.code()),
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                sandboxed,
                sandbox_type: if sandboxed {
                    Some(sandbox_type.to_string())
                } else {
                    None
                },
                sandbox_denied: false,
            })
        }
    }

    /// Spawn a background process (sandboxed).
    fn spawn_background_sandboxed(
        &mut self,
        original_command: &str,
        working_dir: &std::path::Path,
        exec_env: &ExecEnv,
    ) -> Result<ShellResult> {
        let task_id = format!("shell_{}", &Uuid::new_v4().to_string()[..8]);
        let started = Instant::now();
        let sandbox_type = exec_env.sandbox_type;
        let sandboxed = exec_env.is_sandboxed();

        // Build the command from ExecEnv
        let program = exec_env.program();
        let args = exec_env.args();

        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set environment variables from exec_env
        for (key, value) in &exec_env.env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn background: {original_command}"))?;

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // Spawn threads to collect output
        let stdout_thread = stdout_handle.map(|handle| {
            std::thread::spawn(move || {
                let mut reader = handle;
                let mut buf = Vec::new();
                let _ = reader.read_to_end(&mut buf);
                buf
            })
        });

        let stderr_thread = stderr_handle.map(|handle| {
            std::thread::spawn(move || {
                let mut reader = handle;
                let mut buf = Vec::new();
                let _ = reader.read_to_end(&mut buf);
                buf
            })
        });

        let bg_shell = BackgroundShell {
            id: task_id.clone(),
            command: original_command.to_string(),
            working_dir: working_dir.to_path_buf(),
            status: ShellStatus::Running,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            started_at: started,
            sandbox_type,
            child: Some(child),
            stdout_thread,
            stderr_thread,
        };

        self.processes.insert(task_id.clone(), bg_shell);

        Ok(ShellResult {
            task_id: Some(task_id),
            status: ShellStatus::Running,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            sandboxed,
            sandbox_type: if sandboxed {
                Some(sandbox_type.to_string())
            } else {
                None
            },
            sandbox_denied: false,
        })
    }

    /// Get output from a background process
    pub fn get_output(
        &mut self,
        task_id: &str,
        block: bool,
        timeout_ms: u64,
    ) -> Result<ShellResult> {
        let shell = self
            .processes
            .get_mut(task_id)
            .ok_or_else(|| anyhow!("Task {task_id} not found"))?;

        if block && shell.status == ShellStatus::Running {
            let timeout = Duration::from_millis(timeout_ms.clamp(1000, 600_000));
            let deadline = Instant::now() + timeout;

            while shell.status == ShellStatus::Running && Instant::now() < deadline {
                if shell.poll() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            // If still running after timeout
            if shell.status == ShellStatus::Running {
                return Ok(shell.snapshot());
            }
        } else {
            shell.poll();
        }

        Ok(shell.snapshot())
    }

    /// Kill a running background process
    pub fn kill(&mut self, task_id: &str) -> Result<ShellResult> {
        let shell = self
            .processes
            .get_mut(task_id)
            .ok_or_else(|| anyhow!("Task {task_id} not found"))?;

        shell.kill()?;
        Ok(shell.snapshot())
    }

    /// List all background processes
    pub fn list(&mut self) -> Vec<ShellResult> {
        // Poll all processes first
        for shell in self.processes.values_mut() {
            shell.poll();
        }

        self.processes
            .values()
            .map(BackgroundShell::snapshot)
            .collect()
    }

    /// Clean up completed processes older than the given duration
    pub fn cleanup(&mut self, max_age: Duration) {
        let _now = Instant::now();
        self.processes.retain(|_, shell| {
            if shell.status == ShellStatus::Running {
                true
            } else {
                shell.started_at.elapsed() < max_age
            }
        });
    }
}

/// Truncate output to `MAX_OUTPUT_SIZE`
fn truncate_output(output: &str) -> String {
    if output.len() <= MAX_OUTPUT_SIZE {
        output.to_string()
    } else {
        let truncated = &output[..MAX_OUTPUT_SIZE];
        format!(
            "{}...\n\n[Output truncated at {} characters. {} characters omitted.]",
            truncated,
            MAX_OUTPUT_SIZE,
            output.len() - MAX_OUTPUT_SIZE
        )
    }
}

/// Thread-safe wrapper for `ShellManager`
pub type SharedShellManager = Arc<Mutex<ShellManager>>;

/// Create a new shared shell manager with default sandbox policy.
pub fn new_shared_shell_manager(workspace: PathBuf) -> SharedShellManager {
    Arc::new(Mutex::new(ShellManager::new(workspace)))
}

/// Create a new shared shell manager with a specific sandbox policy.
pub fn new_shared_shell_manager_with_sandbox(
    workspace: PathBuf,
    policy: ExecutionSandboxPolicy,
) -> SharedShellManager {
    Arc::new(Mutex::new(ShellManager::with_sandbox(workspace, policy)))
}

// === ToolSpec Implementations ===

use crate::command_safety::{SafetyLevel, analyze_command};
use crate::execpolicy::{ExecPolicyDecision, load_default_policy};
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_bool, optional_u64, required_str,
};
use async_trait::async_trait;
use serde_json::json;

/// Tool for executing shell commands.
pub struct ExecShellTool;

#[async_trait]
impl ToolSpec for ExecShellTool {
    fn name(&self) -> &'static str {
        "exec_shell"
    }

    fn description(&self) -> &'static str {
        "Execute a shell command in the workspace directory. Returns stdout, stderr, and exit code."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 120000, max: 600000)"
                },
                "background": {
                    "type": "boolean",
                    "description": "Run in background and return task_id (default: false)"
                },
                "interactive": {
                    "type": "boolean",
                    "description": "Run interactively with terminal IO (default: false)"
                }
            },
            "required": ["command"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::Sandboxable,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let command = required_str(&input, "command")?;
        let timeout_ms = optional_u64(&input, "timeout_ms", 120_000).min(600_000);
        let background = optional_bool(&input, "background", false);
        let interactive = optional_bool(&input, "interactive", false);

        if interactive && background {
            return Ok(ToolResult::error(
                "Interactive commands cannot run in background mode.",
            ));
        }

        let mut execpolicy_decision: Option<ExecPolicyDecision> = None;
        if let Some(policy) = load_default_policy()
            .map_err(|e| ToolError::execution_failed(format!("execpolicy load failed: {e}")))?
        {
            let decision = policy.evaluate(command);
            execpolicy_decision = Some(decision.clone());
            if let ExecPolicyDecision::Deny(reason) = decision {
                return Ok(ToolResult {
                    content: format!("BLOCKED: {reason}"),
                    success: false,
                    metadata: Some(json!({
                        "execpolicy": {
                            "decision": "deny",
                            "reason": reason,
                        }
                    })),
                });
            }
        }

        // Safety analysis (always run for metadata, but only block when not in YOLO mode)
        let safety = analyze_command(command);
        if !context.auto_approve {
            match safety.level {
                SafetyLevel::Dangerous => {
                    let reasons = safety.reasons.join("; ");
                    let suggestions = if safety.suggestions.is_empty() {
                        String::new()
                    } else {
                        format!("\nSuggestions: {}", safety.suggestions.join("; "))
                    };
                    return Ok(ToolResult {
                        content: format!(
                            "BLOCKED: This command was blocked for safety reasons.\n\nReasons: {reasons}{suggestions}"
                        ),
                        success: false,
                        metadata: Some(json!({
                            "safety_level": "dangerous",
                            "blocked": true,
                            "reasons": safety.reasons,
                            "suggestions": safety.suggestions,
                        })),
                    });
                }
                SafetyLevel::RequiresApproval | SafetyLevel::Safe | SafetyLevel::WorkspaceSafe => {
                    // Proceed normally
                }
            }
        }

        // Create a shell manager for this execution
        // If there's an elevated sandbox policy, use it; otherwise use default
        let mut manager = if let Some(ref policy) = context.elevated_sandbox_policy {
            ShellManager::with_sandbox(context.workspace.clone(), policy.clone())
        } else {
            ShellManager::new(context.workspace.clone())
        };

        // Pass the elevated policy as override if set
        let policy_override = context.elevated_sandbox_policy.clone();

        let result = if interactive {
            manager.execute_interactive(command, None, timeout_ms)
        } else {
            manager.execute_with_policy(command, None, timeout_ms, background, policy_override)
        };

        match result {
            Ok(result) => {
                let task_id_str = result.task_id.clone().unwrap_or_default();
                let output = if interactive {
                    format!(
                        "Interactive command completed (exit code: {:?})",
                        result.exit_code
                    )
                } else if result.status == ShellStatus::Completed {
                    if result.stdout.is_empty() && result.stderr.is_empty() {
                        "(no output)".to_string()
                    } else if result.stderr.is_empty() {
                        result.stdout.clone()
                    } else {
                        format!("{}\n\nSTDERR:\n{}", result.stdout, result.stderr)
                    }
                } else if result.status == ShellStatus::Running {
                    format!("Background task started: {task_id_str}")
                } else {
                    format!(
                        "Command failed (exit code: {:?})\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
                        result.exit_code, result.stdout, result.stderr
                    )
                };

                Ok(ToolResult {
                    content: output,
                    success: result.status == ShellStatus::Completed
                        || result.status == ShellStatus::Running,
                    metadata: Some(json!({
                        "exit_code": result.exit_code,
                        "status": format!("{:?}", result.status),
                        "duration_ms": result.duration_ms,
                        "sandboxed": result.sandboxed,
                        "task_id": result.task_id,
                        "safety_level": format!("{:?}", safety.level),
                        "interactive": interactive,
                        "execpolicy": execpolicy_decision.as_ref().map(|decision| match decision {
                            ExecPolicyDecision::Allow => json!({
                                "decision": "allow",
                            }),
                            ExecPolicyDecision::Deny(reason) => json!({
                                "decision": "deny",
                                "reason": reason,
                            }),
                            ExecPolicyDecision::AskUser(reason) => json!({
                                "decision": "ask_user",
                                "reason": reason,
                            }),
                        }),
                    })),
                })
            }
            Err(e) => Ok(ToolResult::error(format!("Shell execution failed: {e}"))),
        }
    }
}

/// Tool for appending notes to a notes file.
pub struct NoteTool;

#[async_trait]
impl ToolSpec for NoteTool {
    fn name(&self) -> &'static str {
        "note"
    }

    fn description(&self) -> &'static str {
        "Append a note to the agent notes file for persistent context across sessions."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The note content to append"
                }
            },
            "required": ["content"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto // Notes are low-risk
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let note_content = required_str(&input, "content")?;

        // Ensure parent directory exists
        if let Some(parent) = context.notes_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ToolError::execution_failed(format!("Failed to create notes directory: {e}"))
            })?;
        }

        // Append to notes file
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&context.notes_path)
            .map_err(|e| ToolError::execution_failed(format!("Failed to open notes file: {e}")))?;

        writeln!(file, "\n---\n{note_content}")
            .map_err(|e| ToolError::execution_failed(format!("Failed to write note: {e}")))?;

        Ok(ToolResult::success(format!(
            "Note appended to {}",
            context.notes_path.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn echo_command(message: &str) -> String {
        format!("echo {message}")
    }

    fn sleep_command(seconds: u64) -> String {
        #[cfg(windows)]
        {
            let ping_count = seconds.saturating_add(1);
            let ps_path = r#"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"#;
            format!(
                "\"{ps_path}\" -NoProfile -Command \"Start-Sleep -Seconds {seconds}\" || ping 127.0.0.1 -n {ping_count} > NUL"
            )
        }
        #[cfg(not(windows))]
        {
            format!("sleep {seconds}")
        }
    }

    fn sleep_then_echo_command(seconds: u64, message: &str) -> String {
        #[cfg(windows)]
        {
            let ping_count = seconds.saturating_add(1);
            let ps_path = r#"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"#;
            format!(
                "\"{ps_path}\" -NoProfile -Command \"Start-Sleep -Seconds {seconds}; Write-Output {message}\" || (ping 127.0.0.1 -n {ping_count} > NUL && echo {message})"
            )
        }
        #[cfg(not(windows))]
        {
            format!("sleep {seconds} && echo {message}")
        }
    }

    #[test]
    fn test_sync_execution() {
        let tmp = tempdir().expect("tempdir");
        let mut manager = ShellManager::new(tmp.path().to_path_buf());

        let result = manager
            .execute(&echo_command("hello"), None, 5000, false)
            .expect("execute");

        assert_eq!(result.status, ShellStatus::Completed);
        assert!(result.stdout.contains("hello"));
        assert!(result.task_id.is_none());
    }

    #[test]
    fn test_background_execution() {
        let tmp = tempdir().expect("tempdir");
        let mut manager = ShellManager::new(tmp.path().to_path_buf());

        let result = manager
            .execute(&sleep_then_echo_command(1, "done"), None, 5000, true)
            .expect("execute");

        assert_eq!(result.status, ShellStatus::Running);
        assert!(result.task_id.is_some());

        let task_id = result
            .task_id
            .expect("background execution should return task_id");

        // Wait for completion
        let final_result = manager
            .get_output(&task_id, true, 5000)
            .expect("get_output");

        assert_eq!(final_result.status, ShellStatus::Completed);
        assert!(final_result.stdout.contains("done"));
    }

    #[test]
    fn test_timeout() {
        let tmp = tempdir().expect("tempdir");
        let mut manager = ShellManager::new(tmp.path().to_path_buf());

        let result = manager
            .execute(&sleep_command(10), None, 1000, false)
            .expect("execute");

        assert_eq!(result.status, ShellStatus::TimedOut);
    }

    #[test]
    fn test_kill() {
        let tmp = tempdir().expect("tempdir");
        let mut manager = ShellManager::new(tmp.path().to_path_buf());

        let result = manager
            .execute(&sleep_command(60), None, 5000, true)
            .expect("execute");

        let task_id = result
            .task_id
            .expect("background execution should return task_id");

        // Kill it
        let killed = manager.kill(&task_id).expect("kill");
        assert_eq!(killed.status, ShellStatus::Killed);
    }

    #[test]
    fn test_output_truncation() {
        let long_output = "x".repeat(50_000);
        let truncated = truncate_output(&long_output);

        assert!(truncated.len() < long_output.len());
        assert!(truncated.contains("truncated"));
    }
}
