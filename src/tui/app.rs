//! Application state for the `DeepSeek` TUI.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use ratatui::layout::Rect;
use serde_json::Value;
use thiserror::Error;

use crate::compaction::CompactionConfig;
use crate::config::{Config, has_api_key, save_api_key};
use crate::hooks::{HookContext, HookEvent, HookExecutor, HookResult};
use crate::models::{Message, SystemPrompt};
use crate::palette::{self, UiTheme};
use crate::settings::Settings;
use crate::tools::plan::{SharedPlanState, new_shared_plan_state};
use crate::tools::subagent::SubAgentResult;
use crate::tools::todo::{SharedTodoList, new_shared_todo_list};
use crate::tui::approval::ApprovalMode;
use crate::tui::clipboard::{ClipboardContent, ClipboardHandler};
use crate::tui::history::{HistoryCell, TranscriptRenderOptions};
use crate::tui::paste_burst::{FlushResult, PasteBurst};
use crate::tui::scrolling::{MouseScrollState, TranscriptScroll};
use crate::tui::selection::TranscriptSelection;
use crate::tui::transcript::TranscriptViewCache;
use crate::tui::views::ViewStack;

/// Format a nice welcome banner.
fn format_welcome_banner(model: &str, workspace: &PathBuf, yolo: bool) -> String {
    let mode_line = if yolo {
        "\nYOLO mode — shell + trust + auto-approve enabled\n"
    } else {
        ""
    };

    format!(
        "Tips: Tab to switch modes, F1 or /help for commands, Esc to cancel\n\
         {mode_line}\
         Directory: {}\n\
         Model: {}",
        workspace.display(),
        model
    )
}

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
    Normal,
    Agent,
    Yolo,
    Plan,
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

impl AppMode {
    /// Short label used in the UI footer.
    pub fn label(self) -> &'static str {
        match self {
            AppMode::Normal => "NORMAL",
            AppMode::Agent => "AGENT",
            AppMode::Yolo => "YOLO",
            AppMode::Plan => "PLAN",
        }
    }

    #[allow(dead_code)]
    /// Description shown in help or onboarding text.
    pub fn description(self) -> &'static str {
        match self {
            AppMode::Normal => "Chat mode - ask questions, get answers",
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

/// Global UI state for the TUI.
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    pub mode: AppMode,
    pub input: String,
    pub cursor_position: usize,
    pub paste_burst: PasteBurst,
    pub history: Vec<HistoryCell>,
    pub history_version: u64,
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
    pub status_message: Option<String>,
    pub model: String,
    pub workspace: PathBuf,
    pub skills_dir: PathBuf,
    pub use_alt_screen: bool,
    #[allow(dead_code)]
    pub system_prompt: Option<SystemPrompt>,
    pub input_history: Vec<String>,
    pub history_index: Option<usize>,
    pub auto_compact: bool,
    pub show_thinking: bool,
    pub show_tool_details: bool,
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
    // Clipboard handler
    pub clipboard: ClipboardHandler,
    // Tool approval session allowlist
    pub approval_session_approved: HashSet<String>,
    pub approval_mode: ApprovalMode,
    // Modal view stack (approval/help/etc.)
    pub view_stack: ViewStack,
    /// Current session ID for auto-save updates
    pub current_session_id: Option<String>,
    /// Trust mode - allow access outside workspace
    pub trust_mode: bool,
    /// Project documentation (AGENTS.md or CLAUDE.md)
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
    /// Tool execution log
    pub tool_log: Vec<String>,
    /// Session cost tracking
    pub session_cost: f64,
    /// Active skill to apply to next user message
    pub active_skill: Option<String>,
    /// Tool call cells by tool id
    pub tool_cells: HashMap<String, usize>,
    /// Active exploring cell index
    pub exploring_cell: Option<usize>,
    /// Mapping of exploring tool ids to (cell index, entry index)
    pub exploring_entries: HashMap<String, (usize, usize)>,
    /// Tool calls that should be ignored by the UI
    pub ignored_tool_calls: HashSet<String>,
    /// Last exec wait command shown (for duplicate suppression)
    pub last_exec_wait_command: Option<String>,
    /// Current streaming assistant cell
    pub streaming_message_index: Option<usize>,
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
    /// Start time for current turn
    pub turn_started_at: Option<Instant>,
    /// Last prompt token usage
    pub last_prompt_tokens: Option<u32>,
    /// Last completion token usage
    pub last_completion_tokens: Option<u32>,
}

/// Message queued while the engine is busy.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub display: String,
    pub skill_instruction: Option<String>,
}

impl QueuedMessage {
    pub fn new(display: String, skill_instruction: Option<String>) -> Self {
        Self {
            display,
            skill_instruction,
        }
    }

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
    #[allow(clippy::too_many_lines)]
    pub fn new(options: TuiOptions, config: &Config) -> Self {
        let TuiOptions {
            model,
            workspace,
            allow_shell: _allow_shell,
            use_alt_screen,
            max_subagents,
            skills_dir: global_skills_dir,
            memory_path: _,
            notes_path: _,
            mcp_config_path: _,
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
        let show_thinking = settings.show_thinking;
        let show_tool_details = settings.show_tool_details;
        let max_input_history = settings.max_input_history;
        let ui_theme = palette::ui_theme(&settings.theme);
        let model = settings.default_model.clone().unwrap_or_else(|| model);

        // Start in YOLO mode if --yolo flag was passed
        let preferred_mode = match settings.default_mode.as_str() {
            "plan" => AppMode::Plan,
            "agent" | "normal" => AppMode::Agent,
            "yolo" => AppMode::Yolo,
            _ => AppMode::Agent,
        };
        let initial_mode = if yolo {
            AppMode::Yolo
        } else if start_in_agent_mode {
            AppMode::Agent
        } else {
            preferred_mode
        };

        let history = if needs_onboarding {
            Vec::new() // No welcome message during onboarding
        } else {
            vec![HistoryCell::System {
                content: format_welcome_banner(&model, &workspace, yolo),
            }]
        };

        // Initialize hooks executor from config
        let hooks_config = config.hooks_config();
        let hooks = HookExecutor::new(hooks_config, workspace.clone());

        // Initialize plan state
        let plan_state = new_shared_plan_state();

        let history_len = history.len() as u64;

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
            paste_burst: PasteBurst::default(),
            history,
            history_version: history_len,
            api_messages: Vec::new(),
            transcript_scroll: TranscriptScroll::ToBottom,
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
            status_message: None,
            model,
            workspace,
            skills_dir,
            use_alt_screen,
            system_prompt: None,
            input_history: Vec::new(),
            history_index: None,
            auto_compact,
            show_thinking,
            show_tool_details,
            compact_threshold: 50000,
            max_input_history,
            total_tokens: 0,
            total_conversation_tokens: 0,
            allow_shell: true,
            max_subagents,
            subagent_cache: Vec::new(),
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
            clipboard: ClipboardHandler::new(),
            approval_session_approved: HashSet::new(),
            approval_mode: if matches!(initial_mode, AppMode::Yolo) {
                ApprovalMode::Auto
            } else {
                ApprovalMode::Suggest
            },
            view_stack: ViewStack::new(),
            current_session_id: None,
            trust_mode: initial_mode == AppMode::Yolo,
            project_doc: None,
            plan_state,
            plan_prompt_pending: false,
            plan_tool_used_in_turn: false,
            todos: new_shared_todo_list(),
            tool_log: Vec::new(),
            session_cost: 0.0,
            active_skill: None,
            tool_cells: HashMap::new(),
            exploring_cell: None,
            exploring_entries: HashMap::new(),
            ignored_tool_calls: HashSet::new(),
            last_exec_wait_command: None,
            streaming_message_index: None,
            reasoning_buffer: String::new(),
            reasoning_header: None,
            last_reasoning: None,
            pending_tool_uses: Vec::new(),
            queued_messages: VecDeque::new(),
            queued_draft: None,
            turn_started_at: None,
            last_prompt_tokens: None,
            last_completion_tokens: None,
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
        self.add_message(HistoryCell::System {
            content: format_welcome_banner(&self.model, &self.workspace, self.yolo),
        });
    }

    pub fn set_mode(&mut self, mode: AppMode) {
        let previous_mode = self.mode;
        self.mode = mode;
        self.status_message = Some(format!("Switched to {} mode", mode.label()));
        self.allow_shell = true;
        self.trust_mode = matches!(mode, AppMode::Yolo);
        self.yolo = matches!(mode, AppMode::Yolo);
        self.approval_mode = if matches!(mode, AppMode::Yolo) {
            ApprovalMode::Auto
        } else {
            ApprovalMode::Suggest
        };
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
    }

    /// Cycle through modes: Plan -> Agent -> YOLO -> Plan
    pub fn cycle_mode(&mut self) {
        let next = match self.mode {
            AppMode::Plan => AppMode::Agent,
            AppMode::Agent => AppMode::Yolo,
            AppMode::Yolo | AppMode::Normal => AppMode::Plan,
        };
        self.set_mode(next);
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
        self.history.push(msg);
        self.history_version = self.history_version.wrapping_add(1);
        if matches!(self.transcript_scroll, TranscriptScroll::ToBottom)
            && !self.transcript_selection.dragging
        {
            self.scroll_to_bottom();
        }
    }

    pub fn mark_history_updated(&mut self) {
        self.history_version = self.history_version.wrapping_add(1);
    }

    pub fn transcript_render_options(&self) -> TranscriptRenderOptions {
        TranscriptRenderOptions {
            show_thinking: self.show_thinking,
            show_tool_details: self.show_tool_details,
        }
    }

    /// Handle terminal resize event.
    ///
    /// This method properly invalidates all cached layout state to ensure
    /// correct rendering after the terminal dimensions change.
    pub fn handle_resize(&mut self, _width: u16, _height: u16) {
        // Invalidate transcript cache (will be rebuilt on next render)
        self.transcript_cache = TranscriptViewCache::new();

        // Reset scroll to bottom to avoid invalid anchors
        // (anchored cell indices may be invalid at new width)
        self.transcript_scroll = TranscriptScroll::ToBottom;

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
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert_str(byte_index, text);
        self.cursor_position = cursor + char_count(text);
    }

    pub fn insert_paste_text(&mut self, text: &str) {
        let normalized = normalize_paste_text(text);
        if !normalized.is_empty() {
            self.insert_str(&normalized);
        }
        self.paste_burst.clear_after_explicit_paste();
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
            if let Some(pending) = self.paste_burst.flush_before_modified_input() {
                self.insert_str(&pending);
            }
            match content {
                ClipboardContent::Text(text) => {
                    self.insert_paste_text(&text);
                }
                ClipboardContent::Image { path, description } => {
                    // Insert image path reference
                    let reference = format!("[Image: {} at {}]", description, path.display());
                    self.insert_str(&reference);
                    self.paste_burst.clear_after_explicit_paste();
                    self.status_message = Some(format!("Pasted image: {}", path.display()));
                }
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
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let delta = i32::try_from(amount).unwrap_or(i32::MAX);
        self.pending_scroll_delta = self.pending_scroll_delta.saturating_add(delta);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.transcript_scroll = TranscriptScroll::ToBottom;
        self.pending_scroll_delta = 0;
    }

    pub fn insert_char(&mut self, c: char) {
        let cursor = self.cursor_position.min(char_count(&self.input));
        let byte_index = byte_index_at_char(&self.input, cursor);
        self.input.insert(byte_index, c);
        self.cursor_position = cursor + 1;
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position == 0 {
            return;
        }
        let target = self.cursor_position.saturating_sub(1);
        let removed = remove_char_at(&mut self.input, target);
        if removed {
            self.cursor_position = target;
        }
    }

    pub fn delete_char_forward(&mut self) {
        if self.input.is_empty() {
            return;
        }
        let target = self.cursor_position;
        let removed = remove_char_at(&mut self.input, target);
        if !removed {
            self.cursor_position = char_count(&self.input);
        }
    }

    pub fn move_cursor_left(&mut self) {
        self.cursor_position = self.cursor_position.saturating_sub(1);
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < char_count(&self.input) {
            self.cursor_position += 1;
        }
    }

    pub fn move_cursor_start(&mut self) {
        self.cursor_position = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = char_count(&self.input);
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
        self.paste_burst.clear_after_explicit_paste();
    }

    pub fn submit_input(&mut self) -> Option<String> {
        if self.input.trim().is_empty() {
            self.paste_burst.clear_after_explicit_paste();
            return None;
        }
        let input = self.input.clone();
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

    pub fn queued_message_previews(&self, max: usize) -> Vec<String> {
        if max == 0 {
            return Vec::new();
        }

        let mut previews: Vec<String> = self
            .queued_messages
            .iter()
            .take(max)
            .map(|msg| msg.display.clone())
            .collect();
        let extra = self.queued_messages.len().saturating_sub(previews.len());
        if extra > 0 {
            previews.push(format!("+{extra} more"));
        }
        previews
    }

    pub fn queued_message_count(&self) -> usize {
        self.queued_messages.len()
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
                    self.paste_burst.clear_after_explicit_paste();
                } else {
                    self.history_index = None;
                    self.clear_input();
                }
            }
        }
    }

    pub fn clear_todos(&mut self) {
        let mut plan = self.plan_state.blocking_lock();
        *plan = crate::tools::plan::PlanState::default();
    }
}

// === Actions ===

/// Actions emitted by the UI event loop.
#[derive(Debug, Clone)]
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
    SendMessage(String),
    ListSubAgents,
    UpdateCompaction(CompactionConfig),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_options(yolo: bool) -> TuiOptions {
        TuiOptions {
            model: "test-model".to_string(),
            workspace: PathBuf::from("."),
            allow_shell: yolo,
            use_alt_screen: true,
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
}
