//! Core engine for `DeepSeek` CLI.
//!
//! The engine handles all AI interactions in a background task,
//! communicating with the UI via channels. This enables:
//! - Non-blocking UI during API calls
//! - Real-time streaming updates
//! - Proper cancellation support
//! - Tool execution orchestration

use std::fmt::Write;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use serde_json::json;
use tokio::sync::{Mutex as AsyncMutex, RwLock, mpsc};
use tokio_util::sync::CancellationToken;

use crate::client::DeepSeekClient;
use crate::compaction::{
    CompactionConfig, compact_messages_safe, merge_system_prompts, should_compact,
};
use crate::config::Config;
use crate::config::DEFAULT_MAX_SUBAGENTS;
use crate::duo::{DuoSession, SharedDuoSession, session_summary as duo_session_summary};
use crate::features::{Feature, Features};
use crate::llm_client::LlmClient;
use crate::mcp::McpPool;
use crate::models::{
    ContentBlock, ContentBlockStart, Delta, Message, MessageRequest, StreamEvent, Tool, Usage,
};
use crate::prompts;
use crate::rlm::{RlmSession, SharedRlmSession, session_summary as rlm_session_summary};
use crate::tools::plan::{SharedPlanState, new_shared_plan_state};
use crate::tools::spec::{ApprovalRequirement, ToolError, ToolResult};
use crate::tools::subagent::{
    SharedSubAgentManager, SubAgentRuntime, SubAgentType, new_shared_subagent_manager,
};
use crate::tools::todo::{SharedTodoList, new_shared_todo_list};
use crate::tools::{ToolContext, ToolRegistryBuilder};
use crate::tui::app::AppMode;

use super::events::Event;
use super::ops::Op;
use super::session::Session;
use super::tool_parser;
use super::turn::{TurnContext, TurnToolCall};

// === Types ===

/// Configuration for the engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Model identifier to use for responses.
    pub model: String,
    /// Workspace root for tool execution and file operations.
    pub workspace: PathBuf,
    /// Allow shell tool execution when true.
    pub allow_shell: bool,
    /// Enable trust mode (skip approvals) when true.
    pub trust_mode: bool,
    /// Path to the notes file used by the notes tool.
    pub notes_path: PathBuf,
    /// Path to the MCP configuration file.
    pub mcp_config_path: PathBuf,
    /// Maximum number of assistant steps before stopping.
    pub max_steps: u32,
    /// Maximum number of concurrently active subagents.
    pub max_subagents: usize,
    /// Feature flags controlling tool availability.
    pub features: Features,
    /// Shared RLM session state.
    pub rlm_session: SharedRlmSession,
    /// Shared Duo session state.
    pub duo_session: SharedDuoSession,
    /// Auto-compaction settings for long conversations.
    pub compaction: CompactionConfig,
    /// Shared Todo list state.
    pub todos: SharedTodoList,
    /// Shared Plan state.
    pub plan_state: SharedPlanState,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            model: "deepseek-reasoner".to_string(),
            workspace: PathBuf::from("."),
            allow_shell: false,
            trust_mode: false,
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            max_steps: 100,
            max_subagents: DEFAULT_MAX_SUBAGENTS,
            features: Features::with_defaults(),
            rlm_session: Arc::new(Mutex::new(RlmSession::default())),
            duo_session: Arc::new(Mutex::new(DuoSession::new())),
            compaction: CompactionConfig::default(),
            todos: new_shared_todo_list(),
            plan_state: new_shared_plan_state(),
        }
    }
}

/// Handle to communicate with the engine
#[derive(Clone)]
pub struct EngineHandle {
    /// Send operations to the engine
    pub tx_op: mpsc::Sender<Op>,
    /// Receive events from the engine
    pub rx_event: Arc<RwLock<mpsc::Receiver<Event>>>,
    /// Cancellation token for the current request
    cancel_token: CancellationToken,
    /// Send approval decisions to the engine
    tx_approval: mpsc::Sender<ApprovalDecision>,
}

impl EngineHandle {
    /// Send an operation to the engine
    pub async fn send(&self, op: Op) -> Result<()> {
        self.tx_op.send(op).await?;
        Ok(())
    }

    /// Cancel the current request
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Check if a request is currently cancelled
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Approve a pending tool call
    pub async fn approve_tool_call(&self, id: impl Into<String>) -> Result<()> {
        self.tx_approval
            .send(ApprovalDecision::Approved { id: id.into() })
            .await?;
        Ok(())
    }

    /// Deny a pending tool call
    pub async fn deny_tool_call(&self, id: impl Into<String>) -> Result<()> {
        self.tx_approval
            .send(ApprovalDecision::Denied { id: id.into() })
            .await?;
        Ok(())
    }

    /// Retry a tool call with an elevated sandbox policy.
    pub async fn retry_tool_with_policy(
        &self,
        id: impl Into<String>,
        policy: crate::sandbox::SandboxPolicy,
    ) -> Result<()> {
        self.tx_approval
            .send(ApprovalDecision::RetryWithPolicy {
                id: id.into(),
                policy,
            })
            .await?;
        Ok(())
    }
}

// === Engine ===

/// The core engine that processes operations and emits events
pub struct Engine {
    config: EngineConfig,
    deepseek_client: Option<DeepSeekClient>,
    deepseek_client_error: Option<String>,
    session: Session,
    subagent_manager: SharedSubAgentManager,
    mcp_pool: Option<Arc<AsyncMutex<McpPool>>>,
    rx_op: mpsc::Receiver<Op>,
    rx_approval: mpsc::Receiver<ApprovalDecision>,
    tx_event: mpsc::Sender<Event>,
    cancel_token: CancellationToken,
    tool_exec_lock: Arc<RwLock<()>>,
}

#[derive(Debug, Clone)]
enum ApprovalDecision {
    Approved {
        id: String,
    },
    Denied {
        id: String,
    },
    /// Retry a tool with an elevated sandbox policy.
    RetryWithPolicy {
        id: String,
        policy: crate::sandbox::SandboxPolicy,
    },
}

/// Result of awaiting tool approval from the user.
#[derive(Debug)]
enum ApprovalResult {
    /// User approved the tool execution.
    Approved,
    /// User denied the tool execution.
    Denied,
    /// User requested retry with an elevated sandbox policy.
    RetryWithPolicy(crate::sandbox::SandboxPolicy),
}

// === Internal stream helpers ===

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContentBlockKind {
    Text,
    Thinking,
    ToolUse,
}

#[derive(Debug, Clone)]
struct ToolUseState {
    id: String,
    name: String,
    input: serde_json::Value,
    input_buffer: String,
}

struct ToolExecOutcome {
    index: usize,
    id: String,
    name: String,
    input: serde_json::Value,
    started_at: Instant,
    result: Result<ToolResult, ToolError>,
}

#[derive(Debug, Clone)]
struct ToolExecutionPlan {
    index: usize,
    id: String,
    name: String,
    input: serde_json::Value,
    interactive: bool,
    approval_required: bool,
    approval_description: String,
    supports_parallel: bool,
    read_only: bool,
}

// Hold the lock guard for the duration of a tool execution.
enum ToolExecGuard<'a> {
    Read(tokio::sync::RwLockReadGuard<'a, ()>),
    Write(tokio::sync::RwLockWriteGuard<'a, ()>),
}

const TOOL_CALL_START_MARKERS: [&str; 5] = [
    "[TOOL_CALL]",
    "<deepseek:tool_call",
    "<tool_call",
    "<invoke ",
    "<function_calls>",
];
const TOOL_CALL_END_MARKERS: [&str; 5] = [
    "[/TOOL_CALL]",
    "</deepseek:tool_call>",
    "</tool_call>",
    "</invoke>",
    "</function_calls>",
];

fn find_first_marker(text: &str, markers: &[&str]) -> Option<(usize, usize)> {
    markers
        .iter()
        .filter_map(|marker| text.find(marker).map(|idx| (idx, marker.len())))
        .min_by_key(|(idx, _)| *idx)
}

fn filter_tool_call_delta(delta: &str, in_tool_call: &mut bool) -> String {
    if delta.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    let mut rest = delta;

    loop {
        if *in_tool_call {
            let Some((idx, len)) = find_first_marker(rest, &TOOL_CALL_END_MARKERS) else {
                break;
            };
            rest = &rest[idx + len..];
            *in_tool_call = false;
        } else {
            let Some((idx, len)) = find_first_marker(rest, &TOOL_CALL_START_MARKERS) else {
                output.push_str(rest);
                break;
            };
            output.push_str(&rest[..idx]);
            rest = &rest[idx + len..];
            *in_tool_call = true;
        }
    }

    output
}

fn parse_tool_input(buffer: &str) -> Option<serde_json::Value> {
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Some(value);
    }
    if let Some(stripped) = strip_code_fences(trimmed)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&stripped)
    {
        return Some(value);
    }
    if let Ok(serde_json::Value::String(inner)) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&inner)
    {
        return Some(value);
    }
    extract_json_segment(trimmed)
        .and_then(|segment| serde_json::from_str::<serde_json::Value>(&segment).ok())
}

fn strip_code_fences(text: &str) -> Option<String> {
    if !text.contains("```") {
        return None;
    }
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            continue;
        }
        lines.push(line);
    }
    let stripped = lines.join("\n");
    let stripped = stripped.trim();
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

fn extract_json_segment(text: &str) -> Option<String> {
    extract_balanced_segment(text, '{', '}').or_else(|| extract_balanced_segment(text, '[', ']'))
}

fn extract_balanced_segment(text: &str, open: char, close: char) -> Option<String> {
    let start = text.find(open)?;
    let mut depth = 0i32;
    let mut end = None;
    for (offset, ch) in text[start..].char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                end = Some(start + offset + ch.len_utf8());
                break;
            }
        }
    }
    end.map(|end_idx| text[start..end_idx].to_string())
}

fn should_parallelize_tool_batch(plans: &[ToolExecutionPlan]) -> bool {
    !plans.is_empty()
        && plans.iter().all(|plan| {
            plan.read_only && plan.supports_parallel && !plan.approval_required && !plan.interactive
        })
}

fn format_tool_error(err: &ToolError, tool_name: &str) -> String {
    match err {
        ToolError::InvalidInput { message } => {
            format!("Invalid input for tool '{tool_name}': {message}")
        }
        ToolError::MissingField { field } => {
            format!("Tool '{tool_name}' is missing required field '{field}'")
        }
        ToolError::PathEscape { path } => format!(
            "Path escapes workspace: {}. Use a workspace-relative path or enable trust mode.",
            path.display()
        ),
        ToolError::ExecutionFailed { message } => message.clone(),
        ToolError::Timeout { seconds } => format!(
            "Tool '{tool_name}' timed out after {seconds}s. Try a narrower scope or a longer timeout."
        ),
        ToolError::NotAvailable { message } => format!(
            "Tool '{tool_name}' is not available: {message}. Check mode, feature flags, or tool name."
        ),
        ToolError::PermissionDenied { message } => format!(
            "Tool '{tool_name}' was denied: {message}. Adjust approval mode or request permission."
        ),
    }
}

impl Engine {
    /// Create a new engine with the given configuration
    pub fn new(config: EngineConfig, api_config: &Config) -> (Self, EngineHandle) {
        let (tx_op, rx_op) = mpsc::channel(32);
        let (tx_event, rx_event) = mpsc::channel(256);
        let (tx_approval, rx_approval) = mpsc::channel(64);
        let cancel_token = CancellationToken::new();
        let tool_exec_lock = Arc::new(RwLock::new(()));

        // Create clients for both providers
        let (deepseek_client, deepseek_client_error) = match DeepSeekClient::new(api_config) {
            Ok(client) => (Some(client), None),
            Err(err) => (None, Some(err.to_string())),
        };

        let mut session = Session::new(
            config.model.clone(),
            config.workspace.clone(),
            config.allow_shell,
            config.trust_mode,
            config.notes_path.clone(),
            config.mcp_config_path.clone(),
        );

        // Set up system prompt with project context (default to agent mode)
        let working_set_summary = session.working_set.summary_block(&config.workspace);
        let system_prompt = prompts::system_prompt_for_mode_with_context(
            AppMode::Agent,
            &config.workspace,
            working_set_summary.as_deref(),
            None,
            None,
        );
        session.system_prompt = Some(system_prompt);

        let subagent_manager =
            new_shared_subagent_manager(config.workspace.clone(), config.max_subagents);

        let engine = Engine {
            config,
            deepseek_client,
            deepseek_client_error,
            session,
            subagent_manager,
            mcp_pool: None,
            rx_op,
            rx_approval,
            tx_event,
            cancel_token: cancel_token.clone(),
            tool_exec_lock,
        };

        let handle = EngineHandle {
            tx_op,
            rx_event: Arc::new(RwLock::new(rx_event)),
            cancel_token,
            tx_approval,
        };

        (engine, handle)
    }

    /// Run the engine event loop
    #[allow(clippy::too_many_lines)]
    pub async fn run(mut self) {
        while let Some(op) = self.rx_op.recv().await {
            match op {
                Op::SendMessage {
                    content,
                    mode,
                    model,
                    allow_shell,
                    trust_mode,
                } => {
                    self.handle_send_message(content, mode, model, allow_shell, trust_mode)
                        .await;
                }
                Op::CancelRequest => {
                    self.cancel_token.cancel();
                    // Create a new token for the next request
                    self.cancel_token = CancellationToken::new();
                }
                Op::ApproveToolCall { id } => {
                    // Tool approval handling will be implemented in tools module
                    let _ = self
                        .tx_event
                        .send(Event::status(format!("Approved tool call: {id}")))
                        .await;
                }
                Op::DenyToolCall { id } => {
                    let _ = self
                        .tx_event
                        .send(Event::status(format!("Denied tool call: {id}")))
                        .await;
                }
                Op::SpawnSubAgent { prompt } => {
                    let Some(client) = self.deepseek_client.clone() else {
                        let message = self
                            .deepseek_client_error
                            .as_deref()
                            .map(|err| format!("Failed to spawn sub-agent: {err}"))
                            .unwrap_or_else(|| {
                                "Failed to spawn sub-agent: API client not configured".to_string()
                            });
                        let _ = self.tx_event.send(Event::error(message, false)).await;
                        continue;
                    };

                    let runtime = SubAgentRuntime::new(
                        client,
                        self.session.model.clone(),
                        // Sub-agents don't inherit YOLO mode - use Agent mode defaults
                        self.build_tool_context(AppMode::Agent),
                        self.session.allow_shell,
                        Some(self.tx_event.clone()),
                    );

                    let result = {
                        let mut manager = self.subagent_manager.lock().await;
                        manager.spawn_background(
                            Arc::clone(&self.subagent_manager),
                            runtime,
                            SubAgentType::General,
                            prompt.clone(),
                            None,
                        )
                    };

                    match result {
                        Ok(snapshot) => {
                            let _ = self
                                .tx_event
                                .send(Event::status(format!(
                                    "Spawned sub-agent {}",
                                    snapshot.agent_id
                                )))
                                .await;
                        }
                        Err(err) => {
                            let _ = self
                                .tx_event
                                .send(Event::error(
                                    format!("Failed to spawn sub-agent: {err}"),
                                    false,
                                ))
                                .await;
                        }
                    }
                }
                Op::ListSubAgents => {
                    let agents = {
                        let manager = self.subagent_manager.lock().await;
                        manager.list()
                    };
                    let _ = self.tx_event.send(Event::AgentList { agents }).await;
                }
                Op::ChangeMode { mode } => {
                    let _ = self
                        .tx_event
                        .send(Event::status(format!("Mode changed to: {mode:?}")))
                        .await;
                }
                Op::SetModel { model } => {
                    self.session.model = model;
                    self.config.model.clone_from(&self.session.model);
                    let _ = self
                        .tx_event
                        .send(Event::status(format!(
                            "Model set to: {}",
                            self.session.model
                        )))
                        .await;
                }
                Op::SetCompaction { config } => {
                    let enabled = config.enabled;
                    self.config.compaction = config;
                    let _ = self
                        .tx_event
                        .send(Event::status(format!(
                            "Auto-compaction {}",
                            if enabled { "enabled" } else { "disabled" }
                        )))
                        .await;
                }
                Op::SyncSession {
                    messages,
                    system_prompt,
                    model,
                    workspace,
                } => {
                    self.session.messages = messages;
                    self.session.system_prompt = system_prompt;
                    self.session.model = model;
                    self.session.workspace = workspace.clone();
                    self.config.model.clone_from(&self.session.model);
                    self.config.workspace = workspace.clone();
                    let ctx = crate::project_context::load_project_context_with_parents(&workspace);
                    self.session.project_context = if ctx.has_instructions() {
                        Some(ctx)
                    } else {
                        None
                    };
                    self.session.rebuild_working_set();
                    let _ = self
                        .tx_event
                        .send(Event::status("Session context synced".to_string()))
                        .await;
                }
                Op::Shutdown => {
                    break;
                }
            }
        }
    }

    /// Handle a send message operation
    async fn handle_send_message(
        &mut self,
        content: String,
        mode: AppMode,
        model: String,
        allow_shell: bool,
        trust_mode: bool,
    ) {
        // Reset cancel token for fresh turn (in case previous was cancelled)
        self.cancel_token = CancellationToken::new();

        // Emit turn started event
        let _ = self.tx_event.send(Event::TurnStarted).await;

        // Check if we have the appropriate client
        if self.deepseek_client.is_none() {
            let message = self
                .deepseek_client_error
                .as_deref()
                .map(|err| format!("Failed to send message: {err}"))
                .unwrap_or_else(|| "Failed to send message: API client not configured".to_string());
            let _ = self.tx_event.send(Event::error(message, false)).await;
            return;
        }

        self.session
            .working_set
            .observe_user_message(&content, &self.session.workspace);

        // Add user message to session
        let user_msg = Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: content,
                cache_control: None,
            }],
        };
        self.session.add_message(user_msg);

        // Create turn context
        let mut turn = TurnContext::new(self.config.max_steps);

        self.session.model = model;
        self.config.model.clone_from(&self.session.model);
        self.session.allow_shell = allow_shell;
        self.config.allow_shell = allow_shell;
        self.session.trust_mode = trust_mode;
        self.config.trust_mode = trust_mode;

        // Update system prompt to match the current mode
        let rlm_summary = if mode == AppMode::Rlm {
            self.config
                .rlm_session
                .lock()
                .ok()
                .map(|session| rlm_session_summary(&session))
        } else {
            None
        };
        let duo_summary = if mode == AppMode::Duo {
            self.config
                .duo_session
                .lock()
                .ok()
                .map(|s| duo_session_summary(&s))
        } else {
            None
        };
        let working_set_summary = self
            .session
            .working_set
            .summary_block(&self.config.workspace);
        self.session.system_prompt = Some(prompts::system_prompt_for_mode_with_context(
            mode,
            &self.config.workspace,
            working_set_summary.as_deref(),
            rlm_summary.as_deref(),
            duo_summary.as_deref(),
        ));

        // Build tool registry and tool list for the current mode
        let todo_list = self.config.todos.clone();
        let plan_state = self.config.plan_state.clone();

        let tool_context = self.build_tool_context(mode);
        let mut builder = if mode == AppMode::Plan {
            ToolRegistryBuilder::new()
                .with_read_only_file_tools()
                .with_search_tools()
                .with_git_tools()
                .with_diagnostics_tool()
                .with_todo_tool(todo_list.clone())
                .with_plan_tool(plan_state.clone())
        } else {
            let rlm_opt = if self.config.features.enabled(Feature::Rlm) {
                Some(self.config.rlm_session.clone())
            } else {
                None
            };

            ToolRegistryBuilder::new()
                .with_agent_tools(
                    self.session.allow_shell,
                    rlm_opt,
                    self.deepseek_client.clone(),
                    self.session.model.clone(),
                )
                .with_todo_tool(todo_list.clone())
                .with_plan_tool(plan_state.clone())
        };

        builder =
            builder.with_review_tool(self.deepseek_client.clone(), self.session.model.clone());

        if self.config.features.enabled(Feature::ApplyPatch) && mode != AppMode::Plan {
            builder = builder.with_patch_tools();
        }
        if self.config.features.enabled(Feature::WebSearch) {
            builder = builder.with_web_tools();
        }
        if self.config.features.enabled(Feature::ShellTool)
            && self.session.allow_shell
            && mode != AppMode::Plan
        {
            builder = builder.with_shell_tools();
        }
        if mode == AppMode::Rlm {
            if self.config.features.enabled(Feature::Rlm) {
                builder = builder.with_rlm_tools(
                    self.config.rlm_session.clone(),
                    self.deepseek_client.clone(),
                    self.session.model.clone(),
                );
            } else {
                let _ = self
                    .tx_event
                    .send(Event::status("RLM tools are disabled by feature flags"))
                    .await;
            }
        }
        if mode == AppMode::Duo {
            if self.config.features.enabled(Feature::Duo) {
                builder = builder.with_duo_tools(self.config.duo_session.clone());
            } else {
                let _ = self
                    .tx_event
                    .send(Event::status("Duo tools are disabled by feature flags"))
                    .await;
            }
        }

        let tool_registry = match mode {
            AppMode::Agent | AppMode::Yolo | AppMode::Rlm | AppMode::Duo => {
                if self.config.features.enabled(Feature::Subagents) {
                    let runtime = if let Some(client) = self.deepseek_client.clone() {
                        Some(SubAgentRuntime::new(
                            client,
                            self.session.model.clone(),
                            tool_context.clone(),
                            self.session.allow_shell,
                            Some(self.tx_event.clone()),
                        ))
                    } else {
                        None
                    };
                    Some(
                        builder
                            .with_subagent_tools(
                                self.subagent_manager.clone(),
                                runtime.expect("sub-agent runtime should exist with active client"),
                            )
                            .build(tool_context),
                    )
                } else {
                    Some(builder.build(tool_context))
                }
            }
            _ => Some(builder.build(tool_context)),
        };

        let mcp_tools = if self.config.features.enabled(Feature::Mcp) {
            self.mcp_tools().await
        } else {
            Vec::new()
        };
        let tools = tool_registry.as_ref().map(|registry| {
            let mut tools = registry.to_api_tools();
            tools.extend(mcp_tools);
            tools
        });

        // Main turn loop
        self.handle_deepseek_turn(&mut turn, tool_registry.as_ref(), tools, mode)
            .await;

        // Update session usage
        self.session.total_usage.add(&turn.usage);

        // Emit turn complete event
        let _ = self
            .tx_event
            .send(Event::TurnComplete { usage: turn.usage })
            .await;
    }

    fn build_tool_context(&self, mode: AppMode) -> ToolContext {
        ToolContext::with_auto_approve(
            self.session.workspace.clone(),
            self.session.trust_mode,
            self.session.notes_path.clone(),
            self.session.mcp_config_path.clone(),
            mode == AppMode::Yolo,
        )
    }

    /// Automatically offload large tool results to RLM memory if enabled.
    /// Returns either the original content or a pointer to the RLM context.
    fn offload_to_rlm_if_needed(&self, tool_name: &str, content: String) -> String {
        const OFFLOAD_THRESHOLD: usize = 15_000;

        if !self.config.features.enabled(Feature::Rlm) || content.len() < OFFLOAD_THRESHOLD {
            return content;
        }

        let mut session = match self.config.rlm_session.lock() {
            Ok(s) => s,
            Err(_) => return content,
        };

        let context_id = format!(
            "auto_{}_{}",
            tool_name,
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let char_count = content.len();
        let line_count = content.lines().count();

        session.load_context(&context_id, content, None);

        format!(
            "[AUTOMATIC RLM OFFLOAD]\n\
             The output of '{tool_name}' was too large ({char_count} chars, {line_count} lines) \
             and has been moved to RLM memory to preserve your context window.\n\n\
             Context ID: {context_id}\n\n\
             You can explore this data using RLM tools:\n\
             - `rlm_exec(code=\"lines(1, 100)\", context_id=\"{context_id}\")` to see the start\n\
             - `rlm_exec(code=\"search(\\\"pattern\\\")\", context_id=\"{context_id}\")` to search\n\
             - `rlm_query(query=\"...\", context_id=\"{context_id}\")` for deep analysis"
        )
    }

    async fn ensure_mcp_pool(&mut self) -> Result<Arc<AsyncMutex<McpPool>>, ToolError> {
        if let Some(pool) = self.mcp_pool.as_ref() {
            return Ok(Arc::clone(pool));
        }
        let pool = McpPool::from_config_path(&self.session.mcp_config_path)
            .map_err(|e| ToolError::execution_failed(format!("Failed to load MCP config: {e}")))?;
        let pool = Arc::new(AsyncMutex::new(pool));
        self.mcp_pool = Some(Arc::clone(&pool));
        Ok(pool)
    }

    async fn mcp_tools(&mut self) -> Vec<Tool> {
        let pool = match self.ensure_mcp_pool().await {
            Ok(pool) => pool,
            Err(err) => {
                let _ = self.tx_event.send(Event::status(err.to_string())).await;
                return Vec::new();
            }
        };

        let mut pool = pool.lock().await;
        let errors = pool.connect_all().await;
        for (server, err) in errors {
            let _ = self
                .tx_event
                .send(Event::status(format!(
                    "Failed to connect MCP server '{server}': {err}"
                )))
                .await;
        }

        pool.to_api_tools()
    }

    async fn execute_mcp_tool(
        &mut self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let pool = self.ensure_mcp_pool().await?;
        Self::execute_mcp_tool_with_pool(pool, name, input).await
    }

    async fn execute_mcp_tool_with_pool(
        pool: Arc<AsyncMutex<McpPool>>,
        name: &str,
        input: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let mut pool = pool.lock().await;
        let result = pool
            .call_tool(name, input)
            .await
            .map_err(|e| ToolError::execution_failed(format!("MCP tool failed: {e}")))?;
        let content = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
        Ok(ToolResult::success(content))
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_tool_with_lock(
        lock: Arc<RwLock<()>>,
        supports_parallel: bool,
        interactive: bool,
        tx_event: mpsc::Sender<Event>,
        tool_name: String,
        tool_input: serde_json::Value,
        registry: Option<&crate::tools::ToolRegistry>,
        mcp_pool: Option<Arc<AsyncMutex<McpPool>>>,
        context_override: Option<crate::tools::ToolContext>,
    ) -> Result<ToolResult, ToolError> {
        let _guard = if supports_parallel {
            ToolExecGuard::Read(lock.read().await)
        } else {
            ToolExecGuard::Write(lock.write().await)
        };

        if interactive {
            let _ = tx_event.send(Event::PauseEvents).await;
        }

        let result = if McpPool::is_mcp_tool(&tool_name) {
            if let Some(pool) = mcp_pool {
                Engine::execute_mcp_tool_with_pool(pool, &tool_name, tool_input).await
            } else {
                Err(ToolError::not_available(format!(
                    "tool '{tool_name}' is not registered"
                )))
            }
        } else if let Some(registry) = registry {
            registry
                .execute_full_with_context(&tool_name, tool_input, context_override.as_ref())
                .await
        } else {
            Err(ToolError::not_available(format!(
                "tool '{tool_name}' is not registered"
            )))
        };

        if interactive {
            let _ = tx_event.send(Event::ResumeEvents).await;
        }

        result
    }

    async fn await_tool_approval(&mut self, tool_id: &str) -> Result<ApprovalResult, ToolError> {
        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    return Err(ToolError::execution_failed(
                        "Request cancelled while awaiting approval".to_string(),
                    ));
                }
                decision = self.rx_approval.recv() => {
                    let Some(decision) = decision else {
                        return Err(ToolError::execution_failed(
                            "Approval channel closed".to_string(),
                        ));
                    };
                    match decision {
                        ApprovalDecision::Approved { id } if id == tool_id => {
                            return Ok(ApprovalResult::Approved);
                        }
                        ApprovalDecision::Denied { id } if id == tool_id => {
                            return Ok(ApprovalResult::Denied);
                        }
                        ApprovalDecision::RetryWithPolicy { id, policy } if id == tool_id => {
                            return Ok(ApprovalResult::RetryWithPolicy(policy));
                        }
                        _ => continue,
                    }
                }
            }
        }
    }

    /// Handle a turn using the DeepSeek API.
    #[allow(clippy::too_many_lines)]
    async fn handle_deepseek_turn(
        &mut self,
        turn: &mut TurnContext,
        tool_registry: Option<&crate::tools::ToolRegistry>,
        tools: Option<Vec<Tool>>,
        _mode: AppMode,
    ) {
        let client = self
            .deepseek_client
            .clone()
            .expect("DeepSeek client should be configured");

        let mut consecutive_tool_error_steps = 0u32;

        loop {
            if self.cancel_token.is_cancelled() {
                let _ = self.tx_event.send(Event::status("Request cancelled")).await;
                break;
            }

            // Ensure system prompt is up to date with latest session states
            self.refresh_system_prompt(_mode);

            if turn.at_max_steps() {
                let _ = self
                    .tx_event
                    .send(Event::status("Reached maximum steps"))
                    .await;
                break;
            }

            let compaction_pins = self
                .session
                .working_set
                .pinned_message_indices(&self.session.messages, &self.session.workspace);
            let compaction_paths = self.session.working_set.top_paths(24);

            if self.config.compaction.enabled
                && should_compact(
                    &self.session.messages,
                    &self.config.compaction,
                    Some(&self.session.workspace),
                    Some(&compaction_pins),
                    Some(&compaction_paths),
                )
            {
                let _ = self
                    .tx_event
                    .send(Event::status("Auto-compacting context...".to_string()))
                    .await;
                match compact_messages_safe(
                    &client,
                    &self.session.messages,
                    &self.config.compaction,
                    Some(&self.session.workspace),
                    Some(&compaction_pins),
                    Some(&compaction_paths),
                )
                .await
                {
                    Ok(result) => {
                        // Only update if we got valid messages (never corrupt state)
                        if !result.messages.is_empty() || self.session.messages.is_empty() {
                            // Offload removed messages to RLM history if enabled
                            if self.config.features.enabled(Feature::Rlm)
                                && !result.removed_messages.is_empty()
                            {
                                if let Ok(mut rlm) = self.config.rlm_session.lock() {
                                    let mut history_text = String::new();
                                    for msg in &result.removed_messages {
                                        let role = if msg.role == "user" {
                                            "User"
                                        } else {
                                            "Assistant"
                                        };
                                        for block in &msg.content {
                                            match block {
                                                ContentBlock::Text { text, .. } => {
                                                    let _ =
                                                        writeln!(history_text, "{role}: {text}\n");
                                                }
                                                ContentBlock::ToolUse { name, input, .. } => {
                                                    let _ = writeln!(
                                                        history_text,
                                                        "{role}: [Used tool: {name}] Input: {input}\n"
                                                    );
                                                }
                                                ContentBlock::ToolResult { content, .. } => {
                                                    let _ = writeln!(
                                                        history_text,
                                                        "Tool result: {content}\n"
                                                    );
                                                }
                                                ContentBlock::Thinking { thinking } => {
                                                    let _ = writeln!(
                                                        history_text,
                                                        "{role} (thinking): {thinking}\n"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    rlm.append_var("history", history_text);
                                }
                            }

                            self.session.messages = result.messages;
                            self.session.system_prompt = merge_system_prompts(
                                self.session.system_prompt.as_ref(),
                                result.summary_prompt,
                            );
                            let status = if result.retries_used > 0 {
                                format!(
                                    "Auto-compaction complete (after {} retries)",
                                    result.retries_used
                                )
                            } else {
                                "Auto-compaction complete".to_string()
                            };
                            let _ = self.tx_event.send(Event::status(status)).await;
                        } else {
                            let _ = self
                                .tx_event
                                .send(Event::status(
                                    "Auto-compaction skipped: empty result".to_string(),
                                ))
                                .await;
                        }
                    }
                    Err(err) => {
                        // Log error but continue with original messages (never corrupt)
                        let _ = self
                            .tx_event
                            .send(Event::status(format!("Auto-compaction failed: {err}")))
                            .await;
                    }
                }
            }

            // Build the request
            let request = MessageRequest {
                model: self.session.model.clone(),
                messages: self.session.messages.clone(),
                max_tokens: 4096,
                system: self.session.system_prompt.clone(),
                tools: tools.clone(),
                tool_choice: if tools.is_some() {
                    Some(json!({ "type": "auto" }))
                } else {
                    None
                },
                metadata: None,
                thinking: None,
                stream: Some(true),
                temperature: None,
                top_p: None,
            };

            // Stream the response
            let stream_result = client.create_message_stream(request).await;
            let stream = match stream_result {
                Ok(s) => s,
                Err(e) => {
                    let _ = self.tx_event.send(Event::error(e.to_string(), true)).await;
                    break;
                }
            };
            let mut stream = pin!(stream);

            // Track content blocks
            let mut content_blocks: Vec<ContentBlock> = Vec::new();
            let mut current_text_raw = String::new();
            let mut current_text_visible = String::new();
            let mut current_thinking = String::new();
            let mut tool_uses: Vec<ToolUseState> = Vec::new();
            let mut usage = Usage {
                input_tokens: 0,
                output_tokens: 0,
            };
            let mut current_block_kind: Option<ContentBlockKind> = None;
            let mut current_tool_index: Option<usize> = None;
            let mut in_tool_call_block = false;
            let mut pending_message_complete = false;
            let mut last_text_index: Option<usize> = None;
            let mut stream_errors = 0u32;

            // Process stream events
            while let Some(event_result) = stream.next().await {
                if self.cancel_token.is_cancelled() {
                    break;
                }

                let event = match event_result {
                    Ok(e) => e,
                    Err(e) => {
                        stream_errors = stream_errors.saturating_add(1);
                        let _ = self.tx_event.send(Event::error(e.to_string(), true)).await;
                        if stream_errors >= 3 {
                            break;
                        }
                        continue;
                    }
                };

                match event {
                    StreamEvent::MessageStart { message } => {
                        usage = message.usage;
                    }
                    StreamEvent::ContentBlockStart {
                        index,
                        content_block,
                    } => match content_block {
                        ContentBlockStart::Text { text } => {
                            current_text_raw = text;
                            current_text_visible.clear();
                            in_tool_call_block = false;
                            let filtered =
                                filter_tool_call_delta(&current_text_raw, &mut in_tool_call_block);
                            current_text_visible.push_str(&filtered);
                            current_block_kind = Some(ContentBlockKind::Text);
                            last_text_index = Some(index as usize);
                            let _ = self
                                .tx_event
                                .send(Event::MessageStarted {
                                    index: index as usize,
                                })
                                .await;
                        }
                        ContentBlockStart::Thinking { thinking } => {
                            current_thinking = thinking;
                            current_block_kind = Some(ContentBlockKind::Thinking);
                            let _ = self
                                .tx_event
                                .send(Event::ThinkingStarted {
                                    index: index as usize,
                                })
                                .await;
                        }
                        ContentBlockStart::ToolUse { id, name, input } => {
                            crate::logging::info(format!(
                                "Tool '{}' block start. Initial input: {:?}",
                                name, input
                            ));
                            current_block_kind = Some(ContentBlockKind::ToolUse);
                            current_tool_index = Some(tool_uses.len());
                            let _ = self
                                .tx_event
                                .send(Event::ToolCallStarted {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: json!({}),
                                })
                                .await;
                            tool_uses.push(ToolUseState {
                                id,
                                name,
                                input,
                                input_buffer: String::new(),
                            });
                        }
                    },
                    StreamEvent::ContentBlockDelta { index, delta } => match delta {
                        Delta::TextDelta { text } => {
                            current_text_raw.push_str(&text);
                            let filtered = filter_tool_call_delta(&text, &mut in_tool_call_block);
                            if !filtered.is_empty() {
                                current_text_visible.push_str(&filtered);
                                let _ = self
                                    .tx_event
                                    .send(Event::MessageDelta {
                                        index: index as usize,
                                        content: filtered,
                                    })
                                    .await;
                            }
                        }
                        Delta::ThinkingDelta { thinking } => {
                            current_thinking.push_str(&thinking);
                            if !thinking.is_empty() {
                                let _ = self
                                    .tx_event
                                    .send(Event::ThinkingDelta {
                                        index: index as usize,
                                        content: thinking,
                                    })
                                    .await;
                            }
                        }
                        Delta::InputJsonDelta { partial_json } => {
                            if let Some(index) = current_tool_index
                                && let Some(tool_state) = tool_uses.get_mut(index)
                            {
                                tool_state.input_buffer.push_str(&partial_json);
                                crate::logging::info(format!(
                                    "Tool '{}' input delta: {} (buffer now: {})",
                                    tool_state.name, partial_json, tool_state.input_buffer
                                ));
                                if let Some(value) = parse_tool_input(&tool_state.input_buffer) {
                                    tool_state.input = value.clone();
                                    crate::logging::info(format!(
                                        "Tool '{}' input parsed: {:?}",
                                        tool_state.name, value
                                    ));
                                }
                            }
                        }
                    },
                    StreamEvent::ContentBlockStop { index } => {
                        let stopped_kind = current_block_kind.take();
                        match stopped_kind {
                            Some(ContentBlockKind::Text) => {
                                pending_message_complete = true;
                                last_text_index = Some(index as usize);
                            }
                            Some(ContentBlockKind::Thinking) => {
                                let _ = self
                                    .tx_event
                                    .send(Event::ThinkingComplete {
                                        index: index as usize,
                                    })
                                    .await;
                            }
                            Some(ContentBlockKind::ToolUse) | None => {}
                        }
                        if matches!(stopped_kind, Some(ContentBlockKind::ToolUse)) {
                            if let Some(index) = current_tool_index.take()
                                && let Some(tool_state) = tool_uses.get_mut(index)
                            {
                                crate::logging::info(format!(
                                    "Tool '{}' block stop. Buffer: '{}', Current input: {:?}",
                                    tool_state.name, tool_state.input_buffer, tool_state.input
                                ));
                                if !tool_state.input_buffer.trim().is_empty() {
                                    if let Some(value) = parse_tool_input(&tool_state.input_buffer)
                                    {
                                        tool_state.input = value;
                                        crate::logging::info(format!(
                                            "Tool '{}' final input: {:?}",
                                            tool_state.name, tool_state.input
                                        ));
                                    } else {
                                        crate::logging::warn(format!(
                                            "Tool '{}' failed to parse final input buffer: '{}'",
                                            tool_state.name, tool_state.input_buffer
                                        ));
                                    }
                                } else {
                                    crate::logging::warn(format!(
                                        "Tool '{}' input buffer is empty, using initial input: {:?}",
                                        tool_state.name, tool_state.input
                                    ));
                                }
                            }
                        }
                    }
                    StreamEvent::MessageDelta {
                        usage: delta_usage, ..
                    } => {
                        if let Some(u) = delta_usage {
                            usage = u;
                        }
                    }
                    StreamEvent::MessageStop | StreamEvent::Ping => {}
                }
            }

            // Update turn usage
            turn.add_usage(&usage);

            // Build content blocks
            if !current_thinking.is_empty() {
                content_blocks.push(ContentBlock::Thinking {
                    thinking: current_thinking.clone(),
                });
            }
            let mut final_text = current_text_visible.clone();
            if tool_uses.is_empty() && tool_parser::has_tool_call_markers(&current_text_raw) {
                let parsed = tool_parser::parse_tool_calls(&current_text_raw);
                final_text = parsed.clean_text;
                for call in parsed.tool_calls {
                    let _ = self
                        .tx_event
                        .send(Event::ToolCallStarted {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            input: call.args.clone(),
                        })
                        .await;
                    tool_uses.push(ToolUseState {
                        id: call.id,
                        name: call.name,
                        input: call.args,
                        input_buffer: String::new(),
                    });
                }
            }

            if !final_text.is_empty() {
                content_blocks.push(ContentBlock::Text {
                    text: final_text,
                    cache_control: None,
                });
            }
            for tool in &tool_uses {
                content_blocks.push(ContentBlock::ToolUse {
                    id: tool.id.clone(),
                    name: tool.name.clone(),
                    input: tool.input.clone(),
                });
            }

            if pending_message_complete {
                let index = last_text_index.unwrap_or(0);
                let _ = self.tx_event.send(Event::MessageComplete { index }).await;
            }

            // Add assistant message to session
            if !content_blocks.is_empty() {
                self.session.add_message(Message {
                    role: "assistant".to_string(),
                    content: content_blocks,
                });
            }

            // If no tool uses, we're done
            if tool_uses.is_empty() {
                break;
            }

            // Execute tools
            let tool_exec_lock = self.tool_exec_lock.clone();
            let mcp_pool = if tool_uses
                .iter()
                .any(|tool| McpPool::is_mcp_tool(&tool.name))
            {
                match self.ensure_mcp_pool().await {
                    Ok(pool) => Some(pool),
                    Err(err) => {
                        let _ = self.tx_event.send(Event::status(err.to_string())).await;
                        None
                    }
                }
            } else {
                None
            };

            let mut plans: Vec<ToolExecutionPlan> = Vec::with_capacity(tool_uses.len());
            for (index, tool) in tool_uses.iter().enumerate() {
                let tool_id = tool.id.clone();
                let tool_name = tool.name.clone();
                let tool_input = tool.input.clone();
                crate::logging::info(format!(
                    "Planning tool '{}' with input: {:?}",
                    tool_name, tool_input
                ));

                let interactive = tool_name == "exec_shell"
                    && tool_input
                        .get("interactive")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true);

                let mut approval_required = false;
                let mut approval_description = "Tool execution requires approval".to_string();
                let mut supports_parallel = false;
                let mut read_only = false;

                if !McpPool::is_mcp_tool(&tool_name) {
                    if let Some(registry) = tool_registry
                        && let Some(spec) = registry.get(&tool_name)
                    {
                        approval_required =
                            spec.approval_requirement() != ApprovalRequirement::Auto;
                        approval_description = spec.description().to_string();
                        supports_parallel = spec.supports_parallel();
                        read_only = spec.is_read_only();
                    }
                }

                plans.push(ToolExecutionPlan {
                    index,
                    id: tool_id,
                    name: tool_name,
                    input: tool_input,
                    interactive,
                    approval_required,
                    approval_description,
                    supports_parallel,
                    read_only,
                });
            }

            let parallel_allowed = should_parallelize_tool_batch(&plans);
            if parallel_allowed && plans.len() > 1 {
                let _ = self
                    .tx_event
                    .send(Event::status(format!(
                        "Executing {} read-only tools in parallel",
                        plans.len()
                    )))
                    .await;
            } else if plans.len() > 1 {
                let _ = self
                    .tx_event
                    .send(Event::status(
                        "Executing tools sequentially (writes, approvals, or non-parallel tools detected)",
                    ))
                    .await;
            }

            let mut outcomes: Vec<Option<ToolExecOutcome>> = Vec::with_capacity(plans.len());
            outcomes.resize_with(plans.len(), || None);

            if parallel_allowed {
                let mut tool_tasks = FuturesUnordered::new();
                for plan in plans {
                    let registry = tool_registry;
                    let lock = tool_exec_lock.clone();
                    let mcp_pool = mcp_pool.clone();
                    let tx_event = self.tx_event.clone();
                    let started_at = Instant::now();

                    tool_tasks.push(async move {
                        let result = Engine::execute_tool_with_lock(
                            lock,
                            plan.supports_parallel,
                            plan.interactive,
                            tx_event.clone(),
                            plan.name.clone(),
                            plan.input.clone(),
                            registry,
                            mcp_pool,
                            None,
                        )
                        .await;

                        let _ = tx_event
                            .send(Event::ToolCallComplete {
                                id: plan.id.clone(),
                                name: plan.name.clone(),
                                result: result.clone(),
                            })
                            .await;

                        ToolExecOutcome {
                            index: plan.index,
                            id: plan.id,
                            name: plan.name,
                            input: plan.input,
                            started_at,
                            result,
                        }
                    });
                }

                while let Some(outcome) = tool_tasks.next().await {
                    let index = outcome.index;
                    outcomes[index] = Some(outcome);
                }
            } else {
                for plan in plans {
                    let tool_id = plan.id.clone();
                    let tool_name = plan.name.clone();
                    let tool_input = plan.input.clone();

                    // Handle approval flow: returns (result_override, context_override)
                    let (result_override, context_override): (
                        Option<Result<ToolResult, ToolError>>,
                        Option<crate::tools::ToolContext>,
                    ) = if plan.approval_required {
                        let _ = self
                            .tx_event
                            .send(Event::ApprovalRequired {
                                id: tool_id.clone(),
                                tool_name: tool_name.clone(),
                                description: plan.approval_description.clone(),
                            })
                            .await;

                        match self.await_tool_approval(&tool_id).await {
                            Ok(ApprovalResult::Approved) => (None, None),
                            Ok(ApprovalResult::Denied) => (
                                Some(Err(ToolError::permission_denied(format!(
                                    "Tool '{tool_name}' denied by user"
                                )))),
                                None,
                            ),
                            Ok(ApprovalResult::RetryWithPolicy(policy)) => {
                                let elevated_context = tool_registry.map(|r| {
                                    r.context().clone().with_elevated_sandbox_policy(policy)
                                });
                                (None, elevated_context)
                            }
                            Err(err) => (Some(Err(err)), None),
                        }
                    } else {
                        (None, None)
                    };

                    let started_at = Instant::now();
                    let result = if let Some(result_override) = result_override {
                        result_override
                    } else {
                        Self::execute_tool_with_lock(
                            tool_exec_lock.clone(),
                            plan.supports_parallel,
                            plan.interactive,
                            self.tx_event.clone(),
                            tool_name.clone(),
                            tool_input.clone(),
                            tool_registry,
                            mcp_pool.clone(),
                            context_override,
                        )
                        .await
                    };

                    let _ = self
                        .tx_event
                        .send(Event::ToolCallComplete {
                            id: tool_id.clone(),
                            name: tool_name.clone(),
                            result: result.clone(),
                        })
                        .await;

                    outcomes[plan.index] = Some(ToolExecOutcome {
                        index: plan.index,
                        id: tool_id,
                        name: tool_name,
                        input: tool_input,
                        started_at,
                        result,
                    });
                }
            }

            let mut step_error_count = 0usize;

            for outcome in outcomes.into_iter().flatten() {
                let duration = outcome.started_at.elapsed();
                let tool_input = outcome.input.clone();
                let tool_name_for_ws = outcome.name.clone();
                let mut tool_call =
                    TurnToolCall::new(outcome.id.clone(), outcome.name.clone(), outcome.input);

                match outcome.result {
                    Ok(output) => {
                        let original_content = output.content;
                        let output_content =
                            self.offload_to_rlm_if_needed(&outcome.name, original_content);

                        tool_call.set_result(output_content.clone(), duration);
                        self.session.working_set.observe_tool_call(
                            &tool_name_for_ws,
                            &tool_input,
                            Some(&output_content),
                            &self.session.workspace,
                        );
                        self.session.add_message(Message {
                            role: "user".to_string(),
                            content: vec![ContentBlock::ToolResult {
                                tool_use_id: outcome.id,
                                content: output_content,
                            }],
                        });
                    }
                    Err(e) => {
                        step_error_count += 1;
                        let error = format_tool_error(&e, &outcome.name);
                        tool_call.set_error(error.clone(), duration);
                        self.session.working_set.observe_tool_call(
                            &tool_name_for_ws,
                            &tool_input,
                            Some(&error),
                            &self.session.workspace,
                        );
                        self.session.add_message(Message {
                            role: "user".to_string(),
                            content: vec![ContentBlock::ToolResult {
                                tool_use_id: outcome.id,
                                content: format!("Error: {error}"),
                            }],
                        });
                    }
                }

                turn.record_tool_call(tool_call);
            }

            if step_error_count > 0 {
                consecutive_tool_error_steps = consecutive_tool_error_steps.saturating_add(1);
            } else {
                consecutive_tool_error_steps = 0;
            }

            if consecutive_tool_error_steps >= 3 {
                let _ = self
                    .tx_event
                    .send(Event::status(
                        "Stopping after repeated tool failures. Try a narrower scope or adjust approvals.",
                    ))
                    .await;
                break;
            }

            turn.next_step();
        }
    }

    /// Get a reference to the session
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Get a mutable reference to the session
    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Refresh the system prompt based on current mode and context.
    fn refresh_system_prompt(&mut self, mode: AppMode) {
        let rlm_summary = self
            .config
            .rlm_session
            .lock()
            .ok()
            .map(|session| rlm_session_summary(&session));

        let duo_summary = self
            .config
            .duo_session
            .lock()
            .ok()
            .map(|s| duo_session_summary(&s));

        let working_set_summary = self
            .session
            .working_set
            .summary_block(&self.config.workspace);

        self.session.system_prompt = Some(prompts::system_prompt_for_mode_with_context(
            mode,
            &self.config.workspace,
            working_set_summary.as_deref(),
            rlm_summary.as_deref(),
            duo_summary.as_deref(),
        ));
    }
}

/// Spawn the engine in a background task
pub fn spawn_engine(config: EngineConfig, api_config: &Config) -> EngineHandle {
    let (engine, handle) = Engine::new(config, api_config);

    tokio::spawn(async move {
        engine.run().await;
    });

    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::Instant;

    fn make_plan(
        read_only: bool,
        supports_parallel: bool,
        approval_required: bool,
        interactive: bool,
    ) -> ToolExecutionPlan {
        ToolExecutionPlan {
            index: 0,
            id: "tool-1".to_string(),
            name: "grep_files".to_string(),
            input: json!({"pattern": "test"}),
            interactive,
            approval_required,
            approval_description: "desc".to_string(),
            supports_parallel,
            read_only,
        }
    }

    #[test]
    fn parallel_batch_requires_read_only_parallel_tools() {
        let plans = vec![make_plan(true, true, false, false)];
        assert!(should_parallelize_tool_batch(&plans));

        let plans = vec![
            make_plan(true, true, false, false),
            make_plan(true, true, false, false),
        ];
        assert!(should_parallelize_tool_batch(&plans));

        let plans = vec![make_plan(false, true, false, false)];
        assert!(!should_parallelize_tool_batch(&plans));

        let plans = vec![make_plan(true, false, false, false)];
        assert!(!should_parallelize_tool_batch(&plans));

        let plans = vec![make_plan(true, true, true, false)];
        assert!(!should_parallelize_tool_batch(&plans));

        let plans = vec![make_plan(true, true, false, true)];
        assert!(!should_parallelize_tool_batch(&plans));
    }

    #[test]
    fn tool_error_messages_include_actionable_hints() {
        let path_error = ToolError::path_escape(PathBuf::from("../escape.txt"));
        let formatted = format_tool_error(&path_error, "read_file");
        assert!(formatted.contains("escapes workspace"));

        let missing_field = ToolError::missing_field("path");
        let formatted = format_tool_error(&missing_field, "read_file");
        assert!(formatted.contains("missing required field"));

        let timeout = ToolError::Timeout { seconds: 5 };
        let formatted = format_tool_error(&timeout, "exec_shell");
        assert!(formatted.contains("timed out"));
    }

    #[test]
    fn tool_exec_outcome_tracks_duration() {
        let outcome = ToolExecOutcome {
            index: 0,
            id: "tool-1".to_string(),
            name: "grep_files".to_string(),
            input: json!({"pattern": "test"}),
            started_at: Instant::now(),
            result: Ok(ToolResult::success("ok")),
        };

        assert!(outcome.started_at.elapsed().as_nanos() > 0);
    }
}
