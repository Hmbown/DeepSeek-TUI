//! Core engine for `DeepSeek` CLI.
//!
//! The engine handles all AI interactions in a background task,
//! communicating with the UI via channels. This enables:
//! - Non-blocking UI during API calls
//! - Real-time streaming updates
//! - Proper cancellation support
//! - Tool execution orchestration

use std::collections::HashMap;
use std::path::PathBuf;
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
use crate::cycle_manager::{
    CycleBriefing, CycleConfig, StructuredState, archive_cycle, build_seed_messages,
    estimate_briefing_tokens, produce_briefing, should_advance_cycle,
};
use crate::error_taxonomy::{ErrorCategory, ErrorEnvelope, StreamError};
use crate::features::{Feature, Features};
use crate::llm_client::LlmClient;
use crate::mcp::McpPool;
use crate::models::{
    ContentBlock, ContentBlockStart, DEFAULT_CONTEXT_WINDOW_TOKENS, Delta, Message, MessageRequest,
    StreamEvent, SystemBlock, SystemPrompt, Tool, ToolCaller, Usage, context_window_for_model,
};
use crate::prompts;
use crate::seam_manager::{SeamConfig, SeamManager};
use crate::tools::plan::{SharedPlanState, new_shared_plan_state};
use crate::tools::shell::{SharedShellManager, new_shared_shell_manager};
use crate::tools::spec::RuntimeToolServices;
use crate::tools::spec::{ApprovalRequirement, ToolError, ToolResult, required_str};
use crate::tools::subagent::{
    Mailbox, SharedSubAgentManager, SubAgentRuntime, SubAgentType, new_shared_subagent_manager,
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
use super::coherence::{CoherenceSignal, CoherenceState, next_coherence_state};
use super::events::{Event, TurnOutcomeStatus};
use super::ops::Op;
use super::session::Session;
use super::tool_parser;
use super::turn::{TurnContext, TurnToolCall, post_turn_snapshot, pre_turn_snapshot};

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
    ///
    /// As of v0.6.6 the high-level summarization compaction (`compact_messages_safe`)
    /// is **disabled by default**; the checkpoint-restart cycle architecture
    /// (`cycle_manager`) replaces it. The compaction config is still wired through
    /// for the per-tool-result truncation path (`compact_tool_result_for_context`)
    /// and for users who explicitly opt back in through the `auto_compact`
    /// setting or a direct engine config.
    pub compaction: CompactionConfig,
    /// Checkpoint-restart cycle settings (issue #124).
    pub cycle: CycleConfig,
    /// Capacity-controller settings.
    pub capacity: CapacityControllerConfig,
    /// Shared Todo list state.
    pub todos: SharedTodoList,
    /// Shared Plan state.
    pub plan_state: SharedPlanState,
    /// Maximum sub-agent recursion depth (default 3). See
    /// `SubAgentRuntime::max_spawn_depth`. Override via
    /// `[runtime] max_spawn_depth = N` in `~/.deepseek/config.toml`.
    pub max_spawn_depth: u32,
    /// Per-domain network policy decider (#135). Shared across the session so
    /// session-scoped approvals (`/network allow <host>`) persist for the
    /// remainder of the run.
    pub network_policy: Option<crate::network_policy::NetworkPolicyDecider>,
    /// Whether to take side-git workspace snapshots before/after each turn.
    pub snapshots_enabled: bool,
    /// Post-edit LSP diagnostics injection (#136). When `None`, the engine
    /// constructs a disabled manager so the field is always present.
    pub lsp_config: Option<crate::lsp::LspConfig>,
    /// Durable runtime services exposed to model-visible tools.
    pub runtime_services: RuntimeToolServices,
    /// Per-role/type sub-agent model overrides already resolved from config.
    pub subagent_model_overrides: HashMap<String, String>,
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
            cycle: CycleConfig::default(),
            capacity: CapacityControllerConfig::default(),
            todos: new_shared_todo_list(),
            plan_state: new_shared_plan_state(),
            max_spawn_depth: crate::tools::subagent::DEFAULT_MAX_SPAWN_DEPTH,
            network_policy: None,
            snapshots_enabled: true,
            lsp_config: None,
            runtime_services: RuntimeToolServices::default(),
            subagent_model_overrides: HashMap::new(),
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
    #[allow(dead_code)]
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
    /// Append-only layered context manager (#159). Opt-in for v0.7.5 while
    /// cache-hit behavior is audited.
    seam_manager: Option<SeamManager>,
    coherence_state: CoherenceState,
    turn_counter: u64,
    /// Post-edit LSP diagnostics injection (#136). Populated unconditionally
    /// — when LSP is disabled in config, this is an inert manager that
    /// always returns `None` from `diagnostics_for`.
    lsp_manager: Arc<crate::lsp::LspManager>,
    /// Diagnostics collected during the current step's tool calls. Drained
    /// and forwarded as a synthetic user message before the next API call.
    pending_lsp_blocks: Vec<crate::lsp::DiagnosticBlock>,
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
    caller: Option<ToolCaller>,
    input_buffer: String,
}

/// Maximum time to wait for a single stream chunk before assuming a stall.
/// **This is the idle timeout** — it resets on every SSE chunk, so long
/// thinking turns that ARE producing reasoning_content stay alive. Only a
/// genuine `chunk_timeout` window of silence kills the stream.
const STREAM_CHUNK_TIMEOUT_SECS: u64 = 90;
/// Maximum total bytes of text/thinking content before aborting the stream.
const STREAM_MAX_CONTENT_BYTES: usize = 10 * 1024 * 1024; // 10 MB
/// Sanity backstop for total stream wall-clock duration. **Not** a routine
/// kill switch — `STREAM_CHUNK_TIMEOUT_SECS` (idle) is the primary stall
/// detector. The wall-clock cap is here only to bound pathological cases
/// (e.g. a server that keeps sending heartbeats forever without progress).
///
/// History: this used to be 300s (5 min) which was too aggressive — V4
/// thinking turns on hard prompts legitimately exceed 5 minutes wall-clock
/// while still emitting reasoning_content chunks the whole way. Bumped to
/// 30 min in v0.6.6 to address `TODO_FIXES.md` #1. Codex defaults to a
/// per-chunk idle of 300s with no wall-clock cap; we keep both layers but
/// give the wall-clock a generous window so it never fires in practice.
const STREAM_MAX_DURATION_SECS: u64 = 1800; // 30 minutes (was 300s; #103/#1)
/// Hard cap on consecutive recoverable stream errors before we surface a turn
/// failure. Bumped 3 → 5 in v0.6.7 along with the HTTP/2 keepalive defaults
/// (#103) — keepalive should make spurious decode errors rarer, so we can
/// tolerate a longer streak before giving up on the turn.
const MAX_STREAM_ERRORS_BEFORE_FAIL: u32 = 5;
/// Cap on transparent stream-level retries — these only happen when the wire
/// dies before any content was streamed, so DeepSeek hasn't billed us and
/// the user hasn't seen anything. Two attempts is enough to ride out a
/// flaky edge node without amplifying real outages (#103).
const MAX_TRANSPARENT_STREAM_RETRIES: u32 = 2;

/// Decide whether a stream error is eligible for a transparent retry.
///
/// True only when ALL three conditions hold:
/// 1. No content has been received on the current attempt — otherwise DeepSeek
///    has already billed us for output tokens and the user has seen partial
///    deltas; resending would double-bill and desync the UI.
/// 2. We still have transparent-retry budget remaining.
/// 3. The turn has not been cancelled.
///
/// Extracted as a pure function so the four #103 retry cases can be exercised
/// in unit tests without booting the full engine state machine.
fn should_transparently_retry_stream(
    any_content_received: bool,
    transparent_attempts: u32,
    cancelled: bool,
) -> bool {
    !any_content_received && transparent_attempts < MAX_TRANSPARENT_STREAM_RETRIES && !cancelled
}
/// Max output tokens requested for normal agent turns. Generous on purpose:
/// V4 thinking models can produce tens of thousands of reasoning tokens on
/// hard prompts before the visible reply, and DeepSeek V4 ships with a 1M
/// context window. v0.7.5 keeps this cap fixed instead of silently lowering
/// `max_tokens` near pressure; hard-cycle/preflight checks reserve this budget
/// plus safety headroom before sending the next request.
const TURN_MAX_OUTPUT_TOKENS: u32 = 262_144;
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
/// Hard cap for tool output inserted into a large-context model.
const LARGE_CONTEXT_TOOL_RESULT_HARD_LIMIT_CHARS: usize = 180_000;
/// Soft cap for known noisy tools inserted into a large-context model.
const LARGE_CONTEXT_TOOL_RESULT_SOFT_LIMIT_CHARS: usize = 60_000;
/// Snippet length kept when compacting large-context tool output.
const LARGE_CONTEXT_TOOL_RESULT_SNIPPET_CHARS: usize = 40_000;
/// Context window size at which tool output limits can be relaxed.
const LARGE_CONTEXT_WINDOW_TOKENS: u32 = 500_000;
/// Max chars to keep from metadata-provided output summaries.
const TOOL_RESULT_METADATA_SUMMARY_CHARS: usize = 320;
const COMPACTION_SUMMARY_MARKER: &str = "Conversation Summary (Auto-Generated)";
const WORKING_SET_SUMMARY_MARKER: &str = "## Repo Working Set";

pub(crate) const TOOL_CALL_START_MARKERS: [&str; 5] = [
    "[TOOL_CALL]",
    "<deepseek:tool_call",
    "<tool_call",
    "<invoke ",
    "<function_calls>",
];

const MULTI_TOOL_PARALLEL_NAME: &str = "multi_tool_use.parallel";
const REQUEST_USER_INPUT_NAME: &str = "request_user_input";
const CODE_EXECUTION_TOOL_NAME: &str = "code_execution";
const CODE_EXECUTION_TOOL_TYPE: &str = "code_execution_20250825";
const TOOL_SEARCH_REGEX_NAME: &str = "tool_search_tool_regex";
const TOOL_SEARCH_REGEX_TYPE: &str = "tool_search_tool_regex_20251119";
const TOOL_SEARCH_BM25_NAME: &str = "tool_search_tool_bm25";
const TOOL_SEARCH_BM25_TYPE: &str = "tool_search_tool_bm25_20251119";
pub(crate) const TOOL_CALL_END_MARKERS: [&str; 5] = [
    "[/TOOL_CALL]",
    "</deepseek:tool_call>",
    "</tool_call>",
    "</invoke>",
    "</function_calls>",
];

/// Compact one-shot notice emitted when a model attempts to forge a tool-call
/// wrapper in plain text instead of using the API tool channel. The visible
/// content is still scrubbed; this exists so the user can see why their text
/// shrank.
pub(crate) const FAKE_WRAPPER_NOTICE: &str =
    "Stripped non-API tool-call wrapper from model output (use the API tool channel)";

/// True if `text` contains any of the known fake-wrapper start markers. Used by
/// the streaming loop to decide whether to emit `FAKE_WRAPPER_NOTICE`.
pub(crate) fn contains_fake_tool_wrapper(text: &str) -> bool {
    TOOL_CALL_START_MARKERS.iter().any(|m| text.contains(m))
}

fn find_first_marker(text: &str, markers: &[&str]) -> Option<(usize, usize)> {
    markers
        .iter()
        .filter_map(|marker| text.find(marker).map(|idx| (idx, marker.len())))
        .min_by_key(|(idx, _)| *idx)
}

pub(crate) fn filter_tool_call_delta(delta: &str, in_tool_call: &mut bool) -> String {
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

/// Compute the tool input that should be reported when a tool's stream block
/// closes (`ContentBlockStop`). Prefers the parsed `input_buffer` over the
/// initial `input` placeholder so a `ToolCallStarted` event never carries a
/// stale `{}` when args were actually streamed in via `InputJsonDelta`.
///
/// Order of preference:
///   1. `input_buffer` parses cleanly → use that.
///   2. `input_buffer` is empty → fall back to `input` (model embedded args
///      directly in the `ContentBlockStart` frame and sent no deltas).
///   3. `input_buffer` non-empty but unparseable → fall back to `input`
///      (the per-delta parser has already mirrored the most recent valid
///      partial parse into `tool_state.input`).
fn is_tool_search_tool(name: &str) -> bool {
    matches!(name, TOOL_SEARCH_REGEX_NAME | TOOL_SEARCH_BM25_NAME)
}

fn should_default_defer_tool(name: &str, mode: AppMode) -> bool {
    if mode == AppMode::Yolo {
        return false;
    }

    // Shell tools are kept active in Agent so the model can run verification
    // commands (build/test/git/cargo) without first having to discover the
    // tool through ToolSearch. Plan mode never registers shell tools.
    let always_loaded_in_action_modes = matches!(mode, AppMode::Agent)
        && matches!(
            name,
            "exec_shell"
                | "exec_shell_wait"
                | "exec_shell_interact"
                | "exec_wait"
                | "exec_interact"
        );
    if always_loaded_in_action_modes {
        return false;
    }

    !matches!(
        name,
        "read_file"
            | "list_dir"
            | "grep_files"
            | "file_search"
            | "diagnostics"
            | "rlm"
            | "recall_archive"
            | MULTI_TOOL_PARALLEL_NAME
            | "update_plan"
            | "checklist_write"
            | "todo_write"
            | "task_create"
            | "task_list"
            | "task_read"
            | "task_gate_run"
            | "task_shell_start"
            | "task_shell_wait"
            | "github_issue_context"
            | "github_pr_context"
            | REQUEST_USER_INPUT_NAME
    )
}

fn ensure_advanced_tooling(catalog: &mut Vec<Tool>) {
    if !catalog.iter().any(|t| t.name == CODE_EXECUTION_TOOL_NAME) {
        catalog.push(Tool {
            tool_type: Some(CODE_EXECUTION_TOOL_TYPE.to_string()),
            name: CODE_EXECUTION_TOOL_NAME.to_string(),
            description: "Execute Python code in a local sandboxed runtime and return stdout/stderr/return_code as JSON.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Python source code to execute." }
                },
                "required": ["code"]
            }),
            allowed_callers: Some(vec!["direct".to_string()]),
            defer_loading: Some(false),
            input_examples: None,
            strict: None,
            cache_control: None,
        });
    }

    if !catalog.iter().any(|t| t.name == TOOL_SEARCH_REGEX_NAME) {
        catalog.push(Tool {
            tool_type: Some(TOOL_SEARCH_REGEX_TYPE.to_string()),
            name: TOOL_SEARCH_REGEX_NAME.to_string(),
            description: "Search deferred tool definitions using a regex query and return matching tool references.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Regex pattern to search tool names/descriptions/schema." }
                },
                "required": ["query"]
            }),
            allowed_callers: Some(vec!["direct".to_string()]),
            defer_loading: Some(false),
            input_examples: None,
            strict: None,
            cache_control: None,
        });
    }

    if !catalog.iter().any(|t| t.name == TOOL_SEARCH_BM25_NAME) {
        catalog.push(Tool {
            tool_type: Some(TOOL_SEARCH_BM25_TYPE.to_string()),
            name: TOOL_SEARCH_BM25_NAME.to_string(),
            description: "Search deferred tool definitions using natural-language matching and return matching tool references.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural language query for tool discovery." }
                },
                "required": ["query"]
            }),
            allowed_callers: Some(vec!["direct".to_string()]),
            defer_loading: Some(false),
            input_examples: None,
            strict: None,
            cache_control: None,
        });
    }
}

fn initial_active_tools(catalog: &[Tool]) -> std::collections::HashSet<String> {
    let mut active = std::collections::HashSet::new();
    for tool in catalog {
        if !tool.defer_loading.unwrap_or(false) || is_tool_search_tool(&tool.name) {
            active.insert(tool.name.clone());
        }
    }
    if active.is_empty()
        && !catalog.is_empty()
        && let Some(first) = catalog.first()
    {
        active.insert(first.name.clone());
    }
    active
}

fn active_tool_list_from_catalog(
    catalog: &[Tool],
    active: &std::collections::HashSet<String>,
) -> Vec<Tool> {
    catalog
        .iter()
        .filter(|tool| active.contains(&tool.name))
        .cloned()
        .collect()
}

fn active_tools_for_step(
    catalog: &[Tool],
    active: &std::collections::HashSet<String>,
    force_update_plan: bool,
) -> Vec<Tool> {
    // DeepSeek reasoning models reject explicit named tool_choice forcing here, so for
    // obvious quick-plan asks we narrow the first-step tool surface to update_plan instead.
    if force_update_plan {
        let forced: Vec<_> = catalog
            .iter()
            .filter(|tool| tool.name == "update_plan")
            .cloned()
            .collect();
        if !forced.is_empty() {
            return forced;
        }
    }

    active_tool_list_from_catalog(catalog, active)
}

fn tool_search_haystack(tool: &Tool) -> String {
    format!(
        "{}\n{}\n{}",
        tool.name.to_lowercase(),
        tool.description.to_lowercase(),
        tool.input_schema.to_string().to_lowercase()
    )
}

fn discover_tools_with_regex(catalog: &[Tool], query: &str) -> Result<Vec<String>, ToolError> {
    let regex = regex::Regex::new(query)
        .map_err(|err| ToolError::invalid_input(format!("Invalid regex query: {err}")))?;

    let mut matches = Vec::new();
    for tool in catalog {
        if is_tool_search_tool(&tool.name) {
            continue;
        }
        let hay = tool_search_haystack(tool);
        if regex.is_match(&hay) {
            matches.push(tool.name.clone());
        }
        if matches.len() >= 5 {
            break;
        }
    }
    Ok(matches)
}

fn discover_tools_with_bm25_like(catalog: &[Tool], query: &str) -> Vec<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|term| term.trim().to_lowercase())
        .filter(|term| !term.is_empty())
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(i64, String)> = Vec::new();
    for tool in catalog {
        if is_tool_search_tool(&tool.name) {
            continue;
        }
        let hay = tool_search_haystack(tool);
        let mut score = 0i64;
        for term in &terms {
            if hay.contains(term) {
                score += 1;
            }
            if tool.name.to_lowercase().contains(term) {
                score += 2;
            }
        }
        if score > 0 {
            scored.push((score, tool.name.clone()));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().take(5).map(|(_, name)| name).collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.chars().count();
    }
    if b.is_empty() {
        return a.chars().count();
    }

    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0usize; b_chars.len() + 1];

    for (i, a_ch) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, b_ch) in b_chars.iter().enumerate() {
            let cost = if a_ch == *b_ch { 0 } else { 1 };
            let delete = prev[j + 1] + 1;
            let insert = curr[j] + 1;
            let substitute = prev[j] + cost;
            curr[j + 1] = delete.min(insert).min(substitute);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_chars.len()]
}

fn suggest_tool_names(catalog: &[Tool], requested: &str, limit: usize) -> Vec<String> {
    let requested = requested.trim().to_ascii_lowercase();
    if requested.is_empty() || limit == 0 {
        return Vec::new();
    }

    let mut candidates: Vec<(u8, usize, String)> = Vec::new();
    for tool in catalog {
        let candidate = tool.name.to_ascii_lowercase();
        let prefix_match = candidate.starts_with(&requested) || requested.starts_with(&candidate);
        let contains_match = candidate.contains(&requested) || requested.contains(&candidate);
        let distance = edit_distance(&candidate, &requested);
        let close_typo = distance <= 3;

        if !(prefix_match || contains_match || close_typo) {
            continue;
        }

        let rank = if prefix_match {
            0
        } else if contains_match {
            1
        } else {
            2
        };
        candidates.push((rank, distance, tool.name.clone()));
    }

    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    candidates.dedup_by(|a, b| a.2 == b.2);
    candidates
        .into_iter()
        .take(limit)
        .map(|(_, _, name)| name)
        .collect()
}

fn missing_tool_error_message(tool_name: &str, catalog: &[Tool]) -> String {
    let suggestions = suggest_tool_names(catalog, tool_name, 3);
    if suggestions.is_empty() {
        return format!(
            "Tool '{tool_name}' is not available in the current tool catalog. \
             Verify mode/feature flags, or use {TOOL_SEARCH_BM25_NAME} with a short query."
        );
    }

    format!(
        "Tool '{tool_name}' is not available in the current tool catalog. \
         Did you mean: {}? You can also use {TOOL_SEARCH_BM25_NAME} to discover tools.",
        suggestions.join(", ")
    )
}

fn maybe_activate_requested_deferred_tool(
    tool_name: &str,
    catalog: &[Tool],
    active_tools: &mut std::collections::HashSet<String>,
) -> bool {
    let Some(def) = catalog.iter().find(|def| def.name == tool_name) else {
        return false;
    };

    if !def.defer_loading.unwrap_or(false) || active_tools.contains(tool_name) {
        return false;
    }

    active_tools.insert(tool_name.to_string())
}

fn execute_tool_search(
    tool_name: &str,
    input: &serde_json::Value,
    catalog: &[Tool],
    active_tools: &mut std::collections::HashSet<String>,
) -> Result<ToolResult, ToolError> {
    let query = required_str(input, "query")?;
    let discovered = if tool_name == TOOL_SEARCH_REGEX_NAME {
        discover_tools_with_regex(catalog, query)?
    } else {
        discover_tools_with_bm25_like(catalog, query)
    };

    for name in &discovered {
        active_tools.insert(name.clone());
    }

    let references = discovered
        .iter()
        .map(|name| json!({"type": "tool_reference", "tool_name": name}))
        .collect::<Vec<_>>();

    let payload = json!({
        "type": "tool_search_tool_search_result",
        "tool_references": references,
    });

    Ok(ToolResult {
        content: serde_json::to_string(&payload).unwrap_or_else(|_| payload.to_string()),
        success: true,
        metadata: Some(json!({
            "tool_references": discovered,
        })),
    })
}

async fn execute_code_execution_tool(
    input: &serde_json::Value,
    workspace: &std::path::Path,
) -> Result<ToolResult, ToolError> {
    let code = required_str(input, "code")?;
    let mut cmd = tokio::process::Command::new("python3");
    cmd.arg("-c");
    cmd.arg(code);
    cmd.current_dir(workspace);

    let output = tokio::time::timeout(Duration::from_secs(120), cmd.output())
        .await
        .map_err(|_| ToolError::Timeout { seconds: 120 })
        .and_then(|res| res.map_err(|e| ToolError::execution_failed(e.to_string())))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let return_code = output.status.code().unwrap_or(-1);
    let success = output.status.success();
    let payload = json!({
        "type": "code_execution_result",
        "stdout": stdout,
        "stderr": stderr,
        "return_code": return_code,
        "content": [],
    });

    Ok(ToolResult {
        content: serde_json::to_string(&payload).unwrap_or_else(|_| payload.to_string()),
        success,
        metadata: Some(payload),
    })
}

fn caller_type_for_tool_use(caller: Option<&ToolCaller>) -> &str {
    caller.map_or("direct", |c| c.caller_type.as_str())
}

/// #136: derive the file path(s) edited by a tool call. Returns the empty
/// vec for tools that don't modify files. We intentionally only handle the
/// three known edit tools — adding more (e.g. specialized refactor tools)
/// is a one-line change here.
fn edited_paths_for_tool(tool_name: &str, input: &serde_json::Value) -> Vec<PathBuf> {
    match tool_name {
        "edit_file" | "write_file" => {
            if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                vec![PathBuf::from(path)]
            } else {
                Vec::new()
            }
        }
        "apply_patch" => {
            // `apply_patch` accepts either a `path` override or a list of
            // `files` (each `{path, content}`). We try both shapes.
            let mut out = Vec::new();
            if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                out.push(PathBuf::from(path));
            }
            if let Some(files) = input.get("files").and_then(|v| v.as_array()) {
                for entry in files {
                    if let Some(path) = entry.get("path").and_then(|v| v.as_str()) {
                        out.push(PathBuf::from(path));
                    }
                }
            }
            // Fallback: parse `---`/`+++` headers from a unified diff payload.
            if out.is_empty()
                && let Some(patch) = input.get("patch").and_then(|v| v.as_str())
            {
                out.extend(parse_patch_paths(patch));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// Lightweight parser for `+++ b/<path>` lines in a unified diff. Used as a
/// fallback when `apply_patch` is invoked with raw `patch` text and no
/// `path`/`files` override. We deliberately keep this dumb — the real
/// `apply_patch` tool already validates the patch shape; we only need a
/// best-effort hint for the LSP hook.
fn parse_patch_paths(patch: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            let trimmed = rest.trim();
            // Strip leading `b/` per git diff conventions.
            let path = trimmed.strip_prefix("b/").unwrap_or(trimmed);
            // Skip `/dev/null` (deletion).
            if path == "/dev/null" {
                continue;
            }
            out.push(PathBuf::from(path));
        }
    }
    out
}

fn caller_allowed_for_tool(caller: Option<&ToolCaller>, tool_def: Option<&Tool>) -> bool {
    let requested = caller_type_for_tool_use(caller);
    if let Some(def) = tool_def
        && let Some(allowed) = &def.allowed_callers
    {
        if allowed.is_empty() {
            return requested == "direct";
        }
        return allowed.iter().any(|item| item == requested);
    }
    requested == "direct"
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
        ToolError::NotAvailable { message } => {
            let lower = message.to_ascii_lowercase();
            if lower.contains("current tool catalog") || lower.contains("did you mean:") {
                message.clone()
            } else {
                format!(
                    "Tool '{tool_name}' is not available: {message}. Check mode, feature flags, or tool name."
                )
            }
        }
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

fn summarize_text_head_tail(text: &str, limit: usize) -> String {
    let total = text.chars().count();
    if total <= limit {
        return text.to_string();
    }
    if limit <= 20 {
        return summarize_text(text, limit);
    }

    let marker = "\n\n[... output truncated for context ...]\n\n";
    let marker_len = marker.chars().count();
    if limit <= marker_len + 20 {
        return summarize_text(text, limit);
    }

    let remaining = limit - marker_len;
    let head_len = remaining.saturating_mul(2) / 3;
    let tail_len = remaining.saturating_sub(head_len);
    let head: String = text.chars().take(head_len).collect();
    let tail_vec: Vec<char> = text.chars().rev().take(tail_len).collect();
    let tail: String = tail_vec.into_iter().rev().collect();
    format!("{head}{marker}{tail}")
}

fn tool_result_is_noisy(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "exec_shell"
            | "exec_shell_wait"
            | "exec_shell_interact"
            | "multi_tool_use.parallel"
            | "web_search"
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

#[derive(Debug, Clone, Copy)]
struct ToolResultContextLimits {
    hard_limit_chars: usize,
    noisy_soft_limit_chars: usize,
    snippet_chars: usize,
}

fn tool_result_context_limits_for_model(model: &str) -> ToolResultContextLimits {
    let is_large_context =
        context_window_for_model(model).is_some_and(|window| window >= LARGE_CONTEXT_WINDOW_TOKENS);

    if is_large_context {
        ToolResultContextLimits {
            hard_limit_chars: LARGE_CONTEXT_TOOL_RESULT_HARD_LIMIT_CHARS,
            noisy_soft_limit_chars: LARGE_CONTEXT_TOOL_RESULT_SOFT_LIMIT_CHARS,
            snippet_chars: LARGE_CONTEXT_TOOL_RESULT_SNIPPET_CHARS,
        }
    } else {
        ToolResultContextLimits {
            hard_limit_chars: TOOL_RESULT_CONTEXT_HARD_LIMIT_CHARS,
            noisy_soft_limit_chars: TOOL_RESULT_CONTEXT_SOFT_LIMIT_CHARS,
            snippet_chars: TOOL_RESULT_CONTEXT_SNIPPET_CHARS,
        }
    }
}

pub(crate) fn compact_tool_result_for_context(
    model: &str,
    tool_name: &str,
    output: &ToolResult,
) -> String {
    let raw = output.content.trim();
    if raw.is_empty() {
        return String::new();
    }

    let limits = tool_result_context_limits_for_model(model);
    let raw_chars = raw.chars().count();
    let should_compact = raw_chars > limits.hard_limit_chars
        || (tool_result_is_noisy(tool_name) && raw_chars > limits.noisy_soft_limit_chars);
    if !should_compact {
        return raw.to_string();
    }

    let snippet = summarize_text_head_tail(raw, limits.snippet_chars);
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

fn remove_working_set_summary(prompt: Option<&SystemPrompt>) -> Option<SystemPrompt> {
    match prompt {
        Some(SystemPrompt::Blocks(blocks)) => {
            let filtered: Vec<SystemBlock> = blocks
                .iter()
                .filter(|block| !block.text.contains(WORKING_SET_SUMMARY_MARKER))
                .cloned()
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some(SystemPrompt::Blocks(filtered))
            }
        }
        Some(SystemPrompt::Text(text)) => Some(SystemPrompt::Text(text.clone())),
        None => None,
    }
}

fn append_working_set_summary(
    prompt: Option<SystemPrompt>,
    working_set_summary: Option<&str>,
) -> Option<SystemPrompt> {
    let Some(summary) = working_set_summary.map(str::trim).filter(|s| !s.is_empty()) else {
        return prompt;
    };
    let working_set_block = SystemBlock {
        block_type: "text".to_string(),
        text: summary.to_string(),
        cache_control: None,
    };

    match prompt {
        Some(SystemPrompt::Text(text)) => Some(SystemPrompt::Blocks(vec![
            SystemBlock {
                block_type: "text".to_string(),
                text,
                cache_control: None,
            },
            working_set_block,
        ])),
        Some(SystemPrompt::Blocks(mut blocks)) => {
            blocks.retain(|block| !block.text.contains(WORKING_SET_SUMMARY_MARKER));
            blocks.push(working_set_block);
            Some(SystemPrompt::Blocks(blocks))
        }
        None => Some(SystemPrompt::Blocks(vec![working_set_block])),
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

fn turn_response_headroom_tokens() -> u64 {
    u64::from(TURN_MAX_OUTPUT_TOKENS).saturating_add(CONTEXT_HEADROOM_TOKENS as u64)
}

fn is_context_length_error_message(message: &str) -> bool {
    crate::error_taxonomy::classify_error_message(message) == ErrorCategory::InvalidInput
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
        let system_prompt =
            prompts::system_prompt_for_mode_with_context(AppMode::Agent, &config.workspace, None);
        session.system_prompt =
            append_working_set_summary(Some(system_prompt), working_set_summary.as_deref());

        let subagent_manager =
            new_shared_subagent_manager(config.workspace.clone(), config.max_subagents);
        let shell_manager = config
            .runtime_services
            .shell_manager
            .clone()
            .unwrap_or_else(|| new_shared_shell_manager(config.workspace.clone()));
        let capacity_controller = CapacityController::new(config.capacity.clone());

        // Create Flash seam manager for layered context (#159). v0.7.5 keeps
        // this opt-in until the prefix-cache audit proves when seam production
        // is worth the extra request and transcript mutation.
        let seam_manager = deepseek_client.as_ref().map(|main_client| {
            let seam_config = SeamConfig {
                enabled: api_config.context.enabled.unwrap_or(false),
                verbatim_window_turns: api_config
                    .context
                    .verbatim_window_turns
                    .unwrap_or(crate::seam_manager::VERBATIM_WINDOW_TURNS),
                l1_threshold: api_config
                    .context
                    .l1_threshold
                    .unwrap_or(crate::seam_manager::DEFAULT_L1_THRESHOLD),
                l2_threshold: api_config
                    .context
                    .l2_threshold
                    .unwrap_or(crate::seam_manager::DEFAULT_L2_THRESHOLD),
                l3_threshold: api_config
                    .context
                    .l3_threshold
                    .unwrap_or(crate::seam_manager::DEFAULT_L3_THRESHOLD),
                cycle_threshold: api_config
                    .context
                    .cycle_threshold
                    .unwrap_or(crate::seam_manager::DEFAULT_CYCLE_THRESHOLD),
                seam_model: api_config
                    .context
                    .seam_model
                    .clone()
                    .unwrap_or_else(|| crate::seam_manager::DEFAULT_SEAM_MODEL.to_string()),
            };
            SeamManager::new(main_client.clone(), seam_config)
        });

        let lsp_manager = Arc::new(match config.lsp_config.clone() {
            Some(cfg) => crate::lsp::LspManager::new(cfg, config.workspace.clone()),
            None => crate::lsp::LspManager::disabled(),
        });

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
            seam_manager,
            coherence_state: CoherenceState::default(),
            turn_counter: 0,
            lsp_manager,
            pending_lsp_blocks: Vec::new(),
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
                    reasoning_effort,
                    allow_shell,
                    trust_mode,
                    auto_approve,
                } => {
                    self.handle_send_message(
                        content,
                        mode,
                        model,
                        reasoning_effort,
                        allow_shell,
                        trust_mode,
                        auto_approve,
                    )
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
                        let _ = self
                            .tx_event
                            .send(Event::error(ErrorEnvelope::fatal(message)))
                            .await;
                        continue;
                    };

                    let runtime = SubAgentRuntime::new(
                        client,
                        self.session.model.clone(),
                        // Sub-agents don't inherit YOLO mode - use Agent mode defaults
                        self.build_tool_context(AppMode::Agent, self.session.auto_approve),
                        self.session.allow_shell,
                        Some(self.tx_event.clone()),
                        Arc::clone(&self.subagent_manager),
                    )
                    .with_role_models(self.config.subagent_model_overrides.clone())
                    .with_max_spawn_depth(self.config.max_spawn_depth)
                    .background_runtime();

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
                                .send(Event::error(ErrorEnvelope::fatal(format!(
                                    "Failed to spawn sub-agent: {err}"
                                ))))
                                .await;
                        }
                    }
                }
                Op::ListSubAgents => {
                    let agents = {
                        let mut manager = self.subagent_manager.lock().await;
                        manager.cleanup(Duration::from_secs(60 * 60));
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
                    self.emit_session_updated().await;
                    let _ = self
                        .tx_event
                        .send(Event::status("Session context synced".to_string()))
                        .await;
                }
                Op::CompactContext => {
                    self.handle_manual_compaction().await;
                }
                Op::Rlm {
                    content,
                    model,
                    child_model,
                    max_depth,
                } => {
                    self.handle_rlm(content, model, child_model, max_depth)
                        .await;
                }
                Op::Shutdown => {
                    break;
                }
            }
        }
    }

    async fn emit_session_updated(&self) {
        let _ = self
            .tx_event
            .send(Event::SessionUpdated {
                messages: self.session.messages.clone(),
                system_prompt: self.session.system_prompt.clone(),
                model: self.session.model.clone(),
                workspace: self.session.workspace.clone(),
            })
            .await;
    }

    async fn add_session_message(&mut self, message: Message) {
        self.session.add_message(message);
        self.emit_session_updated().await;
    }

    /// #136: post-edit hook. Inspects the tool name + input, derives the
    /// edited file path, and asks the LSP manager for diagnostics. The
    /// rendered block is queued in `pending_lsp_blocks` and flushed to the
    /// session message stream just before the next API request. Failure is
    /// silent by design — a missing/crashing LSP server must never block
    /// the agent.
    async fn run_post_edit_lsp_hook(&mut self, tool_name: &str, tool_input: &serde_json::Value) {
        if !self.lsp_manager.config().enabled {
            return;
        }
        let paths = edited_paths_for_tool(tool_name, tool_input);
        for path in paths {
            let absolute = if path.is_absolute() {
                path.clone()
            } else {
                self.session.workspace.join(&path)
            };
            // Use a short edit-sequence based on the existing turn counter so
            // log output stays correlated even though we do not currently
            // batch by sequence.
            let seq = self.turn_counter;
            if let Some(block) = self.lsp_manager.diagnostics_for(&absolute, seq).await {
                self.pending_lsp_blocks.push(block);
            }
        }
    }

    /// Drain `pending_lsp_blocks` into a single synthetic user message so the
    /// model sees the diagnostics on its next request. Skips when nothing is
    /// pending. The message uses the standard `text` content block shape
    /// (the same shape as the post-tool steer messages) so we don't need to
    /// invent a new envelope.
    async fn flush_pending_lsp_diagnostics(&mut self) {
        if self.pending_lsp_blocks.is_empty() {
            return;
        }
        let blocks = std::mem::take(&mut self.pending_lsp_blocks);
        let rendered = crate::lsp::render_blocks(&blocks);
        if rendered.is_empty() {
            return;
        }
        self.add_session_message(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: rendered,
                cache_control: None,
            }],
        })
        .await;
    }

    /// Handle a send message operation
    #[allow(clippy::too_many_arguments)]
    async fn handle_send_message(
        &mut self,
        content: String,
        mode: AppMode,
        model: String,
        reasoning_effort: Option<String>,
        allow_shell: bool,
        trust_mode: bool,
        auto_approve: bool,
    ) {
        // Reset cancel token for fresh turn (in case previous was cancelled)
        self.reset_cancel_token();

        // Drain stale steer messages from previous turns.
        while self.rx_steer.try_recv().is_ok() {}

        // Create turn context first so start event includes a stable turn id.
        let mut turn = TurnContext::new(self.config.max_steps);
        self.turn_counter = self.turn_counter.saturating_add(1);
        self.capacity_controller.mark_turn_start(self.turn_counter);

        // Snapshot the workspace BEFORE we touch a single tool. Run the git
        // work on the blocking pool so the async runtime stays responsive;
        // failure is non-fatal (the helper logs at WARN).
        if self.config.snapshots_enabled {
            let pre_workspace = self.session.workspace.clone();
            let pre_seq = self.turn_counter;
            let _ = tokio::task::spawn_blocking(move || pre_turn_snapshot(&pre_workspace, pre_seq))
                .await;
        }

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
                .send(Event::error(ErrorEnvelope::fatal_auth(message.clone())))
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
        let force_update_plan_first = should_force_update_plan_first(mode, &content);

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
        self.session.reasoning_effort = reasoning_effort;
        self.session.allow_shell = allow_shell;
        self.config.allow_shell = allow_shell;
        self.session.trust_mode = trust_mode;
        self.config.trust_mode = trust_mode;
        self.session.auto_approve = auto_approve;

        // Update system prompt to match current mode and include persisted compaction context.
        self.refresh_system_prompt(mode);
        self.emit_session_updated().await;

        // Build tool registry and tool list for the current mode
        let todo_list = self.config.todos.clone();
        let plan_state = self.config.plan_state.clone();

        let tool_context = self.build_tool_context(mode, auto_approve);
        let mut builder = if mode == AppMode::Plan {
            ToolRegistryBuilder::new()
                .with_read_only_file_tools()
                .with_search_tools()
                .with_git_tools()
                .with_git_history_tools()
                .with_diagnostics_tool()
                .with_validation_tools()
                .with_runtime_task_tools()
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
            .with_rlm_tool(self.deepseek_client.clone(), self.session.model.clone())
            .with_user_input_tool()
            .with_parallel_tool();

        if self.config.features.enabled(Feature::ApplyPatch) && mode != AppMode::Plan {
            builder = builder.with_patch_tools();
        }
        if self.config.features.enabled(Feature::WebSearch) {
            builder = builder.with_web_tools();
        }
        // Plan mode now keeps shell available — the existing approval flow
        // and command-safety classifier gate destructive commands. Writes
        // and patches stay blocked above; that's the only "destructive"
        // boundary plan mode enforces by tool registration.
        if self.config.features.enabled(Feature::ShellTool) && self.session.allow_shell {
            builder = builder.with_shell_tools();
        }

        // Mailbox for structured sub-agent envelopes (#128/#130). One per
        // turn: the receiver is drained by a short-lived task that converts
        // envelopes into `Event::SubAgentMailbox` so the UI can route them
        // to the matching in-transcript card. The drainer exits naturally
        // when every cloned sender is dropped at turn-end.
        let mailbox_for_runtime = if self.config.features.enabled(Feature::Subagents) {
            let cancel_token = self.cancel_token.child_token();
            let (mailbox, mut receiver) = Mailbox::new(cancel_token.clone());
            let tx_event_clone = self.tx_event.clone();
            tokio::spawn(async move {
                while let Some(envelope) = receiver.recv().await {
                    if tx_event_clone
                        .send(Event::SubAgentMailbox {
                            seq: envelope.seq,
                            message: envelope.message,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
            Some((mailbox, cancel_token))
        } else {
            None
        };

        let tool_registry = match mode {
            AppMode::Agent | AppMode::Yolo => {
                if self.config.features.enabled(Feature::Subagents) {
                    let runtime = if let Some(client) = self.deepseek_client.clone() {
                        let mut rt = SubAgentRuntime::new(
                            client,
                            self.session.model.clone(),
                            tool_context.clone(),
                            self.session.allow_shell,
                            Some(self.tx_event.clone()),
                            Arc::clone(&self.subagent_manager),
                        )
                        .with_role_models(self.config.subagent_model_overrides.clone())
                        .with_max_spawn_depth(self.config.max_spawn_depth);
                        if let Some((mailbox, cancel_token)) = mailbox_for_runtime.as_ref() {
                            rt = rt
                                .with_mailbox(mailbox.clone())
                                .with_cancel_token(cancel_token.clone());
                        }
                        Some(rt)
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
            for tool in &mut tools {
                tool.defer_loading = Some(should_default_defer_tool(&tool.name, mode));
            }
            let mut mcp_tools = mcp_tools;
            for tool in &mut mcp_tools {
                if mode == AppMode::Yolo {
                    tool.defer_loading = Some(false);
                    continue;
                }

                let keep_loaded = matches!(
                    tool.name.as_str(),
                    "list_mcp_resources"
                        | "list_mcp_resource_templates"
                        | "mcp_read_resource"
                        | "read_mcp_resource"
                        | "mcp_get_prompt"
                );
                tool.defer_loading = Some(!keep_loaded);
            }
            tools.extend(mcp_tools);
            tools
        });

        // Main turn loop
        let (status, error) = self
            .handle_deepseek_turn(
                &mut turn,
                tool_registry.as_ref(),
                tools,
                mode,
                force_update_plan_first,
            )
            .await;

        // Checkpoint-restart cycle boundary (issue #124). Run BEFORE
        // TurnComplete so the engine loop doesn't block the terminal after
        // the turn signal (#234). The status chip ("↻ context refreshing...")
        // is visible during the wait, and once TurnComplete fires the
        // terminal is immediately responsive. No-op unless the estimated
        // input tokens have crossed the per-cycle threshold.
        if matches!(status, TurnOutcomeStatus::Completed) {
            self.maybe_advance_cycle(mode).await;
        }

        // Update session usage
        self.session.total_usage.add(&turn.usage);

        // Emit turn complete event — after all post-turn bookkeeping so
        // the terminal is immediately responsive when the UI receives it.
        let _ = self
            .tx_event
            .send(Event::TurnComplete {
                usage: turn.usage,
                status,
                error,
            })
            .await;

        // Post-turn snapshot. Fire-and-forget: TurnComplete is already
        // emitted, so the UI is unblocked and the user can type / select /
        // paste immediately (#234). The git work proceeds on the blocking
        // pool without forcing the engine loop to await it.
        if self.config.snapshots_enabled {
            let post_workspace = self.session.workspace.clone();
            let post_seq = self.turn_counter;
            tokio::task::spawn_blocking(move || {
                post_turn_snapshot(&post_workspace, post_seq);
            });
        }
    }

    async fn handle_manual_compaction(&mut self) {
        let id = format!("compact_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let zero_usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
            ..Usage::default()
        };
        let Some(client) = self.deepseek_client.clone() else {
            let message = "Manual compaction unavailable: API client not configured".to_string();
            self.emit_compaction_failed(id, false, message.clone())
                .await;
            let _ = self
                .tx_event
                .send(Event::error(ErrorEnvelope::fatal_auth(message.clone())))
                .await;
            let _ = self
                .tx_event
                .send(Event::TurnComplete {
                    usage: zero_usage,
                    status: TurnOutcomeStatus::Failed,
                    error: Some(message),
                })
                .await;
            return;
        };

        let start_message = "Manual context compaction started".to_string();
        self.emit_compaction_started(id.clone(), false, start_message)
            .await;

        let compaction_pins = self
            .session
            .working_set
            .pinned_message_indices(&self.session.messages, &self.session.workspace);
        let compaction_paths = self.session.working_set.top_paths(24);
        let messages_before = self.session.messages.len();
        let mut turn_status = TurnOutcomeStatus::Completed;
        let mut turn_error = None;

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
                    let messages_after = result.messages.len();
                    self.session.messages = result.messages;
                    self.merge_compaction_summary(result.summary_prompt);
                    self.emit_session_updated().await;
                    let removed = messages_before.saturating_sub(messages_after);
                    let message = if result.retries_used > 0 {
                        format!(
                            "Compaction complete: {messages_before} → {messages_after} messages ({removed} removed, {} retries)",
                            result.retries_used
                        )
                    } else {
                        format!(
                            "Compaction complete: {messages_before} → {messages_after} messages ({removed} removed)"
                        )
                    };
                    self.emit_compaction_completed(
                        id,
                        false,
                        message,
                        Some(messages_before),
                        Some(messages_after),
                    )
                    .await;
                } else {
                    let message = "Compaction skipped: produced empty result".to_string();
                    self.emit_compaction_failed(id, false, message.clone())
                        .await;
                    turn_status = TurnOutcomeStatus::Failed;
                    turn_error = Some(message);
                }
            }
            Err(err) => {
                let message = format!("Manual context compaction failed: {err}");
                self.emit_compaction_failed(id, false, message.clone())
                    .await;
                let _ = self.tx_event.send(Event::status(message.clone())).await;
                turn_status = TurnOutcomeStatus::Failed;
                turn_error = Some(message);
            }
        }

        let _ = self
            .tx_event
            .send(Event::TurnComplete {
                usage: zero_usage,
                status: turn_status,
                error: turn_error,
            })
            .await;
    }

    /// Handle a Recursive Language Model (RLM) query — Algorithm 1 from
    /// Zhang et al. (arXiv:2512.24601).
    ///
    /// The prompt is stored as PROMPT in a REPL variable. The root LLM
    /// only sees metadata about the REPL state, never the prompt text
    /// directly. The model generates Python code, which is executed by
    /// the REPL. When FINAL() is called, the loop ends.
    async fn handle_rlm(
        &mut self,
        content: String,
        model: String,
        child_model: String,
        max_depth: u32,
    ) {
        use crate::rlm::turn::run_rlm_turn;

        let Some(ref client) = self.deepseek_client else {
            let err = self
                .deepseek_client_error
                .as_deref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "API client not configured".to_string());
            let _ = self
                .tx_event
                .send(Event::error(ErrorEnvelope::fatal_auth(format!(
                    "RLM error: {err}"
                ))))
                .await;
            return;
        };

        let _ = self
            .tx_event
            .send(Event::status("RLM turn started".to_string()))
            .await;

        let result = run_rlm_turn(
            client,
            model,
            content,
            child_model,
            self.tx_event.clone(),
            max_depth,
        )
        .await;

        let has_error = result.error.is_some();
        if let Some(ref err) = result.error {
            let _ = self
                .tx_event
                .send(Event::error(ErrorEnvelope::tool(format!(
                    "RLM error: {err}"
                ))))
                .await;
        }

        if !result.answer.is_empty() {
            // Add the final answer as an assistant message in the session.
            self.add_session_message(crate::models::Message {
                role: "assistant".to_string(),
                content: vec![crate::models::ContentBlock::Text {
                    text: result.answer.clone(),
                    cache_control: None,
                }],
            })
            .await;

            let _ = self
                .tx_event
                .send(Event::MessageDelta {
                    index: 0,
                    content: result.answer.clone(),
                })
                .await;
            let _ = self
                .tx_event
                .send(Event::MessageComplete { index: 0 })
                .await;
        }

        let _ = self
            .tx_event
            .send(Event::TurnComplete {
                usage: result.usage,
                status: if has_error {
                    crate::core::events::TurnOutcomeStatus::Failed
                } else {
                    crate::core::events::TurnOutcomeStatus::Completed
                },
                error: result.error,
            })
            .await;
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
        self.emit_compaction_started(id.clone(), true, start_message)
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
        self.emit_session_updated().await;
        let after_tokens = self.estimated_input_tokens();
        let after_count = self.session.messages.len();
        let recovered = after_tokens <= target_budget
            && (after_tokens < before_tokens || after_count < before_count || trimmed > 0);

        if recovered {
            let removed = before_count.saturating_sub(after_count);
            let mut details = format!(
                "Emergency compaction complete: {before_count} → {after_count} messages ({removed} removed), ~{before_tokens} → ~{after_tokens} tokens"
            );
            if retries_used > 0 {
                details.push_str(&format!(" ({} retries)", retries_used));
            }
            if trimmed > 0 {
                details.push_str(&format!(", trimmed {trimmed} oldest"));
            }
            self.emit_compaction_completed(
                id,
                true,
                details.clone(),
                Some(before_count),
                Some(after_count),
            )
            .await;
            let _ = self.tx_event.send(Event::status(details)).await;
            return true;
        }

        let message = format!(
            "Emergency context compaction failed to reduce request below model limit \
             (estimate ~{} tokens, budget ~{}).",
            after_tokens, target_budget
        );
        self.emit_compaction_failed(id, true, message.clone()).await;
        let _ = self.tx_event.send(Event::status(message)).await;
        false
    }

    fn build_tool_context(&self, mode: AppMode, auto_approve: bool) -> ToolContext {
        // Load the per-workspace trusted-paths list (#29) on every tool-context
        // build. Cheap (a small JSON file) and always reflects the latest
        // `/trust add` / `/trust remove` mutations without an explicit cache
        // refresh hook.
        let trusted = crate::workspace_trust::WorkspaceTrust::load_for(&self.session.workspace);
        let mut ctx = ToolContext::with_auto_approve(
            self.session.workspace.clone(),
            self.session.trust_mode,
            self.session.notes_path.clone(),
            self.session.mcp_config_path.clone(),
            mode == AppMode::Yolo || auto_approve,
        )
        .with_state_namespace(self.session.id.clone())
        .with_features(self.config.features.clone())
        .with_shell_manager(self.shell_manager.clone())
        .with_runtime_services(self.config.runtime_services.clone())
        .with_cancel_token(self.cancel_token.clone())
        .with_trusted_external_paths(trusted.paths().to_vec());

        if let Some(decider) = self.config.network_policy.as_ref() {
            ctx = ctx.with_network_policy(decider.clone());
        }

        if mode == AppMode::Yolo {
            ctx.with_elevated_sandbox_policy(crate::sandbox::SandboxPolicy::WorkspaceWrite {
                writable_roots: vec![self.session.workspace.clone()],
                network_access: true,
                exclude_tmpdir: false,
                exclude_slash_tmp: false,
            })
        } else {
            ctx
        }
    }

    async fn ensure_mcp_pool(&mut self) -> Result<Arc<AsyncMutex<McpPool>>, ToolError> {
        if let Some(pool) = self.mcp_pool.as_ref() {
            return Ok(Arc::clone(pool));
        }
        let mut pool = McpPool::from_config_path(&self.session.mcp_config_path)
            .map_err(|e| ToolError::execution_failed(format!("Failed to load MCP config: {e}")))?;
        if let Some(decider) = self.config.network_policy.as_ref() {
            pool = pool.with_network_policy(decider.clone());
        }
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

    /// Handle a turn using the DeepSeek API.
    #[allow(clippy::too_many_lines)]
    /// Run the pre-request layered-context checkpoint (#159). Checks whether
    /// the active input estimate has crossed a soft-seam threshold and, if so,
    /// produces an `<archived_context>` block via Flash and appends it as an
    /// assistant message. Called from `handle_deepseek_turn` before each API
    /// request so the model always has the latest navigation aids.
    async fn layered_context_checkpoint(&mut self) {
        let Some(ref seam_mgr) = self.seam_manager else {
            return;
        };
        if !seam_mgr.config().enabled {
            return;
        }

        let highest = seam_mgr.highest_level().await;
        let Some(level) = seam_mgr.seam_level_for(self.estimated_input_tokens(), highest) else {
            return;
        };

        // Determine the message range to summarize: everything before the
        // verbatim window. The verbatim window (last ~16 turns) stays
        // untouched so the model always has ground-truth recent context.
        let msg_count = self.session.messages.len();
        let verbatim_start = seam_mgr.verbatim_window_start(msg_count);
        if verbatim_start == 0 {
            return; // Not enough messages to summarize.
        }

        let msg_range_end = verbatim_start;
        let pinned = self
            .session
            .working_set
            .pinned_message_indices(&self.session.messages, &self.session.workspace);

        let _ = self
            .tx_event
            .send(Event::status(format!(
                "⏻ producing L{level} context seam ({msg_range_end} messages)…"
            )))
            .await;

        // If we have existing seams, recompact; otherwise produce fresh.
        let existing_seams = seam_mgr.collect_seam_texts(&self.session.messages).await;
        let seam_text = if existing_seams.is_empty() {
            match seam_mgr
                .produce_soft_seam(
                    &self.session.messages,
                    level,
                    0,
                    msg_range_end,
                    Some(&self.session.workspace),
                    &pinned,
                )
                .await
            {
                Ok(text) => text,
                Err(err) => {
                    crate::logging::warn(format!("L{level} soft seam failed: {err}"));
                    return;
                }
            }
        } else {
            let recent: Vec<&Message> = (0..msg_range_end)
                .filter_map(|i| self.session.messages.get(i))
                .collect();
            match seam_mgr
                .recompact(&existing_seams, &recent, level, 0, msg_range_end)
                .await
            {
                Ok(text) => text,
                Err(err) => {
                    crate::logging::warn(format!("L{level} recompact failed: {err}"));
                    return;
                }
            }
        };

        if seam_text.is_empty() {
            return;
        }

        // Capture seam count before the mutable borrow below.
        let seam_count = seam_mgr.seam_count().await;

        // Append the seam as an assistant message. This is an append-only
        // operation — no messages are deleted. The prefix cache stays hot.
        self.add_session_message(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: seam_text,
                cache_control: None,
            }],
        })
        .await;

        let _ = self
            .tx_event
            .send(Event::status(format!(
                "⏻ L{level} seam complete ({seam_count} total, {msg_range_end} messages covered)"
            )))
            .await;
    }
    /// its token threshold (issue #124). No-op in the common case.
    ///
    /// Caller must invoke this only at a clean turn boundary (no in-flight
    /// tool, no open stream, no pending approval modal). The phase guard
    /// inside `should_advance_cycle` is a defence-in-depth check; the
    /// engine's wider state machine is the primary enforcement layer.
    ///
    /// Sub-agents are intentionally NOT awaited: each sub-agent has its own
    /// context, the parent's reset doesn't invalidate them. Their handles
    /// are captured in the structured-state block so the next cycle can see
    /// they're still running.
    async fn maybe_advance_cycle(&mut self, mode: AppMode) {
        if !should_advance_cycle(
            self.estimated_input_tokens() as u64,
            turn_response_headroom_tokens(),
            &self.session.model,
            &self.config.cycle,
            false,
        ) {
            return;
        }

        let Some(client) = self.deepseek_client.clone() else {
            crate::logging::warn(
                "Cycle boundary skipped: API client not configured for briefing turn",
            );
            return;
        };

        let from = self.session.cycle_count;
        let to = from.saturating_add(1);
        let archive_started = self.session.current_cycle_started;
        let max_briefing_tokens = self.config.cycle.briefing_max_for(&self.session.model);

        let _ = self
            .tx_event
            .send(Event::status(format!(
                "↻ context refreshing (cycle {from} → {to}, generating briefing…)"
            )))
            .await;

        // 1. Generate the model-curated briefing. Prefer the Flash seam
        //    manager (#159) for cost and speed; fall back to the main model
        //    (legacy produce_briefing) when the seam manager isn't available.
        let briefing_text = if let Some(ref seam_mgr) = self.seam_manager {
            let seams = seam_mgr.collect_seam_texts(&self.session.messages).await;
            let state_text = {
                let s = StructuredState::capture(
                    mode.label(),
                    self.config.workspace.clone(),
                    std::env::current_dir().ok(),
                    &self.session.working_set,
                    &self.config.todos,
                    &self.config.plan_state,
                    Some(&self.subagent_manager),
                )
                .await;
                s.to_system_block()
            };
            match seam_mgr
                .produce_flash_briefing(&seams, state_text.as_deref())
                .await
            {
                Ok(text) => text,
                Err(err) => {
                    crate::logging::warn(format!(
                        "Flash briefing failed, falling back to main model: {err}"
                    ));
                    match produce_briefing(
                        &client,
                        &self.session.model,
                        &self.session.messages,
                        max_briefing_tokens,
                    )
                    .await
                    {
                        Ok(text) => text,
                        Err(err2) => {
                            crate::logging::warn(format!(
                                "Cycle briefing turn failed; skipping cycle advance: {err2}"
                            ));
                            let _ = self
                                .tx_event
                                .send(Event::status(format!(
                                    "↻ cycle handoff failed (continuing in cycle {from}): {err2}"
                                )))
                                .await;
                            return;
                        }
                    }
                }
            }
        } else {
            match produce_briefing(
                &client,
                &self.session.model,
                &self.session.messages,
                max_briefing_tokens,
            )
            .await
            {
                Ok(text) => text,
                Err(err) => {
                    crate::logging::warn(format!(
                        "Cycle briefing turn failed; skipping cycle advance: {err}"
                    ));
                    let _ = self
                        .tx_event
                        .send(Event::status(format!(
                            "↻ cycle handoff failed (continuing in cycle {from}): {err}"
                        )))
                        .await;
                    return;
                }
            }
        };

        let briefing_tokens = estimate_briefing_tokens(&briefing_text);
        let now = chrono::Utc::now();
        let briefing = CycleBriefing {
            cycle: to,
            timestamp: now,
            briefing_text: briefing_text.clone(),
            token_estimate: briefing_tokens,
        };

        // 2. Archive the cycle to disk. If the archive write fails we still
        //    proceed with the swap — the briefing alone preserves enough
        //    state to continue, and the user can recover the lost archive
        //    from their session log if needed.
        match archive_cycle(
            &self.session.id,
            to,
            &self.session.messages,
            &self.session.model,
            archive_started,
        ) {
            Ok(path) => {
                crate::logging::info(format!("Cycle {to} archived to {}", path.display()));
            }
            Err(err) => {
                crate::logging::warn(format!(
                    "Failed to archive cycle {to}; continuing with swap: {err}"
                ));
            }
        }

        // 3. Capture structured state. Locks are held only for the snapshot.
        let state = StructuredState::capture(
            mode.label(),
            self.config.workspace.clone(),
            std::env::current_dir().ok(),
            &self.session.working_set,
            &self.config.todos,
            &self.config.plan_state,
            Some(&self.subagent_manager),
        )
        .await;
        let state_block = state.to_system_block();

        // 4. Build the seed messages. The next cycle starts with the
        //    base system prompt (refreshed below) and these seeds.
        let seed_messages = build_seed_messages(
            state_block.as_deref(),
            Some(&briefing),
            None, // pending_user_message — pulled from steer/queue elsewhere
        );

        // 5. Atomic swap.
        self.session.messages = seed_messages;
        self.session.cycle_count = to;
        self.session.current_cycle_started = now;
        self.session.cycle_briefings.push(briefing.clone());
        // Reset seam tracking for the new cycle.
        if let Some(ref seam_mgr) = self.seam_manager {
            seam_mgr.reset().await;
        }
        // Drop any compaction summary — that path is incompatible with the
        // fresh-context model and would Frankenstein-merge with the briefing.
        self.session.compaction_summary_prompt = None;
        self.refresh_system_prompt(mode);
        self.emit_session_updated().await;

        let _ = self
            .tx_event
            .send(Event::CycleAdvanced {
                from,
                to,
                briefing: briefing.clone(),
            })
            .await;
        let _ = self
            .tx_event
            .send(Event::status(format!(
                "↻ context refreshed (cycle {from} → {to}, briefing: {briefing_tokens} tokens carried)"
            )))
            .await;
    }

    /// Refresh the system prompt based on current mode and context.
    fn refresh_system_prompt(&mut self, mode: AppMode) {
        let working_set_summary = self
            .session
            .working_set
            .summary_block(&self.config.workspace);
        let base = prompts::system_prompt_for_mode_with_context(mode, &self.config.workspace, None);
        let stable_prompt =
            merge_system_prompts(Some(&base), self.session.compaction_summary_prompt.clone());
        self.session.system_prompt =
            append_working_set_summary(stable_prompt, working_set_summary.as_deref());
    }

    fn merge_compaction_summary(&mut self, summary_prompt: Option<SystemPrompt>) {
        if summary_prompt.is_none() {
            return;
        }
        self.session.compaction_summary_prompt = merge_system_prompts(
            self.session.compaction_summary_prompt.as_ref(),
            summary_prompt.clone(),
        );
        let current_without_working_set =
            remove_working_set_summary(self.session.system_prompt.as_ref());
        let merged = merge_system_prompts(current_without_working_set.as_ref(), summary_prompt);
        let working_set_summary = self
            .session
            .working_set
            .summary_block(&self.config.workspace);
        self.session.system_prompt =
            append_working_set_summary(merged, working_set_summary.as_deref());
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
    rx_approval: mpsc::Receiver<ApprovalDecision>,
    pub rx_steer: mpsc::Receiver<String>,
    pub tx_event: mpsc::Sender<Event>,
    pub cancel_token: CancellationToken,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MockApprovalEvent {
    Approved {
        id: String,
    },
    Denied {
        id: String,
    },
    RetryWithPolicy {
        id: String,
        policy: crate::sandbox::SandboxPolicy,
    },
}

#[cfg(test)]
impl MockEngineHandle {
    pub(crate) async fn recv_approval_event(&mut self) -> Option<MockApprovalEvent> {
        match self.rx_approval.recv().await? {
            ApprovalDecision::Approved { id } => Some(MockApprovalEvent::Approved { id }),
            ApprovalDecision::Denied { id } => Some(MockApprovalEvent::Denied { id }),
            ApprovalDecision::RetryWithPolicy { id, policy } => {
                Some(MockApprovalEvent::RetryWithPolicy { id, policy })
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn mock_engine_handle() -> MockEngineHandle {
    let (tx_op, rx_op) = mpsc::channel(32);
    let (tx_event, rx_event) = mpsc::channel(256);
    let (tx_approval, rx_approval) = mpsc::channel(64);
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
        rx_approval,
        rx_steer,
        tx_event,
        cancel_token,
    }
}

mod approval;
mod capacity_flow;
mod dispatch;
mod turn_loop;

use self::approval::{ApprovalDecision, ApprovalResult, UserInputDecision};
use self::dispatch::{
    ParallelToolResult, ParallelToolResultEntry, ToolExecGuard, ToolExecOutcome, ToolExecutionPlan,
    final_tool_input, mcp_tool_approval_description, mcp_tool_is_parallel_safe,
    mcp_tool_is_read_only, parse_parallel_tool_calls, parse_tool_input,
    should_force_update_plan_first, should_parallelize_tool_batch, should_stop_after_plan_tool,
};

#[cfg(test)]
mod tests;
