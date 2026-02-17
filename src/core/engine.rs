//! Core engine for `DeepSeek` CLI.
//!
//! The engine handles all AI interactions in a background task,
//! communicating with the UI via channels. This enables:
//! - Non-blocking UI during API calls
//! - Real-time streaming updates
//! - Proper cancellation support
//! - Tool execution orchestration

use std::path::PathBuf;
use std::pin::pin;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use std::{fs::OpenOptions, io::Write};

use anyhow::Result;
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use serde_json::json;
use tokio::sync::{Mutex as AsyncMutex, RwLock, mpsc};
use tokio_util::sync::CancellationToken;

use crate::client::DeepSeekClient;
use crate::compaction::{
    CompactionConfig, compact_messages_safe, estimate_tokens, merge_system_prompts, should_compact,
};
use crate::config::{Config, DEFAULT_MAX_SUBAGENTS, DEFAULT_TEXT_MODEL};
use crate::features::{Feature, Features};
use crate::llm_client::LlmClient;
use crate::mcp::McpPool;
use crate::models::{
    ContentBlock, ContentBlockStart, DEFAULT_CONTEXT_WINDOW_TOKENS, Delta, Message, MessageRequest,
    StreamEvent, SystemPrompt, Tool, Usage, context_window_for_model,
};
use crate::prompts;
use crate::tools::plan::{SharedPlanState, new_shared_plan_state};
use crate::tools::shell::{SharedShellManager, new_shared_shell_manager};
use crate::tools::spec::{ApprovalRequirement, ToolError, ToolResult};
use crate::tools::subagent::{
    SharedSubAgentManager, SubAgentRuntime, SubAgentType, new_shared_subagent_manager,
};
use crate::tools::todo::{SharedTodoList, new_shared_todo_list};
use crate::tools::user_input::{UserInputRequest, UserInputResponse};
use crate::tools::{ToolContext, ToolRegistryBuilder};
use crate::tui::app::AppMode;

use super::capacity::{
    CapacityController, CapacityControllerConfig, CapacityDecision, CapacityObservationInput,
    CapacitySnapshot, GuardrailAction, RiskBand,
};
use super::capacity_memory::{
    CanonicalState, CapacityMemoryRecord, ReplayInfo, append_capacity_record,
    load_last_k_capacity_records, new_record_id, now_rfc3339,
};
use super::events::{Event, TurnOutcomeStatus};
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
    /// Auto-compaction settings for long conversations.
    pub compaction: CompactionConfig,
    /// Capacity-controller settings.
    pub capacity: CapacityControllerConfig,
    /// Shared Todo list state.
    pub todos: SharedTodoList,
    /// Shared Plan state.
    pub plan_state: SharedPlanState,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            model: DEFAULT_TEXT_MODEL.to_string(),
            workspace: PathBuf::from("."),
            allow_shell: true,
            trust_mode: false,
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            max_steps: 100,
            max_subagents: DEFAULT_MAX_SUBAGENTS,
            features: Features::with_defaults(),
            compaction: CompactionConfig::default(),
            capacity: CapacityControllerConfig::default(),
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
    /// Shared pointer to the cancellation token for the current request.
    cancel_token: Arc<StdMutex<CancellationToken>>,
    /// Send approval decisions to the engine
    tx_approval: mpsc::Sender<ApprovalDecision>,
    /// Send user input responses to the engine
    tx_user_input: mpsc::Sender<UserInputDecision>,
    /// Send steer input for an in-flight turn.
    tx_steer: mpsc::Sender<String>,
}

impl EngineHandle {
    /// Send an operation to the engine
    pub async fn send(&self, op: Op) -> Result<()> {
        self.tx_op.send(op).await?;
        Ok(())
    }

    /// Cancel the current request
    pub fn cancel(&self) {
        match self.cancel_token.lock() {
            Ok(token) => token.cancel(),
            Err(poisoned) => poisoned.into_inner().cancel(),
        }
    }

    /// Check if a request is currently cancelled
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        match self.cancel_token.lock() {
            Ok(token) => token.is_cancelled(),
            Err(poisoned) => poisoned.into_inner().is_cancelled(),
        }
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

    /// Submit a response for request_user_input.
    pub async fn submit_user_input(
        &self,
        id: impl Into<String>,
        response: UserInputResponse,
    ) -> Result<()> {
        self.tx_user_input
            .send(UserInputDecision::Submitted {
                id: id.into(),
                response,
            })
            .await?;
        Ok(())
    }

    /// Cancel a request_user_input prompt.
    pub async fn cancel_user_input(&self, id: impl Into<String>) -> Result<()> {
        self.tx_user_input
            .send(UserInputDecision::Cancelled { id: id.into() })
            .await?;
        Ok(())
    }

    /// Steer an in-flight turn with additional user input.
    pub async fn steer(&self, content: impl Into<String>) -> Result<()> {
        self.tx_steer.send(content.into()).await?;
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
    shell_manager: SharedShellManager,
    mcp_pool: Option<Arc<AsyncMutex<McpPool>>>,
    rx_op: mpsc::Receiver<Op>,
    rx_approval: mpsc::Receiver<ApprovalDecision>,
    rx_user_input: mpsc::Receiver<UserInputDecision>,
    rx_steer: mpsc::Receiver<String>,
    tx_event: mpsc::Sender<Event>,
    cancel_token: CancellationToken,
    shared_cancel_token: Arc<StdMutex<CancellationToken>>,
    tool_exec_lock: Arc<RwLock<()>>,
    capacity_controller: CapacityController,
    turn_counter: u64,
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

#[derive(Debug, Clone)]
enum UserInputDecision {
    Submitted {
        id: String,
        response: UserInputResponse,
    },
    Cancelled {
        id: String,
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

#[derive(Debug, serde::Serialize)]
struct ParallelToolResultEntry {
    tool_name: String,
    success: bool,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct ParallelToolResult {
    results: Vec<ParallelToolResultEntry>,
}

// Hold the lock guard for the duration of a tool execution.
enum ToolExecGuard<'a> {
    Read(tokio::sync::RwLockReadGuard<'a, ()>),
    Write(tokio::sync::RwLockWriteGuard<'a, ()>),
}

/// Maximum time to wait for a single stream chunk before assuming a stall.
const STREAM_CHUNK_TIMEOUT_SECS: u64 = 90;
/// Maximum total bytes of text/thinking content before aborting the stream.
const STREAM_MAX_CONTENT_BYTES: usize = 10 * 1024 * 1024; // 10 MB
/// Maximum wall-clock duration for a single streaming response.
const STREAM_MAX_DURATION_SECS: u64 = 300; // 5 minutes
/// Max output tokens requested for normal agent turns.
const TURN_MAX_OUTPUT_TOKENS: u32 = 4096;
/// Keep this many most recent messages when emergency trimming is required.
const MIN_RECENT_MESSAGES_TO_KEEP: usize = 4;
/// Allow a few emergency recovery attempts before failing the turn.
const MAX_CONTEXT_RECOVERY_ATTEMPTS: u8 = 2;
/// Reserve additional headroom to avoid hitting provider hard limits.
const CONTEXT_HEADROOM_TOKENS: usize = 1024;
/// Hard cap for any tool output inserted into model context.
const TOOL_RESULT_CONTEXT_HARD_LIMIT_CHARS: usize = 12_000;
/// Soft cap for known noisy tools inserted into model context.
const TOOL_RESULT_CONTEXT_SOFT_LIMIT_CHARS: usize = 2_000;
/// Snippet length kept when compacting tool output for model context.
const TOOL_RESULT_CONTEXT_SNIPPET_CHARS: usize = 900;
/// Max chars to keep from metadata-provided output summaries.
const TOOL_RESULT_METADATA_SUMMARY_CHARS: usize = 320;
const COMPACTION_SUMMARY_MARKER: &str = "Conversation Summary (Auto-Generated)";

const TOOL_CALL_START_MARKERS: [&str; 5] = [
    "[TOOL_CALL]",
    "<deepseek:tool_call",
    "<tool_call",
    "<invoke ",
    "<function_calls>",
];

const MULTI_TOOL_PARALLEL_NAME: &str = "multi_tool_use.parallel";
const REQUEST_USER_INPUT_NAME: &str = "request_user_input";
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

fn normalize_parallel_tool_name(raw: &str) -> String {
    let mut name = raw.trim();
    for prefix in ["functions.", "tools.", "tool."] {
        if let Some(stripped) = name.strip_prefix(prefix) {
            name = stripped;
            break;
        }
    }
    name.to_string()
}

fn parse_parallel_tool_calls(
    input: &serde_json::Value,
) -> Result<Vec<(String, serde_json::Value)>, ToolError> {
    let tool_uses = input
        .get("tool_uses")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::missing_field("tool_uses"))?;
    if tool_uses.is_empty() {
        return Err(ToolError::invalid_input(
            "multi_tool_use.parallel requires at least one tool call",
        ));
    }

    let mut calls = Vec::with_capacity(tool_uses.len());
    for item in tool_uses {
        let name = item
            .get("recipient_name")
            .or_else(|| item.get("tool_name"))
            .or_else(|| item.get("name"))
            .or_else(|| item.get("tool"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::missing_field("recipient_name"))?;
        let params = item
            .get("parameters")
            .or_else(|| item.get("input"))
            .or_else(|| item.get("args"))
            .or_else(|| item.get("arguments"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        calls.push((normalize_parallel_tool_name(name), params));
    }

    Ok(calls)
}

fn should_parallelize_tool_batch(plans: &[ToolExecutionPlan]) -> bool {
    !plans.is_empty()
        && plans.iter().all(|plan| {
            plan.read_only && plan.supports_parallel && !plan.approval_required && !plan.interactive
        })
}

fn mcp_tool_is_parallel_safe(name: &str) -> bool {
    matches!(
        name,
        "list_mcp_resources"
            | "list_mcp_resource_templates"
            | "mcp_read_resource"
            | "read_mcp_resource"
            | "mcp_get_prompt"
    )
}

fn mcp_tool_is_read_only(name: &str) -> bool {
    matches!(
        name,
        "list_mcp_resources"
            | "list_mcp_resource_templates"
            | "mcp_read_resource"
            | "read_mcp_resource"
            | "mcp_get_prompt"
    )
}

fn mcp_tool_approval_description(name: &str) -> String {
    if mcp_tool_is_read_only(name) {
        format!("Read-only MCP tool '{name}'")
    } else {
        format!("MCP tool '{name}' may have side effects")
    }
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

fn summarize_text(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let take = limit.saturating_sub(3);
    let mut out: String = text.chars().take(take).collect();
    out.push_str("...");
    out
}

fn tool_result_is_noisy(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "exec_shell"
            | "exec_shell_wait"
            | "exec_shell_interact"
            | "multi_tool_use.parallel"
            | "web_search"
            | "weather"
            | "finance"
            | "sports"
            | "time"
    )
}

fn tool_result_metadata_summary(metadata: Option<&serde_json::Value>) -> Option<String> {
    let obj = metadata?.as_object()?;
    for key in ["summary", "stdout_summary", "stderr_summary", "message"] {
        if let Some(text) = obj.get(key).and_then(serde_json::Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(summarize_text(trimmed, TOOL_RESULT_METADATA_SUMMARY_CHARS));
            }
        }
    }
    None
}

fn compact_tool_result_for_context(tool_name: &str, output: &ToolResult) -> String {
    let raw = output.content.trim();
    if raw.is_empty() {
        return String::new();
    }

    let raw_chars = raw.chars().count();
    let should_compact = raw_chars > TOOL_RESULT_CONTEXT_HARD_LIMIT_CHARS
        || (tool_result_is_noisy(tool_name) && raw_chars > TOOL_RESULT_CONTEXT_SOFT_LIMIT_CHARS);
    if !should_compact {
        return raw.to_string();
    }

    let snippet = summarize_text(raw, TOOL_RESULT_CONTEXT_SNIPPET_CHARS);
    let omitted = raw_chars.saturating_sub(snippet.chars().count());
    let summary = tool_result_metadata_summary(output.metadata.as_ref());

    if let Some(summary) = summary {
        format!(
            "[{tool_name} output compacted to protect context]\nSummary: {summary}\nSnippet: {snippet}\n(Original: {raw_chars} chars, omitted: {omitted} chars.)"
        )
    } else {
        format!(
            "[{tool_name} output compacted to protect context]\nSnippet: {snippet}\n(Original: {raw_chars} chars, omitted: {omitted} chars.)"
        )
    }
}

fn extract_compaction_summary_prompt(prompt: Option<SystemPrompt>) -> Option<SystemPrompt> {
    match prompt {
        Some(SystemPrompt::Blocks(blocks)) => {
            let summary_blocks: Vec<_> = blocks
                .into_iter()
                .filter(|block| block.text.contains(COMPACTION_SUMMARY_MARKER))
                .collect();
            if summary_blocks.is_empty() {
                None
            } else {
                Some(SystemPrompt::Blocks(summary_blocks))
            }
        }
        Some(SystemPrompt::Text(text)) => {
            if text.contains(COMPACTION_SUMMARY_MARKER) {
                Some(SystemPrompt::Text(text))
            } else {
                None
            }
        }
        None => None,
    }
}

fn estimate_text_tokens_conservative(text: &str) -> usize {
    text.chars().count().div_ceil(3)
}

fn estimate_system_tokens_conservative(system: Option<&SystemPrompt>) -> usize {
    match system {
        Some(SystemPrompt::Text(text)) => estimate_text_tokens_conservative(text),
        Some(SystemPrompt::Blocks(blocks)) => blocks
            .iter()
            .map(|block| estimate_text_tokens_conservative(&block.text))
            .sum(),
        None => 0,
    }
}

fn estimate_input_tokens_conservative(
    messages: &[Message],
    system: Option<&SystemPrompt>,
) -> usize {
    let message_tokens = estimate_tokens(messages).saturating_mul(3).div_ceil(2);
    let system_tokens = estimate_system_tokens_conservative(system);
    let framing_overhead = messages.len().saturating_mul(12).saturating_add(48);
    message_tokens
        .saturating_add(system_tokens)
        .saturating_add(framing_overhead)
}

fn context_input_budget(model: &str, requested_output_tokens: u32) -> Option<usize> {
    let window = usize::try_from(context_window_for_model(model)?).ok()?;
    let output = usize::try_from(requested_output_tokens).ok()?;
    window
        .checked_sub(output)
        .and_then(|v| v.checked_sub(CONTEXT_HEADROOM_TOKENS))
}

fn is_context_length_error_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("maximum context length")
        || lower.contains("context length")
        || lower.contains("context_length")
        || lower.contains("prompt is too long")
        || (lower.contains("requested") && lower.contains("tokens") && lower.contains("maximum"))
        || lower.contains("context window")
}

fn emit_tool_audit(event: serde_json::Value) {
    let Some(path) = std::env::var_os("DEEPSEEK_TOOL_AUDIT_LOG") else {
        return;
    };
    let line = match serde_json::to_string(&event) {
        Ok(line) => line,
        Err(_) => return,
    };
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

impl Engine {
    fn reset_cancel_token(&mut self) {
        let token = CancellationToken::new();
        self.cancel_token = token.clone();
        match self.shared_cancel_token.lock() {
            Ok(mut shared) => {
                *shared = token;
            }
            Err(poisoned) => {
                *poisoned.into_inner() = token;
            }
        }
    }

    /// Create a new engine with the given configuration
    pub fn new(config: EngineConfig, api_config: &Config) -> (Self, EngineHandle) {
        let (tx_op, rx_op) = mpsc::channel(32);
        let (tx_event, rx_event) = mpsc::channel(256);
        let (tx_approval, rx_approval) = mpsc::channel(64);
        let (tx_user_input, rx_user_input) = mpsc::channel(32);
        let (tx_steer, rx_steer) = mpsc::channel(64);
        let cancel_token = CancellationToken::new();
        let shared_cancel_token = Arc::new(StdMutex::new(cancel_token.clone()));
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
        );
        session.system_prompt = Some(system_prompt);

        let subagent_manager =
            new_shared_subagent_manager(config.workspace.clone(), config.max_subagents);
        let shell_manager = new_shared_shell_manager(config.workspace.clone());
        let capacity_controller = CapacityController::new(config.capacity.clone());

        let mut engine = Engine {
            config,
            deepseek_client,
            deepseek_client_error,
            session,
            subagent_manager,
            shell_manager,
            mcp_pool: None,
            rx_op,
            rx_approval,
            rx_user_input,
            rx_steer,
            tx_event,
            cancel_token: cancel_token.clone(),
            shared_cancel_token: shared_cancel_token.clone(),
            tool_exec_lock,
            capacity_controller,
            turn_counter: 0,
        };
        engine.rehydrate_latest_canonical_state();

        let handle = EngineHandle {
            tx_op,
            rx_event: Arc::new(RwLock::new(rx_event)),
            cancel_token: shared_cancel_token,
            tx_approval,
            tx_user_input,
            tx_steer,
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
                    self.reset_cancel_token();
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
                    self.session.compaction_summary_prompt =
                        extract_compaction_summary_prompt(system_prompt.clone());
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
                    self.rehydrate_latest_canonical_state();
                    let _ = self
                        .tx_event
                        .send(Event::status("Session context synced".to_string()))
                        .await;
                }
                Op::CompactContext => {
                    self.handle_manual_compaction().await;
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
        self.reset_cancel_token();

        // Drain stale steer messages from previous turns.
        while self.rx_steer.try_recv().is_ok() {}

        // Create turn context first so start event includes a stable turn id.
        let mut turn = TurnContext::new(self.config.max_steps);
        self.turn_counter = self.turn_counter.saturating_add(1);
        self.capacity_controller.mark_turn_start(self.turn_counter);

        // Emit turn started event
        let _ = self
            .tx_event
            .send(Event::TurnStarted {
                turn_id: turn.id.clone(),
            })
            .await;

        // Check if we have the appropriate client
        if self.deepseek_client.is_none() {
            let message = self
                .deepseek_client_error
                .as_deref()
                .map(|err| format!("Failed to send message: {err}"))
                .unwrap_or_else(|| "Failed to send message: API client not configured".to_string());
            let _ = self
                .tx_event
                .send(Event::error(message.clone(), false))
                .await;
            let _ = self
                .tx_event
                .send(Event::TurnComplete {
                    usage: turn.usage.clone(),
                    status: TurnOutcomeStatus::Failed,
                    error: Some(message),
                })
                .await;
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

        self.session.model = model;
        self.config.model.clone_from(&self.session.model);
        self.session.allow_shell = allow_shell;
        self.config.allow_shell = allow_shell;
        self.session.trust_mode = trust_mode;
        self.config.trust_mode = trust_mode;

        // Update system prompt to match current mode and include persisted compaction context.
        self.refresh_system_prompt(mode);

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
            ToolRegistryBuilder::new()
                .with_agent_tools(self.session.allow_shell)
                .with_todo_tool(todo_list.clone())
                .with_plan_tool(plan_state.clone())
        };

        builder = builder
            .with_review_tool(self.deepseek_client.clone(), self.session.model.clone())
            .with_user_input_tool()
            .with_parallel_tool()
            .with_structured_data_tools();

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

        let tool_registry = match mode {
            AppMode::Agent | AppMode::Yolo => {
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
        let (status, error) = self
            .handle_deepseek_turn(&mut turn, tool_registry.as_ref(), tools, mode)
            .await;

        // Update session usage
        self.session.total_usage.add(&turn.usage);

        // Emit turn complete event
        let _ = self
            .tx_event
            .send(Event::TurnComplete {
                usage: turn.usage,
                status,
                error,
            })
            .await;
    }

    async fn handle_manual_compaction(&mut self) {
        let id = format!("compact_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let Some(client) = self.deepseek_client.clone() else {
            let message = "Manual compaction unavailable: API client not configured".to_string();
            let _ = self
                .tx_event
                .send(Event::CompactionFailed {
                    id,
                    auto: false,
                    message: message.clone(),
                })
                .await;
            let _ = self.tx_event.send(Event::error(message, false)).await;
            return;
        };

        let start_message = "Manual context compaction started".to_string();
        let _ = self
            .tx_event
            .send(Event::CompactionStarted {
                id: id.clone(),
                auto: false,
                message: start_message,
            })
            .await;

        let compaction_pins = self
            .session
            .working_set
            .pinned_message_indices(&self.session.messages, &self.session.workspace);
        let compaction_paths = self.session.working_set.top_paths(24);

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
                if !result.messages.is_empty() || self.session.messages.is_empty() {
                    self.session.messages = result.messages;
                    self.merge_compaction_summary(result.summary_prompt);
                    let message = if result.retries_used > 0 {
                        format!(
                            "Manual context compaction completed (after {} retries)",
                            result.retries_used
                        )
                    } else {
                        "Manual context compaction completed".to_string()
                    };
                    let _ = self
                        .tx_event
                        .send(Event::CompactionCompleted {
                            id,
                            auto: false,
                            message,
                        })
                        .await;
                } else {
                    let message = "Manual context compaction skipped: empty result".to_string();
                    let _ = self
                        .tx_event
                        .send(Event::CompactionFailed {
                            id,
                            auto: false,
                            message: message.clone(),
                        })
                        .await;
                }
            }
            Err(err) => {
                let message = format!("Manual context compaction failed: {err}");
                let _ = self
                    .tx_event
                    .send(Event::CompactionFailed {
                        id,
                        auto: false,
                        message: message.clone(),
                    })
                    .await;
                let _ = self.tx_event.send(Event::status(message)).await;
            }
        }
    }

    fn estimated_input_tokens(&self) -> usize {
        estimate_input_tokens_conservative(
            &self.session.messages,
            self.session.system_prompt.as_ref(),
        )
    }

    fn trim_oldest_messages_to_budget(&mut self, target_input_budget: usize) -> usize {
        let mut removed = 0usize;
        while self.session.messages.len() > MIN_RECENT_MESSAGES_TO_KEEP
            && self.estimated_input_tokens() > target_input_budget
        {
            self.session.messages.remove(0);
            removed = removed.saturating_add(1);
        }
        removed
    }

    async fn recover_context_overflow(
        &mut self,
        client: &DeepSeekClient,
        reason: &str,
        requested_output_tokens: u32,
    ) -> bool {
        let Some(target_budget) =
            context_input_budget(&self.session.model, requested_output_tokens)
        else {
            return false;
        };

        let id = format!("compact_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let start_message = format!("Emergency context compaction started ({reason})");
        let _ = self
            .tx_event
            .send(Event::CompactionStarted {
                id: id.clone(),
                auto: true,
                message: start_message,
            })
            .await;

        let before_tokens = self.estimated_input_tokens();
        let before_count = self.session.messages.len();

        let mut retries_used = 0u32;
        let mut summary_prompt = None;
        let mut compacted_messages = self.session.messages.clone();

        let mut forced_config = self.config.compaction.clone();
        forced_config.enabled = true;
        forced_config.token_threshold = forced_config
            .token_threshold
            .min(target_budget.saturating_sub(1))
            .max(1);
        forced_config.message_threshold = forced_config.message_threshold.max(1);

        match compact_messages_safe(
            client,
            &self.session.messages,
            &forced_config,
            Some(&self.session.workspace),
            None,
            None,
        )
        .await
        {
            Ok(result) => {
                retries_used = result.retries_used;
                compacted_messages = result.messages;
                summary_prompt = result.summary_prompt;
            }
            Err(err) => {
                let _ = self
                    .tx_event
                    .send(Event::status(format!(
                        "Emergency compaction API pass failed: {err}. Falling back to local trim."
                    )))
                    .await;
            }
        }

        if !compacted_messages.is_empty() || self.session.messages.is_empty() {
            self.session.messages = compacted_messages;
        }
        self.merge_compaction_summary(summary_prompt);

        let trimmed = self.trim_oldest_messages_to_budget(target_budget);
        let after_tokens = self.estimated_input_tokens();
        let after_count = self.session.messages.len();
        let recovered = after_tokens <= target_budget
            && (after_tokens < before_tokens || after_count < before_count || trimmed > 0);

        if recovered {
            let mut details = format!(
                "Emergency context compaction complete: ~{} -> ~{} tokens",
                before_tokens, after_tokens
            );
            if retries_used > 0 {
                details.push_str(&format!(" ({} retries)", retries_used));
            }
            if trimmed > 0 {
                details.push_str(&format!(", trimmed {trimmed} oldest messages"));
            }
            let _ = self
                .tx_event
                .send(Event::CompactionCompleted {
                    id,
                    auto: true,
                    message: details.clone(),
                })
                .await;
            let _ = self.tx_event.send(Event::status(details)).await;
            return true;
        }

        let message = format!(
            "Emergency context compaction failed to reduce request below model limit \
             (estimate ~{} tokens, budget ~{}).",
            after_tokens, target_budget
        );
        let _ = self
            .tx_event
            .send(Event::CompactionFailed {
                id,
                auto: true,
                message: message.clone(),
            })
            .await;
        let _ = self.tx_event.send(Event::status(message)).await;
        false
    }

    fn build_tool_context(&self, mode: AppMode) -> ToolContext {
        ToolContext::with_auto_approve(
            self.session.workspace.clone(),
            self.session.trust_mode,
            self.session.notes_path.clone(),
            self.session.mcp_config_path.clone(),
            mode == AppMode::Yolo,
        )
        .with_shell_manager(self.shell_manager.clone())
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

    async fn execute_parallel_tool(
        &mut self,
        input: serde_json::Value,
        tool_registry: Option<&crate::tools::ToolRegistry>,
        tool_exec_lock: Arc<RwLock<()>>,
    ) -> Result<ToolResult, ToolError> {
        let calls = parse_parallel_tool_calls(&input)?;
        let mcp_pool = if calls.iter().any(|(tool, _)| McpPool::is_mcp_tool(tool)) {
            Some(self.ensure_mcp_pool().await?)
        } else {
            None
        };
        let Some(registry) = tool_registry else {
            return Err(ToolError::not_available(
                "tool registry unavailable for multi_tool_use.parallel",
            ));
        };

        let mut tasks = FuturesUnordered::new();
        for (tool_name, tool_input) in calls {
            if tool_name == MULTI_TOOL_PARALLEL_NAME {
                return Err(ToolError::invalid_input(
                    "multi_tool_use.parallel cannot call itself",
                ));
            }
            if McpPool::is_mcp_tool(&tool_name) {
                if !mcp_tool_is_parallel_safe(&tool_name) {
                    return Err(ToolError::invalid_input(format!(
                        "Tool '{tool_name}' is an MCP tool and cannot run in parallel. \
                         Allowed MCP tools: list_mcp_resources, list_mcp_resource_templates, \
                         mcp_read_resource, read_mcp_resource, mcp_get_prompt."
                    )));
                }
            } else {
                let Some(spec) = registry.get(&tool_name) else {
                    return Err(ToolError::not_available(format!(
                        "tool '{tool_name}' is not registered"
                    )));
                };
                if !spec.is_read_only() {
                    return Err(ToolError::invalid_input(format!(
                        "Tool '{tool_name}' is not read-only and cannot run in parallel"
                    )));
                }
                if spec.approval_requirement() != ApprovalRequirement::Auto {
                    return Err(ToolError::invalid_input(format!(
                        "Tool '{tool_name}' requires approval and cannot run in parallel"
                    )));
                }
                if !spec.supports_parallel() {
                    return Err(ToolError::invalid_input(format!(
                        "Tool '{tool_name}' does not support parallel execution"
                    )));
                }
            }

            let registry_ref = registry;
            let lock = tool_exec_lock.clone();
            let tx_event = self.tx_event.clone();
            let mcp_pool = mcp_pool.clone();
            tasks.push(async move {
                let result = Engine::execute_tool_with_lock(
                    lock,
                    true,
                    false,
                    tx_event,
                    tool_name.clone(),
                    tool_input.clone(),
                    Some(registry_ref),
                    mcp_pool,
                    None,
                )
                .await;
                (tool_name, result)
            });
        }

        let mut results = Vec::new();
        while let Some((tool_name, result)) = tasks.next().await {
            match result {
                Ok(output) => {
                    let mut error = None;
                    if !output.success {
                        error = Some(output.content.clone());
                    }
                    results.push(ParallelToolResultEntry {
                        tool_name,
                        success: output.success,
                        content: output.content,
                        error,
                    });
                }
                Err(err) => {
                    let message = format!("{err}");
                    results.push(ParallelToolResultEntry {
                        tool_name,
                        success: false,
                        content: format!("Error: {message}"),
                        error: Some(message),
                    });
                }
            }
        }

        ToolResult::json(&ParallelToolResult { results })
            .map_err(|e| ToolError::execution_failed(e.to_string()))
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

    async fn await_user_input(
        &mut self,
        tool_id: &str,
        request: UserInputRequest,
    ) -> Result<UserInputResponse, ToolError> {
        let _ = self
            .tx_event
            .send(Event::UserInputRequired {
                id: tool_id.to_string(),
                request,
            })
            .await;

        loop {
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    return Err(ToolError::execution_failed(
                        "Request cancelled while awaiting user input".to_string(),
                    ));
                }
                decision = self.rx_user_input.recv() => {
                    let Some(decision) = decision else {
                        return Err(ToolError::execution_failed(
                            "User input channel closed".to_string(),
                        ));
                    };
                    match decision {
                        UserInputDecision::Submitted { id, response } if id == tool_id => {
                            return Ok(response);
                        }
                        UserInputDecision::Cancelled { id } if id == tool_id => {
                            return Err(ToolError::execution_failed(
                                "User input cancelled".to_string(),
                            ));
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
    ) -> (TurnOutcomeStatus, Option<String>) {
        let client = self
            .deepseek_client
            .clone()
            .expect("DeepSeek client should be configured");

        let mut consecutive_tool_error_steps = 0u32;
        let mut turn_error: Option<String> = None;
        let mut context_recovery_attempts = 0u8;

        loop {
            if self.cancel_token.is_cancelled() {
                let _ = self.tx_event.send(Event::status("Request cancelled")).await;
                return (TurnOutcomeStatus::Interrupted, None);
            }

            while let Ok(steer) = self.rx_steer.try_recv() {
                let steer = steer.trim().to_string();
                if steer.is_empty() {
                    continue;
                }
                self.session
                    .working_set
                    .observe_user_message(&steer, &self.session.workspace);
                self.session.add_message(Message {
                    role: "user".to_string(),
                    content: vec![ContentBlock::Text {
                        text: steer.clone(),
                        cache_control: None,
                    }],
                });
                let _ = self
                    .tx_event
                    .send(Event::status(format!(
                        "Steer input accepted: {}",
                        summarize_text(&steer, 120)
                    )))
                    .await;
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
                let compaction_id = format!("compact_{}", &uuid::Uuid::new_v4().to_string()[..8]);
                let _ = self
                    .tx_event
                    .send(Event::CompactionStarted {
                        id: compaction_id.clone(),
                        auto: true,
                        message: "Auto context compaction started".to_string(),
                    })
                    .await;
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
                            self.session.messages = result.messages;
                            self.merge_compaction_summary(result.summary_prompt);
                            let status = if result.retries_used > 0 {
                                format!(
                                    "Auto-compaction complete (after {} retries)",
                                    result.retries_used
                                )
                            } else {
                                "Auto-compaction complete".to_string()
                            };
                            let _ = self
                                .tx_event
                                .send(Event::CompactionCompleted {
                                    id: compaction_id.clone(),
                                    auto: true,
                                    message: status.clone(),
                                })
                                .await;
                            let _ = self.tx_event.send(Event::status(status)).await;
                        } else {
                            let message = "Auto-compaction skipped: empty result".to_string();
                            let _ = self
                                .tx_event
                                .send(Event::CompactionFailed {
                                    id: compaction_id.clone(),
                                    auto: true,
                                    message: message.clone(),
                                })
                                .await;
                            let _ = self.tx_event.send(Event::status(message)).await;
                        }
                    }
                    Err(err) => {
                        // Log error but continue with original messages (never corrupt)
                        let message = format!("Auto-compaction failed: {err}");
                        let _ = self
                            .tx_event
                            .send(Event::CompactionFailed {
                                id: compaction_id,
                                auto: true,
                                message: message.clone(),
                            })
                            .await;
                        let _ = self.tx_event.send(Event::status(message)).await;
                    }
                }
            }

            if self
                .run_capacity_pre_request_checkpoint(turn, Some(&client), _mode)
                .await
            {
                continue;
            }

            if let Some(input_budget) =
                context_input_budget(&self.session.model, TURN_MAX_OUTPUT_TOKENS)
            {
                let estimated_input = self.estimated_input_tokens();
                if estimated_input > input_budget {
                    if context_recovery_attempts >= MAX_CONTEXT_RECOVERY_ATTEMPTS {
                        let message = format!(
                            "Context remains above model limit after {} recovery attempts \
                             (~{} token estimate, ~{} budget). Please run /compact or /clear.",
                            MAX_CONTEXT_RECOVERY_ATTEMPTS, estimated_input, input_budget
                        );
                        turn_error = Some(message.clone());
                        let _ = self.tx_event.send(Event::error(message, true)).await;
                        return (TurnOutcomeStatus::Failed, turn_error);
                    }

                    if self
                        .recover_context_overflow(
                            &client,
                            "preflight token budget",
                            TURN_MAX_OUTPUT_TOKENS,
                        )
                        .await
                    {
                        context_recovery_attempts = context_recovery_attempts.saturating_add(1);
                        continue;
                    }
                }
            }

            // Build the request
            let request = MessageRequest {
                model: self.session.model.clone(),
                messages: self.session.messages.clone(),
                max_tokens: TURN_MAX_OUTPUT_TOKENS,
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
                Ok(s) => {
                    context_recovery_attempts = 0;
                    s
                }
                Err(e) => {
                    let message = e.to_string();
                    if is_context_length_error_message(&message)
                        && context_recovery_attempts < MAX_CONTEXT_RECOVERY_ATTEMPTS
                        && self
                            .recover_context_overflow(
                                &client,
                                "provider context-length rejection",
                                TURN_MAX_OUTPUT_TOKENS,
                            )
                            .await
                    {
                        context_recovery_attempts = context_recovery_attempts.saturating_add(1);
                        continue;
                    }
                    turn_error = Some(message.clone());
                    let _ = self.tx_event.send(Event::error(message, true)).await;
                    return (TurnOutcomeStatus::Failed, turn_error);
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
            let mut pending_steers: Vec<String> = Vec::new();
            let stream_start = Instant::now();
            let mut stream_content_bytes: usize = 0;
            let chunk_timeout = Duration::from_secs(STREAM_CHUNK_TIMEOUT_SECS);
            let max_duration = Duration::from_secs(STREAM_MAX_DURATION_SECS);

            // Process stream events
            loop {
                let poll_outcome = tokio::select! {
                    _ = self.cancel_token.cancelled() => None,
                    result = tokio::time::timeout(chunk_timeout, stream.next()) => {
                        match result {
                            Ok(Some(event_result)) => Some(event_result),
                            Ok(None) => None, // stream ended normally
                            Err(_) => {
                                let msg = format!(
                                    "Stream stalled: no data received for {}s, closing stream",
                                    STREAM_CHUNK_TIMEOUT_SECS,
                                );
                                crate::logging::warn(&msg);
                                let _ = self.tx_event.send(Event::error(msg, true)).await;
                                None
                            }
                        }
                    }
                };
                let Some(event_result) = poll_outcome else {
                    break;
                };
                while let Ok(steer) = self.rx_steer.try_recv() {
                    let steer = steer.trim().to_string();
                    if steer.is_empty() {
                        continue;
                    }
                    pending_steers.push(steer.clone());
                    let _ = self
                        .tx_event
                        .send(Event::status(format!(
                            "Steer input queued: {}",
                            summarize_text(&steer, 120)
                        )))
                        .await;
                }

                if self.cancel_token.is_cancelled() {
                    break;
                }

                // Guard: max wall-clock duration
                if stream_start.elapsed() > max_duration {
                    let msg = format!(
                        "Stream exceeded maximum duration of {}s, closing",
                        STREAM_MAX_DURATION_SECS,
                    );
                    crate::logging::warn(&msg);
                    turn_error.get_or_insert(msg.clone());
                    let _ = self.tx_event.send(Event::error(msg, true)).await;
                    break;
                }

                // Guard: max accumulated content bytes
                if stream_content_bytes > STREAM_MAX_CONTENT_BYTES {
                    let msg = format!(
                        "Stream exceeded maximum content size of {} bytes, closing",
                        STREAM_MAX_CONTENT_BYTES,
                    );
                    crate::logging::warn(&msg);
                    turn_error.get_or_insert(msg.clone());
                    let _ = self.tx_event.send(Event::error(msg, true)).await;
                    break;
                }

                let event = match event_result {
                    Ok(e) => e,
                    Err(e) => {
                        stream_errors = stream_errors.saturating_add(1);
                        let message = e.to_string();
                        turn_error.get_or_insert(message.clone());
                        let _ = self.tx_event.send(Event::error(message, true)).await;
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
                            stream_content_bytes = stream_content_bytes.saturating_add(text.len());
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
                            stream_content_bytes =
                                stream_content_bytes.saturating_add(thinking.len());
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
                                        let _ = self
                                            .tx_event
                                            .send(Event::status(format!(
                                                "⚠ Tool '{}' received malformed arguments from model",
                                                tool_state.name
                                            )))
                                            .await;
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

            // DeepSeek chat API rejects assistant messages that contain only
            // reasoning/thinking content without visible text or tool calls.
            // Keep thinking for UI stream events, but persist only sendable
            // assistant turns in the conversation state.
            let has_sendable_assistant_content = content_blocks.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Text { .. } | ContentBlock::ToolUse { .. }
                )
            });

            // Add assistant message to session
            if has_sendable_assistant_content {
                self.session.add_message(Message {
                    role: "assistant".to_string(),
                    content: content_blocks,
                });
            }

            // If no tool uses, we're done
            if tool_uses.is_empty() {
                if !pending_steers.is_empty() {
                    for steer in pending_steers.drain(..) {
                        self.session
                            .working_set
                            .observe_user_message(&steer, &self.session.workspace);
                        self.session.add_message(Message {
                            role: "user".to_string(),
                            content: vec![ContentBlock::Text {
                                text: steer,
                                cache_control: None,
                            }],
                        });
                    }
                    turn.next_step();
                    continue;
                }
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

                let interactive = (tool_name == "exec_shell"
                    && tool_input
                        .get("interactive")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true))
                    || tool_name == REQUEST_USER_INPUT_NAME;

                let mut approval_required = false;
                let mut approval_description = "Tool execution requires approval".to_string();
                let mut supports_parallel = false;
                let mut read_only = false;

                if McpPool::is_mcp_tool(&tool_name) {
                    read_only = mcp_tool_is_read_only(&tool_name);
                    supports_parallel = mcp_tool_is_parallel_safe(&tool_name);
                    approval_required = !read_only;
                    approval_description = mcp_tool_approval_description(&tool_name);
                } else if let Some(registry) = tool_registry
                    && let Some(spec) = registry.get(&tool_name)
                {
                    approval_required = spec.approval_requirement() != ApprovalRequirement::Auto;
                    approval_description = spec.description().to_string();
                    supports_parallel = spec.supports_parallel();
                    read_only = spec.is_read_only();
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

                    if tool_name == MULTI_TOOL_PARALLEL_NAME {
                        let started_at = Instant::now();
                        let result = self
                            .execute_parallel_tool(
                                tool_input.clone(),
                                tool_registry,
                                tool_exec_lock.clone(),
                            )
                            .await;

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
                        continue;
                    }

                    if tool_name == REQUEST_USER_INPUT_NAME {
                        let started_at = Instant::now();
                        let result = match UserInputRequest::from_value(&tool_input) {
                            Ok(request) => self.await_user_input(&tool_id, request).await.and_then(
                                |response| {
                                    ToolResult::json(&response)
                                        .map_err(|e| ToolError::execution_failed(e.to_string()))
                                },
                            ),
                            Err(err) => Err(err),
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
                        continue;
                    }

                    // Handle approval flow: returns (result_override, context_override)
                    let (result_override, context_override): (
                        Option<Result<ToolResult, ToolError>>,
                        Option<crate::tools::ToolContext>,
                    ) = if plan.approval_required {
                        emit_tool_audit(json!({
                            "event": "tool.approval_required",
                            "tool_id": tool_id.clone(),
                            "tool_name": tool_name.clone(),
                        }));
                        let _ = self
                            .tx_event
                            .send(Event::ApprovalRequired {
                                id: tool_id.clone(),
                                tool_name: tool_name.clone(),
                                description: plan.approval_description.clone(),
                            })
                            .await;

                        match self.await_tool_approval(&tool_id).await {
                            Ok(ApprovalResult::Approved) => {
                                emit_tool_audit(json!({
                                    "event": "tool.approval_decision",
                                    "tool_id": tool_id.clone(),
                                    "tool_name": tool_name.clone(),
                                    "decision": "approved",
                                }));
                                (None, None)
                            }
                            Ok(ApprovalResult::Denied) => {
                                emit_tool_audit(json!({
                                    "event": "tool.approval_decision",
                                    "tool_id": tool_id.clone(),
                                    "tool_name": tool_name.clone(),
                                    "decision": "denied",
                                }));
                                (
                                    Some(Err(ToolError::permission_denied(format!(
                                        "Tool '{tool_name}' denied by user"
                                    )))),
                                    None,
                                )
                            }
                            Ok(ApprovalResult::RetryWithPolicy(policy)) => {
                                emit_tool_audit(json!({
                                    "event": "tool.approval_decision",
                                    "tool_id": tool_id.clone(),
                                    "tool_name": tool_name.clone(),
                                    "decision": "retry_with_policy",
                                    "policy": format!("{policy:?}"),
                                }));
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
                        emit_tool_audit(json!({
                            "event": "tool.result",
                            "tool_id": outcome.id.clone(),
                            "tool_name": outcome.name.clone(),
                            "success": output.success,
                        }));
                        let output_for_context =
                            compact_tool_result_for_context(&outcome.name, &output);
                        let output_content = output.content;

                        tool_call.set_result(output_content.clone(), duration);
                        self.session.working_set.observe_tool_call(
                            &tool_name_for_ws,
                            &tool_input,
                            Some(&output_for_context),
                            &self.session.workspace,
                        );
                        self.session.add_message(Message {
                            role: "user".to_string(),
                            content: vec![ContentBlock::ToolResult {
                                tool_use_id: outcome.id,
                                content: output_for_context,
                            }],
                        });
                    }
                    Err(e) => {
                        emit_tool_audit(json!({
                            "event": "tool.result",
                            "tool_id": outcome.id.clone(),
                            "tool_name": outcome.name.clone(),
                            "success": false,
                            "error": e.to_string(),
                        }));
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

            if self
                .run_capacity_post_tool_checkpoint(
                    turn,
                    _mode,
                    tool_registry,
                    tool_exec_lock.clone(),
                    mcp_pool.clone(),
                    step_error_count,
                    consecutive_tool_error_steps,
                )
                .await
            {
                turn.next_step();
                continue;
            }

            if !pending_steers.is_empty() {
                for steer in pending_steers.drain(..) {
                    self.session
                        .working_set
                        .observe_user_message(&steer, &self.session.workspace);
                    self.session.add_message(Message {
                        role: "user".to_string(),
                        content: vec![ContentBlock::Text {
                            text: steer,
                            cache_control: None,
                        }],
                    });
                }
            }

            if step_error_count > 0 {
                consecutive_tool_error_steps = consecutive_tool_error_steps.saturating_add(1);
            } else {
                consecutive_tool_error_steps = 0;
            }

            if self
                .run_capacity_error_escalation_checkpoint(
                    turn,
                    _mode,
                    step_error_count,
                    consecutive_tool_error_steps,
                )
                .await
            {
                turn.next_step();
                continue;
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

        if self.cancel_token.is_cancelled() {
            return (TurnOutcomeStatus::Interrupted, None);
        }
        if let Some(err) = turn_error {
            return (TurnOutcomeStatus::Failed, Some(err));
        }
        (TurnOutcomeStatus::Completed, None)
    }

    async fn run_capacity_pre_request_checkpoint(
        &mut self,
        turn: &TurnContext,
        client: Option<&DeepSeekClient>,
        mode: AppMode,
    ) -> bool {
        let snapshot = self
            .capacity_controller
            .observe_pre_turn(self.capacity_observation(turn));
        let decision = self
            .capacity_controller
            .decide(self.turn_counter, snapshot.as_ref());
        self.emit_capacity_decision(turn, snapshot.as_ref(), &decision)
            .await;

        if decision.action != GuardrailAction::TargetedContextRefresh {
            return false;
        }

        self.apply_targeted_context_refresh(turn, client, mode, snapshot.as_ref())
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_capacity_post_tool_checkpoint(
        &mut self,
        turn: &TurnContext,
        mode: AppMode,
        tool_registry: Option<&crate::tools::ToolRegistry>,
        tool_exec_lock: Arc<RwLock<()>>,
        mcp_pool: Option<Arc<AsyncMutex<McpPool>>>,
        _step_error_count: usize,
        _consecutive_tool_error_steps: u32,
    ) -> bool {
        let snapshot = self
            .capacity_controller
            .observe_post_tool(self.capacity_observation(turn));
        let decision = self
            .capacity_controller
            .decide(self.turn_counter, snapshot.as_ref());
        self.emit_capacity_decision(turn, snapshot.as_ref(), &decision)
            .await;

        match decision.action {
            GuardrailAction::VerifyWithToolReplay => {
                let _ = self
                    .apply_verify_with_tool_replay(
                        turn,
                        mode,
                        snapshot.as_ref(),
                        tool_registry,
                        tool_exec_lock,
                        mcp_pool,
                    )
                    .await;
                false
            }
            GuardrailAction::VerifyAndReplan => {
                self.apply_verify_and_replan(turn, mode, snapshot.as_ref(), "high_risk_post_tool")
                    .await
            }
            GuardrailAction::NoIntervention | GuardrailAction::TargetedContextRefresh => false,
        }
    }

    async fn run_capacity_error_escalation_checkpoint(
        &mut self,
        turn: &TurnContext,
        mode: AppMode,
        step_error_count: usize,
        consecutive_tool_error_steps: u32,
    ) -> bool {
        if step_error_count == 0 && consecutive_tool_error_steps < 2 {
            return false;
        }

        let snapshot = self
            .capacity_controller
            .last_snapshot()
            .cloned()
            .or_else(|| {
                self.capacity_controller
                    .observe_pre_turn(self.capacity_observation(turn))
            });
        let Some(snapshot) = snapshot else {
            return false;
        };

        let repeated_failures = step_error_count >= 2 || consecutive_tool_error_steps >= 2;
        let mut forced = snapshot.clone();
        if repeated_failures && !(snapshot.risk_band == RiskBand::High && snapshot.severe) {
            forced.risk_band = RiskBand::High;
            forced.severe = true;
        }

        let decision = self
            .capacity_controller
            .decide(self.turn_counter, Some(&forced));
        self.emit_capacity_decision(turn, Some(&forced), &decision)
            .await;

        if decision.action != GuardrailAction::VerifyAndReplan {
            return false;
        }

        self.apply_verify_and_replan(
            turn,
            mode,
            Some(&forced),
            &format!(
                "error_escalation: step_errors={step_error_count}, consecutive_steps={consecutive_tool_error_steps}"
            ),
        )
        .await
    }

    fn capacity_observation(&self, turn: &TurnContext) -> CapacityObservationInput {
        let message_window = self.config.capacity.profile_window.max(8) * 3;
        let action_count_this_turn = usize::try_from(turn.step)
            .unwrap_or(usize::MAX)
            .saturating_add(turn.tool_calls.len())
            .saturating_add(1);
        let tool_calls_recent_window = self.recent_tool_call_count(message_window);
        let unique_reference_ids_recent_window =
            self.recent_unique_reference_count(message_window, turn);
        let context_window = usize::try_from(
            context_window_for_model(&self.session.model).unwrap_or(DEFAULT_CONTEXT_WINDOW_TOKENS),
        )
        .unwrap_or(usize::try_from(DEFAULT_CONTEXT_WINDOW_TOKENS).unwrap_or(128_000))
        .max(1);
        let context_used_ratio = (self.estimated_input_tokens() as f64) / (context_window as f64);

        CapacityObservationInput {
            turn_index: self.turn_counter,
            model: self.session.model.clone(),
            action_count_this_turn,
            tool_calls_recent_window,
            unique_reference_ids_recent_window,
            context_used_ratio,
        }
    }

    fn recent_tool_call_count(&self, message_window: usize) -> usize {
        self.session
            .messages
            .iter()
            .rev()
            .take(message_window)
            .map(|msg| {
                msg.content
                    .iter()
                    .filter(|block| {
                        matches!(
                            block,
                            ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. }
                        )
                    })
                    .count()
            })
            .sum()
    }

    fn recent_unique_reference_count(&self, message_window: usize, turn: &TurnContext) -> usize {
        let mut refs = std::collections::HashSet::new();
        for msg in self.session.messages.iter().rev().take(message_window) {
            for block in &msg.content {
                match block {
                    ContentBlock::ToolUse { id, .. } => {
                        refs.insert(id.clone());
                    }
                    ContentBlock::ToolResult { tool_use_id, .. } => {
                        refs.insert(tool_use_id.clone());
                    }
                    ContentBlock::Text { text, .. } => {
                        for token in text.split_whitespace() {
                            if token.contains('/') || token.contains('.') {
                                refs.insert(
                                    token
                                        .trim_matches(|c: char| ",.;:()[]{}".contains(c))
                                        .to_string(),
                                );
                            }
                        }
                    }
                    ContentBlock::Thinking { .. } => {}
                }
            }
        }
        for tool_call in turn.tool_calls.iter().rev().take(8) {
            refs.insert(tool_call.id.clone());
        }
        for path in self.session.working_set.top_paths(8) {
            refs.insert(path);
        }
        refs.retain(|item| !item.is_empty());
        refs.len()
    }

    async fn emit_capacity_decision(
        &self,
        turn: &TurnContext,
        snapshot: Option<&CapacitySnapshot>,
        decision: &CapacityDecision,
    ) {
        let Some(snapshot) = snapshot else {
            return;
        };
        let _ = self
            .tx_event
            .send(Event::CapacityDecision {
                session_id: self.session.id.clone(),
                turn_id: turn.id.clone(),
                h_hat: snapshot.h_hat,
                c_hat: snapshot.c_hat,
                slack: snapshot.slack,
                min_slack: snapshot.profile.min_slack,
                violation_ratio: snapshot.profile.violation_ratio,
                p_fail: snapshot.p_fail,
                risk_band: snapshot.risk_band.as_str().to_string(),
                action: decision.action.as_str().to_string(),
                cooldown_blocked: decision.cooldown_blocked,
                reason: decision.reason.clone(),
            })
            .await;
    }

    async fn emit_capacity_intervention(
        &self,
        turn: &TurnContext,
        action: GuardrailAction,
        before_prompt_tokens: usize,
        after_prompt_tokens: usize,
        replay_outcome: Option<String>,
        replan_performed: bool,
    ) {
        let _ = self
            .tx_event
            .send(Event::CapacityIntervention {
                session_id: self.session.id.clone(),
                turn_id: turn.id.clone(),
                action: action.as_str().to_string(),
                before_prompt_tokens,
                after_prompt_tokens,
                compaction_size_reduction: before_prompt_tokens.saturating_sub(after_prompt_tokens),
                replay_outcome,
                replan_performed,
            })
            .await;
    }

    async fn apply_targeted_context_refresh(
        &mut self,
        turn: &TurnContext,
        client: Option<&DeepSeekClient>,
        mode: AppMode,
        snapshot: Option<&CapacitySnapshot>,
    ) -> bool {
        let before_tokens = self.estimated_input_tokens();
        let compaction_pins = self
            .session
            .working_set
            .pinned_message_indices(&self.session.messages, &self.session.workspace);
        let compaction_paths = self.session.working_set.top_paths(24);

        let mut refreshed = false;
        if let Some(client) = client {
            match compact_messages_safe(
                client,
                &self.session.messages,
                &self.config.compaction,
                Some(&self.session.workspace),
                Some(&compaction_pins),
                Some(&compaction_paths),
            )
            .await
            {
                Ok(result) => {
                    if !result.messages.is_empty() || self.session.messages.is_empty() {
                        self.session.messages = result.messages;
                        self.merge_compaction_summary(result.summary_prompt);
                        refreshed = true;
                    }
                }
                Err(err) => {
                    let _ = self
                        .tx_event
                        .send(Event::status(format!(
                            "Capacity refresh compaction failed: {err}. Falling back to local trim."
                        )))
                        .await;
                }
            }
        }

        if !refreshed {
            let target_budget = context_input_budget(&self.session.model, TURN_MAX_OUTPUT_TOKENS)
                .unwrap_or(self.config.compaction.token_threshold.max(1));
            let trimmed = self.trim_oldest_messages_to_budget(target_budget);
            refreshed = trimmed > 0;
        }

        if !refreshed {
            return false;
        }

        let canonical = self.build_canonical_state(turn, None);
        let source_message_ids = self.capacity_source_message_ids(turn);
        let record = self.build_capacity_record(
            turn,
            GuardrailAction::TargetedContextRefresh,
            snapshot,
            canonical.clone(),
            source_message_ids,
            None,
        );
        let pointer = self
            .persist_capacity_record(turn, GuardrailAction::TargetedContextRefresh, &record)
            .await;
        self.merge_compaction_summary(Some(self.canonical_prompt(
            &canonical,
            &pointer,
            GuardrailAction::TargetedContextRefresh,
            None,
        )));
        self.refresh_system_prompt(mode);

        let after_tokens = self.estimated_input_tokens();
        self.emit_capacity_intervention(
            turn,
            GuardrailAction::TargetedContextRefresh,
            before_tokens,
            after_tokens,
            None,
            false,
        )
        .await;
        self.capacity_controller
            .mark_intervention_applied(self.turn_counter, GuardrailAction::TargetedContextRefresh);
        true
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_verify_with_tool_replay(
        &mut self,
        turn: &TurnContext,
        mode: AppMode,
        snapshot: Option<&CapacitySnapshot>,
        tool_registry: Option<&crate::tools::ToolRegistry>,
        tool_exec_lock: Arc<RwLock<()>>,
        mut mcp_pool: Option<Arc<AsyncMutex<McpPool>>>,
    ) -> bool {
        let before_tokens = self.estimated_input_tokens();
        let Some(candidate) = self.select_replay_candidate(turn, tool_registry) else {
            return false;
        };

        if McpPool::is_mcp_tool(&candidate.name) && mcp_pool.is_none() {
            mcp_pool = self.ensure_mcp_pool().await.ok();
        }

        let supports_parallel = if McpPool::is_mcp_tool(&candidate.name) {
            mcp_tool_is_parallel_safe(&candidate.name)
        } else {
            tool_registry
                .and_then(|registry| registry.get(&candidate.name))
                .is_some_and(|spec| spec.supports_parallel())
        };
        let interactive = (candidate.name == "exec_shell"
            && candidate
                .input
                .get("interactive")
                .and_then(serde_json::Value::as_bool)
                == Some(true))
            || candidate.name == REQUEST_USER_INPUT_NAME;

        let replay_result = Self::execute_tool_with_lock(
            tool_exec_lock,
            supports_parallel,
            interactive,
            self.tx_event.clone(),
            candidate.name.clone(),
            candidate.input.clone(),
            tool_registry,
            mcp_pool.clone(),
            None,
        )
        .await;

        let (pass, replay_outcome, diff_summary) = match replay_result {
            Ok(output) => {
                let original = candidate.result.as_deref().unwrap_or_default();
                let replay = output.content.as_str();
                let equal = original.trim() == replay.trim();
                let diff = if equal {
                    "output_match".to_string()
                } else {
                    format!(
                        "output_mismatch: original='{}' replay='{}'",
                        summarize_text(original, 140),
                        summarize_text(replay, 140)
                    )
                };
                (
                    equal,
                    if equal {
                        "pass".to_string()
                    } else {
                        "conflict".to_string()
                    },
                    diff,
                )
            }
            Err(err) => {
                self.capacity_controller
                    .mark_replay_failed(self.turn_counter);
                (
                    false,
                    "error".to_string(),
                    format!("replay_error: {}", summarize_text(&err.to_string(), 180)),
                )
            }
        };

        let verification_note = format!(
            "[verification replay] tool={} pass={} details={}",
            candidate.name, pass, diff_summary
        );
        self.session.add_message(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: candidate.id.clone(),
                content: verification_note.clone(),
            }],
        });

        if !pass {
            self.capacity_controller
                .mark_replay_failed(self.turn_counter);
        }

        let canonical = self.build_canonical_state(
            turn,
            Some(if pass {
                "replay verification passed"
            } else {
                "replay verification failed or conflicted"
            }),
        );
        let replay_info = Some(ReplayInfo {
            tool_id: candidate.id.clone(),
            tool_name: candidate.name.clone(),
            pass,
            diff_summary: diff_summary.clone(),
        });
        let source_message_ids = self.capacity_source_message_ids(turn);
        let record = self.build_capacity_record(
            turn,
            GuardrailAction::VerifyWithToolReplay,
            snapshot,
            canonical.clone(),
            source_message_ids,
            replay_info,
        );
        let pointer = self
            .persist_capacity_record(turn, GuardrailAction::VerifyWithToolReplay, &record)
            .await;
        self.merge_compaction_summary(Some(self.canonical_prompt(
            &canonical,
            &pointer,
            GuardrailAction::VerifyWithToolReplay,
            Some(&verification_note),
        )));
        self.refresh_system_prompt(mode);

        let after_tokens = self.estimated_input_tokens();
        self.emit_capacity_intervention(
            turn,
            GuardrailAction::VerifyWithToolReplay,
            before_tokens,
            after_tokens,
            Some(replay_outcome),
            false,
        )
        .await;
        self.capacity_controller
            .mark_intervention_applied(self.turn_counter, GuardrailAction::VerifyWithToolReplay);
        true
    }

    async fn apply_verify_and_replan(
        &mut self,
        turn: &TurnContext,
        mode: AppMode,
        snapshot: Option<&CapacitySnapshot>,
        reason: &str,
    ) -> bool {
        let before_tokens = self.estimated_input_tokens();
        let canonical = self.build_canonical_state(turn, Some(reason));
        let source_message_ids = self.capacity_source_message_ids(turn);
        let record = self.build_capacity_record(
            turn,
            GuardrailAction::VerifyAndReplan,
            snapshot,
            canonical.clone(),
            source_message_ids,
            None,
        );
        let pointer = self
            .persist_capacity_record(turn, GuardrailAction::VerifyAndReplan, &record)
            .await;

        let latest_user = self
            .session
            .messages
            .iter()
            .rev()
            .find(|msg| {
                msg.role == "user"
                    && msg
                        .content
                        .iter()
                        .any(|block| matches!(block, ContentBlock::Text { .. }))
            })
            .cloned();
        let latest_verified = self
            .session
            .messages
            .iter()
            .rev()
            .find(|msg| {
                msg.role == "user"
                    && msg.content.iter().any(|block| match block {
                        ContentBlock::ToolResult { content, .. } => {
                            content.contains("[verification replay]")
                        }
                        _ => false,
                    })
            })
            .cloned();

        self.session.messages.clear();
        if let Some(msg) = latest_user {
            self.session.messages.push(msg);
        }
        if let Some(msg) = latest_verified {
            self.session.messages.push(msg);
        }

        self.merge_compaction_summary(Some(self.canonical_prompt(
            &canonical,
            &pointer,
            GuardrailAction::VerifyAndReplan,
            Some("Replan now from canonical state. Keep steps minimal and verifiable."),
        )));
        self.refresh_system_prompt(mode);

        let _ = self
            .tx_event
            .send(Event::status(
                "Capacity guardrail: context reset to canonical state; replanning step."
                    .to_string(),
            ))
            .await;

        let after_tokens = self.estimated_input_tokens();
        self.emit_capacity_intervention(
            turn,
            GuardrailAction::VerifyAndReplan,
            before_tokens,
            after_tokens,
            None,
            true,
        )
        .await;
        self.capacity_controller
            .mark_intervention_applied(self.turn_counter, GuardrailAction::VerifyAndReplan);
        true
    }

    fn select_replay_candidate(
        &self,
        turn: &TurnContext,
        tool_registry: Option<&crate::tools::ToolRegistry>,
    ) -> Option<TurnToolCall> {
        turn.tool_calls
            .iter()
            .rev()
            .find(|call| {
                call.error.is_none()
                    && call.result.is_some()
                    && self.tool_is_replayable_read_only(&call.name, tool_registry)
            })
            .cloned()
    }

    fn tool_is_replayable_read_only(
        &self,
        tool_name: &str,
        tool_registry: Option<&crate::tools::ToolRegistry>,
    ) -> bool {
        if tool_name == MULTI_TOOL_PARALLEL_NAME || tool_name == REQUEST_USER_INPUT_NAME {
            return false;
        }
        if McpPool::is_mcp_tool(tool_name) {
            return mcp_tool_is_read_only(tool_name);
        }
        tool_registry
            .and_then(|registry| registry.get(tool_name))
            .is_some_and(|spec| spec.is_read_only())
    }

    fn build_canonical_state(&self, turn: &TurnContext, note: Option<&str>) -> CanonicalState {
        let goal = self
            .session
            .messages
            .iter()
            .rev()
            .find_map(|msg| {
                if msg.role != "user" {
                    return None;
                }
                msg.content.iter().find_map(|block| match block {
                    ContentBlock::Text { text, .. } => Some(summarize_text(text, 220)),
                    _ => None,
                })
            })
            .unwrap_or_else(|| "Continue current task from compact state".to_string());

        let mut constraints = vec![
            format!("model={}", self.session.model),
            format!("workspace={}", self.session.workspace.display()),
        ];
        if let Some(note) = note {
            constraints.push(summarize_text(note, 180));
        }

        let mut confirmed_facts = Vec::new();
        for msg in self.session.messages.iter().rev() {
            for block in &msg.content {
                if let ContentBlock::ToolResult { content, .. } = block {
                    if content.starts_with("Error:") {
                        continue;
                    }
                    confirmed_facts.push(summarize_text(content, 180));
                    if confirmed_facts.len() >= 4 {
                        break;
                    }
                }
            }
            if confirmed_facts.len() >= 4 {
                break;
            }
        }

        let open_loops: Vec<String> = turn
            .tool_calls
            .iter()
            .rev()
            .filter_map(|call| {
                call.error
                    .as_ref()
                    .map(|error| format!("{}: {}", call.name, summarize_text(error, 180)))
            })
            .take(4)
            .collect();

        let pending_actions: Vec<String> = if open_loops.is_empty() {
            vec!["Continue with next smallest verifiable step".to_string()]
        } else {
            vec![
                "Re-evaluate failed tool steps with narrower scope".to_string(),
                "Re-derive plan from canonical facts before further edits".to_string(),
            ]
        };

        let mut critical_refs = self.session.working_set.top_paths(8);
        for tool_call in turn.tool_calls.iter().rev().take(4) {
            critical_refs.push(format!("tool:{}", tool_call.id));
        }
        critical_refs.dedup();

        CanonicalState {
            goal,
            constraints,
            confirmed_facts,
            open_loops,
            pending_actions,
            critical_refs,
        }
    }

    fn canonical_prompt(
        &self,
        canonical: &CanonicalState,
        pointer: &str,
        action: GuardrailAction,
        extra: Option<&str>,
    ) -> SystemPrompt {
        let mut lines = vec![
            COMPACTION_SUMMARY_MARKER.to_string(),
            format!("Capacity Canonical State [{}]", action.as_str()),
            format!("Goal: {}", canonical.goal),
            "Constraints:".to_string(),
        ];
        for item in &canonical.constraints {
            lines.push(format!("- {}", summarize_text(item, 200)));
        }
        lines.push("Confirmed Facts:".to_string());
        for item in &canonical.confirmed_facts {
            lines.push(format!("- {}", summarize_text(item, 200)));
        }
        lines.push("Open Loops:".to_string());
        if canonical.open_loops.is_empty() {
            lines.push("- none".to_string());
        } else {
            for item in &canonical.open_loops {
                lines.push(format!("- {}", summarize_text(item, 200)));
            }
        }
        lines.push("Pending Actions:".to_string());
        for item in &canonical.pending_actions {
            lines.push(format!("- {}", summarize_text(item, 200)));
        }
        lines.push("Critical Refs:".to_string());
        for item in &canonical.critical_refs {
            lines.push(format!("- {}", summarize_text(item, 200)));
        }
        if let Some(extra) = extra {
            lines.push(format!("Instruction: {}", summarize_text(extra, 240)));
        }
        lines.push(format!("Memory Pointer: {pointer}"));

        SystemPrompt::Blocks(vec![crate::models::SystemBlock {
            block_type: "text".to_string(),
            text: lines.join("\n"),
            cache_control: None,
        }])
    }

    fn capacity_source_message_ids(&self, turn: &TurnContext) -> Vec<String> {
        let mut ids: Vec<String> = turn
            .tool_calls
            .iter()
            .rev()
            .take(8)
            .map(|call| call.id.clone())
            .collect();
        ids.reverse();
        ids
    }

    fn build_capacity_record(
        &self,
        turn: &TurnContext,
        action: GuardrailAction,
        snapshot: Option<&CapacitySnapshot>,
        canonical: CanonicalState,
        source_message_ids: Vec<String>,
        replay_info: Option<ReplayInfo>,
    ) -> CapacityMemoryRecord {
        let (h_hat, c_hat, slack, risk_band) = snapshot
            .map(|s| (s.h_hat, s.c_hat, s.slack, s.risk_band.as_str().to_string()))
            .unwrap_or_else(|| (0.0, 0.0, 0.0, "unknown".to_string()));

        CapacityMemoryRecord {
            id: new_record_id(),
            ts: now_rfc3339(),
            turn_index: self.turn_counter,
            action_trigger: action.as_str().to_string(),
            h_hat,
            c_hat,
            slack,
            risk_band,
            canonical_state: canonical,
            source_message_ids: if source_message_ids.is_empty() {
                vec![turn.id.clone()]
            } else {
                source_message_ids
            },
            replay_info,
        }
    }

    async fn persist_capacity_record(
        &mut self,
        turn: &TurnContext,
        action: GuardrailAction,
        record: &CapacityMemoryRecord,
    ) -> String {
        let pointer = format!("memory://{}/{}", self.session.id, record.id);
        if let Err(err) = append_capacity_record(&self.session.id, record) {
            let _ = self
                .tx_event
                .send(Event::CapacityMemoryPersistFailed {
                    session_id: self.session.id.clone(),
                    turn_id: turn.id.clone(),
                    action: action.as_str().to_string(),
                    error: summarize_text(&err.to_string(), 280),
                })
                .await;
            return format!("{pointer}?persist=failed");
        }
        pointer
    }

    fn rehydrate_latest_canonical_state(&mut self) {
        let Ok(records) = load_last_k_capacity_records(&self.session.id, 1) else {
            return;
        };
        let Some(last) = records.last() else {
            return;
        };
        let pointer = format!("memory://{}/{}", self.session.id, last.id);
        let prompt = self.canonical_prompt(
            &last.canonical_state,
            &pointer,
            GuardrailAction::NoIntervention,
            Some("Rehydrated canonical state from memory."),
        );
        self.merge_compaction_summary(Some(prompt));
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
        let working_set_summary = self
            .session
            .working_set
            .summary_block(&self.config.workspace);
        let base = prompts::system_prompt_for_mode_with_context(
            mode,
            &self.config.workspace,
            working_set_summary.as_deref(),
        );
        self.session.system_prompt =
            merge_system_prompts(Some(&base), self.session.compaction_summary_prompt.clone());
    }

    fn merge_compaction_summary(&mut self, summary_prompt: Option<SystemPrompt>) {
        if summary_prompt.is_none() {
            return;
        }
        self.session.compaction_summary_prompt = merge_system_prompts(
            self.session.compaction_summary_prompt.as_ref(),
            summary_prompt.clone(),
        );
        self.session.system_prompt =
            merge_system_prompts(self.session.system_prompt.as_ref(), summary_prompt);
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
pub(crate) struct MockEngineHandle {
    pub handle: EngineHandle,
    pub rx_op: mpsc::Receiver<Op>,
    pub rx_steer: mpsc::Receiver<String>,
    pub tx_event: mpsc::Sender<Event>,
    pub cancel_token: CancellationToken,
}

#[cfg(test)]
pub(crate) fn mock_engine_handle() -> MockEngineHandle {
    let (tx_op, rx_op) = mpsc::channel(32);
    let (tx_event, rx_event) = mpsc::channel(256);
    let (tx_approval, _rx_approval) = mpsc::channel(64);
    let (tx_user_input, _rx_user_input) = mpsc::channel(32);
    let (tx_steer, rx_steer) = mpsc::channel(64);
    let cancel_token = CancellationToken::new();
    let shared_cancel_token = Arc::new(StdMutex::new(cancel_token.clone()));
    let handle = EngineHandle {
        tx_op,
        rx_event: Arc::new(RwLock::new(rx_event)),
        cancel_token: shared_cancel_token,
        tx_approval,
        tx_user_input,
        tx_steer,
    };

    MockEngineHandle {
        handle,
        rx_op,
        rx_steer,
        tx_event,
        cancel_token,
    }
}

#[cfg(test)]
mod tests;
