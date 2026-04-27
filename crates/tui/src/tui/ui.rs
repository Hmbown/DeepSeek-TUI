//! TUI event loop and rendering logic for `DeepSeek` CLI.

use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Span,
    widgets::Block,
};
use tracing;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::audit::log_sensitive_event;
use crate::client::DeepSeekClient;
use crate::commands;
use crate::compaction::estimate_input_tokens_conservative;
use crate::config::{ApiProvider, Config, DEFAULT_NVIDIA_NIM_BASE_URL};
use crate::core::coherence::CoherenceState;
use crate::core::engine::{EngineConfig, EngineHandle, spawn_engine};
use crate::core::events::Event as EngineEvent;
use crate::core::ops::Op;
use crate::hooks::HookEvent;
use crate::models::{ContentBlock, Message, SystemPrompt, context_window_for_model};
use crate::palette;
use crate::prompts;
use crate::session_manager::{
    OfflineQueueState, QueuedSessionMessage, SavedSession, SessionManager,
    create_saved_session_with_mode, update_session,
};
use crate::task_manager::{
    NewTaskRequest, SharedTaskManager, TaskManager, TaskManagerConfig, TaskRecord, TaskStatus,
    TaskSummary,
};
use crate::tools::ReviewOutput;
use crate::tools::spec::{ToolError, ToolResult};
use crate::tools::subagent::{SubAgentResult, SubAgentStatus};
use crate::tui::command_palette::{
    CommandPaletteView, build_entries as build_command_palette_entries,
};
use crate::tui::event_broker::EventBroker;
use crate::tui::onboarding;
use crate::tui::pager::PagerView;
use crate::tui::plan_prompt::PlanPromptView;
use crate::tui::scrolling::{ScrollDirection, TranscriptScroll};
use crate::tui::selection::TranscriptSelectionPoint;
use crate::tui::session_picker::SessionPickerView;
use crate::tui::ui_text::{history_cell_to_text, line_to_plain, slice_text, text_display_width};
use crate::tui::user_input::UserInputView;

use super::active_cell::ActiveCell;
use super::app::{
    App, AppAction, AppMode, OnboardingState, QueuedMessage, SidebarFocus, StatusToastLevel,
    TaskPanelEntry, ToolDetailRecord, TuiOptions,
};
use super::approval::{
    ApprovalMode, ApprovalRequest, ApprovalView, ElevationRequest, ElevationView, ReviewDecision,
};
use super::history::{
    DiffPreviewCell, ExecCell, ExecSource, ExploringEntry, GenericToolCell, HistoryCell,
    McpToolCell, PatchSummaryCell, PlanStep, PlanUpdateCell, ReviewCell, ToolCell, ToolStatus,
    ViewImageCell, WebSearchCell, history_cells_from_message, summarize_mcp_output,
    summarize_tool_args, summarize_tool_output,
};
use super::slash_menu::{
    apply_slash_menu_selection, try_autocomplete_slash_command, visible_slash_menu_entries,
};
use super::views::{ConfigView, HelpView, ModalKind, ViewEvent};
use super::widgets::{
    ChatWidget, ComposerWidget, FooterProps, FooterToast, FooterWidget, HeaderData, HeaderWidget,
    Renderable,
};

// === Constants ===

/// Upper bound on slash-menu entries returned to the renderer. The composer's
/// render path already paginates with center-tracking (see
/// `widgets::ComposerWidget::render`), so this only needs to be high enough to
/// encompass the full filtered command list — never the visible-row budget.
/// Bumped from 6 to 128 to fix #64 (selection couldn't reach commands beyond
/// the visible window because the source list itself was capped).
const SLASH_MENU_LIMIT: usize = 128;
const MENTION_MENU_LIMIT: usize = 6;
const MIN_CHAT_HEIGHT: u16 = 3;
const MIN_COMPOSER_HEIGHT: u16 = 2;
const CONTEXT_WARNING_THRESHOLD_PERCENT: f64 = 85.0;
const CONTEXT_CRITICAL_THRESHOLD_PERCENT: f64 = 95.0;
const UI_IDLE_POLL_MS: u64 = 48;
const UI_ACTIVE_POLL_MS: u64 = 24;
// Forced repaint cadence while a turn is live (model loading, compacting,
// sub-agents running). Drives the footer water-spout animation as well as
// the per-tool spinner pulse — keep this fast enough that the spout reads as
// motion (~12 fps) instead of teleport-frames.
const UI_STATUS_ANIMATION_MS: u64 = 80;
const WORKSPACE_CONTEXT_REFRESH_SECS: u64 = 15;
const SIDEBAR_VISIBLE_MIN_WIDTH: u16 = 100;

/// Run the interactive TUI event loop.
///
/// # Examples
///
/// ```ignore
/// # use crate::config::Config;
/// # use crate::tui::TuiOptions;
/// # async fn example(config: &Config, options: TuiOptions) -> anyhow::Result<()> {
/// crate::tui::run_tui(config, options).await
/// # }
/// ```
pub async fn run_tui(config: &Config, options: TuiOptions) -> Result<()> {
    let use_alt_screen = options.use_alt_screen;
    let use_mouse_capture = options.use_mouse_capture;
    let use_bracketed_paste = options.use_bracketed_paste;
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if use_alt_screen {
        execute!(stdout, EnterAlternateScreen)?;
    }
    if use_mouse_capture {
        execute!(stdout, EnableMouseCapture)?;
    }
    if use_bracketed_paste {
        execute!(stdout, EnableBracketedPaste)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let event_broker = EventBroker::new();

    // Local mutable copy so runtime config flips (e.g. `/provider` switch)
    // can rebuild the API client without restarting the process.
    let mut config = config.clone();
    let config = &mut config;
    let mut app = App::new(options.clone(), config);

    // Load existing session if resuming.
    if let Some(ref session_id) = options.resume_session_id
        && let Ok(manager) = SessionManager::default_location()
    {
        // Try to load by prefix or full ID
        let load_result: std::io::Result<Option<crate::session_manager::SavedSession>> =
            if session_id == "latest" {
                // Special case: resume the most recent session
                match manager.get_latest_session() {
                    Ok(Some(meta)) => manager.load_session(&meta.id).map(Some),
                    Ok(None) => Ok(None),
                    Err(e) => Err(e),
                }
            } else {
                manager.load_session_by_prefix(session_id).map(Some)
            };

        match load_result {
            Ok(Some(saved)) => {
                app.api_messages.clone_from(&saved.messages);
                app.model.clone_from(&saved.metadata.model);
                app.update_model_compaction_budget();
                app.workspace.clone_from(&saved.metadata.workspace);
                app.current_session_id = Some(saved.metadata.id.clone());
                app.total_tokens = u32::try_from(saved.metadata.total_tokens).unwrap_or(u32::MAX);
                app.total_conversation_tokens = app.total_tokens;
                app.last_prompt_tokens = None;
                app.last_completion_tokens = None;
                app.last_prompt_cache_hit_tokens = None;
                app.last_prompt_cache_miss_tokens = None;
                app.last_reasoning_replay_tokens = None;
                if let Some(prompt) = saved.system_prompt {
                    app.system_prompt = Some(SystemPrompt::Text(prompt));
                }
                // Convert saved messages to HistoryCell format for display
                app.clear_history();
                app.push_history_cell(HistoryCell::System {
                    content: format!(
                        "Resumed session: {} ({})",
                        saved.metadata.title,
                        &saved.metadata.id[..8.min(saved.metadata.id.len())]
                    ),
                });

                for msg in &saved.messages {
                    app.extend_history(history_cells_from_message(msg));
                }
                app.mark_history_updated();
                app.status_message = Some(format!(
                    "Resumed session: {}",
                    &saved.metadata.id[..8.min(saved.metadata.id.len())]
                ));
            }
            Ok(None) => {
                app.status_message = Some("No sessions found to resume".to_string());
            }
            Err(e) => {
                app.status_message = Some(format!("Failed to load session: {e}"));
            }
        }
    }

    if let Ok(manager) = SessionManager::default_location() {
        match manager.load_offline_queue_state() {
            Ok(Some(state)) => {
                app.queued_messages = state
                    .messages
                    .into_iter()
                    .map(queued_session_to_ui)
                    .collect();
                app.queued_draft = state.draft.map(queued_session_to_ui);
                if app.status_message.is_none() && app.queued_message_count() > 0 {
                    app.status_message = Some(format!(
                        "Recovered {} queued message(s)",
                        app.queued_message_count()
                    ));
                }
            }
            Ok(None) => {}
            Err(err) => {
                if app.status_message.is_none() {
                    app.status_message = Some(format!("Failed to restore offline queue: {err}"));
                }
            }
        }
    }

    let engine_config = build_engine_config(&app, config);

    // Spawn the Engine - it will handle all API communication
    let engine_handle = spawn_engine(engine_config, config);

    if !app.api_messages.is_empty() {
        let _ = engine_handle
            .send(Op::SyncSession {
                messages: app.api_messages.clone(),
                system_prompt: app.system_prompt.clone(),
                model: app.model.clone(),
                workspace: app.workspace.clone(),
            })
            .await;
    }

    // Fire session start hook
    {
        let context = app.base_hook_context();
        let _ = app.execute_hooks(HookEvent::SessionStart, &context);
    }

    let task_manager = TaskManager::start(
        TaskManagerConfig::from_runtime(
            config,
            app.workspace.clone(),
            Some(app.model.clone()),
            Some(app.max_subagents.clamp(1, 4)),
        ),
        config.clone(),
    )
    .await?;
    app.task_panel = task_manager
        .list_tasks(Some(10))
        .await
        .into_iter()
        .map(task_summary_to_panel_entry)
        .collect();

    let result = run_event_loop(
        &mut terminal,
        &mut app,
        config,
        engine_handle,
        task_manager,
        &event_broker,
    )
    .await;

    // Fire session end hook
    {
        let context = app.base_hook_context();
        let _ = app.execute_hooks(HookEvent::SessionEnd, &context);
    }

    // Clear crash-recovery checkpoint on normal exit so the next launch starts fresh.
    clear_checkpoint();

    disable_raw_mode()?;
    if use_alt_screen {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }
    if use_mouse_capture {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    if use_bracketed_paste {
        execute!(terminal.backend_mut(), DisableBracketedPaste)?;
    }
    terminal.show_cursor()?;

    result
}

fn build_engine_config(app: &App, config: &Config) -> EngineConfig {
    EngineConfig {
        model: app.model.clone(),
        workspace: app.workspace.clone(),
        allow_shell: app.allow_shell,
        trust_mode: app.trust_mode,
        notes_path: config.notes_path(),
        mcp_config_path: config.mcp_config_path(),
        // Effectively unlimited. V4 has a 1M context window and the user
        // wants the model running until it's actually done. The previous cap
        // of 100 hit the ceiling on long multi-step plans (wide refactors,
        // sub-agent orchestration) and presented as the agent "giving up
        // mid-task". `u32::MAX` is the type ceiling; users can still
        // interrupt with Ctrl+C / Esc, and a turn naturally ends when the
        // model stops emitting tool calls. A real runaway is rare and
        // human-noticeable; we trust the operator over a hard step cap.
        max_steps: u32::MAX,
        max_subagents: app.max_subagents,
        features: config.features(),
        compaction: app.compaction_config(),
        capacity: crate::core::capacity::CapacityControllerConfig::from_app_config(config),
        todos: app.todos.clone(),
        plan_state: app.plan_state.clone(),
    }
}

#[allow(clippy::too_many_lines)]
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    config: &mut Config,
    mut engine_handle: EngineHandle,
    task_manager: SharedTaskManager,
    event_broker: &EventBroker,
) -> Result<()> {
    // Track streaming state
    let mut current_streaming_text = String::new();
    let mut last_queue_state = (app.queued_messages.clone(), app.queued_draft.clone());
    let mut last_task_refresh = Instant::now()
        .checked_sub(Duration::from_secs(2))
        .unwrap_or_else(Instant::now);
    let mut last_status_frame = Instant::now()
        .checked_sub(Duration::from_millis(UI_STATUS_ANIMATION_MS))
        .unwrap_or_else(Instant::now);

    loop {
        if last_task_refresh.elapsed() >= Duration::from_millis(2500) {
            let tasks = task_manager.list_tasks(Some(10)).await;
            app.task_panel = tasks.into_iter().map(task_summary_to_panel_entry).collect();
            last_task_refresh = Instant::now();
            app.needs_redraw = true;
        }

        // First, poll for engine events (non-blocking)
        let mut received_engine_event = false;
        let mut transcript_batch_updated = false;
        let mut queued_to_send: Option<QueuedMessage> = None;
        {
            let mut rx = engine_handle.rx_event.write().await;
            while let Ok(event) = rx.try_recv() {
                received_engine_event = true;
                match event {
                    EngineEvent::MessageStarted { .. } => {
                        // Assistant text starting after parallel tool work
                        // means the tool group is done. Flush the active
                        // cell first so the message lands BELOW the
                        // committed tool group (Codex pattern: streamed
                        // assistant content always flows after work).
                        app.flush_active_cell();
                        current_streaming_text.clear();
                        app.streaming_state.reset();
                        app.streaming_state.start_text(0, None);
                        app.streaming_message_index = None;
                    }
                    EngineEvent::MessageDelta { content, .. } => {
                        let sanitized = sanitize_stream_chunk(&content);
                        if sanitized.is_empty() {
                            continue;
                        }
                        // First delta of a fresh stream has no streaming
                        // cell yet; flush active so the tool group settles
                        // before the assistant prose appears below it.
                        if app.streaming_message_index.is_none() {
                            app.flush_active_cell();
                        }
                        current_streaming_text.push_str(&sanitized);
                        let index = ensure_streaming_assistant_history_cell(app);
                        app.streaming_state.push_content(0, &sanitized);
                        let committed = app.streaming_state.commit_text(0);
                        if !committed.is_empty() {
                            append_streaming_text(app, index, &committed);
                            transcript_batch_updated = true;
                        }
                    }
                    EngineEvent::MessageComplete { .. } => {
                        if let Some(index) = app.streaming_message_index.take() {
                            let remaining = app.streaming_state.finalize_block_text(0);
                            if !remaining.is_empty() {
                                append_streaming_text(app, index, &remaining);
                            }
                            if let Some(HistoryCell::Assistant { streaming, .. }) =
                                app.history.get_mut(index)
                            {
                                *streaming = false;
                            }
                            // Streaming flag flipped — the cell's compact /
                            // transcript variants render slightly
                            // differently, so bump its revision so the cache
                            // refreshes this row only.
                            app.bump_history_cell(index);
                            transcript_batch_updated = true;
                        }

                        let mut blocks = Vec::new();
                        let thinking = app.last_reasoning.take();
                        if let Some(thinking) = thinking {
                            blocks.push(ContentBlock::Thinking { thinking });
                        }
                        if !current_streaming_text.is_empty() {
                            blocks.push(ContentBlock::Text {
                                text: current_streaming_text.clone(),
                                cache_control: None,
                            });
                        }
                        for (id, name, input) in app.pending_tool_uses.drain(..) {
                            blocks.push(ContentBlock::ToolUse {
                                id,
                                name,
                                input,
                                caller: None,
                            });
                        }

                        // DeepSeek rejects assistant messages that contain only reasoning blocks.
                        // Keep reasoning in transcript cells, but only persist assistant turns that
                        // include visible text and/or tool calls.
                        let has_sendable_content = blocks.iter().any(|block| {
                            matches!(
                                block,
                                ContentBlock::Text { .. } | ContentBlock::ToolUse { .. }
                            )
                        });
                        if has_sendable_content {
                            app.api_messages.push(Message {
                                role: "assistant".to_string(),
                                content: blocks,
                            });
                        }
                    }
                    EngineEvent::ThinkingStarted { .. } => {
                        // P2.3: thinking lives in the active cell so it groups
                        // visually with the tool calls that follow until the
                        // next assistant prose chunk flushes the group.
                        app.reasoning_buffer.clear();
                        app.reasoning_header = None;
                        app.thinking_started_at = Some(Instant::now());
                        app.streaming_state.reset();
                        app.streaming_state.start_thinking(0, None);
                        let _ = ensure_streaming_thinking_active_entry(app);
                    }
                    EngineEvent::ThinkingDelta { content, .. } => {
                        let sanitized = sanitize_stream_chunk(&content);
                        if sanitized.is_empty() {
                            continue;
                        }
                        app.reasoning_buffer.push_str(&sanitized);
                        if app.reasoning_header.is_none() {
                            app.reasoning_header = extract_reasoning_header(&app.reasoning_buffer);
                        }

                        let entry_idx = ensure_streaming_thinking_active_entry(app);
                        app.streaming_state.push_content(0, &sanitized);
                        let committed = app.streaming_state.commit_text(0);
                        if !committed.is_empty() {
                            append_streaming_thinking(app, entry_idx, &committed);
                            transcript_batch_updated = true;
                        }
                    }
                    EngineEvent::ThinkingComplete { .. } => {
                        let duration = app
                            .thinking_started_at
                            .take()
                            .map(|t| t.elapsed().as_secs_f32());
                        let remaining = app.streaming_state.finalize_block_text(0);
                        if finalize_streaming_thinking_active_entry(app, duration, &remaining) {
                            transcript_batch_updated = true;
                        }

                        if !app.reasoning_buffer.is_empty() {
                            app.last_reasoning = Some(app.reasoning_buffer.clone());
                        }
                        app.reasoning_buffer.clear();
                    }
                    EngineEvent::ToolCallStarted { id, name, input } => {
                        app.pending_tool_uses
                            .push((id.clone(), name.clone(), input.clone()));
                        handle_tool_call_started(app, &id, &name, &input);
                    }
                    EngineEvent::ToolCallComplete { id, name, result } => {
                        if name == "update_plan" {
                            app.plan_tool_used_in_turn = true;
                        }
                        let tool_content = match &result {
                            Ok(output) => sanitize_stream_chunk(
                                &crate::core::engine::compact_tool_result_for_context(
                                    &app.model, &name, output,
                                ),
                            ),
                            Err(err) => sanitize_stream_chunk(&format!("Error: {err}")),
                        };
                        app.api_messages.push(Message {
                            role: "user".to_string(),
                            content: vec![ContentBlock::ToolResult {
                                tool_use_id: id.clone(),
                                content: tool_content,
                                is_error: None,
                                content_blocks: None,
                            }],
                        });
                        handle_tool_call_complete(app, &id, &name, &result);

                        // Immediately refresh the task panel sidebar when a
                        // tool that changes task state completes, so the
                        // Tasks panel stays in sync with tool execution
                        // rather than waiting up to 2.5 s for the periodic
                        // poll.
                        if matches!(
                            name.as_str(),
                            "agent_spawn" | "agent_swarm" | "agent_cancel" | "todo_write"
                        ) {
                            let tasks = task_manager.list_tasks(Some(10)).await;
                            app.task_panel =
                                tasks.into_iter().map(task_summary_to_panel_entry).collect();
                            last_task_refresh = Instant::now();
                        }
                    }
                    EngineEvent::TurnStarted { turn_id } => {
                        app.is_loading = true;
                        app.offline_mode = false;
                        current_streaming_text.clear();
                        app.streaming_state.reset();
                        app.streaming_message_index = None;
                        app.streaming_thinking_active_entry = None;
                        app.turn_started_at = Some(Instant::now());
                        app.runtime_turn_id = Some(turn_id);
                        app.runtime_turn_status = Some("in_progress".to_string());
                        app.reasoning_buffer.clear();
                        app.reasoning_header = None;
                        app.last_reasoning = None;
                        app.pending_tool_uses.clear();
                        app.plan_tool_used_in_turn = false;
                        persist_checkpoint(app);
                        last_status_frame = Instant::now();
                    }
                    EngineEvent::TurnComplete {
                        usage,
                        status,
                        error,
                    } => {
                        // Finalize any in-flight tool group. Cancellation
                        // marks still-running entries as Failed so the user
                        // sees they were interrupted rather than the spinner
                        // hanging forever.
                        if matches!(
                            status,
                            crate::core::events::TurnOutcomeStatus::Interrupted
                                | crate::core::events::TurnOutcomeStatus::Failed
                        ) {
                            app.finalize_active_cell_as_interrupted();
                        } else {
                            app.flush_active_cell();
                        }
                        app.is_loading = false;
                        app.offline_mode = false;
                        app.streaming_state.reset();
                        app.turn_started_at = None;
                        // Stream lock applies per-turn; clear it so the next
                        // turn's chunks pull the view down again until the
                        // user opts out by scrolling up.
                        app.user_scrolled_during_stream = false;
                        app.runtime_turn_status = Some(match status {
                            crate::core::events::TurnOutcomeStatus::Completed => {
                                "completed".to_string()
                            }
                            crate::core::events::TurnOutcomeStatus::Interrupted => {
                                "interrupted".to_string()
                            }
                            crate::core::events::TurnOutcomeStatus::Failed => "failed".to_string(),
                        });
                        let turn_tokens = usage.input_tokens + usage.output_tokens;
                        app.total_tokens = app.total_tokens.saturating_add(turn_tokens);
                        app.total_conversation_tokens =
                            app.total_conversation_tokens.saturating_add(turn_tokens);
                        app.last_prompt_tokens = Some(usage.input_tokens);
                        app.last_completion_tokens = Some(usage.output_tokens);
                        app.last_prompt_cache_hit_tokens = usage.prompt_cache_hit_tokens;
                        app.last_prompt_cache_miss_tokens = usage.prompt_cache_miss_tokens;
                        app.last_reasoning_replay_tokens = usage.reasoning_replay_tokens;
                        if let Some(error) = error {
                            app.status_message = Some(format!("Turn failed: {error}"));
                        }

                        // Update session cost
                        if let Some(turn_cost) =
                            crate::pricing::calculate_turn_cost_from_usage(&app.model, &usage)
                        {
                            app.session_cost += turn_cost;
                        }

                        // Auto-save completed turn and clear crash checkpoint.
                        persist_session_snapshot(app);
                        clear_checkpoint();

                        if app.mode == AppMode::Plan
                            && app.plan_tool_used_in_turn
                            && !app.plan_prompt_pending
                            && app.queued_message_count() == 0
                            && app.queued_draft.is_none()
                        {
                            app.plan_prompt_pending = true;
                            app.add_message(HistoryCell::System {
                                content: plan_next_step_prompt(),
                            });
                            if app.view_stack.top_kind() != Some(ModalKind::PlanPrompt) {
                                app.view_stack.push(PlanPromptView::new());
                            }
                        }
                        app.plan_tool_used_in_turn = false;

                        if queued_to_send.is_none() {
                            queued_to_send = app.pop_queued_message();
                        }
                    }
                    EngineEvent::Error {
                        message,
                        recoverable,
                    } => {
                        apply_engine_error_to_app(app, message, recoverable);
                    }
                    EngineEvent::Status { message } => {
                        app.status_message = Some(message);
                    }
                    EngineEvent::SessionUpdated {
                        messages,
                        system_prompt,
                        model,
                        workspace,
                    } => {
                        app.api_messages = messages;
                        app.system_prompt = system_prompt;
                        app.model = model;
                        app.update_model_compaction_budget();
                        app.workspace = workspace;
                        if app.is_loading || app.is_compacting {
                            persist_checkpoint(app);
                        }
                    }
                    EngineEvent::CompactionStarted { message, .. } => {
                        app.is_compacting = true;
                        app.status_message = Some(message);
                    }
                    EngineEvent::CompactionCompleted { message, .. } => {
                        app.is_compacting = false;
                        app.status_message = Some(message);
                    }
                    EngineEvent::CompactionFailed { message, .. } => {
                        app.is_compacting = false;
                        app.status_message = Some(message);
                    }
                    EngineEvent::CoherenceState { state, .. } => {
                        app.coherence_state = state;
                    }
                    EngineEvent::CapacityDecision { .. } => {
                        // Telemetry-only event. Surface actual interventions and failures
                        // instead of replacing the footer with no-op guardrail chatter.
                    }
                    EngineEvent::CapacityIntervention {
                        action,
                        before_prompt_tokens,
                        after_prompt_tokens,
                        ..
                    } => {
                        app.status_message = Some(format!(
                            "Capacity intervention: {action} (~{before_prompt_tokens} -> ~{after_prompt_tokens} tokens)"
                        ));
                    }
                    EngineEvent::CapacityMemoryPersistFailed { action, error, .. } => {
                        app.status_message = Some(format!(
                            "Capacity memory persist failed ({action}): {error}"
                        ));
                    }
                    EngineEvent::PauseEvents => {
                        if !event_broker.is_paused() {
                            pause_terminal(
                                terminal,
                                app.use_alt_screen,
                                app.use_mouse_capture,
                                app.use_bracketed_paste,
                            )?;
                            event_broker.pause_events();
                        }
                    }
                    EngineEvent::ResumeEvents => {
                        if event_broker.is_paused() {
                            resume_terminal(
                                terminal,
                                app.use_alt_screen,
                                app.use_mouse_capture,
                                app.use_bracketed_paste,
                            )?;
                            event_broker.resume_events();
                        }
                    }
                    EngineEvent::AgentSpawned { id, prompt } => {
                        let prompt_summary = summarize_tool_output(&prompt);
                        app.agent_progress
                            .insert(id.clone(), format!("starting: {prompt_summary}"));
                        if app.agent_activity_started_at.is_none() {
                            app.agent_activity_started_at = Some(Instant::now());
                        }
                        app.status_message =
                            Some(format!("Sub-agent {id} starting: {prompt_summary}"));
                        let _ = engine_handle.send(Op::ListSubAgents).await;
                    }
                    EngineEvent::AgentProgress { id, status } => {
                        app.agent_progress
                            .insert(id.clone(), summarize_tool_output(&status));
                        if app.agent_activity_started_at.is_none() {
                            app.agent_activity_started_at = Some(Instant::now());
                        }
                        app.status_message = Some(format!("Sub-agent {id}: {status}"));
                    }
                    EngineEvent::AgentComplete { id, result } => {
                        app.agent_progress.remove(&id);
                        app.status_message = Some(format!(
                            "Sub-agent {id} completed: {}",
                            summarize_tool_output(&result)
                        ));
                        let _ = engine_handle.send(Op::ListSubAgents).await;
                    }
                    EngineEvent::AgentList { agents } => {
                        let mut sorted = agents.clone();
                        sort_subagents_in_place(&mut sorted);
                        app.subagent_cache = sorted.clone();
                        reconcile_subagent_activity_state(app);
                        if app.view_stack.update_subagents(&sorted) {
                            app.status_message =
                                Some(format!("Sub-agents: {} total", sorted.len()));
                        }
                        // Individual spawn/complete events already log to history;
                        // full list available via /agents command.
                    }
                    EngineEvent::ApprovalRequired {
                        id,
                        tool_name,
                        description,
                    } => {
                        let session_approved = app.approval_session_approved.contains(&tool_name);
                        if session_approved || app.approval_mode == ApprovalMode::Auto {
                            log_sensitive_event(
                                "tool.approval.auto_approve",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "session_id": app.current_session_id,
                                    "mode": app.mode.label(),
                                }),
                            );
                            let _ = engine_handle.approve_tool_call(id.clone()).await;
                        } else if app.approval_mode == ApprovalMode::Never {
                            log_sensitive_event(
                                "tool.approval.auto_deny",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "session_id": app.current_session_id,
                                    "mode": app.mode.label(),
                                }),
                            );
                            let _ = engine_handle.deny_tool_call(id.clone()).await;
                            app.status_message =
                                Some(format!("Blocked tool '{tool_name}' (approval_mode=never)"));
                        } else {
                            let tool_input = app
                                .pending_tool_uses
                                .iter()
                                .find(|(tool_id, _, _)| tool_id == &id)
                                .map(|(_, _, input)| input.clone())
                                .unwrap_or_else(|| serde_json::json!({}));

                            if tool_name == "apply_patch" {
                                maybe_add_patch_preview(app, &tool_input);
                            }

                            // Create approval request and show overlay
                            let request =
                                ApprovalRequest::new(&id, &tool_name, &description, &tool_input);
                            log_sensitive_event(
                                "tool.approval.prompted",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "description": description,
                                    "session_id": app.current_session_id,
                                    "mode": app.mode.label(),
                                }),
                            );
                            app.view_stack.push(ApprovalView::new(request));
                            app.status_message = Some(format!(
                                "Approval required for '{tool_name}': {description}"
                            ));
                        }
                    }
                    EngineEvent::UserInputRequired { id, request } => {
                        app.view_stack.push(UserInputView::new(id.clone(), request));
                        app.status_message = Some(
                            "Action required: answer the popup with 1-4, arrows, or Enter"
                                .to_string(),
                        );
                    }
                    EngineEvent::ToolCallProgress { id, output } => {
                        app.status_message =
                            Some(format!("Tool {id}: {}", summarize_tool_output(&output)));
                    }
                    EngineEvent::ElevationRequired {
                        tool_id,
                        tool_name,
                        command,
                        denial_reason,
                        blocked_network,
                        blocked_write,
                    } => {
                        // In YOLO mode, auto-elevate to full access
                        if app.approval_mode == ApprovalMode::Auto {
                            log_sensitive_event(
                                "tool.sandbox.auto_elevate",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "tool_id": tool_id,
                                    "reason": denial_reason,
                                    "session_id": app.current_session_id,
                                }),
                            );
                            app.add_message(HistoryCell::System {
                                content: format!(
                                    "Sandbox denied {tool_name}: {denial_reason} - auto-elevating to full access"
                                ),
                            });
                            // Auto-elevate to full access (no sandbox)
                            let policy = crate::sandbox::SandboxPolicy::DangerFullAccess;
                            let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                        } else {
                            log_sensitive_event(
                                "tool.sandbox.prompt_elevation",
                                serde_json::json!({
                                    "tool_name": tool_name,
                                    "tool_id": tool_id,
                                    "reason": denial_reason,
                                    "session_id": app.current_session_id,
                                }),
                            );
                            // Show elevation dialog
                            let request = ElevationRequest::for_shell(
                                &tool_id,
                                command.as_deref().unwrap_or(&tool_name),
                                &denial_reason,
                                blocked_network,
                                blocked_write,
                            );
                            app.view_stack.push(ElevationView::new(request));
                            app.status_message =
                                Some(format!("Sandbox blocked {tool_name}: {denial_reason}"));
                        }
                    }
                }
            }
        }
        if transcript_batch_updated {
            app.mark_history_updated();
        }
        if received_engine_event {
            app.needs_redraw = true;
        }

        if let Some(next) = queued_to_send {
            if let Err(err) = dispatch_user_message(app, &engine_handle, next.clone()).await {
                app.queue_message(next);
                app.status_message = Some(format!(
                    "Dispatch failed ({err}); kept {} queued message(s)",
                    app.queued_message_count()
                ));
            }

            app.needs_redraw = true;
        }

        let queue_state = (app.queued_messages.clone(), app.queued_draft.clone());
        if queue_state != last_queue_state {
            persist_offline_queue_state(app);
            last_queue_state = queue_state;
            app.needs_redraw = true;
        }

        if !app.view_stack.is_empty() {
            let events = app.view_stack.tick();
            if !events.is_empty() {
                app.needs_redraw = true;
            }
            if handle_view_events(app, config, &task_manager, &mut engine_handle, events).await? {
                return Ok(());
            }
        }

        let has_running_agents = running_agent_count(app) > 0;
        if (app.is_loading || has_running_agents || app.is_compacting)
            && last_status_frame.elapsed()
                >= Duration::from_millis(status_animation_interval_ms(app))
        {
            if !app.low_motion && history_has_live_motion(&app.history) {
                app.mark_history_updated();
            }
            app.needs_redraw = true;
            last_status_frame = Instant::now();
        }

        if event_broker.is_paused() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            continue;
        }

        let now = Instant::now();
        app.flush_paste_burst_if_due(now);
        app.sync_status_message_to_toasts();
        // Expire the "Press Ctrl+C again to quit" prompt silently after its
        // window. Triggers a redraw if the prompt was visible.
        app.tick_quit_armed();
        let allow_workspace_context_refresh =
            !app.is_loading && !has_running_agents && !app.is_compacting;
        refresh_workspace_context_if_needed(app, now, allow_workspace_context_refresh);

        if app.needs_redraw {
            terminal.draw(|f| render(f, app))?; // app is &mut
            app.needs_redraw = false;
        }

        let mut poll_timeout = if app.is_loading || has_running_agents || app.is_compacting {
            Duration::from_millis(active_poll_ms(app))
        } else {
            Duration::from_millis(idle_poll_ms(app))
        };
        if let Some(until_flush) = app.paste_burst.next_flush_delay(now) {
            poll_timeout = poll_timeout.min(until_flush);
        }
        // While the quit-confirmation prompt is armed, ensure we wake up to
        // expire it on time even if no input event arrives.
        if let Some(deadline) = app.quit_armed_until {
            let remaining = deadline.saturating_duration_since(now);
            poll_timeout = poll_timeout.min(remaining.max(Duration::from_millis(50)));
        }
        if event::poll(poll_timeout)? {
            let evt = event::read()?;
            app.needs_redraw = true;

            // Handle bracketed paste events
            if let Event::Paste(text) = &evt {
                tracing::debug!(
                    paste_len = text.len(),
                    preview = %text.chars().take(80).collect::<String>(),
                    "Received bracketed paste event"
                );
                if app.onboarding == OnboardingState::ApiKey {
                    // Paste into API key input
                    app.insert_api_key_str(text);
                    sync_api_key_validation_status(app, false);
                } else {
                    // Paste into main input
                    if let Some(pending) = app.paste_burst.flush_before_modified_input() {
                        app.insert_str(&pending);
                    }
                    app.insert_paste_text(text);
                }
                continue;
            }

            if let Event::Resize(width, height) = evt {
                tracing::debug!(width, height, "Event::Resize received; clearing terminal");
                // Drain any further Resize events queued in this poll cycle so we
                // act on the final size only, then issue a single clear + redraw.
                // crossterm coalesces some resize events but rapid drag-resizes
                // can still queue several; processing them all here avoids the
                // common "stale art on the right edge" symptom (#65) caused by
                // the diff renderer skipping cells that match a stale back
                // buffer between intermediate sizes.
                let mut final_w = width;
                let mut final_h = height;
                while event::poll(Duration::from_millis(0)).unwrap_or(false) {
                    match event::read() {
                        Ok(Event::Resize(w, h)) => {
                            final_w = w;
                            final_h = h;
                        }
                        Ok(other) => {
                            // Non-resize event during the drain: we can't
                            // un-read it. Drop it and let the user re-issue
                            // — the resize-coalesce window is tiny.
                            tracing::debug!(
                                ?other,
                                "non-resize event during resize coalesce; dropping"
                            );
                            break;
                        }
                        Err(_) => break,
                    }
                }
                terminal.clear()?;
                app.handle_resize(final_w, final_h);
                // Draw immediately so the cleared screen gets repainted before
                // any other events can interleave. Without this, the next
                // iteration's draw can race against fast follow-up input and
                // leave the user staring at a blank/partial frame.
                terminal.draw(|f| render(f, app))?;
                app.needs_redraw = false;
                continue;
            }

            if app.use_mouse_capture
                && let Event::Mouse(mouse) = evt
            {
                handle_mouse_event(app, mouse);
                continue;
            }

            let Event::Key(key) = evt else {
                continue;
            };

            if key.kind != KeyEventKind::Press {
                continue;
            }

            // Handle onboarding flow
            if app.onboarding != OnboardingState::None {
                let advance_onboarding = |app: &mut App| {
                    app.status_message = None;
                    if app.onboarding_needs_api_key {
                        app.onboarding = OnboardingState::ApiKey;
                    } else if !app.trust_mode && onboarding::needs_trust(&app.workspace) {
                        app.onboarding = OnboardingState::TrustDirectory;
                    } else {
                        app.onboarding = OnboardingState::Tips;
                    }
                };

                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let _ = engine_handle.send(Op::Shutdown).await;
                        return Ok(());
                    }
                    KeyCode::Esc if app.onboarding == OnboardingState::ApiKey => {
                        app.onboarding = OnboardingState::Welcome;
                        app.api_key_input.clear();
                        app.api_key_cursor = 0;
                        app.status_message = None;
                    }
                    KeyCode::Enter => match app.onboarding {
                        OnboardingState::Welcome => {
                            advance_onboarding(app);
                        }
                        OnboardingState::ApiKey => {
                            let key = app.api_key_input.trim().to_string();
                            if let ApiKeyValidation::Reject(message) =
                                validate_api_key_for_onboarding(&key)
                            {
                                app.status_message = Some(message);
                                continue;
                            }
                            match app.submit_api_key() {
                                Ok(_) => {
                                    app.status_message = None;
                                    // Recreate the engine so it picks up the newly saved key
                                    // without requiring a full process restart.
                                    let _ = engine_handle.send(Op::Shutdown).await;
                                    let mut refreshed_config = config.clone();
                                    refreshed_config.api_key = Some(key);
                                    let engine_config = build_engine_config(app, &refreshed_config);
                                    engine_handle = spawn_engine(engine_config, &refreshed_config);

                                    if !app.api_messages.is_empty() {
                                        let _ = engine_handle
                                            .send(Op::SyncSession {
                                                messages: app.api_messages.clone(),
                                                system_prompt: app.system_prompt.clone(),
                                                model: app.model.clone(),
                                                workspace: app.workspace.clone(),
                                            })
                                            .await;
                                    }

                                    advance_onboarding(app);
                                }
                                Err(e) => {
                                    app.status_message = Some(e.to_string());
                                }
                            }
                        }
                        OnboardingState::TrustDirectory => {}
                        OnboardingState::Tips => {
                            app.finish_onboarding();
                        }
                        OnboardingState::None => {}
                    },
                    KeyCode::Char('y') | KeyCode::Char('Y')
                        if app.onboarding == OnboardingState::TrustDirectory =>
                    {
                        match onboarding::mark_trusted(&app.workspace) {
                            Ok(_) => {
                                app.trust_mode = true;
                                app.status_message = None;
                                app.onboarding = OnboardingState::Tips;
                            }
                            Err(err) => {
                                app.status_message =
                                    Some(format!("Failed to trust workspace: {err}"));
                            }
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Char('N')
                        if app.onboarding == OnboardingState::TrustDirectory =>
                    {
                        app.status_message = None;
                        app.onboarding = OnboardingState::Tips;
                    }
                    KeyCode::Backspace if app.onboarding == OnboardingState::ApiKey => {
                        app.delete_api_key_char();
                        sync_api_key_validation_status(app, false);
                    }
                    KeyCode::Char(c) if app.onboarding == OnboardingState::ApiKey => {
                        app.insert_api_key_char(c);
                        sync_api_key_validation_status(app, false);
                    }
                    KeyCode::Char('v') | KeyCode::Char('V')
                        if is_paste_shortcut(&key) && app.onboarding == OnboardingState::ApiKey =>
                    {
                        // Cmd+V / Ctrl+V paste (bracketed paste handled above)
                        app.paste_api_key_from_clipboard();
                        sync_api_key_validation_status(app, false);
                    }
                    _ => {}
                }
                continue;
            }

            if key.code == KeyCode::F(1) {
                if app.view_stack.top_kind() == Some(ModalKind::Help) {
                    app.view_stack.pop();
                } else {
                    app.view_stack
                        .push(HelpView::new_for_workspace(app.workspace.clone()));
                }
                continue;
            }

            if key.code == KeyCode::Char('/') && key.modifiers.contains(KeyModifiers::CONTROL) {
                if app.view_stack.top_kind() == Some(ModalKind::Help) {
                    app.view_stack.pop();
                } else {
                    app.view_stack
                        .push(HelpView::new_for_workspace(app.workspace.clone()));
                }
                continue;
            }

            if key.code == KeyCode::Char('k') && key.modifiers.contains(KeyModifiers::CONTROL) {
                // When the composer is the active input target (no modal/pager
                // intercepting keys), Ctrl+K performs an emacs-style kill to
                // end-of-line. If the kill is a no-op (cursor at end of empty
                // input), fall through to the existing command palette.
                if app.view_stack.is_empty() && app.kill_to_end_of_line() {
                    continue;
                }
                app.view_stack
                    .push(CommandPaletteView::new(build_command_palette_entries(
                        &app.skills_dir,
                        &app.workspace,
                    )));
                continue;
            }

            // Ctrl+P opens the fuzzy file-picker overlay. Bound only when the
            // composer is focused (no other modal on top of the stack) and the
            // engine is not actively streaming a turn.
            if key.code == KeyCode::Char('p')
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && app.view_stack.is_empty()
                && !app.is_loading
            {
                app.view_stack
                    .push(crate::tui::file_picker::FilePickerView::new(&app.workspace));
                continue;
            }

            if !app.view_stack.is_empty() {
                let events = app.view_stack.handle_key(key);
                if handle_view_events(app, config, &task_manager, &mut engine_handle, events)
                    .await?
                {
                    return Ok(());
                }
                continue;
            }

            let now = Instant::now();
            app.flush_paste_burst_if_due(now);

            // On Windows, AltGr is delivered as `Ctrl+Alt`; treat
            // AltGr-typed chars (e.g. European layouts producing `@`, `\`,
            // `|`) as plain text rather than swallowing them as a modified
            // shortcut. `key_hint::has_ctrl_or_alt` filters AltGr out.
            let has_ctrl_alt_or_super = super::widgets::key_hint::has_ctrl_or_alt(key.modifiers)
                || key.modifiers.contains(KeyModifiers::SUPER);
            let is_plain_char = matches!(key.code, KeyCode::Char(_)) && !has_ctrl_alt_or_super;
            let is_enter = matches!(key.code, KeyCode::Enter);

            if !is_plain_char
                && !is_enter
                && let Some(pending) = app.paste_burst.flush_before_modified_input()
            {
                app.insert_str(&pending);
            }

            if (is_plain_char || is_enter) && super::paste::handle_paste_burst_key(app, &key, now) {
                continue;
            }

            let slash_menu_entries = visible_slash_menu_entries(app, SLASH_MENU_LIMIT);
            let slash_menu_open = !slash_menu_entries.is_empty();
            if slash_menu_open && app.slash_menu_selected >= slash_menu_entries.len() {
                app.slash_menu_selected = slash_menu_entries.len().saturating_sub(1);
            }
            let mention_menu_entries =
                crate::tui::file_mention::visible_mention_menu_entries(app, MENTION_MENU_LIMIT);
            let mention_menu_open = !mention_menu_entries.is_empty();
            if mention_menu_open && app.mention_menu_selected >= mention_menu_entries.len() {
                app.mention_menu_selected = mention_menu_entries.len().saturating_sub(1);
            }

            // Global keybindings
            match key.code {
                KeyCode::Enter
                    if app.input.is_empty()
                        && app.transcript_selection.is_active()
                        && open_pager_for_selection(app) =>
                {
                    continue;
                }
                KeyCode::Char('l')
                    if key.modifiers.is_empty()
                        && app.input.is_empty()
                        && open_pager_for_last_message(app) =>
                {
                    continue;
                }
                KeyCode::Char('v')
                    if key.modifiers.is_empty()
                        && app.input.is_empty()
                        && open_tool_details_pager(app) =>
                {
                    continue;
                }
                KeyCode::Char('o')
                    if key.modifiers == KeyModifiers::CONTROL && open_thinking_pager(app) =>
                {
                    continue;
                }
                KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::ALT) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.set_sidebar_focus(SidebarFocus::Plan);
                        app.status_message = Some("Sidebar focus: plan".to_string());
                    } else {
                        app.set_mode(AppMode::Plan);
                    }
                    continue;
                }
                KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::ALT) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.set_sidebar_focus(SidebarFocus::Todos);
                        app.status_message = Some("Sidebar focus: todos".to_string());
                    } else {
                        app.set_mode(AppMode::Agent);
                    }
                    continue;
                }
                KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::ALT) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.set_sidebar_focus(SidebarFocus::Tasks);
                        app.status_message = Some("Sidebar focus: tasks".to_string());
                    } else {
                        app.set_mode(AppMode::Yolo);
                    }
                    continue;
                }
                KeyCode::Char('4') if key.modifiers.contains(KeyModifiers::ALT) => {
                    apply_alt_4_shortcut(app, key.modifiers);
                    continue;
                }
                KeyCode::Char('!') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_sidebar_focus(SidebarFocus::Plan);
                    app.status_message = Some("Sidebar focus: plan".to_string());
                    continue;
                }
                KeyCode::Char('@') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_sidebar_focus(SidebarFocus::Todos);
                    app.status_message = Some("Sidebar focus: todos".to_string());
                    continue;
                }
                KeyCode::Char('#') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_sidebar_focus(SidebarFocus::Tasks);
                    app.status_message = Some("Sidebar focus: tasks".to_string());
                    continue;
                }
                KeyCode::Char('$') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_sidebar_focus(SidebarFocus::Agents);
                    app.status_message = Some("Sidebar focus: agents".to_string());
                    continue;
                }
                KeyCode::Char(')') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_sidebar_focus(SidebarFocus::Auto);
                    app.status_message = Some("Sidebar focus: auto".to_string());
                    continue;
                }
                KeyCode::Char('0') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_sidebar_focus(SidebarFocus::Auto);
                    app.status_message = Some("Sidebar focus: auto".to_string());
                    continue;
                }
                KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.view_stack.push(SessionPickerView::new());
                    continue;
                }
                KeyCode::Char('c') | KeyCode::Char('C')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && app.transcript_selection.is_active() =>
                {
                    copy_active_selection(app);
                }
                KeyCode::Char('c') | KeyCode::Char('C') if is_copy_shortcut(&key) => {
                    copy_active_selection(app);
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Three behaviors layered on Ctrl+C, in priority order:
                    //   1. While a turn is in flight, cancel it (unchanged).
                    //   2. Otherwise, on the first press, arm a 2-second
                    //      "press Ctrl+C again to quit" prompt and stay
                    //      running.
                    //   3. On the second press while still armed, exit cleanly.
                    // The prompt expires silently after the window so a
                    // stray Ctrl+C three seconds later re-arms instead of
                    // accidentally exiting.
                    if app.is_loading {
                        engine_handle.cancel();
                        app.is_loading = false;
                        app.streaming_state.reset();
                        app.status_message = Some("Request cancelled".to_string());
                        app.disarm_quit();
                    } else if app.quit_is_armed() {
                        let _ = engine_handle.send(Op::Shutdown).await;
                        return Ok(());
                    } else {
                        app.arm_quit();
                    }
                }
                KeyCode::Char('d')
                    if key.modifiers.contains(KeyModifiers::CONTROL) && app.input.is_empty() =>
                {
                    let _ = engine_handle.send(Op::Shutdown).await;
                    return Ok(());
                }
                KeyCode::Esc if mention_menu_open => {
                    app.mention_menu_hidden = true;
                    app.mention_menu_selected = 0;
                }
                KeyCode::Esc => match next_escape_action(app, slash_menu_open) {
                    EscapeAction::CloseSlashMenu => app.close_slash_menu(),
                    EscapeAction::CancelRequest => {
                        engine_handle.cancel();
                        app.is_loading = false;
                        app.streaming_state.reset();
                        app.status_message = Some("Request cancelled".to_string());
                    }
                    EscapeAction::DiscardQueuedDraft => {
                        app.queued_draft = None;
                        app.status_message = Some("Stopped editing queued message".to_string());
                    }
                    EscapeAction::ClearInput => app.clear_input(),
                    EscapeAction::Noop => {}
                },
                KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.scroll_up(3);
                }
                KeyCode::Up
                    if key.modifiers.is_empty()
                        && mention_menu_open
                        && app.mention_menu_selected > 0 =>
                {
                    app.mention_menu_selected = app.mention_menu_selected.saturating_sub(1);
                }
                KeyCode::Up
                    if key.modifiers.is_empty()
                        && slash_menu_open
                        && app.slash_menu_selected > 0 =>
                {
                    app.slash_menu_selected = app.slash_menu_selected.saturating_sub(1);
                }
                KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.scroll_down(3);
                }
                KeyCode::Down if key.modifiers.is_empty() && mention_menu_open => {
                    app.mention_menu_selected = (app.mention_menu_selected + 1)
                        .min(mention_menu_entries.len().saturating_sub(1));
                }
                KeyCode::Down if key.modifiers.is_empty() && slash_menu_open => {
                    app.slash_menu_selected = (app.slash_menu_selected + 1)
                        .min(slash_menu_entries.len().saturating_sub(1));
                }
                KeyCode::PageUp => {
                    let page = app.last_transcript_visible.max(1);
                    app.scroll_up(page);
                }
                KeyCode::PageDown => {
                    let page = app.last_transcript_visible.max(1);
                    app.scroll_down(page);
                }
                KeyCode::Tab => {
                    if mention_menu_open
                        && crate::tui::file_mention::apply_mention_menu_selection(
                            app,
                            &mention_menu_entries,
                        )
                    {
                        continue;
                    }
                    if slash_menu_open && apply_slash_menu_selection(app, &slash_menu_entries, true)
                    {
                        continue;
                    }
                    if try_autocomplete_slash_command(app) {
                        continue;
                    }
                    if crate::tui::file_mention::try_autocomplete_file_mention(app) {
                        continue;
                    }
                    let prior_model = app.model.clone();
                    app.cycle_mode();
                    if app.model != prior_model {
                        let _ = engine_handle
                            .send(Op::SetModel {
                                model: app.model.clone(),
                            })
                            .await;
                    }
                }
                KeyCode::BackTab => {
                    app.cycle_effort();
                }
                KeyCode::Char('g')
                    if key.modifiers.is_empty() && app.input.is_empty() && !slash_menu_open =>
                {
                    if let Some(anchor) =
                        TranscriptScroll::anchor_for(app.transcript_cache.line_meta(), 0)
                    {
                        app.transcript_scroll = anchor;
                    }
                }
                KeyCode::Char('G')
                    if (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
                        && app.input.is_empty()
                        && !slash_menu_open =>
                {
                    app.scroll_to_bottom();
                }
                KeyCode::Char('[')
                    if key.modifiers.is_empty()
                        && app.input.is_empty()
                        && !slash_menu_open
                        && !jump_to_adjacent_tool_cell(app, SearchDirection::Backward) =>
                {
                    app.status_message = Some("No previous tool output".to_string());
                }
                KeyCode::Char(']')
                    if key.modifiers.is_empty()
                        && app.input.is_empty()
                        && !slash_menu_open
                        && !jump_to_adjacent_tool_cell(app, SearchDirection::Forward) =>
                {
                    app.status_message = Some("No next tool output".to_string());
                }
                // Input handling
                KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.insert_char('\n');
                }
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.insert_char('\n');
                }
                KeyCode::Enter
                    if mention_menu_open
                        && crate::tui::file_mention::apply_mention_menu_selection(
                            app,
                            &mention_menu_entries,
                        ) =>
                {
                    continue;
                }
                KeyCode::Enter => {
                    if let Some(input) = app.submit_input() {
                        if handle_plan_choice(app, &engine_handle, &input).await? {
                            continue;
                        }
                        if input.starts_with('/') {
                            if execute_command_input(
                                app,
                                &mut engine_handle,
                                &task_manager,
                                config,
                                &input,
                            )
                            .await?
                            {
                                return Ok(());
                            }
                        } else {
                            let queued = if let Some(mut draft) = app.queued_draft.take() {
                                draft.display = input;
                                draft
                            } else {
                                build_queued_message(app, input)
                            };
                            submit_or_steer_message(app, &engine_handle, queued).await?;
                        }
                    }
                }
                KeyCode::Backspace => {
                    app.delete_char();
                }
                KeyCode::Delete => {
                    app.delete_char_forward();
                }
                KeyCode::Left => {
                    app.move_cursor_left();
                }
                KeyCode::Right => {
                    app.move_cursor_right();
                }
                KeyCode::Home if key.modifiers.is_empty() => {
                    if let Some(anchor) =
                        TranscriptScroll::anchor_for(app.transcript_cache.line_meta(), 0)
                    {
                        app.transcript_scroll = anchor;
                    }
                }
                KeyCode::End if key.modifiers.is_empty() => {
                    app.scroll_to_bottom();
                }
                KeyCode::Home | KeyCode::Char('a')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.move_cursor_start();
                }
                KeyCode::End => {
                    app.move_cursor_end();
                }
                KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Ctrl+E: spawn $EDITOR on the composer contents (#91).
                    // Only fires when no modal is active (the !view_stack
                    // branch above already returns early in that case) and
                    // the composer is the focused input target. We accept the
                    // shortcut whether or not a model turn is streaming —
                    // editing the buffer never disturbs in-flight work.
                    let seed = app.input.clone();
                    match super::external_editor::spawn_editor_for_input(
                        terminal,
                        app.use_alt_screen,
                        app.use_mouse_capture,
                        app.use_bracketed_paste,
                        &seed,
                    ) {
                        Ok(super::external_editor::EditorOutcome::Edited(new)) => {
                            app.input = new;
                            app.move_cursor_end();
                            let editor = std::env::var("VISUAL")
                                .ok()
                                .filter(|s| !s.trim().is_empty())
                                .or_else(|| {
                                    std::env::var("EDITOR")
                                        .ok()
                                        .filter(|s| !s.trim().is_empty())
                                })
                                .unwrap_or_else(|| "vi".to_string());
                            app.status_message = Some(format!("Edited in {editor}"));
                        }
                        Ok(super::external_editor::EditorOutcome::Unchanged) => {
                            app.status_message = Some("Editor closed (no changes)".to_string());
                        }
                        Ok(super::external_editor::EditorOutcome::Cancelled) => {
                            app.status_message = Some("Editor cancelled".to_string());
                        }
                        Err(err) => {
                            app.status_message = Some(format!("Editor error: {err}"));
                        }
                    }
                    app.needs_redraw = true;
                }
                KeyCode::Up => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.history_up();
                    } else if should_scroll_with_arrows(app) {
                        app.scroll_up(1);
                    } else {
                        app.history_up();
                    }
                }
                KeyCode::Down => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.history_down();
                    } else if should_scroll_with_arrows(app) {
                        app.scroll_down(1);
                    } else {
                        app.history_down();
                    }
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.clear_input();
                }
                KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Emacs-style yank from the kill buffer at the cursor.
                    // No-op when the buffer is empty.
                    app.yank();
                }
                KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let new_mode = match app.mode {
                        AppMode::Plan => AppMode::Agent,
                        _ => AppMode::Plan,
                    };
                    app.set_mode(new_mode);
                }
                KeyCode::Char('v') if is_paste_shortcut(&key) => {
                    app.paste_from_clipboard();
                }
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_mode(AppMode::Agent);
                    continue;
                }
                KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_mode(AppMode::Yolo);
                    continue;
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_mode(AppMode::Plan);
                    continue;
                }
                KeyCode::Char('A') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_mode(AppMode::Agent);
                    continue;
                }
                KeyCode::Char('Y') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_mode(AppMode::Yolo);
                    continue;
                }
                KeyCode::Char('P') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_mode(AppMode::Plan);
                    continue;
                }
                KeyCode::Char(c) => {
                    app.insert_char(c);
                }
                _ => {}
            }

            if !is_plain_char && !is_enter {
                app.paste_burst.clear_window_after_non_char();
            }
        }
    }
}

fn apply_alt_4_shortcut(app: &mut App, modifiers: KeyModifiers) {
    if modifiers.contains(KeyModifiers::CONTROL) {
        app.set_sidebar_focus(SidebarFocus::Agents);
        app.status_message = Some("Sidebar focus: agents".to_string());
    } else {
        app.set_mode(AppMode::Plan);
    }
}

async fn fetch_available_models(config: &Config) -> Result<Vec<String>> {
    use crate::client::DeepSeekClient;

    let client = DeepSeekClient::new(config)?;
    let models = tokio::time::timeout(Duration::from_secs(20), client.list_models()).await??;
    let mut ids = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn format_available_models_message(current_model: &str, models: &[String]) -> String {
    let mut lines = vec![format!("Available models ({})", models.len())];
    for model in models {
        if model == current_model {
            lines.push(format!("* {model} (current)"));
        } else {
            lines.push(format!("  {model}"));
        }
    }
    lines.join("\n")
}

fn build_session_snapshot(app: &App, manager: &SessionManager) -> SavedSession {
    if let Some(ref existing_id) = app.current_session_id
        && let Ok(existing) = manager.load_session(existing_id)
    {
        let mut updated = update_session(
            existing,
            &app.api_messages,
            u64::from(app.total_tokens),
            app.system_prompt.as_ref(),
        );
        updated.metadata.mode = Some(app.mode.as_setting().to_string());
        updated
    } else {
        create_saved_session_with_mode(
            &app.api_messages,
            &app.model,
            &app.workspace,
            u64::from(app.total_tokens),
            app.system_prompt.as_ref(),
            Some(app.mode.as_setting()),
        )
    }
}

fn persist_session_snapshot(app: &mut App) {
    if let Ok(manager) = SessionManager::default_location() {
        let session = build_session_snapshot(app, &manager);
        if let Err(err) = manager.save_session(&session) {
            eprintln!("Failed to save session: {err}");
        } else {
            app.current_session_id = Some(session.metadata.id.clone());
        }
    }
}

fn persist_checkpoint(app: &mut App) {
    if let Ok(manager) = SessionManager::default_location() {
        let session = build_session_snapshot(app, &manager);
        if let Err(err) = manager.save_checkpoint(&session) {
            eprintln!("Failed to save checkpoint: {err}");
        }
    }
}

fn clear_checkpoint() {
    if let Ok(manager) = SessionManager::default_location() {
        let _ = manager.clear_checkpoint();
    }
}

fn queued_ui_to_session(msg: &QueuedMessage) -> QueuedSessionMessage {
    QueuedSessionMessage {
        display: msg.display.clone(),
        skill_instruction: msg.skill_instruction.clone(),
    }
}

fn queued_session_to_ui(msg: QueuedSessionMessage) -> QueuedMessage {
    QueuedMessage {
        display: msg.display,
        skill_instruction: msg.skill_instruction,
    }
}

/// Translate an `EngineEvent::Error` into UI state updates.
///
/// `recoverable` is the engine's own classification: stream stalls, chunk
/// timeouts, transient network errors, and rate-limit/server hiccups arrive
/// with `recoverable = true` and must NOT flip the session into offline mode
/// — the user can resend the turn and the underlying transport will retry.
/// Hard failures (auth, billing, invalid request) arrive with
/// `recoverable = false`; those flip offline mode so subsequent messages get
/// queued instead of silently lost mid-flight.
pub(crate) fn apply_engine_error_to_app(app: &mut App, message: String, recoverable: bool) {
    app.streaming_state.reset();
    app.streaming_message_index = None;
    app.streaming_thinking_active_entry = None;
    app.add_message(HistoryCell::System {
        content: format!("Error: {message}"),
    });
    app.is_loading = false;
    if recoverable {
        app.status_message = Some(format!("Connection interrupted: {message}"));
    } else {
        app.offline_mode = true;
        app.status_message = Some(format!(
            "Engine error; queued messages stay pending: {message}"
        ));
    }
}

fn persist_offline_queue_state(app: &App) {
    if let Ok(manager) = SessionManager::default_location() {
        if app.queued_messages.is_empty() && app.queued_draft.is_none() {
            let _ = manager.clear_offline_queue_state();
            return;
        }
        let state = OfflineQueueState {
            messages: app
                .queued_messages
                .iter()
                .map(queued_ui_to_session)
                .collect(),
            draft: app.queued_draft.as_ref().map(queued_ui_to_session),
            ..OfflineQueueState::default()
        };
        let _ = manager.save_offline_queue_state(&state);
    }
}

fn sanitize_stream_chunk(chunk: &str) -> String {
    // Keep printable characters and common whitespace; drop control bytes.
    chunk
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect()
}

/// Ensure an in-flight streaming Assistant cell exists in history and return
/// its index. Thinking cells go through `ensure_streaming_thinking_active_entry`
/// (active cell) instead.
fn ensure_streaming_assistant_history_cell(app: &mut App) -> usize {
    if let Some(index) = app.streaming_message_index {
        return index;
    }
    app.add_message(HistoryCell::Assistant {
        content: String::new(),
        streaming: true,
    });
    let index = app.history.len().saturating_sub(1);
    app.streaming_message_index = Some(index);
    index
}

fn append_streaming_text(app: &mut App, index: usize, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(HistoryCell::Assistant { content, .. }) = app.history.get_mut(index) {
        content.push_str(text);
        // Bump only the streaming cell's per-cell revision so the transcript
        // cache re-renders just this cell. Without this, the cache would
        // either skip the update entirely (now that the global
        // history_version is no longer fanned out across every cell) or fall
        // back to a full re-wrap of the entire transcript every chunk.
        app.bump_history_cell(index);
    }
}

/// Ensure an in-flight Thinking entry exists in `active_cell` and return its
/// entry index. If no thinking entry is currently streaming, push a fresh one.
/// P2.3: thinking shares the active cell with subsequent tool calls so the
/// pair render as one logical "Working…" block.
fn ensure_streaming_thinking_active_entry(app: &mut App) -> usize {
    if let Some(idx) = app.streaming_thinking_active_entry {
        return idx;
    }
    if app.active_cell.is_none() {
        app.active_cell = Some(ActiveCell::new());
    }
    let active = app.active_cell.as_mut().expect("active_cell just ensured");
    let entry_idx = active.push_thinking(HistoryCell::Thinking {
        content: String::new(),
        streaming: true,
        duration_secs: None,
    });
    app.streaming_thinking_active_entry = Some(entry_idx);
    app.bump_active_cell_revision();
    entry_idx
}

/// Append text to a streaming Thinking entry inside `active_cell`. Bumps the
/// active-cell revision so the renderer re-draws the live tail.
fn append_streaming_thinking(app: &mut App, entry_idx: usize, text: &str) {
    if text.is_empty() {
        return;
    }
    let mutated = if let Some(active) = app.active_cell.as_mut()
        && let Some(HistoryCell::Thinking { content, .. }) = active.entry_mut(entry_idx)
    {
        content.push_str(text);
        true
    } else {
        false
    };
    if mutated {
        app.bump_active_cell_revision();
    }
}

/// Finalize the in-flight thinking entry in `active_cell`: append the
/// collector's remaining buffered text, stop the spinner, and stamp the
/// duration. Returns `true` when a thinking entry was finalized (so the
/// dispatch loop knows the transcript was touched). No-op if no thinking
/// entry is currently streaming.
fn finalize_streaming_thinking_active_entry(
    app: &mut App,
    duration: Option<f32>,
    remaining: &str,
) -> bool {
    let Some(entry_idx) = app.streaming_thinking_active_entry.take() else {
        return false;
    };
    if !remaining.is_empty() {
        append_streaming_thinking(app, entry_idx, remaining);
    }
    if let Some(active) = app.active_cell.as_mut()
        && let Some(HistoryCell::Thinking {
            streaming,
            duration_secs,
            ..
        }) = active.entry_mut(entry_idx)
    {
        *streaming = false;
        *duration_secs = duration;
    }
    app.bump_active_cell_revision();
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscapeAction {
    CloseSlashMenu,
    CancelRequest,
    DiscardQueuedDraft,
    ClearInput,
    Noop,
}

fn next_escape_action(app: &App, slash_menu_open: bool) -> EscapeAction {
    if slash_menu_open {
        EscapeAction::CloseSlashMenu
    } else if app.is_loading {
        EscapeAction::CancelRequest
    } else if app.queued_draft.is_some() && app.input.is_empty() {
        EscapeAction::DiscardQueuedDraft
    } else if !app.input.is_empty() {
        EscapeAction::ClearInput
    } else {
        EscapeAction::Noop
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ApiKeyValidation {
    Accept { warning: Option<String> },
    Reject(String),
}

fn validate_api_key_for_onboarding(api_key: &str) -> ApiKeyValidation {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return ApiKeyValidation::Reject("API key cannot be empty.".to_string());
    }
    if trimmed.contains(char::is_whitespace) {
        return ApiKeyValidation::Reject(
            "API key appears malformed (contains whitespace).".to_string(),
        );
    }
    if trimmed.len() < 16 {
        return ApiKeyValidation::Accept {
            warning: Some(
                "API key looks short. Double-check it, but unusual formats are allowed."
                    .to_string(),
            ),
        };
    }
    if !trimmed.contains('-') {
        return ApiKeyValidation::Accept {
            warning: Some(
                "API key format looks unusual. Check that the full key was copied.".to_string(),
            ),
        };
    }
    ApiKeyValidation::Accept { warning: None }
}

fn sync_api_key_validation_status(app: &mut App, show_empty_error: bool) {
    if app.api_key_input.trim().is_empty() && !show_empty_error {
        app.status_message = None;
        return;
    }

    match validate_api_key_for_onboarding(&app.api_key_input) {
        ApiKeyValidation::Accept { warning } => {
            app.status_message = warning;
        }
        ApiKeyValidation::Reject(message) => {
            app.status_message = Some(message);
        }
    }
}

fn build_queued_message(app: &mut App, input: String) -> QueuedMessage {
    let skill_instruction = app.active_skill.take();
    QueuedMessage::new(input, skill_instruction)
}

fn queued_message_content_for_app(app: &App, message: &QueuedMessage) -> String {
    let user_request =
        crate::tui::file_mention::user_request_with_file_mentions(&message.display, &app.workspace);
    if let Some(skill_instruction) = message.skill_instruction.as_ref() {
        format!("{skill_instruction}\n\n---\n\nUser request: {user_request}")
    } else {
        user_request
    }
}

async fn dispatch_user_message(
    app: &mut App,
    engine_handle: &EngineHandle,
    message: QueuedMessage,
) -> Result<()> {
    // Set immediately to prevent double-dispatch before TurnStarted event arrives.
    app.is_loading = true;
    app.last_send_at = Some(Instant::now());

    let content = queued_message_content_for_app(app, &message);
    app.system_prompt = Some(prompts::system_prompt_for_mode_with_context(
        app.mode,
        &app.workspace,
        None,
    ));
    app.add_message(HistoryCell::User {
        content: message.display.clone(),
    });
    app.scroll_to_bottom();
    app.api_messages.push(Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: content.clone(),
            cache_control: None,
        }],
    });
    maybe_warn_context_pressure(app);
    if should_auto_compact_before_send(app) {
        app.status_message = Some("Context critical; compacting before send...".to_string());
        let _ = engine_handle.send(Op::CompactContext).await;
    }
    app.last_prompt_tokens = None;
    app.last_completion_tokens = None;
    app.last_prompt_cache_hit_tokens = None;
    app.last_prompt_cache_miss_tokens = None;
    app.last_reasoning_replay_tokens = None;
    // Persist immediately so abrupt termination can recover this in-flight turn.
    persist_checkpoint(app);

    engine_handle
        .send(Op::SendMessage {
            content,
            mode: app.mode,
            model: app.model.clone(),
            reasoning_effort: app.reasoning_effort.api_value().map(str::to_string),
            allow_shell: app.allow_shell,
            trust_mode: app.trust_mode,
            auto_approve: app.mode == AppMode::Yolo,
        })
        .await?;

    Ok(())
}

async fn apply_model_and_compaction_update(
    engine_handle: &EngineHandle,
    compaction: crate::compaction::CompactionConfig,
) {
    let _ = engine_handle
        .send(Op::SetModel {
            model: compaction.model.clone(),
        })
        .await;
    let _ = engine_handle
        .send(Op::SetCompaction { config: compaction })
        .await;
}

/// Apply the choice made in the `/model` picker (#39): mutate App state so
/// the next turn uses the new model/effort, persist the selection to
/// `~/.deepseek/settings.toml` so it survives a restart, push the change to
/// the running engine via `Op::SetModel`/`Op::SetCompaction`, and surface
/// a one-line status describing what changed.
async fn apply_model_picker_choice(
    app: &mut App,
    engine_handle: &EngineHandle,
    model: String,
    effort: crate::tui::app::ReasoningEffort,
    previous_model: String,
    previous_effort: crate::tui::app::ReasoningEffort,
) {
    let model_changed = model != previous_model;
    let effort_changed = effort != previous_effort;
    if !model_changed && !effort_changed {
        app.status_message = Some(format!(
            "Model unchanged: {model} · thinking {}",
            effort.short_label()
        ));
        return;
    }

    if model_changed {
        app.model = model.clone();
        app.update_model_compaction_budget();
        app.last_prompt_tokens = None;
        app.last_completion_tokens = None;
        app.last_prompt_cache_hit_tokens = None;
        app.last_prompt_cache_miss_tokens = None;
        app.last_reasoning_replay_tokens = None;
    }
    if effort_changed {
        app.reasoning_effort = effort;
    }

    // Best-effort persist; surface a status warning if the settings file
    // can't be written rather than aborting the in-memory change.
    let mut persist_warning: Option<String> = None;
    match crate::settings::Settings::load() {
        Ok(mut settings) => {
            if model_changed {
                let _ = settings.set("default_model", &model);
            }
            if effort_changed {
                let _ = settings.set("reasoning_effort", effort.as_setting());
            }
            if let Err(err) = settings.save() {
                persist_warning = Some(format!("(not persisted: {err})"));
            }
        }
        Err(err) => {
            persist_warning = Some(format!("(not persisted: {err})"));
        }
    }

    if model_changed {
        apply_model_and_compaction_update(engine_handle, app.compaction_config()).await;
    }

    let mut summary = match (model_changed, effort_changed) {
        (true, true) => format!(
            "Model: {previous_model} → {model} · thinking: {} → {}",
            previous_effort.short_label(),
            effort.short_label()
        ),
        (true, false) => format!(
            "Model: {previous_model} → {model} · thinking {}",
            effort.short_label()
        ),
        (false, true) => format!(
            "Thinking: {} → {} · model {model}",
            previous_effort.short_label(),
            effort.short_label()
        ),
        (false, false) => unreachable!(),
    };
    if let Some(warning) = persist_warning {
        summary.push(' ');
        summary.push_str(&warning);
    }
    app.status_message = Some(summary);
}

/// Apply a `/provider` switch by mutating the in-memory config, validating
/// that credentials exist for the new provider, then respawning the engine
/// so the API client picks up the new base URL/key. When `model_override`
/// is set, it replaces the active model post-switch (already normalized,
/// will be provider-prefixed by `Config::default_model`).
async fn switch_provider(
    app: &mut App,
    engine_handle: &mut EngineHandle,
    config: &mut Config,
    target: ApiProvider,
    model_override: Option<String>,
) {
    let previous_provider = app.api_provider;
    let previous_model = app.model.clone();
    let previous_provider_str = config.provider.clone();
    let previous_base_url = config.base_url.clone();
    let previous_default_text_model = config.default_text_model.clone();

    config.provider = Some(target.as_str().to_string());
    if matches!(target, ApiProvider::NvidiaNim)
        && config
            .base_url
            .as_deref()
            .map(|base| !base.contains("integrate.api.nvidia.com"))
            .unwrap_or(true)
    {
        config.base_url = Some(DEFAULT_NVIDIA_NIM_BASE_URL.to_string());
    }
    if matches!(target, ApiProvider::Deepseek)
        && config
            .base_url
            .as_deref()
            .map(|base| base.contains("integrate.api.nvidia.com"))
            .unwrap_or(false)
    {
        config.base_url = None;
    }
    if let Some(ref model) = model_override {
        config.default_text_model = Some(model.clone());
    }

    if let Err(err) = DeepSeekClient::new(config) {
        config.provider = previous_provider_str;
        config.base_url = previous_base_url;
        config.default_text_model = previous_default_text_model;
        app.add_message(HistoryCell::System {
            content: format!(
                "Failed to switch provider to {}: {err}\nProvider unchanged ({}).",
                target.as_str(),
                previous_provider.as_str()
            ),
        });
        return;
    }

    let new_model = config.default_model();
    app.api_provider = target;
    app.model = new_model.clone();
    app.update_model_compaction_budget();
    app.last_prompt_tokens = None;
    app.last_completion_tokens = None;

    let _ = engine_handle.send(Op::Shutdown).await;
    let engine_config = build_engine_config(app, config);
    *engine_handle = spawn_engine(engine_config, config);

    if !app.api_messages.is_empty() {
        let _ = engine_handle
            .send(Op::SyncSession {
                messages: app.api_messages.clone(),
                system_prompt: app.system_prompt.clone(),
                model: app.model.clone(),
                workspace: app.workspace.clone(),
            })
            .await;
    }
    let _ = engine_handle
        .send(Op::SetCompaction {
            config: app.compaction_config(),
        })
        .await;

    app.add_message(HistoryCell::System {
        content: format!(
            "Provider switched: {} → {}\nModel: {} → {}",
            previous_provider.as_str(),
            target.as_str(),
            previous_model,
            new_model
        ),
    });
    app.status_message = Some(format!("Provider: {}", target.as_str()));
}

fn open_text_pager(app: &mut App, title: String, content: String) {
    let width = app
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(80);
    app.view_stack.push(PagerView::from_text(
        title,
        &content,
        width.saturating_sub(2),
    ));
}

async fn apply_command_result(
    app: &mut App,
    engine_handle: &mut EngineHandle,
    task_manager: &SharedTaskManager,
    config: &mut Config,
    result: commands::CommandResult,
) -> Result<bool> {
    if let Some(msg) = result.message {
        app.add_message(HistoryCell::System { content: msg });
    }

    if let Some(action) = result.action {
        match action {
            AppAction::Quit => {
                let _ = engine_handle.send(Op::Shutdown).await;
                return Ok(true);
            }
            AppAction::SaveSession(path) => {
                app.status_message = Some(format!("Session saved to {}", path.display()));
            }
            AppAction::LoadSession(path) => {
                app.status_message = Some(format!("Session loaded from {}", path.display()));
            }
            AppAction::SyncSession {
                messages,
                system_prompt,
                model,
                workspace,
            } => {
                let is_full_reset = messages.is_empty() && system_prompt.is_none();
                let _ = engine_handle
                    .send(Op::SyncSession {
                        messages,
                        system_prompt,
                        model,
                        workspace,
                    })
                    .await;
                let _ = engine_handle
                    .send(Op::SetCompaction {
                        config: app.compaction_config(),
                    })
                    .await;
                if is_full_reset {
                    persist_session_snapshot(app);
                    clear_checkpoint();
                }
            }
            AppAction::SendMessage(content) => {
                let queued = build_queued_message(app, content);
                submit_or_steer_message(app, engine_handle, queued).await?;
            }
            AppAction::RlmQuery {
                prompt,
                model,
                child_model,
                max_depth,
            } => {
                app.status_message = Some("RLM turn starting (Algorithm 1)...".to_string());
                let _ = engine_handle
                    .send(Op::RlmQuery {
                        content: prompt,
                        model,
                        child_model,
                        max_depth,
                    })
                    .await;
            }
            AppAction::ListSubAgents => {
                let _ = engine_handle.send(Op::ListSubAgents).await;
            }
            AppAction::FetchModels => {
                app.status_message = Some("Fetching models...".to_string());
                match fetch_available_models(config).await {
                    Ok(models) => {
                        app.add_message(HistoryCell::System {
                            content: format_available_models_message(&app.model, &models),
                        });
                        app.status_message = Some(format!("Found {} model(s)", models.len()));
                    }
                    Err(error) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Failed to fetch models: {error}"),
                        });
                    }
                }
            }
            AppAction::SwitchProvider { provider, model } => {
                switch_provider(app, engine_handle, config, provider, model).await;
            }
            AppAction::UpdateCompaction(compaction) => {
                apply_model_and_compaction_update(engine_handle, compaction).await;
            }
            AppAction::OpenConfigView => {
                if app.view_stack.top_kind() != Some(ModalKind::Config) {
                    app.view_stack.push(ConfigView::new_for_app(app));
                }
            }
            AppAction::OpenModelPicker => {
                if app.view_stack.top_kind() != Some(ModalKind::ModelPicker) {
                    app.view_stack
                        .push(crate::tui::model_picker::ModelPickerView::new(app));
                }
            }
            AppAction::CompactContext => {
                app.status_message = Some("Compacting context...".to_string());
                let _ = engine_handle.send(Op::CompactContext).await;
            }
            AppAction::TaskAdd { prompt } => {
                let request = NewTaskRequest {
                    prompt: prompt.clone(),
                    model: Some(app.model.clone()),
                    workspace: Some(app.workspace.clone()),
                    mode: Some(task_mode_label(app.mode).to_string()),
                    allow_shell: Some(app.allow_shell),
                    trust_mode: Some(app.trust_mode),
                    auto_approve: Some(app.approval_mode == ApprovalMode::Auto),
                };
                match task_manager.add_task(request).await {
                    Ok(task) => {
                        app.add_message(HistoryCell::System {
                            content: format!(
                                "Task queued: {} ({})",
                                task.id,
                                summarize_tool_output(&task.prompt)
                            ),
                        });
                        app.status_message = Some(format!("Queued {}", task.id));
                    }
                    Err(err) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Failed to queue task: {err}"),
                        });
                    }
                }
                app.task_panel = task_manager
                    .list_tasks(Some(10))
                    .await
                    .into_iter()
                    .map(task_summary_to_panel_entry)
                    .collect();
            }
            AppAction::TaskList => {
                let tasks = task_manager.list_tasks(Some(30)).await;
                app.task_panel = tasks
                    .iter()
                    .cloned()
                    .map(task_summary_to_panel_entry)
                    .collect();
                app.add_message(HistoryCell::System {
                    content: format_task_list(&tasks),
                });
            }
            AppAction::TaskShow { id } => match task_manager.get_task(&id).await {
                Ok(task) => open_task_pager(app, &task),
                Err(err) => {
                    app.add_message(HistoryCell::System {
                        content: format!("Task lookup failed: {err}"),
                    });
                }
            },
            AppAction::TaskCancel { id } => {
                match task_manager.cancel_task(&id).await {
                    Ok(task) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Task {} status: {:?}", task.id, task.status),
                        });
                    }
                    Err(err) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Task cancel failed: {err}"),
                        });
                    }
                }
                app.task_panel = task_manager
                    .list_tasks(Some(10))
                    .await
                    .into_iter()
                    .map(task_summary_to_panel_entry)
                    .collect();
            }
        }
    }

    Ok(false)
}

async fn execute_command_input(
    app: &mut App,
    engine_handle: &mut EngineHandle,
    task_manager: &SharedTaskManager,
    config: &mut Config,
    input: &str,
) -> Result<bool> {
    let result = commands::execute(input, app);
    apply_command_result(app, engine_handle, task_manager, config, result).await
}

async fn steer_user_message(
    app: &mut App,
    engine_handle: &EngineHandle,
    message: QueuedMessage,
) -> Result<()> {
    let content = queued_message_content_for_app(app, &message);

    // Mirror steer input in local transcript/session state.
    app.add_message(HistoryCell::User {
        content: format!("+ {}", message.display),
    });
    app.api_messages.push(Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: content.clone(),
            cache_control: None,
        }],
    });

    engine_handle.steer(content).await?;
    app.status_message = Some("Steering current turn...".to_string());
    Ok(())
}

async fn submit_or_steer_message(
    app: &mut App,
    engine_handle: &EngineHandle,
    message: QueuedMessage,
) -> Result<()> {
    if app.offline_mode && !app.is_loading {
        app.queue_message(message);
        app.status_message = Some(format!(
            "Offline mode: queued {} message(s) - /queue to review",
            app.queued_message_count()
        ));
        return Ok(());
    }
    if app.is_loading {
        if let Err(err) = steer_user_message(app, engine_handle, message.clone()).await {
            app.queue_message(message);
            app.status_message = Some(format!(
                "Steer failed ({err}); queued {} message(s) - /queue to view/edit",
                app.queued_message_count()
            ));
        }
        Ok(())
    } else {
        dispatch_user_message(app, engine_handle, message).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanChoice {
    AcceptAgent,
    AcceptYolo,
    RevisePlan,
    ExitPlan,
}

fn plan_next_step_prompt() -> String {
    [
        "Action required: choose the next step for this plan.",
        "  1) Accept + implement in Agent mode",
        "  2) Accept + implement in YOLO mode",
        "  3) Revise the plan / ask follow-ups",
        "  4) Return to Agent mode without implementing",
        "",
        "Use the plan confirmation popup, or type 1-4 and press Enter.",
    ]
    .join("\n")
}

fn plan_choice_from_option(option: usize) -> Option<PlanChoice> {
    match option {
        1 => Some(PlanChoice::AcceptAgent),
        2 => Some(PlanChoice::AcceptYolo),
        3 => Some(PlanChoice::RevisePlan),
        4 => Some(PlanChoice::ExitPlan),
        _ => None,
    }
}

fn parse_plan_choice(input: &str) -> Option<PlanChoice> {
    // Once the modal is dismissed, only the advertised 1-4 fallback remains active.
    // Letter shortcuts stay modal-only so normal messages like "yolo" are not captured.
    match input.trim() {
        "1" => Some(PlanChoice::AcceptAgent),
        "2" => Some(PlanChoice::AcceptYolo),
        "3" => Some(PlanChoice::RevisePlan),
        "4" => Some(PlanChoice::ExitPlan),
        _ => None,
    }
}

async fn apply_plan_choice(
    app: &mut App,
    engine_handle: &EngineHandle,
    choice: PlanChoice,
) -> Result<()> {
    match choice {
        PlanChoice::AcceptAgent => {
            app.set_mode(AppMode::Agent);
            app.add_message(HistoryCell::System {
                content: "Plan accepted. Switching to Agent mode and starting implementation."
                    .to_string(),
            });
            let followup = QueuedMessage::new("Proceed with the accepted plan.".to_string(), None);
            if app.is_loading {
                app.queue_message(followup);
                app.status_message =
                    Some("Queued accepted plan execution (agent mode).".to_string());
            } else {
                dispatch_user_message(app, engine_handle, followup).await?;
            }
        }
        PlanChoice::AcceptYolo => {
            app.set_mode(AppMode::Yolo);
            app.add_message(HistoryCell::System {
                content: "Plan accepted. Switching to YOLO mode and starting implementation."
                    .to_string(),
            });
            let followup = QueuedMessage::new("Proceed with the accepted plan.".to_string(), None);
            if app.is_loading {
                app.queue_message(followup);
                app.status_message =
                    Some("Queued accepted plan execution (YOLO mode).".to_string());
            } else {
                dispatch_user_message(app, engine_handle, followup).await?;
            }
        }
        PlanChoice::RevisePlan => {
            let prompt = "Revise the plan: ";
            app.input = prompt.to_string();
            app.cursor_position = prompt.chars().count();
            app.status_message = Some("Revise the plan and press Enter.".to_string());
        }
        PlanChoice::ExitPlan => {
            app.set_mode(AppMode::Agent);
            app.add_message(HistoryCell::System {
                content: "Exited Plan mode. Switched to Agent mode.".to_string(),
            });
        }
    }

    Ok(())
}

async fn handle_plan_choice(
    app: &mut App,
    engine_handle: &EngineHandle,
    input: &str,
) -> Result<bool> {
    if !app.plan_prompt_pending {
        return Ok(false);
    }

    let choice = parse_plan_choice(input);
    app.plan_prompt_pending = false;

    let Some(choice) = choice else {
        return Ok(false);
    };

    apply_plan_choice(app, engine_handle, choice).await?;
    Ok(true)
}

fn running_agent_count(app: &App) -> usize {
    let mut ids: std::collections::HashSet<&str> =
        app.agent_progress.keys().map(String::as_str).collect();
    for agent in app
        .subagent_cache
        .iter()
        .filter(|agent| matches!(agent.status, SubAgentStatus::Running))
    {
        ids.insert(agent.agent_id.as_str());
    }
    ids.len()
}

fn reconcile_subagent_activity_state(app: &mut App) {
    let running_agents: Vec<(String, String)> = app
        .subagent_cache
        .iter()
        .filter(|agent| matches!(agent.status, SubAgentStatus::Running))
        .map(|agent| {
            (
                agent.agent_id.clone(),
                summarize_tool_output(&agent.assignment.objective),
            )
        })
        .collect();

    let running_ids: std::collections::HashSet<String> =
        running_agents.iter().map(|(id, _)| id.clone()).collect();
    app.agent_progress
        .retain(|id, _| running_ids.contains(id.as_str()));
    for (id, objective) in running_agents {
        app.agent_progress.entry(id).or_insert(objective);
    }

    if running_ids.is_empty() {
        app.agent_activity_started_at = None;
    } else if app.agent_activity_started_at.is_none() {
        app.agent_activity_started_at = Some(Instant::now());
    }
}

fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Clear entire area with background color
    let background = Block::default().style(Style::default().bg(app.ui_theme.header_bg));
    f.render_widget(background, size);

    // Show onboarding screen if needed
    if app.onboarding != OnboardingState::None {
        onboarding::render(f, size, app);
        return;
    }

    let header_height = 1;
    let footer_height = 1;
    let body_height = size.height.saturating_sub(header_height + footer_height);
    let slash_menu_entries = visible_slash_menu_entries(app, SLASH_MENU_LIMIT);
    let mention_menu_entries =
        crate::tui::file_mention::visible_mention_menu_entries(app, MENTION_MENU_LIMIT);
    if !mention_menu_entries.is_empty() && app.mention_menu_selected >= mention_menu_entries.len() {
        app.mention_menu_selected = mention_menu_entries.len().saturating_sub(1);
    }
    let context_usage = context_usage_snapshot(app);
    let composer_max_height = body_height
        .saturating_sub(MIN_CHAT_HEIGHT)
        .max(MIN_COMPOSER_HEIGHT);
    let composer_height = {
        let composer_widget = ComposerWidget::new(
            app,
            composer_max_height,
            &slash_menu_entries,
            &mention_menu_entries,
        );
        composer_widget.desired_height(size.width)
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),   // Header
            Constraint::Min(1),                  // Chat area
            Constraint::Length(composer_height), // Composer
            Constraint::Length(footer_height),   // Footer
        ])
        .split(size);

    // Render header
    {
        let sanitized_context_window = context_usage
            .as_ref()
            .map(|(_, max, _)| *max)
            .or_else(|| crate::models::context_window_for_model(&app.model));
        let sanitized_prompt_tokens = context_usage
            .as_ref()
            .and_then(|(used, _, _)| u32::try_from(*used).ok());
        let workspace_name = app
            .workspace
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("workspace");
        let effort_label = app.reasoning_effort.short_label();
        let provider_label = match app.api_provider {
            crate::config::ApiProvider::Deepseek => None,
            crate::config::ApiProvider::NvidiaNim => Some("NIM"),
        };
        let header_data = HeaderData::new(
            app.mode,
            &app.model,
            workspace_name,
            app.is_loading,
            app.ui_theme.header_bg,
        )
        .with_usage(
            app.total_conversation_tokens,
            sanitized_context_window,
            app.session_cost,
            sanitized_prompt_tokens,
        )
        .with_reasoning_effort(Some(effort_label))
        .with_provider(provider_label);
        let header_widget = HeaderWidget::new(header_data);
        let buf = f.buffer_mut();
        header_widget.render(chunks[0], buf);
    }

    // Render chat + sidebar
    {
        let mut chat_area = chunks[1];
        let mut sidebar_area = None;

        if chunks[1].width >= SIDEBAR_VISIBLE_MIN_WIDTH {
            let preferred_sidebar = (u32::from(chunks[1].width)
                * u32::from(app.sidebar_width_percent.clamp(10, 50))
                / 100) as u16;
            let sidebar_width = preferred_sidebar
                .max(24)
                .min(chunks[1].width.saturating_sub(40));
            if sidebar_width >= 20 {
                let split = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Min(1), Constraint::Length(sidebar_width)])
                    .split(chunks[1]);
                chat_area = split[0];
                sidebar_area = Some(split[1]);
            }
        }

        let chat_widget = ChatWidget::new(app, chat_area);
        let buf = f.buffer_mut();
        chat_widget.render(chat_area, buf);

        if let Some(sidebar_area) = sidebar_area {
            super::sidebar::render_sidebar(f, sidebar_area, app);
        }
    }

    // Render composer
    let cursor_pos = {
        let composer_widget = ComposerWidget::new(
            app,
            composer_max_height,
            &slash_menu_entries,
            &mention_menu_entries,
        );
        let buf = f.buffer_mut();
        composer_widget.render(chunks[2], buf);
        composer_widget.cursor_pos(chunks[2])
    };
    if let Some(cursor_pos) = cursor_pos {
        f.set_cursor_position(cursor_pos);
    }

    // Render footer
    render_footer(f, chunks[3], app);

    if !app.view_stack.is_empty() {
        let buf = f.buffer_mut();
        app.view_stack.render(size, buf);
    }
}

async fn handle_view_events(
    app: &mut App,
    config: &mut Config,
    task_manager: &SharedTaskManager,
    engine_handle: &mut EngineHandle,
    events: Vec<ViewEvent>,
) -> Result<bool> {
    for event in events {
        match event {
            ViewEvent::CommandPaletteSelected { action } => match action {
                crate::tui::views::CommandPaletteAction::ExecuteCommand { command } => {
                    if execute_command_input(app, engine_handle, task_manager, config, &command)
                        .await?
                    {
                        return Ok(true);
                    }
                }
                crate::tui::views::CommandPaletteAction::InsertText { text } => {
                    app.input = text;
                    app.cursor_position = app.input.chars().count();
                    app.status_message = Some(
                        "Inserted into composer. Finish the input or press Enter.".to_string(),
                    );
                }
                crate::tui::views::CommandPaletteAction::OpenTextPager { title, content } => {
                    open_text_pager(app, title, content);
                }
            },
            ViewEvent::OpenTextPager { title, content } => {
                open_text_pager(app, title, content);
            }
            ViewEvent::ApprovalDecision {
                tool_id,
                tool_name,
                decision,
                timed_out,
            } => {
                if decision == ReviewDecision::ApprovedForSession {
                    app.approval_session_approved.insert(tool_name);
                }

                match decision {
                    ReviewDecision::Approved | ReviewDecision::ApprovedForSession => {
                        let _ = engine_handle.approve_tool_call(tool_id).await;
                    }
                    ReviewDecision::Denied | ReviewDecision::Abort => {
                        let _ = engine_handle.deny_tool_call(tool_id).await;
                    }
                }

                if timed_out {
                    app.add_message(HistoryCell::System {
                        content: "Approval request timed out - denied".to_string(),
                    });
                }
            }
            ViewEvent::ElevationDecision {
                tool_id,
                tool_name,
                option,
            } => {
                use crate::tui::approval::ElevationOption;
                match option {
                    ElevationOption::Abort => {
                        let _ = engine_handle.deny_tool_call(tool_id).await;
                        app.add_message(HistoryCell::System {
                            content: format!("Sandbox elevation aborted for {tool_name}"),
                        });
                    }
                    ElevationOption::WithNetwork => {
                        app.add_message(HistoryCell::System {
                            content: format!("Retrying {tool_name} with network access enabled"),
                        });
                        let policy = option.to_policy(&app.workspace);
                        let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                    }
                    ElevationOption::WithWriteAccess(_) => {
                        app.add_message(HistoryCell::System {
                            content: format!("Retrying {tool_name} with write access enabled"),
                        });
                        let policy = option.to_policy(&app.workspace);
                        let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                    }
                    ElevationOption::FullAccess => {
                        app.add_message(HistoryCell::System {
                            content: format!("Retrying {tool_name} with full access (no sandbox)"),
                        });
                        let policy = option.to_policy(&app.workspace);
                        let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                    }
                }
            }
            ViewEvent::UserInputSubmitted { tool_id, response } => {
                let _ = engine_handle.submit_user_input(tool_id, response).await;
            }
            ViewEvent::UserInputCancelled { tool_id } => {
                let _ = engine_handle.cancel_user_input(tool_id).await;
                app.add_message(HistoryCell::System {
                    content: "User input cancelled".to_string(),
                });
            }
            ViewEvent::PlanPromptSelected { option } => {
                if app.plan_prompt_pending {
                    app.plan_prompt_pending = false;
                    if let Some(choice) = plan_choice_from_option(option)
                        && let Err(err) = apply_plan_choice(app, engine_handle, choice).await
                    {
                        app.status_message = Some(format!("Failed to apply plan selection: {err}"));
                    }
                }
            }
            ViewEvent::PlanPromptDismissed => {
                app.plan_prompt_pending = true;
                app.status_message =
                    Some("Plan prompt closed. Type 1-4 and press Enter to choose.".to_string());
            }
            ViewEvent::SessionSelected { session_id } => {
                let manager = match SessionManager::default_location() {
                    Ok(manager) => manager,
                    Err(err) => {
                        app.status_message =
                            Some(format!("Failed to open sessions directory: {err}"));
                        continue;
                    }
                };

                match manager.load_session(&session_id) {
                    Ok(session) => {
                        apply_loaded_session(app, &session);
                        let _ = engine_handle
                            .send(Op::SyncSession {
                                messages: app.api_messages.clone(),
                                system_prompt: app.system_prompt.clone(),
                                model: app.model.clone(),
                                workspace: app.workspace.clone(),
                            })
                            .await;
                        let _ = engine_handle
                            .send(Op::SetCompaction {
                                config: app.compaction_config(),
                            })
                            .await;
                        app.status_message = Some(format!(
                            "Session loaded (ID: {})",
                            &session_id[..8.min(session_id.len())]
                        ));
                    }
                    Err(err) => {
                        app.status_message =
                            Some(format!("Failed to load session {session_id}: {err}"));
                    }
                }
            }
            ViewEvent::SessionDeleted { session_id, title } => {
                app.status_message = Some(format!(
                    "Deleted session {} ({})",
                    &session_id[..8.min(session_id.len())],
                    title
                ));
            }
            ViewEvent::ConfigUpdated {
                key,
                value,
                persist,
            } => {
                let result = commands::set_config_value(app, &key, &value, persist);
                if let Some(msg) = result.message {
                    app.add_message(HistoryCell::System { content: msg });
                }

                if let Some(action) = result.action {
                    match action {
                        AppAction::UpdateCompaction(compaction) => {
                            apply_model_and_compaction_update(engine_handle, compaction).await;
                        }
                        AppAction::OpenConfigView => {}
                        _ => {}
                    }
                }

                if app.view_stack.top_kind() == Some(ModalKind::Config) {
                    app.view_stack.pop();
                    app.view_stack.push(ConfigView::new_for_app(app));
                }
            }
            ViewEvent::SubAgentsRefresh => {
                app.status_message = Some("Refreshing sub-agents...".to_string());
                let _ = engine_handle.send(Op::ListSubAgents).await;
            }
            ViewEvent::FilePickerSelected { path } => {
                // Insert `@<path>` at the composer's cursor with surrounding
                // whitespace so the existing `@`-mention parser picks it up.
                let cursor = app.cursor_position;
                let needs_leading_space = cursor > 0
                    && !app
                        .input
                        .chars()
                        .nth(cursor.saturating_sub(1))
                        .is_some_and(|c| c.is_whitespace());
                let mut insertion = String::new();
                if needs_leading_space {
                    insertion.push(' ');
                }
                insertion.push('@');
                insertion.push_str(&path);
                insertion.push(' ');
                app.insert_str(&insertion);
                app.status_message = Some(format!("Attached @{path}"));
            }
            ViewEvent::ModelPickerApplied {
                model,
                effort,
                previous_model,
                previous_effort,
            } => {
                apply_model_picker_choice(
                    app,
                    engine_handle,
                    model,
                    effort,
                    previous_model,
                    previous_effort,
                )
                .await;
            }
        }
    }

    Ok(false)
}

fn apply_loaded_session(app: &mut App, session: &SavedSession) {
    app.api_messages.clone_from(&session.messages);
    app.clear_history();
    app.tool_cells.clear();
    app.tool_details_by_cell.clear();
    app.active_cell = None;
    app.active_tool_details.clear();
    app.active_cell_revision = app.active_cell_revision.wrapping_add(1);
    app.exploring_cell = None;
    app.exploring_entries.clear();
    app.ignored_tool_calls.clear();
    app.pending_tool_uses.clear();
    app.last_exec_wait_command = None;

    let cells_to_add: Vec<_> = app
        .api_messages
        .iter()
        .flat_map(history_cells_from_message)
        .collect();
    app.extend_history(cells_to_add);
    app.mark_history_updated();
    app.transcript_selection.clear();
    app.model.clone_from(&session.metadata.model);
    app.update_model_compaction_budget();
    app.workspace.clone_from(&session.metadata.workspace);
    app.total_tokens = u32::try_from(session.metadata.total_tokens).unwrap_or(u32::MAX);
    app.total_conversation_tokens = app.total_tokens;
    app.last_prompt_tokens = None;
    app.last_completion_tokens = None;
    app.last_prompt_cache_hit_tokens = None;
    app.last_prompt_cache_miss_tokens = None;
    app.current_session_id = Some(session.metadata.id.clone());
    app.workspace_context = None;
    app.workspace_context_refreshed_at = None;
    if let Some(sp) = session.system_prompt.as_ref() {
        app.system_prompt = Some(SystemPrompt::Text(sp.clone()));
    } else {
        app.system_prompt = None;
    }
    app.scroll_to_bottom();
}

fn refresh_workspace_context_if_needed(app: &mut App, now: Instant, allow_blocking_refresh: bool) {
    if app
        .workspace_context_refreshed_at
        .is_some_and(|refreshed_at| {
            now.duration_since(refreshed_at) < Duration::from_secs(WORKSPACE_CONTEXT_REFRESH_SECS)
        })
    {
        return;
    }

    if !allow_blocking_refresh {
        return;
    }

    app.workspace_context = collect_workspace_context(&app.workspace);
    app.workspace_context_refreshed_at = Some(now);
}

#[derive(Debug, Default, Clone, Copy)]
struct WorkspaceChangeSummary {
    staged: usize,
    modified: usize,
    untracked: usize,
    conflicts: usize,
}

impl WorkspaceChangeSummary {
    fn is_clean(&self) -> bool {
        self.staged == 0 && self.modified == 0 && self.untracked == 0 && self.conflicts == 0
    }
}

fn collect_workspace_context(workspace: &Path) -> Option<String> {
    let branch = workspace_git_branch(workspace)?;
    let summary = workspace_git_change_summary(workspace)?;

    let mut parts = Vec::new();
    if summary.staged > 0 {
        parts.push(format!("{} staged", summary.staged));
    }
    if summary.modified > 0 {
        parts.push(format!("{} modified", summary.modified));
    }
    if summary.untracked > 0 {
        parts.push(format!("{} untracked", summary.untracked));
    }
    if summary.conflicts > 0 {
        parts.push(format!("{} conflicts", summary.conflicts));
    }

    let status = if summary.is_clean() {
        "clean".to_string()
    } else {
        parts.join(", ")
    };

    Some(format!("{branch} | {status}"))
}

fn workspace_git_branch(workspace: &Path) -> Option<String> {
    let branch = run_git_query(workspace, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()?;
    let branch = branch.trim().to_string();
    if branch == "HEAD" || branch.is_empty() {
        let short_hash = run_git_query(workspace, &["rev-parse", "--short", "HEAD"]).ok()?;
        let short_hash = short_hash.trim();
        if short_hash.is_empty() {
            return None;
        }
        return Some(format!("detached:{short_hash}"));
    }
    Some(branch)
}

fn workspace_git_change_summary(workspace: &Path) -> Option<WorkspaceChangeSummary> {
    let status = run_git_query(
        workspace,
        &["status", "--short", "--untracked-files=normal"],
    )
    .ok()?;

    if status.trim().is_empty() {
        return Some(WorkspaceChangeSummary::default());
    }

    let mut summary = WorkspaceChangeSummary::default();
    for line in status.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let mut chars = line.chars();
        let staged = chars.next()?;
        let modified = chars.next().unwrap_or(' ');

        if staged == ' ' && modified == ' ' {
            continue;
        }
        if staged == '?' && modified == '?' {
            summary.untracked = summary.untracked.saturating_add(1);
            continue;
        }

        if staged == 'U' || modified == 'U' {
            summary.conflicts = summary.conflicts.saturating_add(1);
        }
        if staged != ' ' && staged != '?' {
            summary.staged = summary.staged.saturating_add(1);
        }
        if modified != ' ' && modified != '?' {
            summary.modified = summary.modified.saturating_add(1);
        }
    }

    Some(summary)
}

fn run_git_query(workspace: &Path, args: &[&str]) -> std::io::Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other("git command failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn pause_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    use_alt_screen: bool,
    use_mouse_capture: bool,
    use_bracketed_paste: bool,
) -> Result<()> {
    disable_raw_mode()?;
    if use_alt_screen {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }
    if use_mouse_capture {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    if use_bracketed_paste {
        execute!(terminal.backend_mut(), DisableBracketedPaste)?;
    }
    Ok(())
}

fn resume_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    use_alt_screen: bool,
    use_mouse_capture: bool,
    use_bracketed_paste: bool,
) -> Result<()> {
    enable_raw_mode()?;
    if use_alt_screen {
        execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    }
    if use_mouse_capture {
        execute!(terminal.backend_mut(), EnableMouseCapture)?;
    }
    if use_bracketed_paste {
        execute!(terminal.backend_mut(), EnableBracketedPaste)?;
    }
    terminal.clear()?;
    Ok(())
}

fn status_color(level: StatusToastLevel) -> ratatui::style::Color {
    match level {
        StatusToastLevel::Info => palette::DEEPSEEK_SKY,
        StatusToastLevel::Success => palette::STATUS_SUCCESS,
        StatusToastLevel::Warning => palette::STATUS_WARNING,
        StatusToastLevel::Error => palette::STATUS_ERROR,
    }
}

fn render_footer(f: &mut Frame, area: Rect, app: &mut App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Pull in the toast first so we don't re-borrow `app` mutably mid-build,
    // then build the FooterProps once. The widget itself is a pure render —
    // it owns no `App` knowledge; all width-aware layout lives in the widget.
    //
    // The quit-confirmation prompt takes precedence over normal status toasts
    // because it represents a transient instruction the user must respond to
    // within ~2s. Mirrors codex-rs's `FooterMode::QuitShortcutReminder`.
    let quit_prompt = if app.quit_is_armed() {
        Some(FooterToast {
            text: "Press Ctrl+C again to quit".to_string(),
            color: palette::STATUS_WARNING,
        })
    } else {
        None
    };
    let toast = quit_prompt.or_else(|| {
        app.active_status_toast().map(|toast| FooterToast {
            text: toast.text,
            color: status_color(toast.level),
        })
    });

    let (state_label, state_color) = footer_state_label(app);
    let coherence = footer_coherence_spans(app);
    let agents = crate::tui::widgets::footer_agents_chip(running_agent_count(app));
    let reasoning_replay = footer_reasoning_replay_spans(app);
    let cache = footer_cache_spans(app);
    let cost = if app.session_cost > 0.001 {
        vec![Span::styled(
            format!("${:.2}", app.session_cost),
            Style::default().fg(palette::TEXT_MUTED),
        )]
    } else {
        Vec::new()
    };

    let mut props = FooterProps::from_app(
        app,
        toast,
        state_label,
        state_color,
        coherence,
        agents,
        reasoning_replay,
        cache,
        cost,
    );

    // Animate the spacer between the left status line and the right-hand
    // chips whenever a turn is live: model loading/streaming, compacting, or
    // sub-agents in flight. Honors the `low_motion` setting — calm terminals
    // get the plain whitespace gap. Strip frame counter ticks every 80 ms;
    // dot-pulse counter ticks every 400 ms so `working` → `working...` reads
    // at a calm pace. The renderer is deterministic given the frame.
    if footer_working_strip_active(app) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let dot_frame = now_ms / 400;
        // Surface a `working`-with-dot-pulse label whenever a turn is live.
        // This replaces the plain "working" / no-label state for the
        // duration of the turn so the user always has a textual signal,
        // even on terminals where the spout strip is disabled.
        let working_label = crate::tui::widgets::footer_working_label(dot_frame);
        props.state_label = working_label;
        props.state_color = palette::DEEPSEEK_SKY;

        // Spout drift: only animate when low_motion is off. The textual
        // `working...` pulse stays even in low-motion mode so the user still
        // sees that something is happening.
        if !app.low_motion {
            let strip_frame = now_ms / 80;
            props.working_strip_frame = Some(strip_frame);
        }
    }

    let widget = FooterWidget::new(props);
    let buf = f.buffer_mut();
    widget.render(area, buf);
}

/// Whether the footer should animate the water-spout strip. Driven by the
/// underlying live-work flags so the strip stays visible for the *entire*
/// turn — not just the moments where bytes are streaming. `is_loading` can
/// flicker off between LLM rounds within a single turn (tool execution,
/// reasoning replay, capacity refresh, etc.), so we ALSO gate on the turn
/// itself still being in flight via `runtime_turn_status == "in_progress"`.
/// Without that, the user sees the strip vanish for seconds at a time even
/// though the agent is still working.
fn footer_working_strip_active(app: &App) -> bool {
    let turn_in_progress = app.runtime_turn_status.as_deref() == Some("in_progress");
    app.is_loading || app.is_compacting || running_agent_count(app) > 0 || turn_in_progress
}

/// Test-only helper retained as a parity reference for `FooterWidget`'s
/// auxiliary-span composition. Production rendering is performed by the
/// widget itself; the existing footer parity tests still exercise this
/// function directly to guard against drift.
#[allow(dead_code)]
fn footer_auxiliary_spans(app: &App, max_width: usize) -> Vec<Span<'static>> {
    // Context % is already shown in the header signal bar — don't
    // duplicate it in the footer. The footer carries unique info only:
    // coherence, in-flight sub-agents, reasoning replay tokens, cache hit
    // rate, and session cost.
    let coherence_spans = footer_coherence_spans(app);
    let agents_spans = crate::tui::widgets::footer_agents_chip(running_agent_count(app));
    let replay_spans = footer_reasoning_replay_spans(app);
    let cache_spans = footer_cache_spans(app);
    let cost_spans = if app.session_cost > 0.001 {
        vec![Span::styled(
            format!("${:.2}", app.session_cost),
            Style::default().fg(palette::TEXT_MUTED),
        )]
    } else {
        Vec::new()
    };

    let parts: Vec<&Vec<Span<'static>>> = [
        &coherence_spans,
        &agents_spans,
        &replay_spans,
        &cache_spans,
        &cost_spans,
    ]
    .iter()
    .filter(|spans| !spans.is_empty())
    .copied()
    .collect();

    // Try to fit as many parts as possible, dropping from the end.
    for end in (0..=parts.len()).rev() {
        let mut combined = Vec::new();
        for (i, part) in parts[..end].iter().enumerate() {
            if i > 0 {
                combined.push(Span::raw("  "));
            }
            combined.extend(part.iter().cloned());
        }
        if spans_width(&combined) <= max_width {
            return combined;
        }
    }
    Vec::new()
}

fn footer_coherence_spans(app: &App) -> Vec<Span<'static>> {
    // Only surface coherence when the engine is actively intervening — the
    // user-facing signal is "we're doing something different now," not
    // "your conversation is getting complex," which the context-percent
    // header already covers. `GettingCrowded` is just a soft hint, so we
    // suppress it; the active interventions get their own visible label.
    let (label, color) = match app.coherence_state {
        CoherenceState::Healthy | CoherenceState::GettingCrowded => return Vec::new(),
        CoherenceState::RefreshingContext => ("refreshing context", palette::STATUS_WARNING),
        CoherenceState::VerifyingRecentWork => ("verifying", palette::DEEPSEEK_SKY),
        CoherenceState::ResettingPlan => ("resetting plan", palette::STATUS_ERROR),
    };

    vec![Span::styled(label.to_string(), Style::default().fg(color))]
}

fn footer_cache_spans(app: &App) -> Vec<Span<'static>> {
    let Some(hit_tokens) = app.last_prompt_cache_hit_tokens else {
        return Vec::new();
    };
    let miss_tokens = app.last_prompt_cache_miss_tokens.unwrap_or(0);
    let total = hit_tokens.saturating_add(miss_tokens);
    if total == 0 {
        return Vec::new();
    }

    let percent = (f64::from(hit_tokens) / f64::from(total) * 100.0).clamp(0.0, 100.0);
    vec![Span::styled(
        format!("cache {:.0}%", percent),
        Style::default().fg(palette::TEXT_MUTED),
    )]
}

/// Render a footer chip showing the size of the `reasoning_content` block
/// replayed on the most recent thinking-mode tool-calling turn (#30).
///
/// Stays hidden when the count is zero (non-thinking models, first turn, or
/// turns with no tool calls). When replay tokens dominate the input budget
/// (>50%), the chip turns warning-coloured so users notice that thinking
/// replay is the main consumer of context.
fn footer_reasoning_replay_spans(app: &App) -> Vec<Span<'static>> {
    let Some(replay) = app.last_reasoning_replay_tokens else {
        return Vec::new();
    };
    if replay == 0 {
        return Vec::new();
    }
    let label = format!("rsn {}", format_token_count_compact(u64::from(replay)));
    let color = match app.last_prompt_tokens {
        Some(input) if input > 0 && f64::from(replay) / f64::from(input) > 0.5 => {
            palette::STATUS_WARNING
        }
        _ => palette::TEXT_MUTED,
    };
    vec![Span::styled(label, Style::default().fg(color))]
}

#[allow(dead_code)]
fn footer_toast_spans(
    toast: &crate::tui::app::StatusToast,
    max_width: usize,
) -> Vec<Span<'static>> {
    let truncated = truncate_line_to_width(&toast.text, max_width.max(1));
    vec![Span::styled(
        truncated,
        Style::default().fg(status_color(toast.level)),
    )]
}

#[allow(dead_code)]
fn footer_status_line_spans(app: &App, max_width: usize) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }

    let (mode_label, mode_color) = footer_mode_style(app);
    let (status_label, status_color) = footer_state_label(app);
    let sep = " \u{00B7} ";
    let show_status = status_label != "ready";

    let fixed_width = mode_label.width()
        + sep.width()
        + if show_status {
            sep.width() + status_label.width()
        } else {
            0
        };

    if max_width <= mode_label.width() {
        return vec![Span::styled(
            truncate_line_to_width(mode_label, max_width),
            Style::default().fg(mode_color),
        )];
    }

    let model_budget = max_width.saturating_sub(fixed_width).max(1);
    let model_label = truncate_line_to_width(&app.model, model_budget);

    let mut spans = vec![
        Span::styled(mode_label.to_string(), Style::default().fg(mode_color)),
        Span::styled(sep.to_string(), Style::default().fg(palette::TEXT_DIM)),
        Span::styled(model_label, Style::default().fg(palette::TEXT_HINT)),
    ];

    if show_status {
        spans.push(Span::styled(
            sep.to_string(),
            Style::default().fg(palette::TEXT_DIM),
        ));
        spans.push(Span::styled(
            status_label.to_string(),
            Style::default().fg(status_color),
        ));
    }

    spans
}

fn footer_state_label(app: &App) -> (&'static str, ratatui::style::Color) {
    if app.is_compacting {
        return ("compacting \u{238B}", palette::STATUS_WARNING);
    }
    // Note: we deliberately do NOT show a "thinking" label for `is_loading`.
    // The animated water-spout strip in the footer's spacer is the visual
    // signal that the model is live; "thinking" was misleading because it
    // fired for every kind of in-flight work (tool calls, streaming, etc.),
    // not strictly reasoning. Sub-agents still surface "working" because
    // that's a distinct lifecycle the user can act on (open `/agents`).
    if running_agent_count(app) > 0 {
        return ("working", palette::DEEPSEEK_SKY);
    }
    if app.queued_draft.is_some() {
        return ("draft", palette::TEXT_MUTED);
    }

    if !app.view_stack.is_empty() {
        return ("overlay", palette::TEXT_MUTED);
    }

    if !app.input.is_empty() {
        return ("draft", palette::TEXT_MUTED);
    }

    ("ready", palette::TEXT_MUTED)
}

#[allow(dead_code)]
fn footer_mode_style(app: &App) -> (&'static str, ratatui::style::Color) {
    let label = app.mode.as_setting();
    let color = match app.mode {
        crate::tui::app::AppMode::Agent => palette::MODE_AGENT,
        crate::tui::app::AppMode::Yolo => palette::MODE_YOLO,
        crate::tui::app::AppMode::Plan => palette::MODE_PLAN,
    };
    (label, color)
}

fn format_token_count_compact(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

#[allow(dead_code)]
fn format_context_budget(used: i64, max: u32) -> String {
    let max_u64 = u64::from(max);
    let max_i64 = i64::from(max);

    if used > max_i64 {
        return format!(
            ">{}/{}",
            format_token_count_compact(max_u64),
            format_token_count_compact(max_u64)
        );
    }

    let used_u64 = u64::try_from(used.max(0)).unwrap_or(0);
    format!(
        "{}/{}",
        format_token_count_compact(used_u64),
        format_token_count_compact(max_u64)
    )
}

#[allow(dead_code)]
fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}

#[allow(dead_code)]
fn transcript_scroll_percent(top: usize, visible: usize, total: usize) -> Option<u16> {
    if total <= visible {
        return None;
    }

    let max_top = total.saturating_sub(visible);
    if max_top == 0 {
        return None;
    }

    let clamped_top = top.min(max_top);
    let percent = ((clamped_top as f64 / max_top as f64) * 100.0).round() as u16;
    Some(percent.min(100))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchDirection {
    Forward,
    Backward,
}

fn jump_to_adjacent_tool_cell(app: &mut App, direction: SearchDirection) -> bool {
    let line_meta = app.transcript_cache.line_meta();
    if line_meta.is_empty() {
        return false;
    }

    let top = app
        .last_transcript_top
        .min(line_meta.len().saturating_sub(1));
    let current_cell = line_meta
        .get(top)
        .and_then(crate::tui::scrolling::TranscriptLineMeta::cell_line)
        .map(|(cell_index, _)| cell_index);

    let mut scan_indices = Vec::new();
    match direction {
        SearchDirection::Forward => {
            scan_indices.extend((top.saturating_add(1))..line_meta.len());
        }
        SearchDirection::Backward => {
            scan_indices.extend((0..top).rev());
        }
    }

    for idx in scan_indices {
        let Some((cell_index, _)) = line_meta[idx].cell_line() else {
            continue;
        };
        if current_cell.is_some_and(|current| current == cell_index) {
            continue;
        }
        if !matches!(app.history.get(cell_index), Some(HistoryCell::Tool(_))) {
            continue;
        }
        if let Some(anchor) = TranscriptScroll::anchor_for(line_meta, idx) {
            app.transcript_scroll = anchor;
            app.pending_scroll_delta = 0;
            app.needs_redraw = true;
            return true;
        }
    }

    false
}

fn estimated_context_tokens(app: &App) -> Option<i64> {
    i64::try_from(estimate_input_tokens_conservative(
        &app.api_messages,
        app.system_prompt.as_ref(),
    ))
    .ok()
}

fn context_usage_snapshot(app: &App) -> Option<(i64, u32, f64)> {
    let max = context_window_for_model(&app.model)?;
    let max_i64 = i64::from(max);
    let reported = app
        .last_prompt_tokens
        .map(i64::from)
        .map(|tokens| tokens.max(0));
    let estimated = estimated_context_tokens(app).map(|tokens| tokens.max(0));

    // Always prefer the estimated current-context size (computed from
    // `app.api_messages`) when we have it. Reported `last_prompt_tokens`
    // comes from `Event::TurnComplete.usage`, which the engine builds with
    // `turn.add_usage` — that SUMS input_tokens across every round in the
    // turn, so a multi-round tool-call turn reports a value much larger
    // than the actual context window state, then the next single-round
    // turn drops back to a single round's input_tokens. User-visible %
    // was bouncing 31% → 9% (#115) because of this. The estimate is
    // monotonic wrt conversation growth, which is what a "context filling
    // up" indicator should show. We still consult `reported` only as a
    // fallback when no estimate is available (e.g., immediately after a
    // session restore before the api_messages are populated).
    let used = match (estimated, reported) {
        (Some(estimated), _) => estimated.min(max_i64),
        (None, Some(reported)) => reported.min(max_i64),
        (None, None) => return None,
    };

    let max_f64 = f64::from(max);
    let used_f64 = used as f64;
    let percent = ((used_f64 / max_f64) * 100.0).clamp(0.0, 100.0);
    Some((used, max, percent))
}

/// Retained as a callable utility — `context_usage_snapshot` no longer uses
/// it directly (#115 makes the estimate the primary signal), but tests in
/// `ui/tests.rs` still exercise it and a future heuristic may want to
/// distinguish "obviously inflated reported tokens" from healthy reports.
#[allow(dead_code)]
fn is_reported_context_inflated(reported: i64, estimated: i64) -> bool {
    const MIN_ABSOLUTE_GAP: i64 = 4_096;
    if estimated <= 0 || reported <= estimated {
        return false;
    }

    reported.saturating_sub(estimated) >= MIN_ABSOLUTE_GAP
        && reported >= estimated.saturating_mul(4)
}

fn maybe_warn_context_pressure(app: &mut App) {
    let Some((used, max, percent)) = context_usage_snapshot(app) else {
        return;
    };

    if percent < CONTEXT_WARNING_THRESHOLD_PERCENT {
        return;
    }

    let recommendation = if app.auto_compact {
        "Auto-compaction is enabled."
    } else {
        "Consider /compact or /clear."
    };

    if percent >= CONTEXT_CRITICAL_THRESHOLD_PERCENT {
        app.status_message = Some(format!(
            "Context critical: {:.0}% ({used}/{max} tokens). {recommendation}",
            percent
        ));
        return;
    }

    if app.status_message.is_none() {
        app.status_message = Some(format!(
            "Context high: {:.0}% ({used}/{max} tokens). {recommendation}",
            percent
        ));
    }
}

fn should_auto_compact_before_send(app: &App) -> bool {
    if !app.auto_compact {
        return false;
    }
    context_usage_snapshot(app)
        .map(|(_, _, pct)| pct >= CONTEXT_CRITICAL_THRESHOLD_PERCENT)
        .unwrap_or(false)
}

fn status_animation_interval_ms(app: &App) -> u64 {
    if app.low_motion {
        2_400
    } else {
        UI_STATUS_ANIMATION_MS
    }
}

fn active_poll_ms(app: &App) -> u64 {
    if app.low_motion {
        96
    } else {
        UI_ACTIVE_POLL_MS
    }
}

fn idle_poll_ms(app: &App) -> u64 {
    if app.low_motion { 120 } else { UI_IDLE_POLL_MS }
}

fn history_has_live_motion(history: &[HistoryCell]) -> bool {
    history.iter().any(|cell| match cell {
        HistoryCell::Thinking { streaming, .. } => *streaming,
        HistoryCell::Tool(tool) => match tool {
            ToolCell::Exec(cell) => cell.status == ToolStatus::Running,
            ToolCell::Exploring(cell) => cell
                .entries
                .iter()
                .any(|entry| entry.status == ToolStatus::Running),
            ToolCell::PlanUpdate(cell) => cell.status == ToolStatus::Running,
            ToolCell::PatchSummary(cell) => cell.status == ToolStatus::Running,
            ToolCell::Review(cell) => cell.status == ToolStatus::Running,
            ToolCell::DiffPreview(_) => false,
            ToolCell::Mcp(cell) => cell.status == ToolStatus::Running,
            ToolCell::ViewImage(_) => false,
            ToolCell::WebSearch(cell) => cell.status == ToolStatus::Running,
            ToolCell::Generic(cell) => cell.status == ToolStatus::Running,
        },
        _ => false,
    })
}

pub(crate) fn truncate_line_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    // For very small budgets, take chars until we exceed the *display* width.
    // Counting characters instead of widths (the previous behavior) overran
    // the budget for any double-width grapheme and contributed to mid-character
    // sidebar artifacts on resize (issue #65).
    if max_width <= 3 {
        let mut out = String::new();
        let mut width = 0usize;
        for ch in text.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + ch_width > max_width {
                break;
            }
            out.push(ch);
            width += ch_width;
        }
        return out;
    }

    let mut out = String::new();
    let mut width = 0usize;
    let limit = max_width.saturating_sub(3);
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > limit {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push_str("...");
    out
}

fn handle_mouse_event(app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            let update = app.mouse_scroll.on_scroll(ScrollDirection::Up);
            app.pending_scroll_delta += update.delta_lines;
        }
        MouseEventKind::ScrollDown => {
            let update = app.mouse_scroll.on_scroll(ScrollDirection::Down);
            app.pending_scroll_delta += update.delta_lines;
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(point) = selection_point_from_mouse(app, mouse) {
                app.transcript_selection.anchor = Some(point);
                app.transcript_selection.head = Some(point);
                app.transcript_selection.dragging = true;

                if app.is_loading
                    && app.transcript_scroll.is_at_tail()
                    && let Some(anchor) = TranscriptScroll::anchor_for(
                        app.transcript_cache.line_meta(),
                        app.last_transcript_top,
                    )
                {
                    app.transcript_scroll = anchor;
                }
            } else if app.transcript_selection.is_active() {
                app.transcript_selection.clear();
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.transcript_selection.dragging
                && let Some(point) = selection_point_from_mouse(app, mouse)
            {
                app.transcript_selection.head = Some(point);
            }
        }
        MouseEventKind::Up(MouseButton::Left) if app.transcript_selection.dragging => {
            app.transcript_selection.dragging = false;
            if selection_has_content(app) {
                copy_active_selection(app);
            }
        }
        _ => {}
    }
}

fn selection_point_from_mouse(app: &App, mouse: MouseEvent) -> Option<TranscriptSelectionPoint> {
    selection_point_from_position(
        app.last_transcript_area?,
        mouse.column,
        mouse.row,
        app.last_transcript_top,
        app.last_transcript_total,
        app.last_transcript_padding_top,
    )
}

fn selection_point_from_position(
    area: Rect,
    column: u16,
    row: u16,
    transcript_top: usize,
    transcript_total: usize,
    padding_top: usize,
) -> Option<TranscriptSelectionPoint> {
    if column < area.x
        || column >= area.x + area.width
        || row < area.y
        || row >= area.y + area.height
    {
        return None;
    }

    if transcript_total == 0 {
        return None;
    }

    let row = row.saturating_sub(area.y) as usize;
    if row < padding_top {
        return None;
    }
    let row = row.saturating_sub(padding_top);

    let col = column.saturating_sub(area.x) as usize;
    let line_index = transcript_top
        .saturating_add(row)
        .min(transcript_total.saturating_sub(1));

    Some(TranscriptSelectionPoint {
        line_index,
        column: col,
    })
}

fn selection_has_content(app: &App) -> bool {
    match app.transcript_selection.ordered_endpoints() {
        Some((start, end)) => start != end,
        None => false,
    }
}

fn copy_active_selection(app: &mut App) {
    if !app.transcript_selection.is_active() {
        return;
    }
    if let Some(text) = selection_to_text(app) {
        if app.clipboard.write_text(&text).is_ok() {
            app.status_message = Some("Selection copied".to_string());
        } else {
            app.status_message = Some("Copy failed".to_string());
        }
    }
}

fn selection_to_text(app: &App) -> Option<String> {
    let (start, end) = app.transcript_selection.ordered_endpoints()?;
    let lines = app.transcript_cache.lines();
    if lines.is_empty() {
        return None;
    }
    let end_index = end.line_index.min(lines.len().saturating_sub(1));
    let start_index = start.line_index.min(end_index);

    let mut out = String::new();
    #[allow(clippy::needless_range_loop)]
    for line_index in start_index..=end_index {
        let line_text = line_to_plain(&lines[line_index]);
        let slice = if start_index == end_index {
            slice_text(&line_text, start.column, end.column)
        } else if line_index == start_index {
            slice_text(&line_text, start.column, text_display_width(&line_text))
        } else if line_index == end_index {
            slice_text(&line_text, 0, end.column)
        } else {
            line_text
        };
        out.push_str(&slice);
        if line_index != end_index {
            out.push('\n');
        }
    }
    Some(out)
}

fn open_pager_for_selection(app: &mut App) -> bool {
    let Some(text) = selection_to_text(app) else {
        return false;
    };
    let width = app
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(80);
    let pager = PagerView::from_text("Selection", &text, width.saturating_sub(2));
    app.view_stack.push(pager);
    true
}

fn open_pager_for_last_message(app: &mut App) -> bool {
    let Some(cell) = app.history.last() else {
        return false;
    };
    let width = app
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(80);
    let text = history_cell_to_text(cell, width);
    let pager = PagerView::from_text("Message", &text, width.saturating_sub(2));
    app.view_stack.push(pager);
    true
}

/// Open a pager showing the full thinking block. Targets the cell at the
/// current selection if it's a Thinking cell; otherwise falls back to the
/// most recent Thinking cell in history. Bound to Ctrl+O so users can read
/// reasoning content that's been collapsed in calm-mode rendering.
fn open_thinking_pager(app: &mut App) -> bool {
    let selected_cell = app
        .transcript_selection
        .ordered_endpoints()
        .and_then(|(start, _)| {
            app.transcript_cache
                .line_meta()
                .get(start.line_index)
                .and_then(|meta| meta.cell_line())
                .map(|(cell_index, _)| cell_index)
        })
        .filter(|&idx| {
            matches!(
                app.history.get(idx),
                Some(crate::tui::history::HistoryCell::Thinking { .. })
            )
        });

    let target_idx = selected_cell.or_else(|| {
        app.history
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, cell)| {
                if matches!(cell, crate::tui::history::HistoryCell::Thinking { .. }) {
                    Some(idx)
                } else {
                    None
                }
            })
    });

    let Some(idx) = target_idx else {
        app.status_message = Some("No thinking blocks to expand".to_string());
        return true;
    };

    let cell = &app.history[idx];
    let width = app
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(80);
    let text = history_cell_to_text(cell, width);
    app.view_stack.push(PagerView::from_text(
        "Thinking",
        &text,
        width.saturating_sub(2),
    ));
    true
}

fn open_tool_details_pager(app: &mut App) -> bool {
    let target_cell = if let Some((start, _)) = app.transcript_selection.ordered_endpoints() {
        app.transcript_cache
            .line_meta()
            .get(start.line_index)
            .and_then(|meta| meta.cell_line())
            .map(|(cell_index, _)| cell_index)
    } else {
        app.history.len().checked_sub(1)
    };

    let Some(cell_index) = target_cell else {
        return false;
    };
    if let Some(detail) = app.tool_details_by_cell.get(&cell_index) {
        let input = serde_json::to_string_pretty(&detail.input)
            .unwrap_or_else(|_| detail.input.to_string());
        let output = detail.output.as_deref().map_or(
            "(not available)".to_string(),
            std::string::ToString::to_string,
        );
        let content = format!(
            "Tool ID: {}\nTool: {}\n\nInput:\n{}\n\nOutput:\n{}",
            detail.tool_id, detail.tool_name, input, output
        );

        let width = app
            .last_transcript_area
            .map(|area| area.width)
            .unwrap_or(80);
        app.view_stack.push(PagerView::from_text(
            format!("Tool: {}", detail.tool_name),
            &content,
            width.saturating_sub(2),
        ));
        return true;
    }

    let Some(cell) = app.history.get(cell_index) else {
        app.status_message = Some("No details available for the selected line".to_string());
        return false;
    };
    let title = match cell {
        HistoryCell::User { .. } => "You".to_string(),
        HistoryCell::Assistant { .. } => "Assistant".to_string(),
        HistoryCell::System { .. } => "Note".to_string(),
        HistoryCell::Thinking { .. } => "Reasoning".to_string(),
        HistoryCell::Tool(_) => "Message".to_string(),
    };
    let width = app
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(80);
    let content = history_cell_to_text(cell, width);
    app.view_stack.push(PagerView::from_text(
        title,
        &content,
        width.saturating_sub(2),
    ));
    true
}

fn is_copy_shortcut(key: &KeyEvent) -> bool {
    let is_c = matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'));
    if !is_c {
        return false;
    }

    if key.modifiers.contains(KeyModifiers::SUPER) {
        return true;
    }

    key.modifiers.contains(KeyModifiers::CONTROL) && key.modifiers.contains(KeyModifiers::SHIFT)
}

fn is_paste_shortcut(key: &KeyEvent) -> bool {
    let is_v = matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'));
    if !is_v {
        return false;
    }

    // Cmd+V on macOS
    if key.modifiers.contains(KeyModifiers::SUPER) {
        return true;
    }

    // Ctrl+V on Linux/Windows
    key.modifiers.contains(KeyModifiers::CONTROL)
}

fn should_scroll_with_arrows(_app: &App) -> bool {
    false
}

fn extract_reasoning_header(text: &str) -> Option<String> {
    let start = text.find("**")?;
    let rest = &text[start + 2..];
    let end = rest.find("**")?;
    let header = rest[..end].trim().trim_end_matches(':');
    if header.is_empty() {
        None
    } else {
        Some(header.to_string())
    }
}

fn subagent_status_rank(status: &SubAgentStatus) -> u8 {
    match status {
        SubAgentStatus::Running => 0,
        SubAgentStatus::Interrupted(_) => 1,
        SubAgentStatus::Failed(_) => 2,
        SubAgentStatus::Completed => 3,
        SubAgentStatus::Cancelled => 4,
    }
}

fn sort_subagents_in_place(agents: &mut [SubAgentResult]) {
    agents.sort_by(|a, b| {
        subagent_status_rank(&a.status)
            .cmp(&subagent_status_rank(&b.status))
            .then_with(|| a.agent_type.as_str().cmp(b.agent_type.as_str()))
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
}

fn task_mode_label(mode: AppMode) -> &'static str {
    mode.as_setting()
}

fn task_summary_to_panel_entry(summary: TaskSummary) -> TaskPanelEntry {
    TaskPanelEntry {
        id: summary.id,
        status: task_status_label(summary.status).to_string(),
        prompt_summary: summary.prompt_summary,
        duration_ms: summary.duration_ms,
    }
}

fn task_status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Queued => "queued",
        TaskStatus::Running => "running",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Canceled => "canceled",
    }
}

fn format_task_list(tasks: &[TaskSummary]) -> String {
    if tasks.is_empty() {
        return "No tasks found.".to_string();
    }

    let mut lines = vec![
        format!("Tasks ({})", tasks.len()),
        "----------------------------------------".to_string(),
    ];
    for task in tasks {
        let duration = task
            .duration_ms
            .map(|ms| format!("{:.2}s", ms as f64 / 1000.0))
            .unwrap_or_else(|| "-".to_string());
        lines.push(format!(
            "{}  {:9}  {}  {}",
            task.id,
            task_status_label(task.status),
            duration,
            task.prompt_summary
        ));
    }
    lines.push("Use /task show <id> for timeline details.".to_string());
    lines.join("\n")
}

fn open_task_pager(app: &mut App, task: &TaskRecord) {
    let width = app
        .last_transcript_area
        .map(|area| area.width)
        .unwrap_or(100)
        .saturating_sub(4);
    app.view_stack.push(PagerView::from_text(
        format!("Task {}", task.id),
        &format_task_detail(task),
        width.max(60),
    ));
}

fn format_task_detail(task: &TaskRecord) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Task: {}", task.id));
    lines.push(format!("Status: {}", task_status_label(task.status)));
    lines.push(format!("Mode: {}", task.mode));
    lines.push(format!("Model: {}", task.model));
    lines.push(format!("Workspace: {}", task.workspace.display()));
    if let Some(thread_id) = task.thread_id.as_ref() {
        lines.push(format!("Runtime Thread: {thread_id}"));
    }
    if let Some(turn_id) = task.turn_id.as_ref() {
        lines.push(format!("Runtime Turn: {turn_id}"));
    }
    if task.runtime_event_count > 0 {
        lines.push(format!("Runtime Events: {}", task.runtime_event_count));
    }
    lines.push(format!("Created: {}", task.created_at));
    if let Some(started_at) = task.started_at {
        lines.push(format!("Started: {}", started_at));
    }
    if let Some(ended_at) = task.ended_at {
        lines.push(format!("Ended: {}", ended_at));
    }
    if let Some(duration) = task.duration_ms {
        lines.push(format!("Duration: {:.2}s", duration as f64 / 1000.0));
    }
    lines.push(String::new());
    lines.push("Prompt:".to_string());
    lines.push(task.prompt.clone());

    if let Some(summary) = task.result_summary.as_ref() {
        lines.push(String::new());
        lines.push("Result Summary:".to_string());
        lines.push(summary.clone());
    }
    if let Some(path) = task.result_detail_path.as_ref() {
        lines.push(format!("Result Artifact: {}", path.display()));
    }
    if let Some(error) = task.error.as_ref() {
        lines.push(String::new());
        lines.push(format!("Error: {error}"));
    }

    lines.push(String::new());
    lines.push("Tool Calls:".to_string());
    if task.tool_calls.is_empty() {
        lines.push("- (none)".to_string());
    } else {
        for tool in &task.tool_calls {
            let status = match tool.status {
                crate::task_manager::TaskToolStatus::Running => "running",
                crate::task_manager::TaskToolStatus::Success => "success",
                crate::task_manager::TaskToolStatus::Failed => "failed",
                crate::task_manager::TaskToolStatus::Canceled => "canceled",
            };
            let mut line = format!(
                "- {} [{}] {}",
                tool.name,
                status,
                tool.output_summary.as_deref().unwrap_or("(no summary)")
            );
            if let Some(duration) = tool.duration_ms {
                line.push_str(&format!(" ({:.2}s)", duration as f64 / 1000.0));
            }
            lines.push(line);
            if let Some(path) = tool.detail_path.as_ref() {
                lines.push(format!("  detail: {}", path.display()));
            }
            if let Some(path) = tool.patch_ref.as_ref() {
                lines.push(format!("  patch: {}", path.display()));
            }
        }
    }

    lines.push(String::new());
    lines.push("Timeline:".to_string());
    if task.timeline.is_empty() {
        lines.push("- (none)".to_string());
    } else {
        for entry in &task.timeline {
            lines.push(format!(
                "- [{}] {}: {}",
                entry.timestamp, entry.kind, entry.summary
            ));
            if let Some(path) = entry.detail_path.as_ref() {
                lines.push(format!("  detail: {}", path.display()));
            }
        }
    }

    lines.join("\n")
}

#[allow(clippy::too_many_lines)]
fn handle_tool_call_started(app: &mut App, id: &str, name: &str, input: &serde_json::Value) {
    let id = id.to_string();

    // All in-flight tool work for the current turn lives in `app.active_cell`
    // until the turn completes. This mirrors Codex's contract: ONE active cell
    // mutates in place; finalized history isn't touched until flush. This
    // keeps the transcript stable while parallel completions arrive in any
    // order.
    if app.active_cell.is_none() {
        app.active_cell = Some(ActiveCell::new());
    }

    if is_exploring_tool(name) {
        let label = exploring_label(name, input);
        // ensure_exploring + append_to_exploring keeps all parallel exploring
        // starts in a single ExploringCell entry.
        let active = app.active_cell.as_mut().expect("active_cell just ensured");
        let entry_idx = active.ensure_exploring();
        let inner = active
            .append_to_exploring(
                id.clone(),
                ExploringEntry {
                    label,
                    status: ToolStatus::Running,
                },
            )
            .map_or(0, |(_, inner)| inner);
        app.exploring_cell = Some(entry_idx);
        let virtual_index = app.history.len() + entry_idx;
        app.exploring_entries
            .insert(id.clone(), (virtual_index, inner));
        register_tool_cell(app, &id, name, input, virtual_index);
        app.mark_history_updated();
        return;
    }

    // Non-exploring tool: each is its own entry inside the active cell. We
    // intentionally do NOT clear `exploring_cell` here — the active cell can
    // hold both an exploring aggregate AND independent tool entries
    // simultaneously, which is exactly the case CX#7 fixes.

    if is_exec_tool(name) {
        let command = exec_command_from_input(input).unwrap_or_else(|| "<command>".to_string());
        let source = exec_source_from_input(input);
        let interaction = exec_interaction_summary(name, input);
        let mut is_wait = false;

        if let Some((summary, wait)) = interaction.as_ref() {
            is_wait = *wait;
            if is_wait
                && app
                    .last_exec_wait_command
                    .as_ref()
                    .is_some_and(|last| last == &command)
            {
                app.ignored_tool_calls.insert(id);
                return;
            }
            if is_wait {
                app.last_exec_wait_command = Some(command.clone());
            }

            push_active_tool_cell(
                app,
                &id,
                name,
                input,
                HistoryCell::Tool(ToolCell::Exec(ExecCell {
                    command,
                    status: ToolStatus::Running,
                    output: None,
                    started_at: Some(Instant::now()),
                    duration_ms: None,
                    source,
                    interaction: Some(summary.clone()),
                })),
            );
            return;
        }

        if exec_is_background(input)
            && app
                .last_exec_wait_command
                .as_ref()
                .is_some_and(|last| last == &command)
        {
            app.ignored_tool_calls.insert(id);
            return;
        }
        if exec_is_background(input) && !is_wait {
            app.last_exec_wait_command = Some(command.clone());
        }

        push_active_tool_cell(
            app,
            &id,
            name,
            input,
            HistoryCell::Tool(ToolCell::Exec(ExecCell {
                command,
                status: ToolStatus::Running,
                output: None,
                started_at: Some(Instant::now()),
                duration_ms: None,
                source,
                interaction: None,
            })),
        );
        return;
    }

    if name == "update_plan" {
        let (explanation, steps) = parse_plan_input(input);
        push_active_tool_cell(
            app,
            &id,
            name,
            input,
            HistoryCell::Tool(ToolCell::PlanUpdate(PlanUpdateCell {
                explanation,
                steps,
                status: ToolStatus::Running,
            })),
        );
        return;
    }

    if name == "apply_patch" {
        let (path, summary) = parse_patch_summary(input);
        push_active_tool_cell(
            app,
            &id,
            name,
            input,
            HistoryCell::Tool(ToolCell::PatchSummary(PatchSummaryCell {
                path,
                summary,
                status: ToolStatus::Running,
                error: None,
            })),
        );
        return;
    }

    if name == "review" {
        let target = review_target_label(input);
        push_active_tool_cell(
            app,
            &id,
            name,
            input,
            HistoryCell::Tool(ToolCell::Review(ReviewCell {
                target,
                status: ToolStatus::Running,
                output: None,
                error: None,
            })),
        );
        return;
    }

    if is_mcp_tool(name) {
        push_active_tool_cell(
            app,
            &id,
            name,
            input,
            HistoryCell::Tool(ToolCell::Mcp(McpToolCell {
                tool: name.to_string(),
                status: ToolStatus::Running,
                content: None,
                is_image: false,
            })),
        );
        return;
    }

    if is_view_image_tool(name) {
        if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
            let raw_path = PathBuf::from(path);
            let display_path = raw_path
                .strip_prefix(&app.workspace)
                .unwrap_or(&raw_path)
                .to_path_buf();
            push_active_tool_cell(
                app,
                &id,
                name,
                input,
                HistoryCell::Tool(ToolCell::ViewImage(ViewImageCell { path: display_path })),
            );
        }
        return;
    }

    if is_web_search_tool(name) {
        let query = web_search_query(input);
        push_active_tool_cell(
            app,
            &id,
            name,
            input,
            HistoryCell::Tool(ToolCell::WebSearch(WebSearchCell {
                query,
                status: ToolStatus::Running,
                summary: None,
            })),
        );
        return;
    }

    let input_summary = summarize_tool_args(input);
    let prompts = extract_fanout_prompts(name, input);
    push_active_tool_cell(
        app,
        &id,
        name,
        input,
        HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
            name: name.to_string(),
            status: ToolStatus::Running,
            input_summary,
            output: None,
            prompts,
        })),
    );
}

/// Extract per-child prompts from a fan-out tool's input. For `rlm_query` the
/// renderer shows one row per child instead of an inline JSON summary so the
/// user can read what each child was asked. Returns `None` for tools that
/// don't expose a prompt list.
fn extract_fanout_prompts(name: &str, input: &serde_json::Value) -> Option<Vec<String>> {
    if name != "rlm_query" {
        return None;
    }
    if let Some(arr) = input.get("prompts").and_then(|v| v.as_array()) {
        let prompts: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        if prompts.is_empty() {
            return None;
        }
        return Some(prompts);
    }
    if let Some(s) = input.get("prompt").and_then(|v| v.as_str()) {
        return Some(vec![s.to_string()]);
    }
    None
}

/// Push a tool cell as a new entry in `active_cell`, register the tool id,
/// and write a stub detail record so the pager / Ctrl+O can find it.
fn push_active_tool_cell(
    app: &mut App,
    tool_id: &str,
    tool_name: &str,
    input: &serde_json::Value,
    cell: HistoryCell,
) {
    if app.active_cell.is_none() {
        app.active_cell = Some(ActiveCell::new());
    }
    let active = app.active_cell.as_mut().expect("active_cell just ensured");
    let entry_idx = active.push_tool(tool_id.to_string(), cell);
    let virtual_index = app.history.len() + entry_idx;
    register_tool_cell(app, tool_id, tool_name, input, virtual_index);
    app.mark_history_updated();
}

fn register_tool_cell(
    app: &mut App,
    tool_id: &str,
    tool_name: &str,
    input: &serde_json::Value,
    cell_index: usize,
) {
    app.tool_cells.insert(tool_id.to_string(), cell_index);
    let record = ToolDetailRecord {
        tool_id: tool_id.to_string(),
        tool_name: tool_name.to_string(),
        input: input.clone(),
        output: None,
    };
    if cell_index < app.history.len() {
        app.tool_details_by_cell.insert(cell_index, record);
    } else {
        // Active-cell entry: keep the detail record in `active_tool_details`
        // until the active cell flushes. `flush_active_cell` migrates these
        // records into `tool_details_by_cell` keyed by the eventual real
        // cell index.
        app.active_tool_details.insert(tool_id.to_string(), record);
    }
}

fn store_tool_detail_output(
    app: &mut App,
    tool_id: &str,
    cell_index: usize,
    result: &Result<ToolResult, ToolError>,
) {
    let payload = Some(match result {
        Ok(tool_result) => tool_result.content.clone(),
        Err(err) => err.to_string(),
    });
    if cell_index < app.history.len()
        && let Some(detail) = app.tool_details_by_cell.get_mut(&cell_index)
    {
        detail.output = payload.clone();
    }
    // Also write to the active table while the entry might still live there;
    // some callsites pre-rewrite cell_index but the active_tool_details map is
    // the canonical source for in-flight outputs.
    if let Some(detail) = app.active_tool_details.get_mut(tool_id) {
        detail.output = payload;
    }
}

#[allow(clippy::too_many_lines)]
fn handle_tool_call_complete(
    app: &mut App,
    id: &str,
    name: &str,
    result: &Result<ToolResult, ToolError>,
) {
    if app.ignored_tool_calls.remove(id) {
        return;
    }

    // Exploring entries land in the per-tool map regardless of whether they
    // live in the active cell or in finalized history; the path is the same.
    if let Some((cell_index, entry_index)) = app.exploring_entries.remove(id) {
        app.tool_cells.remove(id);
        store_tool_detail_output(app, id, cell_index, result);
        if let Some(HistoryCell::Tool(ToolCell::Exploring(cell))) =
            app.cell_at_virtual_index_mut(cell_index)
            && let Some(entry) = cell.entries.get_mut(entry_index)
        {
            entry.status = match result.as_ref() {
                Ok(tool_result) if tool_result.success => ToolStatus::Success,
                Ok(_) | Err(_) => ToolStatus::Failed,
            };
            app.mark_history_updated();
            // Mutating the in-flight exploring cell needs an active-cell
            // revision bump so the transcript cache invalidates the synthetic
            // tail row.
            if cell_index >= app.history.len() {
                app.active_cell_revision = app.active_cell_revision.wrapping_add(1);
                if let Some(active) = app.active_cell.as_mut() {
                    active.bump_revision();
                }
            }
        }
        return;
    }

    // Look up the cell by tool id. If the id isn't registered, that's an
    // orphan completion (race condition where the started event was lost or
    // a tool result arrived after the active cell was already flushed). Build
    // a finalized standalone cell from the result so the user can still see
    // the output, but DO NOT touch the active cell.
    let Some(cell_index) = app.tool_cells.remove(id) else {
        push_orphan_tool_completion(app, id, name, result);
        return;
    };

    store_tool_detail_output(app, id, cell_index, result);
    let in_active = cell_index >= app.history.len();

    let status = match result.as_ref() {
        Ok(tool_result) => match tool_result.metadata.as_ref() {
            Some(meta)
                if meta
                    .get("status")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s == "Running") =>
            {
                ToolStatus::Running
            }
            _ => {
                if tool_result.success {
                    ToolStatus::Success
                } else {
                    ToolStatus::Failed
                }
            }
        },
        Err(_) => ToolStatus::Failed,
    };

    if let Some(cell) = app.cell_at_virtual_index_mut(cell_index) {
        match cell {
            HistoryCell::Tool(ToolCell::Exec(exec)) => {
                exec.status = status;
                if let Ok(tool_result) = result.as_ref() {
                    exec.duration_ms = tool_result
                        .metadata
                        .as_ref()
                        .and_then(|m| m.get("duration_ms"))
                        .and_then(serde_json::Value::as_u64);
                    if status != ToolStatus::Running && exec.interaction.is_none() {
                        exec.output = Some(tool_result.content.clone());
                    }
                } else if let Err(err) = result.as_ref()
                    && exec.interaction.is_none()
                {
                    exec.output = Some(err.to_string());
                }
                app.mark_history_updated();
            }
            HistoryCell::Tool(ToolCell::PlanUpdate(plan)) => {
                plan.status = status;
                app.mark_history_updated();
            }
            HistoryCell::Tool(ToolCell::PatchSummary(patch)) => {
                patch.status = status;
                match result.as_ref() {
                    Ok(tool_result) => {
                        if let Ok(json) =
                            serde_json::from_str::<serde_json::Value>(&tool_result.content)
                            && let Some(message) = json.get("message").and_then(|v| v.as_str())
                        {
                            patch.summary = message.to_string();
                        }
                    }
                    Err(err) => {
                        patch.error = Some(err.to_string());
                    }
                }
                app.mark_history_updated();
            }
            HistoryCell::Tool(ToolCell::Review(review)) => {
                review.status = status;
                match result.as_ref() {
                    Ok(tool_result) => {
                        if tool_result.success {
                            review.output = Some(ReviewOutput::from_str(&tool_result.content));
                        } else {
                            review.error = Some(tool_result.content.clone());
                        }
                    }
                    Err(err) => {
                        review.error = Some(err.to_string());
                    }
                }
                app.mark_history_updated();
            }
            HistoryCell::Tool(ToolCell::Mcp(mcp)) => {
                match result.as_ref() {
                    Ok(tool_result) => {
                        let summary = summarize_mcp_output(&tool_result.content);
                        if summary.is_error == Some(true) {
                            mcp.status = ToolStatus::Failed;
                        } else {
                            mcp.status = status;
                        }
                        mcp.is_image = summary.is_image;
                        mcp.content = summary.content;
                    }
                    Err(err) => {
                        mcp.status = status;
                        mcp.content = Some(err.to_string());
                    }
                }
                app.mark_history_updated();
            }
            HistoryCell::Tool(ToolCell::WebSearch(search)) => {
                search.status = status;
                match result.as_ref() {
                    Ok(tool_result) => {
                        search.summary = Some(summarize_tool_output(&tool_result.content));
                    }
                    Err(err) => {
                        search.summary = Some(err.to_string());
                    }
                }
                app.mark_history_updated();
            }
            HistoryCell::Tool(ToolCell::Generic(generic)) => {
                generic.status = status;
                match result.as_ref() {
                    Ok(tool_result) => {
                        generic.output = Some(summarize_tool_output(&tool_result.content));
                    }
                    Err(err) => {
                        generic.output = Some(err.to_string());
                    }
                }
                app.mark_history_updated();
            }
            _ => {}
        }
    }

    // If the mutated cell lived inside the active group, bump the active-cell
    // revision so the transcript cache re-renders the synthetic tail row.
    if in_active {
        app.active_cell_revision = app.active_cell_revision.wrapping_add(1);
        if let Some(active) = app.active_cell.as_mut() {
            active.bump_revision();
        }
    }
}

/// Build a finalized standalone history cell for a tool completion whose
/// start was never registered (orphan). This preserves the contract that
/// every tool result is visible somewhere; the alternative (silently
/// dropping it) hides errors and breaks debuggability.
///
/// Choice of cell type: we use `GenericToolCell` because we have no input
/// payload to reconstruct a more specific cell. The pager remains usable —
/// `tool_details_by_cell` is populated with the result text.
///
/// ## Index drift
///
/// If an active cell is in flight when the orphan arrives, pushing the
/// orphan into `app.history` shifts every active-cell virtual index forward
/// by 1. We must rewrite `tool_cells` / `exploring_entries` accordingly so
/// later completion lookups still find the right entries.
fn push_orphan_tool_completion(
    app: &mut App,
    tool_id: &str,
    name: &str,
    result: &Result<ToolResult, ToolError>,
) {
    let status = match result.as_ref() {
        Ok(tool_result) => {
            if tool_result.success {
                ToolStatus::Success
            } else {
                ToolStatus::Failed
            }
        }
        Err(_) => ToolStatus::Failed,
    };
    let output = match result.as_ref() {
        Ok(tool_result) => Some(summarize_tool_output(&tool_result.content)),
        Err(err) => Some(err.to_string()),
    };
    let history_threshold_before_push = app.history.len();
    let active_in_flight = app.active_cell.is_some();
    app.add_message(HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: name.to_string(),
        status,
        input_summary: None,
        output,
        prompts: None,
    })));
    let cell_index = app.history.len().saturating_sub(1);
    app.tool_details_by_cell.insert(
        cell_index,
        ToolDetailRecord {
            tool_id: tool_id.to_string(),
            tool_name: name.to_string(),
            input: serde_json::Value::Null,
            output: match result.as_ref() {
                Ok(tool_result) => Some(tool_result.content.clone()),
                Err(err) => Some(err.to_string()),
            },
        },
    );

    // Shift active-cell virtual indices forward by 1 to absorb the new
    // history cell. Without this, the next completion would address the
    // wrong entry.
    if active_in_flight {
        let threshold = history_threshold_before_push;
        for idx in app.tool_cells.values_mut() {
            if *idx >= threshold {
                *idx = idx.wrapping_add(1);
            }
        }
        for (cell_idx, _) in app.exploring_entries.values_mut() {
            if *cell_idx >= threshold {
                *cell_idx = cell_idx.wrapping_add(1);
            }
        }
        if let Some(idx) = app.exploring_cell.as_mut()
            && *idx >= threshold
        {
            *idx = idx.wrapping_add(1);
        }
    }
}

fn is_exploring_tool(name: &str) -> bool {
    matches!(name, "read_file" | "list_dir" | "grep_files" | "list_files")
}

fn is_exec_tool(name: &str) -> bool {
    matches!(
        name,
        "exec_shell" | "exec_shell_wait" | "exec_shell_interact" | "exec_wait" | "exec_interact"
    )
}

fn exploring_label(name: &str, input: &serde_json::Value) -> String {
    let fallback = format!("{name} tool");
    let obj = input.as_object();
    match name {
        "read_file" => obj
            .and_then(|o| o.get("path"))
            .and_then(|v| v.as_str())
            .map_or(fallback, |path| format!("Reading {path}")),
        "list_dir" => obj
            .and_then(|o| o.get("path"))
            .and_then(|v| v.as_str())
            .map_or("Listing directory".to_string(), |path| {
                format!("Listing {path}")
            }),
        "grep_files" => {
            let pattern = obj
                .and_then(|o| o.get("pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("pattern");
            format!("Searching for `{pattern}`")
        }
        "list_files" => "Listing files".to_string(),
        _ => fallback,
    }
}

fn is_mcp_tool(name: &str) -> bool {
    name.starts_with("mcp_")
}

fn is_view_image_tool(name: &str) -> bool {
    matches!(name, "view_image" | "view_image_file" | "view_image_tool")
}

fn is_web_search_tool(name: &str) -> bool {
    matches!(name, "web_search" | "search_web" | "search" | "web.run")
        || name.ends_with("_web_search")
}

fn web_search_query(input: &serde_json::Value) -> String {
    if let Some(searches) = input.get("search_query").and_then(|v| v.as_array())
        && let Some(first) = searches.first()
        && let Some(q) = first.get("q").and_then(|v| v.as_str())
    {
        return q.to_string();
    }

    input
        .get("query")
        .or_else(|| input.get("q"))
        .or_else(|| input.get("search"))
        .and_then(|v| v.as_str())
        .unwrap_or("Web search")
        .to_string()
}

fn review_target_label(input: &serde_json::Value) -> String {
    let target = input
        .get("target")
        .and_then(|v| v.as_str())
        .unwrap_or("review")
        .trim();
    let kind = input
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let staged = input
        .get("staged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let target_lower = target.to_ascii_lowercase();

    if kind == "diff"
        || target_lower == "diff"
        || target_lower == "git diff"
        || target_lower == "staged"
        || target_lower == "cached"
    {
        if staged || target_lower == "staged" || target_lower == "cached" {
            return "git diff --cached".to_string();
        }
        return "git diff".to_string();
    }

    target.to_string()
}

fn parse_plan_input(input: &serde_json::Value) -> (Option<String>, Vec<PlanStep>) {
    let explanation = input
        .get("explanation")
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string);
    let mut steps = Vec::new();
    if let Some(items) = input.get("plan").and_then(|v| v.as_array()) {
        for item in items {
            let step = item.get("step").and_then(|v| v.as_str()).unwrap_or("");
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending");
            if !step.is_empty() {
                steps.push(PlanStep {
                    step: step.to_string(),
                    status: status.to_string(),
                });
            }
        }
    }
    (explanation, steps)
}

fn parse_patch_summary(input: &serde_json::Value) -> (String, String) {
    if let Some(changes) = input.get("changes").and_then(|v| v.as_array()) {
        let count = changes.len();
        let path = changes
            .first()
            .and_then(|c| c.get("path"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| "<file>".to_string());
        let label = if count <= 1 {
            path
        } else {
            format!("{count} files")
        };
        let summary = format!("Changes: {count} file(s)");
        return (label, summary);
    }

    let patch_text = input.get("patch").and_then(|v| v.as_str()).unwrap_or("");
    let paths = extract_patch_paths(patch_text);
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            if paths.len() == 1 {
                paths.first().cloned()
            } else if paths.is_empty() {
                None
            } else {
                Some(format!("{} files", paths.len()))
            }
        })
        .unwrap_or_else(|| "<file>".to_string());

    let (adds, removes) = count_patch_changes(patch_text);
    let summary = if adds == 0 && removes == 0 {
        "Patch applied".to_string()
    } else {
        format!("Changes: +{adds} / -{removes}")
    };
    (path, summary)
}

fn extract_patch_paths(patch: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            let raw = rest.trim();
            if raw == "/dev/null" || raw == "dev/null" {
                continue;
            }
            let raw = raw.strip_prefix("b/").unwrap_or(raw);
            if !paths.contains(&raw.to_string()) {
                paths.push(raw.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("diff --git ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(path) = parts.get(1).or_else(|| parts.first()) {
                let raw = path.trim();
                let raw = raw
                    .strip_prefix("b/")
                    .or_else(|| raw.strip_prefix("a/"))
                    .unwrap_or(raw);
                if !paths.contains(&raw.to_string()) {
                    paths.push(raw.to_string());
                }
            }
        }
    }
    paths
}

fn maybe_add_patch_preview(app: &mut App, input: &serde_json::Value) {
    if let Some(patch) = input.get("patch").and_then(|v| v.as_str()) {
        app.add_message(HistoryCell::Tool(ToolCell::DiffPreview(DiffPreviewCell {
            title: "Patch Preview".to_string(),
            diff: patch.to_string(),
        })));
        app.mark_history_updated();
        return;
    }

    if let Some(changes) = input.get("changes").and_then(|v| v.as_array()) {
        let preview = format_changes_preview(changes);
        if !preview.trim().is_empty() {
            app.add_message(HistoryCell::Tool(ToolCell::DiffPreview(DiffPreviewCell {
                title: "Changes Preview".to_string(),
                diff: preview,
            })));
            app.mark_history_updated();
        }
    }
}

fn format_changes_preview(changes: &[serde_json::Value]) -> String {
    let mut out = String::new();
    for change in changes {
        let path = change
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("<file>");
        let content = change.get("content").and_then(|v| v.as_str()).unwrap_or("");

        out.push_str(&format!("diff --git a/{path} b/{path}\n"));
        out.push_str(&format!("--- a/{path}\n+++ b/{path}\n"));
        out.push_str("@@ -0,0 +1,1 @@\n");

        let mut count = 0usize;
        for line in content.lines() {
            out.push('+');
            out.push_str(line);
            out.push('\n');
            count += 1;
            if count >= 20 {
                out.push_str("+... (truncated)\n");
                break;
            }
        }
        if content.is_empty() {
            out.push_str("+\n");
        }
    }
    out
}

fn count_patch_changes(patch: &str) -> (usize, usize) {
    let mut adds = 0;
    let mut removes = 0;
    for line in patch.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            adds += 1;
        } else if line.starts_with('-') {
            removes += 1;
        }
    }
    (adds, removes)
}

fn exec_command_from_input(input: &serde_json::Value) -> Option<String> {
    input
        .get("command")
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
}

fn exec_source_from_input(input: &serde_json::Value) -> ExecSource {
    match input.get("source").and_then(|v| v.as_str()) {
        Some(source) if source.eq_ignore_ascii_case("user") => ExecSource::User,
        _ => ExecSource::Assistant,
    }
}

fn exec_interaction_summary(name: &str, input: &serde_json::Value) -> Option<(String, bool)> {
    let command = exec_command_from_input(input).unwrap_or_else(|| "<command>".to_string());
    let command_display = format!("\"{command}\"");
    let interaction_input = input
        .get("input")
        .or_else(|| input.get("stdin"))
        .or_else(|| input.get("data"))
        .and_then(|v| v.as_str());

    let is_wait_tool = matches!(name, "exec_shell_wait" | "exec_wait");
    let is_interact_tool = matches!(name, "exec_shell_interact" | "exec_interact");

    if is_interact_tool || interaction_input.is_some() {
        let preview = interaction_input.map(summarize_interaction_input);
        let summary = if let Some(preview) = preview {
            format!("Interacted with {command_display}, sent {preview}")
        } else {
            format!("Interacted with {command_display}")
        };
        return Some((summary, false));
    }

    if is_wait_tool || input.get("wait").and_then(serde_json::Value::as_bool) == Some(true) {
        return Some((format!("Waited for {command_display}"), true));
    }

    None
}

fn summarize_interaction_input(input: &str) -> String {
    let mut single_line = input.replace('\r', "");
    single_line = single_line.replace('\n', "\\n");
    single_line = single_line.replace('\"', "'");
    let max_len = 80;
    if single_line.chars().count() <= max_len {
        return format!("\"{single_line}\"");
    }
    let mut out = String::new();
    for ch in single_line.chars().take(max_len.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    format!("\"{out}\"")
}

fn exec_is_background(input: &serde_json::Value) -> bool {
    input
        .get("background")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests;
