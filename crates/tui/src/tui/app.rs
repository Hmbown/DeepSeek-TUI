//! Application state for the `DeepSeek` TUI.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use serde_json::Value;
use thiserror::Error;

use crate::compaction::CompactionConfig;
use crate::config::{ApiProvider, Config, has_api_key, save_api_key};
use crate::core::coherence::CoherenceState;
use crate::cycle_manager::{CycleBriefing, CycleConfig};
use crate::hooks::{HookContext, HookEvent, HookExecutor, HookResult};
use crate::localization::{Locale, MessageId, resolve_locale, tr};
use crate::models::{
    Message, SystemPrompt, compaction_message_threshold_for_model,
    compaction_threshold_for_model_and_effort,
};
use crate::palette::{self, UiTheme};
use crate::session_manager::SessionContextReference;
use crate::settings::Settings;
use crate::tools::plan::{SharedPlanState, new_shared_plan_state};
use crate::tools::shell::new_shared_shell_manager;
use crate::tools::spec::RuntimeToolServices;
use crate::tools::subagent::SubAgentResult;
use crate::tools::swarm::SwarmOutcome;
use crate::tools::todo::{SharedTodoList, new_shared_todo_list};
use crate::tui::active_cell::ActiveCell;
use crate::tui::approval::ApprovalMode;
use crate::tui::clipboard::{ClipboardContent, ClipboardHandler};
use crate::tui::file_mention::ContextReference;
use crate::tui::history::{HistoryCell, TranscriptRenderOptions};
use crate::tui::paste_burst::{FlushResult, PasteBurst};
use crate::tui::scrolling::{MouseScrollState, TranscriptLineMeta, TranscriptScroll};
use crate::tui::selection::TranscriptSelection;
use crate::tui::streaming::StreamingState;
use crate::tui::transcript::TranscriptViewCache;
use crate::tui::views::ViewStack;

// === Types ===

/// State machine for onboarding new users.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingState {
    Welcome,
    ApiKey,
    TrustDirectory,
    Tips,
    None,
}

/// Supported application modes for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Agent,
    Yolo,
    Plan,
}

/// DeepSeek reasoning-effort tier, mirrored on ChatGPT/Claude effort pickers.
///
/// The config file accepts all five string values for forward-compat with
/// providers that expose the full spectrum; DeepSeek currently collapses
/// `Low`/`Medium` → `high` and `Max` → `max` at the API boundary. The
/// keyboard cycler (Shift+Tab) walks only the three behaviorally distinct
/// tiers: `Off` → `High` → `Max` → `Off`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    High,
    #[default]
    Max,
}

impl ReasoningEffort {
    /// Parse a config-file string into an effort tier. Unknown values fall
    /// back to the default (`Max`) rather than erroring out.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "disabled" | "none" | "false" => Self::Off,
            "low" | "minimal" => Self::Low,
            "medium" | "mid" => Self::Medium,
            "high" => Self::High,
            "max" | "maximum" | "xhigh" => Self::Max,
            _ => Self::default(),
        }
    }

    /// Canonical lowercase label used for config storage and UI hints.
    #[must_use]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
        }
    }

    /// Short label for the header chip.
    #[must_use]
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "med",
            Self::High => "high",
            Self::Max => "max",
        }
    }

    /// Value forwarded to the engine/client. `None` means "provider default"
    /// (for `Off` we still emit `"off"` so the client can inject
    /// `thinking = {"type": "disabled"}`).
    #[must_use]
    pub fn api_value(self) -> Option<&'static str> {
        Some(self.as_setting())
    }

    /// Cycle through the three behaviorally distinct tiers.
    #[must_use]
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Off => Self::High,
            Self::Low | Self::Medium | Self::High => Self::Max,
            Self::Max => Self::Off,
        }
    }
}

/// Sidebar content focus mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarFocus {
    Auto,
    Plan,
    Todos,
    Tasks,
    Agents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerDensity {
    Compact,
    Comfortable,
    Spacious,
}

impl ComposerDensity {
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" | "tight" => Self::Compact,
            "spacious" | "loose" => Self::Spacious,
            _ => Self::Comfortable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptSpacing {
    Compact,
    Comfortable,
    Spacious,
}

impl TranscriptSpacing {
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" | "tight" => Self::Compact,
            "spacious" | "loose" => Self::Spacious,
            _ => Self::Comfortable,
        }
    }
}

impl SidebarFocus {
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "plan" => Self::Plan,
            "todos" => Self::Todos,
            "tasks" => Self::Tasks,
            "agents" | "subagents" | "sub-agents" => Self::Agents,
            _ => Self::Auto,
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Plan => "plan",
            Self::Todos => "todos",
            Self::Tasks => "tasks",
            Self::Agents => "agents",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct StatusToast {
    pub text: String,
    pub level: StatusToastLevel,
    pub created_at: Instant,
    pub ttl_ms: Option<u64>,
}

impl StatusToast {
    #[must_use]
    pub fn new(text: impl Into<String>, level: StatusToastLevel, ttl_ms: Option<u64>) -> Self {
        Self {
            text: text.into(),
            level,
            created_at: Instant::now(),
            ttl_ms,
        }
    }

    #[must_use]
    pub fn is_expired(&self, now: Instant) -> bool {
        self.ttl_ms
            .is_some_and(|ttl| now.duration_since(self.created_at).as_millis() >= u128::from(ttl))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerHistorySearch {
    pre_search_input: String,
    pre_search_cursor: usize,
    query: String,
    selected: usize,
}

impl ComposerHistorySearch {
    fn new(pre_search_input: String, pre_search_cursor: usize) -> Self {
        Self {
            pre_search_input,
            pre_search_cursor,
            query: String::new(),
            selected: 0,
        }
    }
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn byte_index_at_char(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| text.len())
}

fn remove_char_at(text: &mut String, char_index: usize) -> bool {
    let start = byte_index_at_char(text, char_index);
    if start >= text.len() {
        return false;
    }
    let ch = text[start..].chars().next().unwrap();
    let end = start + ch.len_utf8();
    text.replace_range(start..end, "");
    true
}

fn normalize_paste_text(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "")
    } else {
        text.to_string()
    }
}

fn sanitize_api_key_text(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

const MAX_SUBMITTED_INPUT_CHARS: usize = 16_000;
const MAX_DRAFT_HISTORY: usize = 50;

impl AppMode {
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "plan" => Self::Plan,
            "yolo" => Self::Yolo,
            _ => Self::Agent,
        }
    }

    #[must_use]
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Yolo => "yolo",
            Self::Plan => "plan",
        }
    }

    /// Short label used in the UI footer.
    pub fn label(self) -> &'static str {
        match self {
            AppMode::Agent => "AGENT",
            AppMode::Yolo => "YOLO",
            AppMode::Plan => "PLAN",
        }
    }

    #[allow(dead_code)]
    /// Description shown in help or onboarding text.
    pub fn description(self) -> &'static str {
        match self {
            AppMode::Agent => "Agent mode - autonomous task execution with tools",
            AppMode::Yolo => "YOLO mode - full tool access without approvals",
            AppMode::Plan => "Plan mode - design before implementing",
        }
    }
}

/// Configuration required to bootstrap the TUI.
#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct TuiOptions {
    pub model: String,
    pub workspace: PathBuf,
    pub allow_shell: bool,
    /// Use the alternate screen buffer (fullscreen TUI).
    pub use_alt_screen: bool,
    /// Capture mouse input for internal scrolling/selection.
    pub use_mouse_capture: bool,
    /// Enable terminal bracketed-paste mode (OSC `?2004h` / `?2004l`). Defaults
    /// on; settable via `bracketed_paste = false` in `settings.toml` for the
    /// rare terminal that mishandles it.
    pub use_bracketed_paste: bool,
    /// Maximum number of concurrent sub-agents.
    pub max_subagents: usize,
    #[allow(dead_code)]
    pub skills_dir: PathBuf,
    #[allow(dead_code)]
    pub memory_path: PathBuf,
    #[allow(dead_code)]
    pub notes_path: PathBuf,
    #[allow(dead_code)]
    pub mcp_config_path: PathBuf,
    #[allow(dead_code)]
    pub use_memory: bool,
    /// Start in agent mode (defaults to agent; --yolo starts in YOLO)
    pub start_in_agent_mode: bool,
    /// Skip onboarding screens
    pub skip_onboarding: bool,
    /// Auto-approve tool executions (yolo mode)
    pub yolo: bool,
    /// Resume a previous session by ID
    pub resume_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct YoloRestoreState {
    allow_shell: bool,
    trust_mode: bool,
    approval_mode: ApprovalMode,
}

/// Global UI state for the TUI.
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    pub mode: AppMode,
    pub input: String,
    pub cursor_position: usize,
    /// Single-entry kill buffer for emacs-style `Ctrl+K` cut / `Ctrl+Y` yank.
    /// Populated by `kill_to_end_of_line`; restored by `yank`. Persists across
    /// composer clears (e.g. submit) so a yank can recover an accidental kill.
    pub kill_buffer: String,
    pub paste_burst: PasteBurst,
    pub history: Vec<HistoryCell>,
    pub history_version: u64,
    /// Per-cell revision counter, kept in lockstep with `history`. Bumped only
    /// for the cell whose content actually changed; appended (with a fresh
    /// value) when a new cell is pushed; truncated when cells are removed. The
    /// transcript cache compares each entry against its previously rendered
    /// revision to skip re-wrap on unchanged cells.
    ///
    /// Critical for transcript scroll perf (issue #78): without per-cell
    /// revisions, every history mutation forces a full re-render of every
    /// cell, which scales O(N) with transcript length and stalls the UI when
    /// scrolled far back.
    pub history_revisions: Vec<u64>,
    /// Monotonic counter used to issue fresh per-cell revisions. Wrapping is
    /// fine — the chance of a wrap-around revision collision in a single
    /// session is astronomical.
    pub next_history_revision: u64,
    pub api_messages: Vec<Message>,
    pub transcript_scroll: TranscriptScroll,
    pub pending_scroll_delta: i32,
    pub mouse_scroll: MouseScrollState,
    pub transcript_cache: TranscriptViewCache,
    pub transcript_selection: TranscriptSelection,
    pub last_transcript_area: Option<Rect>,
    pub last_transcript_top: usize,
    pub last_transcript_visible: usize,
    pub last_transcript_total: usize,
    pub last_transcript_padding_top: usize,
    pub is_loading: bool,
    /// Degraded connectivity mode; new user inputs are queued for later retry.
    pub offline_mode: bool,
    /// Legacy status text sink retained for compatibility with existing call sites.
    pub status_message: Option<String>,
    /// Recent status toasts (ephemeral, newest at back).
    pub status_toasts: VecDeque<StatusToast>,
    /// Sticky status toast used for important warnings/errors.
    pub sticky_status: Option<StatusToast>,
    /// Last status text already promoted from `status_message` into toast state.
    pub last_status_message_seen: Option<String>,
    pub model: String,
    /// Current API provider (mirrors `Config::api_provider`).
    /// Updated by `/provider` switches so the UI/commands can read the
    /// active backend without re-deriving it from the live config.
    pub api_provider: ApiProvider,
    /// Current reasoning-effort tier for DeepSeek thinking mode.
    /// Cycled via Shift+Tab; initialized from config at startup.
    pub reasoning_effort: ReasoningEffort,
    pub workspace: PathBuf,
    pub mcp_config_path: PathBuf,
    pub skills_dir: PathBuf,
    pub use_alt_screen: bool,
    pub use_mouse_capture: bool,
    pub use_bracketed_paste: bool,
    pub use_paste_burst_detection: bool,
    #[allow(dead_code)]
    pub system_prompt: Option<SystemPrompt>,
    pub input_history: Vec<String>,
    pub draft_history: VecDeque<String>,
    pub history_index: Option<usize>,
    pub composer_history_search: Option<ComposerHistorySearch>,
    pub selected_attachment_index: Option<usize>,
    pub auto_compact: bool,
    pub calm_mode: bool,
    pub low_motion: bool,
    /// Pending #61 (animated working strip). Set from config but not read
    /// until the footer widget consumes it.
    #[allow(dead_code)]
    pub fancy_animations: bool,
    pub show_thinking: bool,
    pub show_tool_details: bool,
    pub ui_locale: Locale,
    pub composer_density: ComposerDensity,
    pub composer_border: bool,
    pub transcript_spacing: TranscriptSpacing,
    pub sidebar_width_percent: u16,
    pub sidebar_focus: SidebarFocus,
    /// Slash menu selection index in composer.
    pub slash_menu_selected: usize,
    /// Temporary hide flag for slash menu until next input edit.
    pub slash_menu_hidden: bool,
    /// `@`-mention completion popup selection index in composer.
    pub mention_menu_selected: usize,
    /// Temporary hide flag for the @-mention popup until next input edit.
    pub mention_menu_hidden: bool,
    #[allow(dead_code)]
    pub compact_threshold: usize,
    pub max_input_history: usize,
    pub total_tokens: u32,
    /// Tokens used in the current conversation (reset on clear/load)
    pub total_conversation_tokens: u32,
    pub allow_shell: bool,
    pub max_subagents: usize,
    /// Cached sub-agent snapshots for UI views.
    pub subagent_cache: Vec<SubAgentResult>,
    /// Last known per-agent progress text for running sub-agents.
    pub agent_progress: HashMap<String, String>,
    /// In-transcript sub-agent card index by `agent_id` (issue #128).
    /// Maps each live sub-agent to the `HistoryCell::SubAgent` it renders
    /// into, so successive mailbox envelopes mutate the same cell rather
    /// than spawning duplicates.
    pub subagent_card_index: HashMap<String, usize>,
    /// History index of the most recent FanoutCard. Sibling sub-agents
    /// spawned by the same `agent_swarm` / `rlm` invocation route into
    /// this card; reset when a fresh fanout-family tool call starts.
    pub last_fanout_card_index: Option<usize>,
    /// Number of tasks declared by a pending `agent_swarm` invocation that
    /// hasn't yet received its first SwarmProgress event. Used by the
    /// sidebar to show "dispatching N" before the FanoutCard exists (#236/#238).
    /// Cleared once sync_fanout_card_from_swarm_outcome creates the card.
    pub pending_swarm_task_count: Option<usize>,
    /// Canonical swarm/job snapshots by swarm id. Transcript cards, sidebar
    /// counts, and footer status read from this model instead of recomputing
    /// worker totals independently.
    pub swarm_jobs: HashMap<String, SwarmOutcome>,
    pub last_swarm_id: Option<String>,
    /// Swarm-id → history index for the FanoutCard that visualises that
    /// swarm. Bound on first sight of a SwarmProgress event, so background
    /// swarms keep updating their *own* card even when the user starts a
    /// second fanout in parallel. Pruned by `prune_history_state_after_clear`.
    pub swarm_card_index: HashMap<String, usize>,
    /// Highest cumulative session cost ever displayed. Used to keep the
    /// footer cost monotonic across reconciliation events: provisional
    /// estimates can be revised, but the visible total never decreases
    /// during a single session unless explicitly reset (#244).
    pub displayed_cost_high_water: f64,
    /// Most recently observed sub-agent dispatch tool name (set on
    /// `ToolCallStarted` for `agent_spawn` / `agent_swarm` / etc., cleared
    /// after the first `Started` mailbox envelope routes through it).
    pub pending_subagent_dispatch: Option<String>,
    /// Animation anchor for status-strip active sub-agent spinner.
    pub agent_activity_started_at: Option<Instant>,
    pub ui_theme: UiTheme,
    // Onboarding
    pub onboarding: OnboardingState,
    pub onboarding_needs_api_key: bool,
    pub api_key_input: String,
    pub api_key_cursor: usize,
    // Hooks system
    pub hooks: HookExecutor,
    #[allow(dead_code)]
    pub yolo: bool,
    yolo_restore: Option<YoloRestoreState>,
    // Clipboard handler
    pub clipboard: ClipboardHandler,
    // Tool approval session allowlist
    pub approval_session_approved: HashSet<String>,
    pub approval_mode: ApprovalMode,
    // Modal view stack (approval/help/etc.)
    pub view_stack: ViewStack,
    /// Esc-Esc backtrack state machine (#133). `Inactive` by default; first
    /// Esc primes, second Esc opens the live-transcript overlay scoped to
    /// previous user messages so the user can rewind a turn.
    pub backtrack: crate::tui::backtrack::BacktrackState,
    /// Current session ID for auto-save updates
    pub current_session_id: Option<String>,
    /// Trust mode - allow access outside workspace
    pub trust_mode: bool,
    /// Ordered list of footer items the user wants visible. Sourced from
    /// `tui.status_items` in `~/.deepseek/config.toml` at startup; mutated
    /// live by `/statusline`. The renderer iterates this slice; no item is
    /// hardcoded in the footer code path.
    pub status_items: Vec<crate::config::StatusItem>,
    /// Project documentation (AGENTS.md or CLAUDE.md)
    #[allow(dead_code)]
    pub project_doc: Option<String>,
    /// Plan state for tracking tasks
    pub plan_state: SharedPlanState,
    /// Whether a plan follow-up prompt is waiting for user input
    pub plan_prompt_pending: bool,
    /// Whether update_plan was called during the current turn
    pub plan_tool_used_in_turn: bool,
    /// Todo list for `TodoWriteTool`
    #[allow(dead_code)] // For future engine integration
    pub todos: SharedTodoList,
    /// Durable runtime services exposed to model-visible task/automation tools.
    pub runtime_services: RuntimeToolServices,
    /// Last MCP manager/discovery snapshot shown in the UI.
    pub mcp_snapshot: Option<crate::mcp::McpManagerSnapshot>,
    /// Set after in-TUI MCP config edits because the engine caches its MCP pool.
    pub mcp_restart_required: bool,
    /// Tool execution log
    pub tool_log: Vec<String>,
    /// Session cost tracking
    pub session_cost: f64,
    /// Running cost from active sub-agents (updated live via mailbox).
    pub subagent_cost: f64,
    /// Mailbox TokenUsage sequence ids already accounted in subagent_cost.
    pub subagent_cost_event_seqs: HashSet<u64>,
    /// Active skill to apply to next user message
    pub active_skill: Option<String>,
    /// Tool call cells by tool id (for cells already finalized in `history`).
    /// While a tool call is in flight inside `active_cell`, it is tracked by
    /// `active_tool_entries` instead and migrated here at flush time.
    pub tool_cells: HashMap<String, usize>,
    /// Full tool input/output keyed by history cell index.
    pub tool_details_by_cell: HashMap<usize, ToolDetailRecord>,
    /// Linked context references keyed by the visible user history cell that
    /// introduced them.
    pub context_references_by_cell: HashMap<usize, Vec<SessionContextReference>>,
    /// Session-wide context references persisted with saved sessions.
    pub session_context_references: Vec<SessionContextReference>,
    /// In-flight tool/exec group for the current turn. Mutated in place as
    /// parallel tool calls start and complete; flushed into `history` on
    /// `TurnComplete`.
    pub active_cell: Option<ActiveCell>,
    /// Revision counter for `active_cell`. Combined with `active_cell.revision`
    /// when feeding the transcript cache so cached lines for the synthetic
    /// active-cell row are invalidated on every mutation.
    pub active_cell_revision: u64,
    /// Pending tool details for entries that live inside `active_cell`.
    /// Keyed by tool id rather than cell index because the active cell's
    /// virtual index can shift (orphan completions push real cells in
    /// between). Migrated into `tool_details_by_cell` on flush.
    pub active_tool_details: HashMap<String, ToolDetailRecord>,
    /// Active exploring cell entry index (within `active_cell.entries`).
    /// `None` once the active cell flushes or no exploring entry exists.
    pub exploring_cell: Option<usize>,
    /// Mapping of exploring tool ids to `(entry index in active_cell, entry
    /// within ExploringCell)`. Used to update individual exploring entries
    /// when their tools complete.
    pub exploring_entries: HashMap<String, (usize, usize)>,
    /// Tool calls that should be ignored by the UI
    pub ignored_tool_calls: HashSet<String>,
    /// Last exec wait command shown (for duplicate suppression)
    pub last_exec_wait_command: Option<String>,
    /// Current streaming assistant cell
    pub streaming_message_index: Option<usize>,
    /// Index into `active_cell.entries` of the thinking entry currently being
    /// streamed. `None` when no thinking block is in flight. P2.3 routes
    /// thinking into the active cell so it groups visually with tool calls
    /// until the next assistant prose chunk flushes the group into history.
    pub streaming_thinking_active_entry: Option<usize>,
    /// Newline-gated streaming collector state.
    pub streaming_state: StreamingState,
    /// Accumulated reasoning text
    pub reasoning_buffer: String,
    /// Live reasoning header extracted from bold text
    pub reasoning_header: Option<String>,
    /// Last completed reasoning block
    pub last_reasoning: Option<String>,
    /// Tool calls captured for the pending assistant message
    pub pending_tool_uses: Vec<(String, String, Value)>,
    /// User messages queued while a turn is running
    pub queued_messages: VecDeque<QueuedMessage>,
    /// Draft queued message being edited
    pub queued_draft: Option<QueuedMessage>,
    /// Legacy pending-steer bucket retained for session compatibility. New
    /// in-flight input uses Enter for same-turn steering and Tab for queued
    /// follow-ups; Esc only cancels the active turn.
    pub pending_steers: VecDeque<QueuedMessage>,
    /// Engine-rejected steers (e.g. a tool was already running and couldn't be
    /// cancelled cleanly). Surfaced in the pending-input preview so the user
    /// knows the steer was deferred to end-of-turn. Today no engine path
    /// produces these; the field is scaffolding for a future signalling
    /// channel and the bucket renders identically when populated.
    pub rejected_steers: VecDeque<String>,
    /// Legacy resend flag for pending steer recovery.
    pub submit_pending_steers_after_interrupt: bool,
    /// Start time for current turn
    pub turn_started_at: Option<Instant>,
    /// Current runtime turn id (if known).
    pub runtime_turn_id: Option<String>,
    /// Current runtime turn status (if known).
    pub runtime_turn_status: Option<String>,
    /// Provider-reported input tokens from the last completed turn. This is
    /// telemetry/cost data and may sum repeated stable prefixes across tool
    /// rounds; active context pressure is estimated from `api_messages`.
    pub last_prompt_tokens: Option<u32>,
    /// Provider-reported output tokens from the last completed turn.
    pub last_completion_tokens: Option<u32>,
    /// DeepSeek context-cache hit tokens from the last API call. Telemetry only.
    pub last_prompt_cache_hit_tokens: Option<u32>,
    /// DeepSeek context-cache miss tokens from the last API call. Telemetry only.
    pub last_prompt_cache_miss_tokens: Option<u32>,
    /// Approximate input tokens spent re-sending prior `reasoning_content` on
    /// the last thinking-mode tool-calling turn (V4 §5.1.1 "Interleaved
    /// Thinking"). Computed client-side at ~4 chars/token.
    pub last_reasoning_replay_tokens: Option<u32>,
    /// Cached git context snapshot for the footer.
    pub workspace_context: Option<String>,
    /// Timestamp for cached workspace context.
    pub workspace_context_refreshed_at: Option<Instant>,
    /// Cached background tasks for sidebar rendering.
    pub task_panel: Vec<TaskPanelEntry>,
    /// Whether the UI needs to be redrawn.
    pub needs_redraw: bool,
    /// When the current thinking block started (for duration tracking).
    pub thinking_started_at: Option<Instant>,
    /// Whether context compaction is currently in progress.
    pub is_compacting: bool,
    /// Set when the user scrolls up/down during a streaming turn so subsequent
    /// streamed chunks don't yank the view back to the live tail. Cleared
    /// when the user explicitly returns to bottom or the turn completes.
    pub user_scrolled_during_stream: bool,
    /// Plain-language session coherence state for the footer.
    pub coherence_state: CoherenceState,
    /// Timestamp of the last user message send (for brief visual feedback).
    pub last_send_at: Option<Instant>,
    /// Two-tap quit confirmation. When set, a prior Ctrl+C in idle state has
    /// armed the quit shortcut; a second Ctrl+C before this `Instant` exits
    /// the app, while expiry silently re-arms the prompt for next time.
    /// Stays `None` while a turn is in flight or a modal/picker is open so
    /// Ctrl+C keeps its current "interrupt this turn" semantics in those
    /// states. See [`App::arm_quit`] / [`App::quit_is_armed`].
    pub quit_armed_until: Option<Instant>,

    /// Number of checkpoint-restart cycles crossed in this session
    /// (issue #124). Mirrors `Session.cycle_count` on the engine side.
    pub cycle_count: u32,

    /// Briefings produced at past cycle boundaries, in chronological order.
    /// Used by `/cycles` and `/cycle <n>` slash commands.
    pub cycle_briefings: Vec<CycleBriefing>,

    /// Active cycle configuration (token threshold, briefing cap, per-model
    /// overrides). Loaded from config and forwarded to the engine.
    pub cycle: CycleConfig,
}

/// Message queued while the engine is busy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub display: String,
    pub skill_instruction: Option<String>,
}

/// How a freshly-typed user input should be sent.
///
/// Picked by [`App::decide_submit_disposition`] when the user hits Enter on a
/// non-empty composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitDisposition {
    /// Engine idle and online: send immediately.
    Immediate,
    /// Offline mode: park on `queued_messages`.
    Queue,
    /// Engine busy and online: forward as a mid-turn steer.
    Steer,
    /// Model is actively streaming text; park on `queued_messages` for
    /// dispatch after TurnComplete.
    QueueFollowUp,
}

/// Detailed tool payload attached to a history cell.
#[derive(Debug, Clone)]
pub struct ToolDetailRecord {
    pub tool_id: String,
    pub tool_name: String,
    pub input: Value,
    pub output: Option<String>,
}

/// Lightweight task view for sidebar rendering.
#[derive(Debug, Clone)]
pub struct TaskPanelEntry {
    pub id: String,
    pub status: String,
    pub prompt_summary: String,
    pub duration_ms: Option<u64>,
}

impl QueuedMessage {
    pub fn new(display: String, skill_instruction: Option<String>) -> Self {
        Self {
            display,
            skill_instruction,
        }
    }

    #[allow(dead_code)] // Tests and queue helpers use the display-only form; send path resolves @mentions.
    pub fn content(&self) -> String {
        if let Some(skill_instruction) = self.skill_instruction.as_ref() {
            format!(
                "{skill_instruction}\n\n---\n\nUser request: {}",
                self.display
            )
        } else {
            self.display.clone()
        }
    }
}

// === Errors ===

/// Errors that can occur while submitting API keys during onboarding.
#[derive(Debug, Error)]
pub enum ApiKeyError {
    /// The provided API key was empty.
    #[error("Failed to save API key: API key cannot be empty")]
    Empty,
    /// Persisting the API key failed.
    #[error("Failed to save API key: {source}")]
    SaveFailed { source: anyhow::Error },
}

// === App State ===

impl App {
    pub fn tr(&self, id: MessageId) -> &'static str {
        tr(self.ui_locale, id)
    }

    #[allow(clippy::too_many_lines)]
    pub fn new(options: TuiOptions, config: &Config) -> Self {
        let TuiOptions {
            model,
            workspace,
            allow_shell,
            use_alt_screen,
            use_mouse_capture,
            use_bracketed_paste,
            max_subagents,
            skills_dir: global_skills_dir,
            memory_path: _,
            notes_path: _,
            mcp_config_path,
            use_memory: _,
            start_in_agent_mode,
            skip_onboarding,
            yolo,
            resume_session_id: _,
        } = options;
        // Check if API key exists
        let needs_api_key = !has_api_key(config);
        let was_onboarded = crate::tui::onboarding::is_onboarded();
        let needs_onboarding = !skip_onboarding && (!was_onboarded || needs_api_key);
        let settings = Settings::load().unwrap_or_else(|_| Settings::default());
        let auto_compact = settings.auto_compact;
        let calm_mode = settings.calm_mode;
        let low_motion = settings.low_motion;
        let fancy_animations = settings.fancy_animations;
        let show_thinking = settings.show_thinking;
        let show_tool_details = settings.show_tool_details;
        let ui_locale = resolve_locale(&settings.locale);
        let composer_density = ComposerDensity::from_setting(&settings.composer_density);
        let composer_border = settings.composer_border;
        let transcript_spacing = TranscriptSpacing::from_setting(&settings.transcript_spacing);
        let sidebar_width_percent = settings.sidebar_width_percent;
        let sidebar_focus = SidebarFocus::from_setting(&settings.sidebar_focus);
        let max_input_history = settings.max_input_history;
        let use_paste_burst_detection = settings.paste_burst_detection;
        let ui_theme = palette::UI_THEME;
        let model = settings.default_model.clone().unwrap_or(model);
        let compact_threshold =
            compaction_threshold_for_model_and_effort(&model, config.reasoning_effort());

        // Start in YOLO mode if --yolo flag was passed
        let preferred_mode = AppMode::from_setting(&settings.default_mode);
        let initial_mode = if yolo {
            AppMode::Yolo
        } else if start_in_agent_mode {
            AppMode::Agent
        } else {
            preferred_mode
        };

        let yolo_restore = if initial_mode == AppMode::Yolo {
            Some(YoloRestoreState {
                allow_shell: config.allow_shell(),
                trust_mode: false,
                approval_mode: ApprovalMode::Suggest,
            })
        } else {
            None
        };
        let allow_shell = allow_shell || initial_mode == AppMode::Yolo;
        let shell_manager = new_shared_shell_manager(workspace.clone());

        // Initialize hooks executor from config
        let hooks_config = config.hooks_config();
        let hooks = HookExecutor::new(hooks_config, workspace.clone());

        // Initialize plan state
        let plan_state = new_shared_plan_state();

        let agents_skills_dir = workspace.join(".agents").join("skills");
        let local_skills_dir = workspace.join("skills");
        let skills_dir = if agents_skills_dir.exists() {
            agents_skills_dir
        } else if local_skills_dir.exists() {
            local_skills_dir
        } else {
            global_skills_dir
        };

        Self {
            mode: initial_mode,
            input: String::new(),
            cursor_position: 0,
            kill_buffer: String::new(),
            paste_burst: PasteBurst::default(),
            history: Vec::new(),
            history_version: 0,
            history_revisions: Vec::new(),
            next_history_revision: 1,
            api_messages: Vec::new(),
            transcript_scroll: TranscriptScroll::to_bottom(),
            pending_scroll_delta: 0,
            mouse_scroll: MouseScrollState::new(),
            transcript_cache: TranscriptViewCache::new(),
            transcript_selection: TranscriptSelection::default(),
            last_transcript_area: None,
            last_transcript_top: 0,
            last_transcript_visible: 0,
            last_transcript_total: 0,
            last_transcript_padding_top: 0,
            is_loading: false,
            offline_mode: false,
            status_message: None,
            status_toasts: VecDeque::new(),
            sticky_status: None,
            last_status_message_seen: None,
            model,
            api_provider: config.api_provider(),
            reasoning_effort: config
                .reasoning_effort()
                .map_or_else(ReasoningEffort::default, |s| {
                    ReasoningEffort::from_setting(s)
                }),
            workspace,
            mcp_config_path,
            skills_dir,
            use_alt_screen,
            use_mouse_capture,
            use_bracketed_paste,
            use_paste_burst_detection,
            system_prompt: None,
            input_history: Vec::new(),
            draft_history: VecDeque::new(),
            history_index: None,
            composer_history_search: None,
            selected_attachment_index: None,
            auto_compact,
            calm_mode,
            low_motion,
            fancy_animations,
            show_thinking,
            show_tool_details,
            ui_locale,
            composer_density,
            composer_border,
            transcript_spacing,
            sidebar_width_percent,
            sidebar_focus,
            slash_menu_selected: 0,
            mention_menu_selected: 0,
            mention_menu_hidden: false,
            slash_menu_hidden: false,
            compact_threshold,
            max_input_history,
            total_tokens: 0,
            total_conversation_tokens: 0,
            allow_shell,
            max_subagents,
            subagent_cache: Vec::new(),
            agent_progress: HashMap::new(),
            subagent_card_index: HashMap::new(),
            last_fanout_card_index: None,
            pending_swarm_task_count: None,
            swarm_jobs: HashMap::new(),
            last_swarm_id: None,
            swarm_card_index: HashMap::new(),
            displayed_cost_high_water: 0.0,
            pending_subagent_dispatch: None,
            agent_activity_started_at: None,
            ui_theme,
            onboarding: if needs_onboarding {
                if was_onboarded && needs_api_key {
                    OnboardingState::ApiKey
                } else {
                    OnboardingState::Welcome
                }
            } else {
                OnboardingState::None
            },
            onboarding_needs_api_key: needs_api_key,
            api_key_input: String::new(),
            api_key_cursor: 0,
            hooks,
            yolo: initial_mode == AppMode::Yolo,
            yolo_restore,
            clipboard: ClipboardHandler::new(),
            approval_session_approved: HashSet::new(),
            approval_mode: if matches!(initial_mode, AppMode::Yolo) {
                ApprovalMode::Auto
            } else {
                ApprovalMode::Suggest
            },
            view_stack: ViewStack::new(),
            backtrack: crate::tui::backtrack::BacktrackState::new(),
            current_session_id: None,
            trust_mode: initial_mode == AppMode::Yolo,
            // Honour `tui.status_items` from config; fall back to the v0.6.6
            // default footer composition when unset so upgraders see no
            // change. Empty `Some(vec![])` is respected (user explicitly
            // wants a bare footer).
            status_items: config
                .tui
                .as_ref()
                .and_then(|tui| tui.status_items.clone())
                .unwrap_or_else(crate::config::StatusItem::default_footer),
            project_doc: None,
            plan_state,
            plan_prompt_pending: false,
            plan_tool_used_in_turn: false,
            todos: new_shared_todo_list(),
            runtime_services: RuntimeToolServices {
                shell_manager: Some(shell_manager),
                ..RuntimeToolServices::default()
            },
            mcp_snapshot: None,
            mcp_restart_required: false,
            tool_log: Vec::new(),
            session_cost: 0.0,
            subagent_cost: 0.0,
            subagent_cost_event_seqs: HashSet::new(),
            active_skill: None,
            tool_cells: HashMap::new(),
            tool_details_by_cell: HashMap::new(),
            context_references_by_cell: HashMap::new(),
            session_context_references: Vec::new(),
            active_cell: None,
            active_cell_revision: 0,
            active_tool_details: HashMap::new(),
            exploring_cell: None,
            exploring_entries: HashMap::new(),
            ignored_tool_calls: HashSet::new(),
            last_exec_wait_command: None,
            streaming_message_index: None,
            streaming_thinking_active_entry: None,
            streaming_state: StreamingState::new(),
            reasoning_buffer: String::new(),
            reasoning_header: None,
            last_reasoning: None,
            pending_tool_uses: Vec::new(),
            queued_messages: VecDeque::new(),
            queued_draft: None,
            pending_steers: VecDeque::new(),
            rejected_steers: VecDeque::new(),
            submit_pending_steers_after_interrupt: false,
            turn_started_at: None,
            runtime_turn_id: None,
            runtime_turn_status: None,
            last_prompt_tokens: None,
            last_completion_tokens: None,
            last_prompt_cache_hit_tokens: None,
            last_prompt_cache_miss_tokens: None,
            last_reasoning_replay_tokens: None,
            workspace_context: None,
            workspace_context_refreshed_at: None,
            task_panel: Vec::new(),
            needs_redraw: true,
            thinking_started_at: None,
            is_compacting: false,
            user_scrolled_during_stream: false,
            coherence_state: CoherenceState::default(),
            last_send_at: None,
            quit_armed_until: None,
            cycle_count: 0,
            cycle_briefings: Vec::new(),
            cycle: CycleConfig::default(),
        }
    }

    pub fn submit_api_key(&mut self) -> Result<PathBuf, ApiKeyError> {
        let key = self.api_key_input.trim().to_string();
        if key.is_empty() {
            return Err(ApiKeyError::Empty);
        }

        match save_api_key(&key) {
            Ok(path) => {
                self.api_key_input.clear();
                self.api_key_cursor = 0;
                self.onboarding_needs_api_key = false;
                Ok(path)
            }
            Err(source) => Err(ApiKeyError::SaveFailed { source }),
        }
    }

    pub fn finish_onboarding(&mut self) {
        self.onboarding = OnboardingState::None;
        if let Err(err) = crate::tui::onboarding::mark_onboarded() {
            self.status_message = Some(format!("Failed to mark onboarding: {err}"));
        }
        self.needs_redraw = true;
    }

    pub fn set_mode(&mut self, mode: AppMode) -> bool {
        let previous_mode = self.mode;
        if previous_mode == mode {
            return false;
        }

        let entering_yolo = mode == AppMode::Yolo && previous_mode != AppMode::Yolo;
        let leaving_yolo = previous_mode == AppMode::Yolo && mode != AppMode::Yolo;
        self.mode = mode;
        self.status_message = Some(format!("Switched to {} mode", mode.label()));

        if entering_yolo {
            self.yolo_restore = Some(YoloRestoreState {
                allow_shell: self.allow_shell,
                trust_mode: self.trust_mode,
                approval_mode: self.approval_mode,
            });
            self.allow_shell = true;
            self.trust_mode = true;
            self.approval_mode = ApprovalMode::Auto;
        } else if leaving_yolo && let Some(restore) = self.yolo_restore.take() {
            self.allow_shell = restore.allow_shell;
            self.trust_mode = restore.trust_mode;
            self.approval_mode = restore.approval_mode;
        }

        self.yolo = mode == AppMode::Yolo;
        if mode != AppMode::Plan {
            self.plan_prompt_pending = false;
            self.plan_tool_used_in_turn = false;
        }

        // Execute mode change hooks
        let context = HookContext::new()
            .with_mode(mode.label())
            .with_previous_mode(previous_mode.label())
            .with_workspace(self.workspace.clone())
            .with_model(&self.model);
        let _ = self.hooks.execute(HookEvent::ModeChange, &context);
        self.needs_redraw = true;
        true
    }

    /// Cycle through modes: Plan → Agent → YOLO → Plan.
    pub fn cycle_mode(&mut self) {
        let next = match self.mode {
            AppMode::Plan => AppMode::Agent,
            AppMode::Agent => AppMode::Yolo,
            AppMode::Yolo => AppMode::Plan,
        };
        let _ = self.set_mode(next);
    }

    /// Cycle through modes in reverse.
    #[allow(dead_code)]
    pub fn cycle_mode_reverse(&mut self) {
        let next = match self.mode {
            AppMode::Agent => AppMode::Plan,
            AppMode::Yolo => AppMode::Agent,
            AppMode::Plan => AppMode::Yolo,
        };
        let _ = self.set_mode(next);
    }

    /// Cycle reasoning-effort through the three behaviorally distinct tiers:
    /// `Off` → `High` → `Max` → `Off`.
    pub fn cycle_effort(&mut self) {
        self.reasoning_effort = self.reasoning_effort.cycle_next();
        self.needs_redraw = true;
        self.push_status_toast(
            format!("Thinking: {}", self.reasoning_effort.short_label()),
            StatusToastLevel::Info,
            Some(1_500),
        );
    }

    /// Execute hooks for a specific event with the given context
    pub fn execute_hooks(&self, event: HookEvent, context: &HookContext) -> Vec<HookResult> {
        self.hooks.execute(event, context)
    }

    /// Create a hook context with common fields pre-populated
    pub fn base_hook_context(&self) -> HookContext {
        HookContext::new()
            .with_mode(self.mode.label())
            .with_workspace(self.workspace.clone())
            .with_model(&self.model)
            .with_session_id(self.hooks.session_id())
            .with_tokens(self.total_tokens)
    }

    pub fn add_message(&mut self, msg: HistoryCell) {
        let rev = self.fresh_history_revision();
        self.history.push(msg);
        self.history_revisions.push(rev);
        self.history_version = self.history_version.wrapping_add(1);
        let selection_has_range = self
            .transcript_selection
            .ordered_endpoints()
            .is_some_and(|(start, end)| start != end);
        // Auto-pin to live tail only when:
        //   1. We're already at the tail (nothing to do otherwise)
        //   2. The user isn't actively selecting text
        //   3. The user hasn't scrolled away during this streaming turn
        // Without (3), pressing Up while a tool result streams in would lose
        // the keypress: scroll_up sets pending_scroll_delta, but before the
        // render frame consumes it, mark_history_updated would fire here,
        // call scroll_to_bottom, and zero the delta.
        if self.transcript_scroll.is_at_tail()
            && !self.transcript_selection.dragging
            && !selection_has_range
            && !self.user_scrolled_during_stream
        {
            self.scroll_to_bottom();
        }
    }

    /// Add `delta` to the parent-turn session cost and bump the displayed
    /// high-water mark so the footer total never reverses (#244).
    pub fn accrue_session_cost(&mut self, delta: f64) {
        self.session_cost += delta;
        self.refresh_displayed_cost_high_water();
    }

    /// Add `delta` to the running sub-agent cost and bump the displayed
    /// high-water mark so the footer total never reverses (#244).
    pub fn accrue_subagent_cost(&mut self, delta: f64) {
        self.subagent_cost += delta;
        self.refresh_displayed_cost_high_water();
    }

    /// Recompute the displayed cost high-water mark. Called any time a cost
    /// counter is mutated; never decreases.
    pub fn refresh_displayed_cost_high_water(&mut self) {
        let current = self.session_cost + self.subagent_cost;
        if current > self.displayed_cost_high_water {
            self.displayed_cost_high_water = current;
        }
    }

    /// Read the visible session+sub-agent cost. Guaranteed monotonic across
    /// reconciliation events (cache adjustments, provisional → final swaps)
    /// for the lifetime of one session (#244).
    pub fn displayed_session_cost(&self) -> f64 {
        let current = self.session_cost + self.subagent_cost;
        current.max(self.displayed_cost_high_water)
    }

    pub fn mark_history_updated(&mut self) {
        self.history_version = self.history_version.wrapping_add(1);
        // Resync per-cell revisions to history.len(). This is the
        // "I-don't-know-which-cell-changed" path: if cells were appended in
        // bulk (e.g. session resume, compaction), every new cell gets a
        // fresh revision; if cells were removed, drop trailing revs. We
        // intentionally do NOT bump revisions for indices that already had
        // one — the cache will reuse those. Callers that mutate a specific
        // cell's content must call `bump_history_cell(idx)` instead.
        self.resync_history_revisions();
        self.needs_redraw = true;
    }

    /// Issue a fresh, monotonically increasing revision counter for a new
    /// history cell. Wrapping is acceptable — collisions are astronomically
    /// rare and at worst trigger one extra re-render.
    fn fresh_history_revision(&mut self) -> u64 {
        let rev = self.next_history_revision;
        self.next_history_revision = self.next_history_revision.wrapping_add(1);
        rev
    }

    /// Bring `history_revisions` back into shape (`history_revisions.len() ==
    /// history.len()`). Pushes fresh revs for newly appended cells, truncates
    /// for cells that were removed. **Does not** invalidate existing entries.
    pub fn resync_history_revisions(&mut self) {
        if self.history_revisions.len() < self.history.len() {
            let needed = self.history.len() - self.history_revisions.len();
            for _ in 0..needed {
                let rev = self.fresh_history_revision();
                self.history_revisions.push(rev);
            }
        } else if self.history_revisions.len() > self.history.len() {
            self.history_revisions.truncate(self.history.len());
        }
    }

    /// Bump the revision counter of a single history cell so the transcript
    /// cache re-renders it on the next frame. Use this whenever a cell's
    /// content (e.g. a streaming Assistant body) is mutated in place.
    pub fn bump_history_cell(&mut self, idx: usize) {
        // Resync first in case callers mutated `history` directly without
        // pushing through `add_message`. After resync, the index is valid
        // (or out of bounds — in which case there's nothing to bump).
        self.resync_history_revisions();
        if let Some(rev) = self.history_revisions.get_mut(idx) {
            let new_rev = self.next_history_revision;
            self.next_history_revision = self.next_history_revision.wrapping_add(1);
            *rev = new_rev;
        }
        self.history_version = self.history_version.wrapping_add(1);
        self.needs_redraw = true;
    }

    /// Append a single history cell, allocating a fresh per-cell revision.
    /// Equivalent to `add_message` but exposed as a generic alias so call
    /// sites currently doing `app.history.push(...)` followed by
    /// `app.mark_history_updated()` can collapse to one helper.
    pub fn push_history_cell(&mut self, cell: HistoryCell) {
        let rev = self.fresh_history_revision();
        self.history.push(cell);
        self.history_revisions.push(rev);
        self.history_version = self.history_version.wrapping_add(1);
        self.needs_redraw = true;
    }

    /// Append a batch of history cells, allocating fresh revisions.
    pub fn extend_history<I>(&mut self, cells: I)
    where
        I: IntoIterator<Item = HistoryCell>,
    {
        for cell in cells {
            let rev = self.fresh_history_revision();
            self.history.push(cell);
            self.history_revisions.push(rev);
        }
        self.history_version = self.history_version.wrapping_add(1);
        self.needs_redraw = true;
    }

    /// Clear the history and its revision tracking. Used by /clear, session
    /// reset, and other "wipe and reload" flows.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.history_revisions.clear();
        self.context_references_by_cell.clear();
        self.session_context_references.clear();
        self.history_version = self.history_version.wrapping_add(1);
        self.needs_redraw = true;
    }

    /// Pop the trailing history cell, keeping revisions in sync.
    pub fn pop_history(&mut self) -> Option<HistoryCell> {
        let cell = self.history.pop();
        if cell.is_some() {
            self.history_revisions.pop();
            self.context_references_by_cell.remove(&self.history.len());
            self.rebuild_session_context_references();
            self.history_version = self.history_version.wrapping_add(1);
            self.needs_redraw = true;
        }
        cell
    }

    /// Truncate `history` (and the parallel `history_revisions` + auxiliary
    /// per-cell maps) so that only cells with index `< new_len` remain.
    /// Used by Esc-Esc backtrack (#133) to roll the visible transcript
    /// back to a chosen user message. Cells dropped here are gone — the
    /// caller is expected to also trim the matching `api_messages` so the
    /// next turn matches what the user sees.
    pub fn truncate_history_to(&mut self, new_len: usize) {
        if new_len >= self.history.len() {
            return;
        }
        self.history.truncate(new_len);
        if self.history_revisions.len() > new_len {
            self.history_revisions.truncate(new_len);
        }
        // Drop any auxiliary maps keyed on history indices that now point
        // past the new tail. We keep the rest intact so unaffected tool
        // cells continue to render correctly.
        self.tool_cells.retain(|_, idx| *idx < new_len);
        self.tool_details_by_cell.retain(|idx, _| *idx < new_len);
        self.context_references_by_cell
            .retain(|idx, _| *idx < new_len);
        self.rebuild_session_context_references();
        self.subagent_card_index.retain(|_, idx| *idx < new_len);
        self.swarm_card_index.retain(|_, idx| *idx < new_len);
        if self
            .last_fanout_card_index
            .is_some_and(|idx| idx >= new_len)
        {
            self.last_fanout_card_index = None;
        }
        self.history_version = self.history_version.wrapping_add(1);
        self.needs_redraw = true;
    }

    /// Bump the active-cell revision counter and request a redraw.
    ///
    /// Use this whenever an entry inside `active_cell` is mutated. The
    /// transcript cache combines this counter with `history_version` to
    /// produce a per-cell revision so the synthetic active-cell row can be
    /// re-rendered without invalidating committed history cells.
    pub fn bump_active_cell_revision(&mut self) {
        self.active_cell_revision = self.active_cell_revision.wrapping_add(1);
        if let Some(active) = self.active_cell.as_mut() {
            active.bump_revision();
        }
        self.history_version = self.history_version.wrapping_add(1);
        self.needs_redraw = true;
    }

    /// Total number of cells in the *virtual* transcript: `history.len()`
    /// plus active cell entries (if any).
    #[must_use]
    #[allow(dead_code)] // Reserved for renderers that need a unified cell count.
    pub fn virtual_cell_count(&self) -> usize {
        self.history.len() + self.active_cell.as_ref().map_or(0, ActiveCell::entry_count)
    }

    /// The next cell index a freshly-pushed entry would occupy in the virtual
    /// transcript. Used by `register_tool_cell`-style callsites that record
    /// cell-index metadata before the active cell flushes to history.
    #[must_use]
    #[allow(dead_code)] // Reserved for the eventual merged push helper.
    pub fn next_virtual_cell_index(&self) -> usize {
        self.virtual_cell_count()
    }

    /// Resolve a virtual cell index to either a committed history cell or an
    /// active-cell entry. Used by the pager / details lookup code so it can
    /// transparently address still-in-flight cells.
    #[must_use]
    #[allow(dead_code)] // Used by the upcoming pager rewrite (read-only resolver).
    pub fn cell_at_virtual_index(&self, index: usize) -> Option<&HistoryCell> {
        if index < self.history.len() {
            self.history.get(index)
        } else {
            let entry_idx = index - self.history.len();
            self.active_cell
                .as_ref()
                .and_then(|active| active.entries().get(entry_idx))
        }
    }

    /// Resolve the tool-detail record for a committed or still-active virtual
    /// transcript cell.
    #[must_use]
    pub fn tool_detail_record_for_cell(&self, index: usize) -> Option<&ToolDetailRecord> {
        if let Some(detail) = self.tool_details_by_cell.get(&index) {
            return Some(detail);
        }
        self.active_tool_details
            .values()
            .find(|detail| self.tool_cells.get(&detail.tool_id).copied() == Some(index))
    }

    /// Whether a virtual transcript cell can open a meaningful Alt+V detail
    /// view.
    #[must_use]
    pub fn cell_has_detail_target(&self, index: usize) -> bool {
        self.tool_detail_record_for_cell(index).is_some()
            || matches!(
                self.cell_at_virtual_index(index),
                Some(HistoryCell::Tool(_) | HistoryCell::SubAgent(_))
            )
    }

    /// Pick the detail target for the current viewport. This is used by the
    /// transcript highlight and footer hint so they agree with Alt+V.
    #[must_use]
    pub fn detail_cell_index_for_viewport(
        &self,
        top: usize,
        visible: usize,
        line_meta: &[TranscriptLineMeta],
    ) -> Option<usize> {
        let selected_cell = self
            .transcript_selection
            .ordered_endpoints()
            .and_then(|(start, _)| line_meta.get(start.line_index))
            .and_then(TranscriptLineMeta::cell_line)
            .map(|(cell_index, _)| cell_index)
            .filter(|&idx| self.cell_has_detail_target(idx));
        if selected_cell.is_some() {
            return selected_cell;
        }

        let start = top.min(line_meta.len().saturating_sub(1));
        let end = start.saturating_add(visible).min(line_meta.len());
        for meta in line_meta.iter().take(end).skip(start) {
            let Some((cell_index, _)) = meta.cell_line() else {
                continue;
            };
            if self.cell_has_detail_target(cell_index) {
                return Some(cell_index);
            }
        }

        (0..self.virtual_cell_count())
            .rev()
            .find(|&idx| self.cell_has_detail_target(idx))
    }

    pub fn record_context_references(
        &mut self,
        history_cell: usize,
        message_index: usize,
        references: Vec<ContextReference>,
    ) {
        if references.is_empty() {
            return;
        }
        let records: Vec<SessionContextReference> = references
            .into_iter()
            .map(|reference| SessionContextReference {
                message_index,
                reference,
            })
            .collect();
        self.context_references_by_cell
            .insert(history_cell, records.clone());
        self.rebuild_session_context_references();
        self.needs_redraw = true;
    }

    pub fn sync_context_references_from_session(
        &mut self,
        references: &[SessionContextReference],
        message_to_cell: &HashMap<usize, usize>,
    ) {
        self.context_references_by_cell.clear();
        for record in references {
            let Some(&cell_index) = message_to_cell.get(&record.message_index) else {
                continue;
            };
            self.context_references_by_cell
                .entry(cell_index)
                .or_default()
                .push(record.clone());
        }
        self.rebuild_session_context_references();
    }

    fn rebuild_session_context_references(&mut self) {
        let mut records: Vec<SessionContextReference> = self
            .context_references_by_cell
            .values()
            .flat_map(|records| records.iter().cloned())
            .collect();
        records.sort_by_key(|record| record.message_index);
        self.session_context_references = records;
    }

    /// Mutable variant of [`Self::cell_at_virtual_index`]. Bumps the
    /// appropriate revision counter (active-cell revision when targeting an
    /// in-flight entry, history version otherwise).
    pub fn cell_at_virtual_index_mut(&mut self, index: usize) -> Option<&mut HistoryCell> {
        if index < self.history.len() {
            // Bump only the targeted cell's revision; leave every other
            // cell's cached render intact.
            self.resync_history_revisions();
            if let Some(rev) = self.history_revisions.get_mut(index) {
                let new_rev = self.next_history_revision;
                self.next_history_revision = self.next_history_revision.wrapping_add(1);
                *rev = new_rev;
            }
            self.history_version = self.history_version.wrapping_add(1);
            self.history.get_mut(index)
        } else {
            let entry_idx = index - self.history.len();
            self.active_cell_revision = self.active_cell_revision.wrapping_add(1);
            self.history_version = self.history_version.wrapping_add(1);
            self.active_cell
                .as_mut()
                .and_then(|active| active.entry_mut(entry_idx))
        }
    }

    /// Drain the active cell into history. Companion maps that reference
    /// active-cell entries by virtual index (`tool_cells`,
    /// `tool_details_by_cell`) are rewritten to point at the new history
    /// indices. Idempotent — calling this when there is no active cell is a
    /// no-op.
    ///
    /// Caller is responsible for first marking in-progress entries with the
    /// terminal status they want (e.g. via
    /// [`ActiveCell::mark_in_progress_as_interrupted`]).
    pub fn flush_active_cell(&mut self) {
        let Some(mut active) = self.active_cell.take() else {
            // Even with no active cell, the thinking-stream pointer must not
            // outlive a flush — a stale index would point at the wrong cell
            // after subsequent pushes.
            self.streaming_thinking_active_entry = None;
            return;
        };
        if active.is_empty() {
            // Reset auxiliary state regardless so a future tool can start a
            // fresh active cell.
            self.exploring_cell = None;
            self.exploring_entries.clear();
            self.active_tool_details.clear();
            self.streaming_thinking_active_entry = None;
            self.bump_active_cell_revision();
            return;
        }

        // P2.3 safety net: stop any still-streaming thinking spinner before
        // the entry migrates into history. Normal flow finalizes via
        // `ThinkingComplete`; this guards against engine misbehaviour and
        // race conditions.
        if let Some(entry_idx) = self.streaming_thinking_active_entry.take()
            && let Some(HistoryCell::Thinking { streaming, .. }) = active.entry_mut(entry_idx)
        {
            *streaming = false;
        }

        let drained = active.drain();
        let base_index = self.history.len();

        // Rewrite per-tool indices that targeted entries inside the active
        // group: their new home is `base_index + entry_offset`.
        let mut details = std::mem::take(&mut self.active_tool_details);
        for (tool_id, detail) in details.drain() {
            // Try to recover the entry offset from `tool_cells`-style maps.
            // Tool ids registered for active-cell entries live in
            // `tool_cells` with `index = base_index_at_register_time +
            // entry_offset`. After rewriting once, those indices are correct.
            self.tool_details_by_cell
                .entry(self.tool_cells.get(&tool_id).copied().unwrap_or(base_index))
                .or_insert(detail);
        }

        // tool_cells already contains the virtual index. After the drain,
        // history.len() == base_index + drained.len(), so any virtual index
        // in [base_index, base_index + drained.len()) is now a real history
        // index. No rewrite needed.
        self.exploring_cell = None;
        self.exploring_entries.clear();

        for cell in drained {
            let rev = self.fresh_history_revision();
            self.history.push(cell);
            self.history_revisions.push(rev);
        }
        self.history_version = self.history_version.wrapping_add(1);
        self.needs_redraw = true;
        let selection_has_range = self
            .transcript_selection
            .ordered_endpoints()
            .is_some_and(|(start, end)| start != end);
        if self.transcript_scroll.is_at_tail()
            && !self.transcript_selection.dragging
            && !selection_has_range
            && !self.user_scrolled_during_stream
        {
            self.scroll_to_bottom();
        }
    }

    /// Mark every still-running entry in the active cell as interrupted, then
    /// flush. Convenience helper for cancellation paths.
    pub fn finalize_active_cell_as_interrupted(&mut self) {
        if let Some(active) = self.active_cell.as_mut() {
            active.mark_in_progress_as_interrupted();
        }
        self.flush_active_cell();
    }

    pub fn push_status_toast(
        &mut self,
        text: impl Into<String>,
        level: StatusToastLevel,
        ttl_ms: Option<u64>,
    ) {
        let toast = StatusToast::new(text, level, ttl_ms);
        self.status_toasts.push_back(toast);
        while self.status_toasts.len() > 24 {
            self.status_toasts.pop_front();
        }
        self.needs_redraw = true;
    }

    /// How long the "press Ctrl+C again to quit" prompt stays armed before it
    /// silently expires.
    pub const QUIT_CONFIRMATION_WINDOW: Duration = Duration::from_secs(2);

    /// Arm the quit confirmation timer. The next Ctrl+C within
    /// [`Self::QUIT_CONFIRMATION_WINDOW`] should exit the app cleanly. Call this only
    /// from idle state — while a turn is in flight or a modal is open Ctrl+C
    /// retains its existing "interrupt this turn" / "close modal" semantics.
    pub fn arm_quit(&mut self) {
        self.quit_armed_until = Some(Instant::now() + Self::QUIT_CONFIRMATION_WINDOW);
        self.needs_redraw = true;
    }

    /// Whether the quit timer is currently armed (i.e. a prior Ctrl+C set it
    /// and it hasn't expired yet).
    pub fn quit_is_armed(&self) -> bool {
        self.quit_armed_until
            .map(|deadline| Instant::now() < deadline)
            .unwrap_or(false)
    }

    /// Clear the quit-armed timer. Call when expiry is detected on a tick or
    /// when the user takes any other action that should disarm the prompt
    /// (typing, sending a message, etc.).
    pub fn disarm_quit(&mut self) {
        if self.quit_armed_until.is_some() {
            self.quit_armed_until = None;
            self.needs_redraw = true;
        }
    }

    /// Tick called from the redraw loop. Lets time-based UI state (the
    /// quit-armed prompt) expire even when no input event is delivered.
    pub fn tick_quit_armed(&mut self) {
        if let Some(deadline) = self.quit_armed_until
            && Instant::now() >= deadline
        {
            self.quit_armed_until = None;
            self.needs_redraw = true;
        }
    }

    pub fn set_sticky_status(
        &mut self,
        text: impl Into<String>,
        level: StatusToastLevel,
        ttl_ms: Option<u64>,
    ) {
        self.sticky_status = Some(StatusToast::new(text, level, ttl_ms));
        self.needs_redraw = true;
    }

    pub fn clear_sticky_status(&mut self) {
        self.sticky_status = None;
    }

    pub fn set_sidebar_focus(&mut self, focus: SidebarFocus) {
        self.sidebar_focus = focus;
        self.needs_redraw = true;
    }

    pub fn close_slash_menu(&mut self) {
        self.slash_menu_hidden = true;
        self.needs_redraw = true;
    }

    fn classify_status_text(text: &str) -> (StatusToastLevel, Option<u64>, bool) {
        let lower = text.to_ascii_lowercase();
        let has = |needle: &str| lower.contains(needle);

        if has("offline mode") || has("context critical") {
            return (StatusToastLevel::Warning, None, true);
        }
        if has("error")
            || has("failed")
            || has("denied")
            || has("timeout")
            || has("aborted")
            || has("critical")
        {
            return (StatusToastLevel::Error, Some(15_000), true);
        }
        if has("saved")
            || has("loaded")
            || has("queued")
            || has("found")
            || has("enabled")
            || has("completed")
        {
            return (StatusToastLevel::Success, Some(5_000), false);
        }
        if has("cancelled") || has("warning") {
            return (StatusToastLevel::Warning, Some(5_000), false);
        }
        (StatusToastLevel::Info, Some(4_000), false)
    }

    pub fn sync_status_message_to_toasts(&mut self) {
        let current = self.status_message.clone();
        if self.last_status_message_seen == current {
            return;
        }
        self.last_status_message_seen = current.clone();

        let Some(message) = current else {
            return;
        };
        if message.trim().is_empty() {
            return;
        }

        let (level, ttl_ms, sticky) = Self::classify_status_text(&message);
        if sticky {
            self.set_sticky_status(message, level, ttl_ms);
        } else {
            if matches!(level, StatusToastLevel::Success)
                && self
                    .sticky_status
                    .as_ref()
                    .is_some_and(|toast| matches!(toast.level, StatusToastLevel::Error))
            {
                self.clear_sticky_status();
            }
            self.push_status_toast(message, level, ttl_ms);
        }
    }

    pub fn active_status_toast(&mut self) -> Option<StatusToast> {
        self.sync_status_message_to_toasts();
        let now = Instant::now();
        let mut removed = false;

        while self
            .status_toasts
            .front()
            .is_some_and(|toast| toast.is_expired(now))
        {
            self.status_toasts.pop_front();
            removed = true;
        }

        if self
            .sticky_status
            .as_ref()
            .is_some_and(|toast| toast.is_expired(now))
        {
            self.sticky_status = None;
            removed = true;
        }

        if removed {
            self.needs_redraw = true;
        }

        self.sticky_status
            .clone()
            .or_else(|| self.status_toasts.back().cloned())
    }

    pub fn transcript_render_options(&self) -> TranscriptRenderOptions {
        TranscriptRenderOptions {
            show_thinking: self.show_thinking,
            show_tool_details: self.show_tool_details,
            calm_mode: self.calm_mode,
            low_motion: self.low_motion,
            spacing: self.transcript_spacing,
        }
    }

    /// Handle terminal resize event.
    ///
    /// This method properly invalidates all cached layout state to ensure
    /// correct rendering after the terminal dimensions change.
    pub fn handle_resize(&mut self, _width: u16, _height: u16) {
        // Invalidate transcript cache (will be rebuilt on next render)
        self.transcript_cache = TranscriptViewCache::new();

        // The flat line-offset model is width-dependent (line wrapping
        // changes the meta length on resize), so a stored offset can no
        // longer point at the same logical content. Snapping back to the
        // tail keeps the user where they intuitively expect — at the
        // most recent output — and matches what Codex does on resize.
        // The renderer will clamp anyway, but resetting to tail avoids
        // a frame where the offset shows stale wrapping.
        if !self.transcript_scroll.is_at_tail() {
            self.transcript_scroll = TranscriptScroll::to_bottom();
        }

        // Clear pending scroll delta
        self.pending_scroll_delta = 0;

        // Clear selection (endpoints may be invalid at new width)
        self.transcript_selection.clear();

        // Clear stale layout info
        self.last_transcript_area = None;
        self.last_transcript_top = 0;
        self.last_transcript_visible = 0;
        self.last_transcript_total = 0;
        self.last_transcript_padding_top = 0;

        // Mark history updated to force cache rebuild
        self.mark_history_updated();
    }

    pub fn cursor_byte_index(&self) -> usize {
        byte_index_at_char(&self.input, self.cursor_position)
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.selected_attachment_index = None;
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert_str(byte_index, text);
        self.cursor_position = cursor + char_count(text);
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
    }

    pub fn insert_paste_text(&mut self, text: &str) {
        if let Some(pending) = self.paste_burst.flush_before_modified_input() {
            self.insert_str(&pending);
        }
        let normalized = normalize_paste_text(text);
        if !normalized.is_empty() {
            self.insert_str(&normalized);
        }
        self.paste_burst.clear_after_explicit_paste();
    }

    pub fn insert_media_attachment(&mut self, kind: &str, path: &Path, description: Option<&str>) {
        let reference = media_attachment_reference(kind, path, description);
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        let needs_prefix_newline = self.input[..byte_index]
            .chars()
            .last()
            .is_some_and(|ch| !ch.is_whitespace());
        let needs_suffix_newline = self.input[byte_index..]
            .chars()
            .next()
            .is_some_and(|ch| !ch.is_whitespace());

        let mut inserted = String::new();
        if needs_prefix_newline {
            inserted.push('\n');
        }
        inserted.push_str(&reference);
        if needs_suffix_newline || self.input[byte_index..].is_empty() {
            inserted.push('\n');
        }
        self.insert_str(&inserted);
        self.paste_burst.clear_after_explicit_paste();
    }

    pub fn composer_attachment_count(&self) -> usize {
        crate::tui::file_mention::media_attachment_references(&self.input).len()
    }

    pub fn selected_composer_attachment_index(&self) -> Option<usize> {
        let count = self.composer_attachment_count();
        self.selected_attachment_index
            .filter(|index| *index < count)
    }

    pub fn select_previous_composer_attachment(&mut self) -> bool {
        let count = self.composer_attachment_count();
        if count == 0 {
            self.selected_attachment_index = None;
            return false;
        }

        let next = self
            .selected_composer_attachment_index()
            .map_or(count.saturating_sub(1), |index| index.saturating_sub(1));
        self.selected_attachment_index = Some(next);
        self.cursor_position = 0;
        self.status_message = Some("Attachment selected - Backspace/Delete removes it".to_string());
        self.needs_redraw = true;
        true
    }

    pub fn select_next_composer_attachment(&mut self) -> bool {
        let count = self.composer_attachment_count();
        let Some(index) = self.selected_composer_attachment_index() else {
            return false;
        };
        if index + 1 < count {
            self.selected_attachment_index = Some(index + 1);
            self.status_message =
                Some("Attachment selected - Backspace/Delete removes it".to_string());
        } else {
            self.selected_attachment_index = None;
            self.status_message = Some("Composer focused".to_string());
        }
        self.needs_redraw = true;
        true
    }

    pub fn clear_composer_attachment_selection(&mut self) -> bool {
        if self.selected_attachment_index.take().is_some() {
            self.status_message = Some("Composer focused".to_string());
            self.needs_redraw = true;
            true
        } else {
            false
        }
    }

    pub fn remove_selected_composer_attachment(&mut self) -> bool {
        let references = crate::tui::file_mention::media_attachment_references(&self.input);
        let Some(index) = self
            .selected_composer_attachment_index()
            .filter(|index| *index < references.len())
        else {
            self.selected_attachment_index = None;
            return false;
        };
        let reference = references[index].clone();
        let cursor_byte = byte_index_at_char(&self.input, self.cursor_position);
        let new_cursor_byte = if cursor_byte <= reference.start_byte {
            cursor_byte
        } else if cursor_byte >= reference.end_byte {
            cursor_byte.saturating_sub(reference.end_byte - reference.start_byte)
        } else {
            reference.start_byte
        };

        self.input
            .replace_range(reference.start_byte..reference.end_byte, "");
        self.cursor_position = self.input[..new_cursor_byte.min(self.input.len())]
            .chars()
            .count();
        let remaining = self.composer_attachment_count();
        self.selected_attachment_index = if remaining == 0 {
            None
        } else {
            Some(index.min(remaining.saturating_sub(1)))
        };
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.status_message = Some(format!("Removed attachment: {}", reference.path));
        self.needs_redraw = true;
        true
    }

    pub fn flush_paste_burst_if_due(&mut self, now: Instant) -> bool {
        match self.paste_burst.flush_if_due(now) {
            FlushResult::Paste(text) => {
                self.insert_str(&text);
                true
            }
            FlushResult::Typed(ch) => {
                self.insert_char(ch);
                true
            }
            FlushResult::None => false,
        }
    }

    pub fn flush_paste_burst_if_enabled(&mut self, now: Instant) -> bool {
        self.use_paste_burst_detection && self.flush_paste_burst_if_due(now)
    }

    pub fn paste_burst_next_flush_delay_if_enabled(&self, now: Instant) -> Option<Duration> {
        if self.use_paste_burst_detection {
            self.paste_burst.next_flush_delay(now)
        } else {
            None
        }
    }

    pub fn flush_paste_burst_before_modified_input_if_enabled(&mut self) -> Option<String> {
        if self.use_paste_burst_detection {
            self.paste_burst.flush_before_modified_input()
        } else {
            None
        }
    }

    pub fn insert_api_key_char(&mut self, c: char) {
        let cursor = self.api_key_cursor.min(char_count(&self.api_key_input));
        let byte_index = byte_index_at_char(&self.api_key_input, cursor);
        self.api_key_input.insert(byte_index, c);
        self.api_key_cursor = cursor + 1;
    }

    pub fn insert_api_key_str(&mut self, text: &str) {
        let sanitized = sanitize_api_key_text(text);
        if sanitized.is_empty() {
            return;
        }
        let cursor = self.api_key_cursor.min(char_count(&self.api_key_input));
        let byte_index = byte_index_at_char(&self.api_key_input, cursor);
        self.api_key_input.insert_str(byte_index, &sanitized);
        self.api_key_cursor = cursor + char_count(&sanitized);
    }

    pub fn delete_api_key_char(&mut self) {
        if self.api_key_cursor == 0 {
            return;
        }
        let target = self.api_key_cursor.saturating_sub(1);
        if remove_char_at(&mut self.api_key_input, target) {
            self.api_key_cursor = target;
        }
    }

    /// Paste from clipboard into input
    pub fn paste_from_clipboard(&mut self) {
        if let Some(content) = self.clipboard.read(self.workspace.as_path()) {
            self.apply_clipboard_content(content);
        }
    }

    pub fn apply_clipboard_content(&mut self, content: ClipboardContent) {
        match content {
            ClipboardContent::Text(text) => {
                self.insert_paste_text(&text);
            }
            ClipboardContent::Image(pasted) => {
                let description = format!("{} ({})", pasted.short_label(), pasted.size_label());
                self.insert_media_attachment("image", &pasted.path, Some(&description));
                self.status_message = Some(format!("Attached image: {description}"));
            }
        }
    }

    pub fn paste_api_key_from_clipboard(&mut self) {
        if let Some(ClipboardContent::Text(text)) = self.clipboard.read(self.workspace.as_path()) {
            self.insert_api_key_str(&text);
        }
    }

    pub fn scroll_up(&mut self, amount: usize) {
        let delta = i32::try_from(amount).unwrap_or(i32::MAX);
        self.pending_scroll_delta = self.pending_scroll_delta.saturating_sub(delta);
        // Sticky intent: once the user has scrolled up during a stream, they
        // shouldn't be yanked back to the live tail by subsequent chunks.
        // Cleared when they explicitly return to bottom or the stream ends.
        self.user_scrolled_during_stream = true;
        self.needs_redraw = true;
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let delta = i32::try_from(amount).unwrap_or(i32::MAX);
        self.pending_scroll_delta = self.pending_scroll_delta.saturating_add(delta);
        self.user_scrolled_during_stream = true;
        self.needs_redraw = true;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.transcript_scroll = TranscriptScroll::to_bottom();
        self.pending_scroll_delta = 0;
        // Explicit return-to-tail clears the stream-lock; new chunks will
        // again pull the view down with them.
        self.user_scrolled_during_stream = false;
        self.needs_redraw = true;
    }

    pub fn insert_char(&mut self, c: char) {
        self.selected_attachment_index = None;
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert(byte_index, c);
        self.cursor_position = cursor + 1;
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
    }

    pub fn delete_char(&mut self) {
        self.selected_attachment_index = None;
        if self.cursor_position == 0 {
            return;
        }
        let target = self.cursor_position.saturating_sub(1);
        let removed = remove_char_at(&mut self.input, target);
        if removed {
            self.cursor_position = target;
            self.slash_menu_hidden = false;
            self.mention_menu_hidden = false;
            self.mention_menu_selected = 0;
            self.needs_redraw = true;
        }
    }

    pub fn delete_char_forward(&mut self) {
        self.selected_attachment_index = None;
        if self.input.is_empty() {
            return;
        }
        let target = self.cursor_position;
        let removed = remove_char_at(&mut self.input, target);
        if !removed {
            self.cursor_position = char_count(&self.input);
        }
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
    }

    /// Cut from the cursor to the end of the current logical line into the
    /// kill buffer. If the cursor is already at end-of-line and a trailing
    /// newline exists, that newline is consumed so repeated invocations
    /// continue to make progress (matching emacs/codex semantics).
    ///
    /// Returns `true` when bytes were moved into the kill buffer.
    pub fn kill_to_end_of_line(&mut self) -> bool {
        let total_chars = char_count(&self.input);
        let cursor = self.cursor_position.min(total_chars);
        let start_byte = byte_index_at_char(&self.input, cursor);

        // Find the byte offset of the next '\n' (relative to the whole string)
        // or the end of the buffer if no newline exists at/after the cursor.
        let eol_byte = self.input[start_byte..]
            .find('\n')
            .map(|rel| start_byte + rel)
            .unwrap_or_else(|| self.input.len());

        let end_byte = if start_byte == eol_byte {
            // Cursor is at EOL — consume the newline itself if one is there.
            if eol_byte < self.input.len() {
                eol_byte + 1
            } else {
                return false;
            }
        } else {
            eol_byte
        };

        let removed: String = self.input[start_byte..end_byte].to_string();
        if removed.is_empty() {
            return false;
        }

        self.kill_buffer = removed;
        self.input.replace_range(start_byte..end_byte, "");
        // Cursor stays at the same character index (start of removed range).
        self.cursor_position = cursor;
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
        true
    }

    /// Insert the contents of the kill buffer at the cursor, advancing it.
    /// The kill buffer is left intact so multiple yanks duplicate the text.
    /// Returns `true` if any text was inserted.
    pub fn yank(&mut self) -> bool {
        if self.kill_buffer.is_empty() {
            return false;
        }
        let text = self.kill_buffer.clone();
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert_str(byte_index, &text);
        self.cursor_position = cursor + char_count(&text);
        self.slash_menu_hidden = false;
        self.mention_menu_hidden = false;
        self.mention_menu_selected = 0;
        self.needs_redraw = true;
        true
    }

    pub fn move_cursor_left(&mut self) {
        self.cursor_position = self.cursor_position.saturating_sub(1);
        self.needs_redraw = true;
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < char_count(&self.input) {
            self.cursor_position += 1;
            self.needs_redraw = true;
        }
    }

    pub fn move_cursor_start(&mut self) {
        self.cursor_position = 0;
        self.needs_redraw = true;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = char_count(&self.input);
        self.needs_redraw = true;
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
        self.selected_attachment_index = None;
        self.slash_menu_selected = 0;
        self.slash_menu_hidden = false;
        self.paste_burst.clear_after_explicit_paste();
        self.needs_redraw = true;
    }

    pub fn clear_input_recoverable(&mut self) {
        self.stash_current_input_for_recovery();
        self.clear_input();
    }

    pub fn stash_current_input_for_recovery(&mut self) {
        let draft = self.input.clone();
        self.remember_draft_for_recovery(draft);
    }

    fn remember_draft_for_recovery(&mut self, draft: String) {
        if draft.trim().is_empty() {
            return;
        }
        self.draft_history.retain(|existing| existing != &draft);
        self.draft_history.push_back(draft);
        while self.draft_history.len() > MAX_DRAFT_HISTORY {
            let _ = self.draft_history.pop_front();
        }
    }

    pub fn start_history_search(&mut self) {
        if self.composer_history_search.is_some() {
            return;
        }
        self.composer_history_search = Some(ComposerHistorySearch::new(
            self.input.clone(),
            self.cursor_position,
        ));
        self.slash_menu_hidden = true;
        self.mention_menu_hidden = true;
        self.paste_burst.clear_after_explicit_paste();
        self.status_message = Some("History search: type to filter, Enter accepts".to_string());
        self.needs_redraw = true;
    }

    pub fn is_history_search_active(&self) -> bool {
        self.composer_history_search.is_some()
    }

    pub fn history_search_query(&self) -> Option<&str> {
        self.composer_history_search
            .as_ref()
            .map(|search| search.query.as_str())
    }

    pub fn history_search_selected_index(&self) -> usize {
        self.composer_history_search
            .as_ref()
            .map_or(0, |search| search.selected)
    }

    pub fn composer_display_input(&self) -> &str {
        self.history_search_query().unwrap_or(&self.input)
    }

    pub fn composer_display_cursor(&self) -> usize {
        self.composer_history_search
            .as_ref()
            .map_or(self.cursor_position, |search| char_count(&search.query))
    }

    pub fn history_search_matches(&self) -> Vec<String> {
        let Some(query) = self.history_search_query() else {
            return Vec::new();
        };
        self.history_search_matches_for_query(query)
    }

    fn history_search_matches_for_query(&self, query: &str) -> Vec<String> {
        let normalized_query = query.trim().to_lowercase();
        let mut seen: HashSet<&str> = HashSet::new();
        let mut matches = Vec::new();

        for candidate in self
            .draft_history
            .iter()
            .rev()
            .chain(self.input_history.iter().rev())
        {
            if candidate.trim().is_empty() || !seen.insert(candidate.as_str()) {
                continue;
            }
            if normalized_query.is_empty() || candidate.to_lowercase().contains(&normalized_query) {
                matches.push(candidate.clone());
            }
        }

        matches
    }

    fn clamp_history_search_selection(&mut self) {
        let Some(search) = self.composer_history_search.as_ref() else {
            return;
        };
        let selected = search.selected;
        let query = search.query.clone();
        let match_count = self.history_search_matches_for_query(&query).len();
        if let Some(search) = self.composer_history_search.as_mut() {
            search.selected = if match_count == 0 {
                0
            } else {
                selected.min(match_count.saturating_sub(1))
            };
        }
    }

    pub fn history_search_insert_char(&mut self, ch: char) {
        if let Some(search) = self.composer_history_search.as_mut() {
            search.query.push(ch);
            search.selected = 0;
            self.status_message = Some("History search: Enter accepts, Esc restores".to_string());
            self.needs_redraw = true;
        }
    }

    pub fn history_search_insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(search) = self.composer_history_search.as_mut() {
            search.query.push_str(&normalize_paste_text(text));
            search.selected = 0;
            self.status_message = Some("History search: Enter accepts, Esc restores".to_string());
            self.needs_redraw = true;
        }
    }

    pub fn history_search_backspace(&mut self) {
        if let Some(search) = self.composer_history_search.as_mut() {
            search.query.pop();
            search.selected = 0;
            self.needs_redraw = true;
        }
        self.clamp_history_search_selection();
    }

    pub fn history_search_select_previous(&mut self) {
        if let Some(search) = self.composer_history_search.as_mut() {
            search.selected = search.selected.saturating_sub(1);
            self.needs_redraw = true;
        }
    }

    pub fn history_search_select_next(&mut self) {
        let Some(search) = self.composer_history_search.as_ref() else {
            return;
        };
        let query = search.query.clone();
        let selected = search.selected;
        let match_count = self.history_search_matches_for_query(&query).len();
        if let Some(search) = self.composer_history_search.as_mut()
            && match_count > 0
        {
            search.selected = (selected + 1).min(match_count.saturating_sub(1));
            self.needs_redraw = true;
        }
    }

    pub fn accept_history_search(&mut self) -> bool {
        let Some(search) = self.composer_history_search.take() else {
            return false;
        };
        let matches = self.history_search_matches_for_query(&search.query);
        if let Some(selected) = matches
            .get(search.selected.min(matches.len().saturating_sub(1)))
            .cloned()
        {
            self.input = selected;
            self.cursor_position = char_count(&self.input);
            self.history_index = None;
            self.status_message = Some("History match inserted into composer".to_string());
            self.needs_redraw = true;
            true
        } else {
            self.composer_history_search = Some(search);
            self.status_message = Some("No history matches".to_string());
            self.needs_redraw = true;
            false
        }
    }

    pub fn cancel_history_search(&mut self) {
        let Some(search) = self.composer_history_search.take() else {
            return;
        };
        self.input = search.pre_search_input;
        self.cursor_position = search.pre_search_cursor.min(char_count(&self.input));
        self.status_message = Some("History search canceled".to_string());
        self.needs_redraw = true;
    }

    pub fn submit_input(&mut self) -> Option<String> {
        if self.input.trim().is_empty() {
            self.paste_burst.clear_after_explicit_paste();
            return None;
        }
        let mut input = self.input.clone();
        if char_count(&input) > MAX_SUBMITTED_INPUT_CHARS {
            input = input.chars().take(MAX_SUBMITTED_INPUT_CHARS).collect();
            self.status_message = Some(format!(
                "Input truncated to {} characters for safety",
                MAX_SUBMITTED_INPUT_CHARS
            ));
        }
        if !input.starts_with('/') {
            self.input_history.push(input.clone());
            if self.max_input_history == 0 {
                self.input_history.clear();
            } else if self.input_history.len() > self.max_input_history {
                let excess = self.input_history.len() - self.max_input_history;
                self.input_history.drain(0..excess);
            }
        }
        self.history_index = None;
        self.clear_input();
        Some(input)
    }

    pub fn queue_message(&mut self, message: QueuedMessage) {
        self.queued_messages.push_back(message);
    }

    pub fn pop_queued_message(&mut self) -> Option<QueuedMessage> {
        self.queued_messages.pop_front()
    }

    pub fn remove_queued_message(&mut self, index: usize) -> Option<QueuedMessage> {
        self.queued_messages.remove(index)
    }

    pub fn queued_message_count(&self) -> usize {
        self.queued_messages.len()
    }

    /// Pop the most-recently queued message back into the composer for editing
    /// (issue #85 — Alt+↑ affordance). The popped message is parked in
    /// [`Self::queued_draft`] so the next Enter re-queues it carrying its
    /// original skill instruction. No-op if the composer already has typed
    /// content or a draft is already being edited — surfacing the affordance
    /// would be ambiguous in either case.
    ///
    /// Returns `true` when the composer state was mutated.
    pub fn pop_last_queued_into_draft(&mut self) -> bool {
        if !self.input.is_empty() || self.queued_draft.is_some() {
            return false;
        }
        let Some(msg) = self.queued_messages.pop_back() else {
            return false;
        };
        self.input = msg.display.clone();
        self.cursor_position = char_count(&self.input);
        self.selected_attachment_index = None;
        self.queued_draft = Some(msg);
        self.needs_redraw = true;
        true
    }

    /// Park a legacy pending steer. New keyboard handling routes running-turn
    /// drafts through Enter (same-turn steer) or Tab (next-turn follow-up).
    #[allow(dead_code)]
    pub fn push_pending_steer(&mut self, message: QueuedMessage) {
        self.pending_steers.push_back(message);
        self.submit_pending_steers_after_interrupt = true;
        self.needs_redraw = true;
    }

    /// Drain the pending-steer queue and clear the resend flag. Returns the
    /// messages in submit order (oldest first).
    pub fn drain_pending_steers(&mut self) -> Vec<QueuedMessage> {
        self.submit_pending_steers_after_interrupt = false;
        if self.pending_steers.is_empty() {
            return Vec::new();
        }
        self.needs_redraw = true;
        self.pending_steers.drain(..).collect()
    }

    /// Decide how to route a fresh composer submit.
    ///
    /// Truth table:
    ///   offline=F, busy=F → Immediate
    ///   offline=F, busy=T, streaming → QueueFollowUp
    ///   offline=F, busy=T, not streaming → Steer
    ///   offline=T, busy=F → Queue
    ///   offline=T, busy=T → Queue
    #[must_use]
    pub fn decide_submit_disposition(&self) -> SubmitDisposition {
        if self.offline_mode {
            return SubmitDisposition::Queue;
        }
        if !self.is_loading {
            return SubmitDisposition::Immediate;
        }
        // Busy + streaming text: queue for after TurnComplete.
        // Busy + not streaming (tool execution): forward as a steer.
        if self.streaming_message_index.is_some() {
            SubmitDisposition::QueueFollowUp
        } else {
            SubmitDisposition::Steer
        }
    }

    /// Mark the in-flight streaming Assistant cell as interrupted: prepend
    /// `[interrupted]` to whatever streamed so far (so the user can see what
    /// was salvaged) and flip `streaming` off so the spinner halts. No-op if
    /// no Assistant cell is currently streaming.
    ///
    /// Deliberate divergence from openai/codex which discards partial output
    /// on abort — V4 thinking is expensive and the user usually wants to see
    /// what the model produced before steering.
    pub fn finalize_streaming_assistant_as_interrupted(&mut self) {
        let Some(index) = self.streaming_message_index.take() else {
            return;
        };
        if let Some(HistoryCell::Assistant { content, streaming }) = self.history.get_mut(index) {
            *streaming = false;
            if content.is_empty() {
                *content = "[interrupted]".to_string();
            } else if !content.starts_with("[interrupted]") {
                content.insert_str(0, "[interrupted] ");
            }
        }
        self.bump_history_cell(index);
    }

    pub fn history_up(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        let new_index = match self.history_index {
            None => self.input_history.len().saturating_sub(1),
            Some(i) => i.saturating_sub(1),
        };
        self.history_index = Some(new_index);
        self.input = self.input_history[new_index].clone();
        self.cursor_position = char_count(&self.input);
        self.selected_attachment_index = None;
        self.slash_menu_hidden = false;
        self.paste_burst.clear_after_explicit_paste();
    }

    pub fn history_down(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        match self.history_index {
            None => {}
            Some(i) => {
                if i + 1 < self.input_history.len() {
                    self.history_index = Some(i + 1);
                    self.input = self.input_history[i + 1].clone();
                    self.cursor_position = char_count(&self.input);
                    self.selected_attachment_index = None;
                    self.slash_menu_hidden = false;
                    self.paste_burst.clear_after_explicit_paste();
                } else {
                    self.history_index = None;
                    self.clear_input();
                }
            }
        }
    }

    pub fn clear_todos(&mut self) -> bool {
        if let Ok(mut plan) = self.plan_state.try_lock() {
            *plan = crate::tools::plan::PlanState::default();
            return true;
        }
        false
    }

    pub fn update_model_compaction_budget(&mut self) {
        self.compact_threshold = compaction_threshold_for_model_and_effort(
            &self.model,
            self.reasoning_effort.api_value(),
        );
    }

    pub fn compaction_config(&self) -> CompactionConfig {
        CompactionConfig {
            enabled: self.auto_compact,
            token_threshold: self.compact_threshold,
            message_threshold: compaction_message_threshold_for_model(&self.model),
            model: self.model.clone(),
            ..Default::default()
        }
    }

    /// Forward the active cycle configuration to the engine. Cloned so the
    /// engine has its own copy to mutate per-session.
    pub fn cycle_config(&self) -> CycleConfig {
        self.cycle.clone()
    }
}

pub fn media_attachment_reference(kind: &str, path: &Path, description: Option<&str>) -> String {
    match description {
        Some(description) if !description.trim().is_empty() => {
            format!(
                "[Attached {kind}: {} at {}]",
                description.trim(),
                path.display()
            )
        }
        _ => format!("[Attached {kind}: {}]", path.display()),
    }
}

// === Actions ===

/// Actions emitted by the UI event loop.
#[derive(Debug, Clone, PartialEq)]
pub enum AppAction {
    Quit,
    #[allow(dead_code)] // For explicit /save command
    SaveSession(PathBuf),
    #[allow(dead_code)] // For explicit /load command
    LoadSession(PathBuf),
    SyncSession {
        messages: Vec<Message>,
        system_prompt: Option<SystemPrompt>,
        model: String,
        workspace: PathBuf,
    },
    OpenConfigView,
    /// Open the `/model` two-pane picker (Pro/Flash + Off/High/Max).
    OpenModelPicker,
    /// Open the `/provider` picker modal — DeepSeek / NVIDIA NIM / OpenRouter
    /// / Novita with inline API-key prompt for un-configured providers (#52).
    OpenProviderPicker,
    /// Open the `/statusline` multi-select picker for footer items.
    OpenStatusPicker,
    /// Send a message to the AI (normal chat mode).
    SendMessage(String),
    /// Run a Recursive Language Model (RLM) turn — Algorithm 1 from
    /// Zhang et al. (arXiv:2512.24601). The prompt is stored in the REPL;
    /// the root LLM only sees metadata.
    Rlm {
        /// The user's prompt — stored in REPL, NOT in LLM context.
        prompt: String,
        /// Model for the root LLM.
        model: String,
        /// Model for sub-LLM (llm_query) calls.
        child_model: String,
        /// Recursion budget for `sub_rlm()` calls.
        max_depth: u32,
    },
    ListSubAgents,
    FetchModels,
    /// Switch the active LLM backend (DeepSeek vs NVIDIA NIM) without
    /// restarting the process. The runtime rebuilds its API client from
    /// the updated config. `model` overrides the post-switch model
    /// (already normalized but not yet provider-prefixed).
    SwitchProvider {
        provider: ApiProvider,
        model: Option<String>,
    },
    UpdateCompaction(CompactionConfig),
    OpenContextInspector,
    CompactContext,
    TaskAdd {
        prompt: String,
    },
    TaskList,
    TaskShow {
        id: String,
    },
    TaskCancel {
        id: String,
    },
    ShellJob(ShellJobAction),
    Mcp(McpUiAction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellJobAction {
    List,
    Show {
        id: String,
    },
    Poll {
        id: String,
        wait: bool,
    },
    SendStdin {
        id: String,
        input: String,
        close: bool,
    },
    Cancel {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpUiAction {
    Show,
    Init {
        force: bool,
    },
    AddStdio {
        name: String,
        command: String,
        args: Vec<String>,
    },
    AddHttp {
        name: String,
        url: String,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    Remove {
        name: String,
    },
    Validate,
    Reload,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tools::plan::{PlanItemArg, StepStatus, UpdatePlanArgs};
    use crate::tui::clipboard::PastedImage;

    fn test_options(yolo: bool) -> TuiOptions {
        TuiOptions {
            model: "test-model".to_string(),
            workspace: PathBuf::from("."),
            allow_shell: yolo,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: yolo,
            skip_onboarding: false,
            yolo,
            resume_session_id: None,
        }
    }

    #[test]
    fn test_trust_mode_follows_yolo_on_startup() {
        let app = App::new(test_options(true), &Config::default());
        assert!(app.trust_mode);
    }

    #[test]
    fn submit_input_truncates_oversized_payloads() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input = "x".repeat(MAX_SUBMITTED_INPUT_CHARS + 128);
        app.cursor_position = app.input.chars().count();

        let submitted = app.submit_input().expect("expected submitted input");
        assert_eq!(submitted.chars().count(), MAX_SUBMITTED_INPUT_CHARS);
        assert!(
            app.status_message
                .as_ref()
                .is_some_and(|msg| msg.contains("Input truncated"))
        );
    }

    #[test]
    fn app_starts_without_seeded_transcript_messages() {
        let app = App::new(test_options(false), &Config::default());
        assert!(app.history.is_empty());
        assert_eq!(app.history_version, 0);
    }

    #[test]
    fn clear_todos_resets_plan_state() {
        let mut app = App::new(test_options(false), &Config::default());

        {
            let mut plan = app
                .plan_state
                .try_lock()
                .expect("plan lock should be available");
            plan.update(UpdatePlanArgs {
                explanation: Some("test plan".to_string()),
                plan: vec![PlanItemArg {
                    step: "step 1".to_string(),
                    status: StepStatus::InProgress,
                }],
            });
            assert!(!plan.is_empty());
        }

        assert!(app.clear_todos());

        let plan = app
            .plan_state
            .try_lock()
            .expect("plan lock should be available");
        assert!(plan.is_empty());
    }

    #[test]
    fn test_cycle_mode_transitions() {
        let mut app = App::new(test_options(false), &Config::default());
        // Default mode should be Agent based on settings
        let initial_mode = app.mode;
        app.cycle_mode();
        // Mode should have changed
        assert_ne!(app.mode, initial_mode);
    }

    #[test]
    fn test_cycle_mode_reverse_transitions() {
        let mut app = App::new(test_options(false), &Config::default());

        app.mode = AppMode::Plan;
        app.cycle_mode_reverse();
        assert_eq!(app.mode, AppMode::Yolo);

        app.mode = AppMode::Agent;
        app.cycle_mode_reverse();
        assert_eq!(app.mode, AppMode::Plan);
    }

    #[test]
    fn test_clear_input() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input = "test input".to_string();
        app.cursor_position = app.input.len();
        app.clear_input();
        assert!(app.input.is_empty());
        assert_eq!(app.cursor_position, 0);
    }

    #[test]
    fn test_queue_message() {
        let mut app = App::new(test_options(false), &Config::default());
        app.queue_message(QueuedMessage::new("test message".to_string(), None));
        assert_eq!(app.queued_message_count(), 1);
        assert!(app.queued_messages.front().is_some());
    }

    #[test]
    fn test_remove_queued_message() {
        let mut app = App::new(test_options(false), &Config::default());
        app.queue_message(QueuedMessage::new("first".to_string(), None));
        app.queue_message(QueuedMessage::new("second".to_string(), None));

        // Remove first (index 0)
        let removed = app.remove_queued_message(0);
        assert!(removed.is_some());
        assert_eq!(app.queued_message_count(), 1);

        // Remove second (now at index 0)
        let removed = app.remove_queued_message(0);
        assert!(removed.is_some());
        assert_eq!(app.queued_message_count(), 0);
    }

    #[test]
    fn test_remove_queued_message_invalid_index() {
        let mut app = App::new(test_options(false), &Config::default());
        app.queue_message(QueuedMessage::new("test".to_string(), None));

        // Try to remove non-existent index
        let removed = app.remove_queued_message(100);
        assert!(removed.is_none());
    }

    #[test]
    fn test_set_mode_updates_state() {
        let mut app = App::new(test_options(false), &Config::default());
        let initial_mode = app.mode;
        app.set_mode(AppMode::Yolo);
        assert_eq!(app.mode, AppMode::Yolo);
        assert_ne!(app.mode, initial_mode);
        // Yolo mode should enable trust and shell
        assert!(app.trust_mode);
        assert!(app.allow_shell);
    }

    #[test]
    fn app_new_respects_allow_shell_option_when_not_yolo() {
        let mut options = test_options(false);
        options.allow_shell = false;
        options.start_in_agent_mode = true; // avoid coupling to settings.default_mode
        let app = App::new(options, &Config::default());
        assert!(!app.allow_shell);
    }

    #[test]
    fn set_mode_yolo_restores_previous_policies_on_exit() {
        let mut options = test_options(false);
        options.allow_shell = false;
        options.start_in_agent_mode = true; // avoid coupling to settings.default_mode
        let mut app = App::new(options, &Config::default());
        app.allow_shell = false;
        app.trust_mode = false;
        app.approval_mode = ApprovalMode::Never;

        app.set_mode(AppMode::Yolo);
        assert!(app.allow_shell);
        assert!(app.trust_mode);
        assert_eq!(app.approval_mode, ApprovalMode::Auto);

        app.set_mode(AppMode::Agent);
        assert!(!app.allow_shell);
        assert!(!app.trust_mode);
        assert_eq!(app.approval_mode, ApprovalMode::Never);
    }

    #[test]
    fn leaving_yolo_after_startup_restores_baseline_policies() {
        let config = Config {
            allow_shell: Some(false),
            ..Default::default()
        };

        let mut app = App::new(test_options(true), &config);
        assert_eq!(app.mode, AppMode::Yolo);
        assert!(app.allow_shell);
        assert!(app.trust_mode);
        assert_eq!(app.approval_mode, ApprovalMode::Auto);

        app.set_mode(AppMode::Agent);
        assert!(!app.allow_shell);
        assert!(!app.trust_mode);
        assert_eq!(app.approval_mode, ApprovalMode::Suggest);
    }

    #[test]
    fn test_mark_history_updated() {
        let mut app = App::new(test_options(false), &Config::default());
        let initial_version = app.history_version;
        app.mark_history_updated();
        assert!(app.history_version > initial_version);
    }

    #[test]
    fn test_scroll_operations() {
        let mut app = App::new(test_options(false), &Config::default());
        // Just verify scroll methods can be called without panic
        app.scroll_up(5);
        app.scroll_down(3);
    }

    #[test]
    fn test_add_message() {
        let mut app = App::new(test_options(false), &Config::default());
        let initial_len = app.history.len();
        app.add_message(HistoryCell::User {
            content: "test".to_string(),
        });
        assert_eq!(app.history.len(), initial_len + 1);
    }

    #[test]
    fn test_compaction_config() {
        let app = App::new(test_options(false), &Config::default());
        let config = app.compaction_config();
        // Config should be valid (just checking it returns something)
        let _ = config.enabled;
    }

    #[test]
    fn test_update_model_compaction_budget() {
        let mut app = App::new(test_options(false), &Config::default());
        app.model = "unknown-test-model".to_string();
        app.update_model_compaction_budget();
        let initial_threshold = app.compact_threshold;
        app.model = "deepseek-v3.2-128k".to_string();
        app.update_model_compaction_budget();
        // Threshold may have changed based on model
        // Explicit 128k DeepSeek model IDs have a higher threshold than unknown models.
        assert!(app.compact_threshold >= initial_threshold);
    }

    #[test]
    fn test_input_history_navigation() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input_history.push("first".to_string());
        app.input_history.push("second".to_string());

        // Navigate up
        app.history_up();
        assert!(app.history_index.is_some());

        // Navigate down
        app.history_down();
    }

    #[test]
    fn history_search_filters_matches_and_skips_duplicates() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input_history.push("alpha one".to_string());
        app.input_history.push("beta two".to_string());
        app.input_history.push("alpha one".to_string());
        app.draft_history.push_back("draft alpha".to_string());

        app.start_history_search();
        app.history_search_insert_str("alpha");

        assert_eq!(
            app.history_search_matches(),
            vec!["draft alpha".to_string(), "alpha one".to_string()]
        );
    }

    #[test]
    fn history_search_matches_unicode_case_insensitively() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input_history.push("CAFÉ prompt".to_string());

        app.start_history_search();
        app.history_search_insert_str("café");

        assert_eq!(
            app.history_search_matches(),
            vec!["CAFÉ prompt".to_string()]
        );
    }

    #[test]
    fn history_search_accepts_match_without_submitting() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input_history.push("older prompt".to_string());

        app.start_history_search();
        app.history_search_insert_str("older");

        assert!(app.accept_history_search());
        assert_eq!(app.input, "older prompt");
        assert_eq!(app.cursor_position, "older prompt".chars().count());
        assert!(app.composer_history_search.is_none());
    }

    #[test]
    fn history_search_cancel_restores_pre_search_draft() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input = "current draft".to_string();
        app.cursor_position = 7;
        app.input_history.push("older prompt".to_string());

        app.start_history_search();
        app.history_search_insert_str("older");
        app.cancel_history_search();

        assert_eq!(app.input, "current draft");
        assert_eq!(app.cursor_position, 7);
        assert!(app.composer_history_search.is_none());
    }

    #[test]
    fn recoverable_clear_stashes_nonempty_draft() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input = "recover this".to_string();
        app.cursor_position = app.input.chars().count();

        app.clear_input_recoverable();
        app.start_history_search();
        app.history_search_insert_str("recover");

        assert_eq!(
            app.history_search_matches(),
            vec!["recover this".to_string()]
        );
    }

    #[test]
    fn composer_paste_flushes_pending_burst_and_normalizes_crlf() {
        let mut app = App::new(test_options(false), &Config::default());
        app.use_paste_burst_detection = true;
        let now = Instant::now();
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::NONE,
        );

        assert!(crate::tui::paste::handle_paste_burst_key(
            &mut app, &key, now
        ));
        assert!(
            app.input.is_empty(),
            "first burst char should stay buffered"
        );

        app.insert_paste_text("a\r\nb\rc");

        assert_eq!(app.input, "xa\nbc");
        assert_eq!(app.cursor_position, "xa\nbc".chars().count());
        assert!(!app.paste_burst.is_active());
    }

    #[test]
    fn clipboard_text_paste_matches_bracketed_paste_state() {
        let text = "alpha\r\nbeta";
        let mut bracketed = App::new(test_options(false), &Config::default());
        let mut clipboard = App::new(test_options(false), &Config::default());

        bracketed.insert_paste_text(text);
        clipboard.apply_clipboard_content(ClipboardContent::Text(text.to_string()));

        assert_eq!(clipboard.input, bracketed.input);
        assert_eq!(clipboard.cursor_position, bracketed.cursor_position);
        assert_eq!(clipboard.slash_menu_hidden, bracketed.slash_menu_hidden);
        assert_eq!(clipboard.mention_menu_hidden, bracketed.mention_menu_hidden);
    }

    #[test]
    fn clipboard_image_paste_keeps_adjacent_text_and_concise_status() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input = "before after".to_string();
        app.cursor_position = "before".chars().count();

        app.apply_clipboard_content(ClipboardContent::Image(PastedImage {
            path: PathBuf::from("/tmp/pasted.png"),
            width: 8,
            height: 4,
            byte_len: 2048,
        }));

        assert!(
            app.input
                .contains("before\n[Attached image: 8x4 PNG (2KB) at /tmp/pasted.png]")
        );
        assert!(app.input.contains("] after"));
        let status = app.status_message.as_deref().expect("status message");
        assert_eq!(status, "Attached image: 8x4 PNG (2KB)");
    }

    #[test]
    fn pasted_text_and_image_placeholders_survive_history_and_queue_paths() {
        let mut app = App::new(test_options(false), &Config::default());
        app.insert_paste_text("line 1\r\nline 2");
        app.insert_media_attachment("image", Path::new("/tmp/pasted.png"), Some("8x4 PNG (2KB)"));

        let submitted = app.submit_input().expect("submitted input");
        assert!(submitted.contains("line 1\nline 2"));
        assert!(submitted.contains("[Attached image: 8x4 PNG (2KB) at /tmp/pasted.png]"));

        app.history_up();
        assert_eq!(app.input, submitted);
        assert_eq!(app.composer_attachment_count(), 1);

        app.clear_input();
        app.queue_message(QueuedMessage::new(
            submitted.clone(),
            Some("Use this skill".to_string()),
        ));
        assert!(app.pop_last_queued_into_draft());
        assert_eq!(app.input, submitted);
        assert_eq!(app.composer_attachment_count(), 1);
        assert_eq!(
            app.queued_draft
                .as_ref()
                .and_then(|draft| draft.skill_instruction.as_deref()),
            Some("Use this skill")
        );

        app.push_pending_steer(QueuedMessage::new(submitted.clone(), None));
        let steers = app.drain_pending_steers();
        assert_eq!(steers[0].display, submitted);
    }

    #[test]
    fn selected_attachment_row_removes_placeholder_without_manual_editing() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input = "before".to_string();
        app.cursor_position = "before".chars().count();
        app.insert_media_attachment("image", Path::new("/tmp/pasted.png"), Some("8x4 PNG"));
        app.insert_str("after");

        app.move_cursor_start();
        assert!(app.select_previous_composer_attachment());
        assert_eq!(app.selected_composer_attachment_index(), Some(0));
        assert!(app.remove_selected_composer_attachment());

        assert!(!app.input.contains("[Attached image:"));
        assert!(app.input.contains("before"));
        assert!(app.input.contains("after"));
        assert_eq!(app.composer_attachment_count(), 0);
        assert!(app.selected_composer_attachment_index().is_none());
    }

    #[test]
    fn kill_to_end_of_line_cuts_from_middle_of_word() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input = "hello world".to_string();
        app.cursor_position = 6; // before 'w'
        assert!(app.kill_to_end_of_line());
        assert_eq!(app.input, "hello ");
        assert_eq!(app.cursor_position, 6);
        assert_eq!(app.kill_buffer, "world");
    }

    #[test]
    fn kill_at_eol_consumes_following_newline() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input = "line one\nline two".to_string();
        app.cursor_position = 8; // sitting on the '\n'
        assert!(app.kill_to_end_of_line());
        assert_eq!(app.input, "line oneline two");
        assert_eq!(app.cursor_position, 8);
        assert_eq!(app.kill_buffer, "\n");

        // Empty input: kill is a no-op and the buffer is untouched.
        let mut empty = App::new(test_options(false), &Config::default());
        assert!(!empty.kill_to_end_of_line());
        assert!(empty.input.is_empty());
        assert!(empty.kill_buffer.is_empty());
    }

    #[test]
    fn yank_inserts_kill_buffer_and_preserves_it() {
        let mut app = App::new(test_options(false), &Config::default());
        app.input = "abc def".to_string();
        app.cursor_position = 4; // before 'd'
        assert!(app.kill_to_end_of_line());
        assert_eq!(app.input, "abc ");
        assert_eq!(app.kill_buffer, "def");

        // Move cursor to the start and yank twice — kill_buffer must persist.
        app.cursor_position = 0;
        assert!(app.yank());
        assert!(app.yank());
        assert_eq!(app.input, "defdefabc ");
        assert_eq!(app.cursor_position, 6);
        assert_eq!(app.kill_buffer, "def");

        // Yank with empty buffer is a no-op.
        let mut empty = App::new(test_options(false), &Config::default());
        assert!(!empty.yank());
        assert!(empty.input.is_empty());
    }

    // ---- Issue #90: quit confirmation timeout ----

    #[test]
    fn quit_is_not_armed_by_default() {
        let app = App::new(test_options(false), &Config::default());
        assert!(!app.quit_is_armed());
        assert!(app.quit_armed_until.is_none());
    }

    #[test]
    fn arm_quit_sets_two_second_window() {
        let mut app = App::new(test_options(false), &Config::default());
        app.arm_quit();
        assert!(app.quit_is_armed());
        let deadline = app.quit_armed_until.expect("deadline set");
        let remaining = deadline.saturating_duration_since(Instant::now());
        // Allow a generous margin for slow CI machines: 1.5s..=2.0s.
        assert!(
            remaining >= Duration::from_millis(1500) && remaining <= Duration::from_secs(2),
            "expected ~2s window, got {remaining:?}",
        );
        assert!(app.needs_redraw, "armed prompt should request a redraw");
    }

    #[test]
    fn disarm_quit_clears_the_timer() {
        let mut app = App::new(test_options(false), &Config::default());
        app.arm_quit();
        app.needs_redraw = false;
        app.disarm_quit();
        assert!(!app.quit_is_armed());
        assert!(app.quit_armed_until.is_none());
        assert!(app.needs_redraw, "disarming should request a redraw");
    }

    #[test]
    fn disarm_quit_when_not_armed_is_a_noop() {
        let mut app = App::new(test_options(false), &Config::default());
        app.needs_redraw = false;
        app.disarm_quit();
        assert!(!app.needs_redraw, "no redraw when nothing changed");
    }

    #[test]
    fn quit_armed_expires_after_window() {
        let mut app = App::new(test_options(false), &Config::default());
        // Pin the deadline in the past to simulate a stale timer.
        app.quit_armed_until = Some(Instant::now() - Duration::from_millis(10));
        assert!(
            !app.quit_is_armed(),
            "expired timer must not count as armed"
        );

        app.needs_redraw = false;
        app.tick_quit_armed();
        assert!(app.quit_armed_until.is_none(), "tick clears expired timer");
        assert!(
            app.needs_redraw,
            "expiry triggers a redraw to repaint footer"
        );
    }

    #[test]
    fn quit_armed_tick_is_noop_within_window() {
        let mut app = App::new(test_options(false), &Config::default());
        app.arm_quit();
        app.needs_redraw = false;
        app.tick_quit_armed();
        assert!(
            app.quit_is_armed(),
            "tick within window keeps the timer armed"
        );
        assert!(!app.needs_redraw, "no redraw when nothing changed");
    }

    #[test]
    fn re_arming_after_expiry_starts_a_fresh_window() {
        let mut app = App::new(test_options(false), &Config::default());
        app.quit_armed_until = Some(Instant::now() - Duration::from_secs(5));
        app.tick_quit_armed();
        assert!(app.quit_armed_until.is_none());
        app.arm_quit();
        let deadline = app.quit_armed_until.expect("re-armed");
        assert!(deadline > Instant::now(), "fresh deadline in the future");
    }

    // ---- Issue #208: in-flight input routing ----

    #[test]
    fn submit_disposition_immediate_when_idle_and_online() {
        let app = App::new(test_options(false), &Config::default());
        assert!(!app.is_loading);
        assert!(!app.offline_mode);
        assert_eq!(
            app.decide_submit_disposition(),
            SubmitDisposition::Immediate
        );
    }

    #[test]
    fn submit_disposition_steer_when_busy_and_online_not_streaming() {
        // Busy + not streaming (tool execution phase) → Steer
        let mut app = App::new(test_options(false), &Config::default());
        app.is_loading = true;
        app.offline_mode = false;
        // streaming_message_index is None (default) → tool execution phase
        assert_eq!(app.decide_submit_disposition(), SubmitDisposition::Steer);
    }

    #[test]
    fn submit_disposition_queue_follow_up_when_streaming() {
        // Busy + actively streaming → QueueFollowUp
        let mut app = App::new(test_options(false), &Config::default());
        app.is_loading = true;
        app.offline_mode = false;
        app.streaming_message_index = Some(0);
        assert_eq!(
            app.decide_submit_disposition(),
            SubmitDisposition::QueueFollowUp
        );
    }

    #[test]
    fn submit_disposition_queue_when_offline_and_idle() {
        let mut app = App::new(test_options(false), &Config::default());
        app.is_loading = false;
        app.offline_mode = true;
        assert_eq!(app.decide_submit_disposition(), SubmitDisposition::Queue);
    }

    #[test]
    fn submit_disposition_offline_busy_queues() {
        let mut app = App::new(test_options(false), &Config::default());
        app.is_loading = true;
        app.offline_mode = true;
        // Offline mode always queues, even when streaming
        app.streaming_message_index = Some(0);
        assert_eq!(app.decide_submit_disposition(), SubmitDisposition::Queue);
    }

    #[test]
    fn push_pending_steer_arms_resend_flag() {
        let mut app = App::new(test_options(false), &Config::default());
        assert!(!app.submit_pending_steers_after_interrupt);
        app.push_pending_steer(QueuedMessage::new("steer me".to_string(), None));
        assert_eq!(app.pending_steers.len(), 1);
        assert!(app.submit_pending_steers_after_interrupt);
    }

    #[test]
    fn drain_pending_steers_clears_flag_and_returns_in_order() {
        let mut app = App::new(test_options(false), &Config::default());
        app.push_pending_steer(QueuedMessage::new("first".to_string(), None));
        app.push_pending_steer(QueuedMessage::new("second".to_string(), None));
        app.push_pending_steer(QueuedMessage::new("third".to_string(), None));

        let drained = app.drain_pending_steers();
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].display, "first");
        assert_eq!(drained[2].display, "third");
        assert!(app.pending_steers.is_empty());
        assert!(!app.submit_pending_steers_after_interrupt);
    }

    #[test]
    fn drain_pending_steers_when_empty_is_safe() {
        let mut app = App::new(test_options(false), &Config::default());
        // Flag-only set (someone armed it manually): drain still clears it.
        app.submit_pending_steers_after_interrupt = true;
        let drained = app.drain_pending_steers();
        assert!(drained.is_empty());
        assert!(!app.submit_pending_steers_after_interrupt);
    }

    #[test]
    fn double_push_pending_steer_is_idempotent_on_flag() {
        let mut app = App::new(test_options(false), &Config::default());
        app.push_pending_steer(QueuedMessage::new("a".to_string(), None));
        app.push_pending_steer(QueuedMessage::new("b".to_string(), None));
        assert!(app.submit_pending_steers_after_interrupt);
        assert_eq!(app.pending_steers.len(), 2);
    }

    #[test]
    fn pop_last_queued_into_draft_pops_back_and_arms_draft() {
        let mut app = App::new(test_options(false), &Config::default());
        app.queue_message(QueuedMessage::new(
            "first".to_string(),
            Some("skill-A".to_string()),
        ));
        app.queue_message(QueuedMessage::new(
            "last".to_string(),
            Some("skill-B".to_string()),
        ));

        assert!(app.pop_last_queued_into_draft());
        assert_eq!(app.input, "last");
        assert_eq!(app.cursor_position, "last".chars().count());
        assert_eq!(app.queued_messages.len(), 1);
        let draft = app.queued_draft.clone().expect("draft is set");
        assert_eq!(draft.display, "last");
        assert_eq!(draft.skill_instruction.as_deref(), Some("skill-B"));
    }

    #[test]
    fn pop_last_queued_into_draft_noop_when_composer_dirty() {
        let mut app = App::new(test_options(false), &Config::default());
        app.queue_message(QueuedMessage::new("queued".to_string(), None));
        app.input = "typing".to_string();
        app.cursor_position = char_count(&app.input);

        assert!(!app.pop_last_queued_into_draft());
        assert_eq!(app.input, "typing");
        assert_eq!(app.queued_messages.len(), 1);
        assert!(app.queued_draft.is_none());
    }

    #[test]
    fn pop_last_queued_into_draft_noop_when_draft_already_armed() {
        let mut app = App::new(test_options(false), &Config::default());
        app.queue_message(QueuedMessage::new("queued".to_string(), None));
        app.queued_draft = Some(QueuedMessage::new("editing".to_string(), None));

        assert!(!app.pop_last_queued_into_draft());
        assert_eq!(app.queued_messages.len(), 1);
        assert_eq!(
            app.queued_draft.as_ref().map(|d| d.display.as_str()),
            Some("editing")
        );
    }

    #[test]
    fn pop_last_queued_into_draft_noop_when_queue_empty() {
        let mut app = App::new(test_options(false), &Config::default());
        assert!(!app.pop_last_queued_into_draft());
        assert!(app.input.is_empty());
        assert!(app.queued_draft.is_none());
    }

    #[test]
    fn finalize_streaming_assistant_marks_existing_cell_interrupted() {
        let mut app = App::new(test_options(false), &Config::default());
        app.add_message(HistoryCell::Assistant {
            content: "partial reply so far".to_string(),
            streaming: true,
        });
        let idx = app.history.len() - 1;
        app.streaming_message_index = Some(idx);

        app.finalize_streaming_assistant_as_interrupted();

        assert!(app.streaming_message_index.is_none());
        match &app.history[idx] {
            HistoryCell::Assistant { content, streaming } => {
                assert!(content.starts_with("[interrupted]"), "got: {content}");
                assert!(content.contains("partial reply so far"));
                assert!(!*streaming);
            }
            other => panic!("expected Assistant cell, got {other:?}"),
        }
    }

    #[test]
    fn finalize_streaming_assistant_handles_empty_content() {
        let mut app = App::new(test_options(false), &Config::default());
        app.add_message(HistoryCell::Assistant {
            content: String::new(),
            streaming: true,
        });
        let idx = app.history.len() - 1;
        app.streaming_message_index = Some(idx);

        app.finalize_streaming_assistant_as_interrupted();

        match &app.history[idx] {
            HistoryCell::Assistant { content, streaming } => {
                assert_eq!(content, "[interrupted]");
                assert!(!*streaming);
            }
            other => panic!("expected Assistant cell, got {other:?}"),
        }
    }

    #[test]
    fn finalize_streaming_assistant_no_op_without_index() {
        let mut app = App::new(test_options(false), &Config::default());
        // No streaming index set; should not panic and should leave history unchanged.
        let prev_len = app.history.len();
        app.finalize_streaming_assistant_as_interrupted();
        assert_eq!(app.history.len(), prev_len);
        assert!(app.streaming_message_index.is_none());
    }

    #[test]
    fn finalize_streaming_assistant_is_idempotent_on_double_call() {
        let mut app = App::new(test_options(false), &Config::default());
        app.add_message(HistoryCell::Assistant {
            content: "something".to_string(),
            streaming: true,
        });
        let idx = app.history.len() - 1;
        app.streaming_message_index = Some(idx);

        app.finalize_streaming_assistant_as_interrupted();
        // Second call without resetting state must be safe.
        app.finalize_streaming_assistant_as_interrupted();

        match &app.history[idx] {
            HistoryCell::Assistant { content, .. } => {
                // Second call still finds index None — content unchanged from first.
                assert!(content.starts_with("[interrupted] "));
                assert_eq!(content.matches("[interrupted]").count(), 1);
            }
            other => panic!("expected Assistant cell, got {other:?}"),
        }
    }

    #[test]
    fn kill_and_yank_handle_multibyte_utf8() {
        let mut app = App::new(test_options(false), &Config::default());
        // "café 你好" — char_count = 7 (c,a,f,é, ,你,好); UTF-8 bytes differ.
        app.input = "café 你好".to_string();
        app.cursor_position = 5; // before '你'
        assert!(app.kill_to_end_of_line());
        assert_eq!(app.input, "café ");
        assert_eq!(app.cursor_position, 5);
        assert_eq!(app.kill_buffer, "你好");

        // Yank back at the same spot — must not panic on char boundaries.
        assert!(app.yank());
        assert_eq!(app.input, "café 你好");
        assert_eq!(app.cursor_position, 7);
    }
}
