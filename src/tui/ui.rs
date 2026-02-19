//! TUI event loop and rendering logic for `DeepSeek` CLI.

use std::fmt::Write;
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
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::audit::log_sensitive_event;
use crate::commands;
use crate::compaction::estimate_input_tokens_conservative;
use crate::config::Config;
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
use crate::tools::plan::StepStatus;
use crate::tools::spec::{ToolError, ToolResult};
use crate::tools::subagent::{SubAgentResult, SubAgentStatus, SubAgentType};
use crate::tools::todo::TodoStatus;
use crate::tui::command_palette::{
    CommandPaletteView, build_entries as build_command_palette_entries,
};
use crate::tui::event_broker::EventBroker;
use crate::tui::onboarding;
use crate::tui::pager::PagerView;
use crate::tui::paste_burst::CharDecision;
use crate::tui::plan_prompt::PlanPromptView;
use crate::tui::scrolling::{ScrollDirection, TranscriptScroll};
use crate::tui::selection::TranscriptSelectionPoint;
use crate::tui::session_picker::SessionPickerView;
use crate::tui::ui_text::{history_cell_to_text, line_to_plain, slice_text};
use crate::tui::user_input::UserInputView;

use super::app::{
    App, AppAction, AppMode, OnboardingState, QueuedMessage, SidebarFocus, StatusToastLevel,
    TaskPanelEntry, ToolDetailRecord, TuiOptions,
};
use super::approval::{
    ApprovalMode, ApprovalRequest, ApprovalView, ElevationRequest, ElevationView, ReviewDecision,
};
use super::history::{
    DiffPreviewCell, ExecCell, ExecSource, ExploringCell, ExploringEntry, GenericToolCell,
    HistoryCell, McpToolCell, PatchSummaryCell, PlanStep, PlanUpdateCell, ReviewCell, ToolCell,
    ToolStatus, ViewImageCell, WebSearchCell, history_cells_from_message, summarize_mcp_output,
    summarize_tool_args, summarize_tool_output,
};
use super::views::{HelpView, ModalKind, ViewEvent};
use super::widgets::{
    ChatWidget, ComposerWidget, HeaderData, HeaderWidget, Renderable, slash_completion_hints,
};

// === Constants ===

const MAX_QUEUED_PREVIEW: usize = 3;
const CONTEXT_WARNING_THRESHOLD_PERCENT: f64 = 85.0;
const CONTEXT_CRITICAL_THRESHOLD_PERCENT: f64 = 85.0;
const UI_IDLE_POLL_MS: u64 = 33;
const UI_ACTIVE_POLL_MS: u64 = 16;
const UI_DEEPSEEK_SQUIGGLE_MS: u64 = 120;
const UI_TYPING_INDICATOR_MS: u64 = 120;
const UI_STATUS_ANIMATION_MS: u64 = UI_DEEPSEEK_SQUIGGLE_MS;
const WORKSPACE_CONTEXT_REFRESH_SECS: u64 = 15;

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
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if use_alt_screen {
        execute!(stdout, EnterAlternateScreen)?;
    }
    execute!(stdout, EnableBracketedPaste, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let event_broker = EventBroker::new();

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
                if let Some(prompt) = saved.system_prompt {
                    app.system_prompt = Some(SystemPrompt::Text(prompt));
                }
                // Convert saved messages to HistoryCell format for display
                app.history.clear();
                app.history.push(HistoryCell::System {
                    content: format!(
                        "Resumed session: {} ({})",
                        saved.metadata.title,
                        &saved.metadata.id[..8.min(saved.metadata.id.len())]
                    ),
                });

                for msg in &saved.messages {
                    app.history.extend(history_cells_from_message(msg));
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
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        DisableMouseCapture
    )?;
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
        max_steps: 100,
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
    config: &Config,
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
                        current_streaming_text.clear();
                        app.streaming_message_index = None;
                    }
                    EngineEvent::MessageDelta { content, .. } => {
                        let sanitized = sanitize_stream_chunk(&content);
                        if sanitized.is_empty() {
                            continue;
                        }
                        current_streaming_text.push_str(&sanitized);
                        let index = if let Some(index) = app.streaming_message_index {
                            index
                        } else {
                            app.add_message(HistoryCell::Assistant {
                                content: String::new(),
                                streaming: true,
                            });
                            let index = app.history.len().saturating_sub(1);
                            app.streaming_message_index = Some(index);
                            index
                        };

                        if let Some(cell) = app.history.get_mut(index) {
                            if let HistoryCell::Assistant { content, .. } = cell {
                                content.push_str(&sanitized);
                            }
                            transcript_batch_updated = true;
                        }
                    }
                    EngineEvent::MessageComplete { .. } => {
                        if let Some(index) = app.streaming_message_index.take()
                            && let Some(HistoryCell::Assistant { streaming, .. }) =
                                app.history.get_mut(index)
                        {
                            *streaming = false;
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
                        app.reasoning_buffer.clear();
                        app.reasoning_header = None;

                        app.add_message(HistoryCell::Thinking {
                            content: String::new(),
                            streaming: true,
                        });
                        app.streaming_message_index = Some(app.history.len().saturating_sub(1));
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

                        if let Some(index) = app.streaming_message_index {
                            if let Some(HistoryCell::Thinking { content: c, .. }) =
                                app.history.get_mut(index)
                            {
                                c.push_str(&sanitized);
                                transcript_batch_updated = true;
                            }
                        }
                    }
                    EngineEvent::ThinkingComplete { .. } => {
                        if let Some(index) = app.streaming_message_index.take() {
                            if let Some(HistoryCell::Thinking { streaming, .. }) =
                                app.history.get_mut(index)
                            {
                                *streaming = false;
                                transcript_batch_updated = true;
                            }
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
                                    &name, output,
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
                    }
                    EngineEvent::TurnStarted { turn_id } => {
                        app.is_loading = true;
                        app.offline_mode = false;
                        current_streaming_text.clear();
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
                        app.is_loading = false;
                        app.offline_mode = false;
                        app.turn_started_at = None;
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
                        if let Some(error) = error {
                            app.status_message = Some(format!("Turn failed: {error}"));
                        }

                        // Update session cost
                        if let Some(turn_cost) = crate::pricing::calculate_turn_cost(
                            &app.model,
                            usage.input_tokens,
                            usage.output_tokens,
                        ) {
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
                    EngineEvent::Error { message, .. } => {
                        app.add_message(HistoryCell::System {
                            content: format!("Error: {message}"),
                        });
                        app.offline_mode = true;
                        app.is_loading = false;
                        app.status_message = Some(format!(
                            "Engine error; queued messages stay pending: {message}"
                        ));
                    }
                    EngineEvent::Status { message } => {
                        app.status_message = Some(message);
                    }
                    EngineEvent::CompactionStarted { message, .. } => {
                        app.status_message = Some(message);
                    }
                    EngineEvent::CompactionCompleted { message, .. } => {
                        app.status_message = Some(message);
                    }
                    EngineEvent::CompactionFailed { message, .. } => {
                        app.status_message = Some(message);
                    }
                    EngineEvent::CapacityDecision {
                        risk_band,
                        action,
                        reason,
                        ..
                    } => {
                        app.status_message = Some(format!(
                            "Capacity decision: risk={risk_band} action={action} ({reason})"
                        ));
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
                            pause_terminal(terminal, app.use_alt_screen)?;
                            event_broker.pause_events();
                        }
                    }
                    EngineEvent::ResumeEvents => {
                        if event_broker.is_paused() {
                            resume_terminal(terminal, app.use_alt_screen)?;
                            event_broker.resume_events();
                        }
                    }
                    EngineEvent::AgentSpawned { id, prompt } => {
                        app.add_message(HistoryCell::System {
                            content: format!(
                                "Sub-agent {id} spawned: {}",
                                summarize_tool_output(&prompt)
                            ),
                        });
                        if app.view_stack.top_kind() == Some(ModalKind::SubAgents) {
                            let _ = engine_handle.send(Op::ListSubAgents).await;
                        }
                    }
                    EngineEvent::AgentProgress { id, status } => {
                        app.status_message = Some(format!("Sub-agent {id}: {status}"));
                    }
                    EngineEvent::AgentComplete { id, result } => {
                        app.add_message(HistoryCell::System {
                            content: format!(
                                "Sub-agent {id} completed: {}",
                                summarize_tool_output(&result)
                            ),
                        });
                        if app.view_stack.top_kind() == Some(ModalKind::SubAgents) {
                            let _ = engine_handle.send(Op::ListSubAgents).await;
                        }
                    }
                    EngineEvent::AgentList { agents } => {
                        app.subagent_cache = agents.clone();
                        if app.view_stack.update_subagents(&agents) {
                            app.status_message =
                                Some(format!("Sub-agents: {} total", agents.len()));
                        } else {
                            app.add_message(HistoryCell::System {
                                content: format_subagent_list(&agents),
                            });
                        }
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
                            app.add_message(HistoryCell::System {
                                content: format!(
                                    "Blocked tool '{tool_name}' (approval_mode=never)"
                                ),
                            });
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
                            let request = ApprovalRequest::new(&id, &tool_name, &tool_input);
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
                            app.add_message(HistoryCell::System {
                                content: format!(
                                    "Approval required for tool '{tool_name}': {description}"
                                ),
                            });
                        }
                    }
                    EngineEvent::UserInputRequired { id, request } => {
                        app.view_stack.push(UserInputView::new(id.clone(), request));
                        app.add_message(HistoryCell::System {
                            content: "User input requested".to_string(),
                        });
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
                            app.add_message(HistoryCell::System {
                                content: format!("Sandbox blocked {tool_name}: {denial_reason}"),
                            });
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
            handle_view_events(app, &engine_handle, events).await;
        }

        if app.is_loading
            && last_status_frame.elapsed() >= Duration::from_millis(UI_STATUS_ANIMATION_MS)
        {
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
        refresh_workspace_context_if_needed(app, now);

        if app.needs_redraw {
            terminal.draw(|f| render(f, app))?; // app is &mut
            app.needs_redraw = false;
        }

        let mut poll_timeout = if app.is_loading {
            Duration::from_millis(UI_ACTIVE_POLL_MS)
        } else {
            Duration::from_millis(UI_IDLE_POLL_MS)
        };
        if let Some(until_flush) = app.paste_burst.next_flush_delay(now) {
            poll_timeout = poll_timeout.min(until_flush);
        }
        if event::poll(poll_timeout)? {
            let evt = event::read()?;
            app.needs_redraw = true;

            // Handle bracketed paste events
            if let Event::Paste(text) = &evt {
                if app.onboarding == OnboardingState::ApiKey {
                    // Paste into API key input
                    app.insert_api_key_str(text);
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
                terminal.clear()?;
                app.handle_resize(width, height);
                continue;
            }

            if let Event::Mouse(mouse) = evt {
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
                    KeyCode::Esc => {
                        if app.onboarding == OnboardingState::ApiKey {
                            app.onboarding = OnboardingState::Welcome;
                            app.api_key_input.clear();
                            app.api_key_cursor = 0;
                            app.status_message = None;
                        }
                    }
                    KeyCode::Enter => match app.onboarding {
                        OnboardingState::Welcome => {
                            advance_onboarding(app);
                        }
                        OnboardingState::ApiKey => {
                            let key = app.api_key_input.trim().to_string();
                            if let Some(warning) = validate_api_key_for_onboarding(&key) {
                                app.status_message = Some(warning);
                                continue;
                            }
                            match app.submit_api_key() {
                                Ok(_) => {
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
                    }
                    KeyCode::Char(c) if app.onboarding == OnboardingState::ApiKey => {
                        app.insert_api_key_char(c);
                    }
                    KeyCode::Char('v') | KeyCode::Char('V')
                        if is_paste_shortcut(&key) && app.onboarding == OnboardingState::ApiKey =>
                    {
                        // Cmd+V / Ctrl+V paste (bracketed paste handled above)
                        app.paste_api_key_from_clipboard();
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
                app.view_stack
                    .push(CommandPaletteView::new(build_command_palette_entries(
                        &app.skills_dir,
                        &app.workspace,
                    )));
                continue;
            }

            if !app.view_stack.is_empty() {
                let events = app.view_stack.handle_key(key);
                handle_view_events(app, &engine_handle, events).await;
                continue;
            }

            let now = Instant::now();
            app.flush_paste_burst_if_due(now);

            let has_ctrl_alt_or_super = key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT)
                || key.modifiers.contains(KeyModifiers::SUPER);
            let is_plain_char = matches!(key.code, KeyCode::Char(_)) && !has_ctrl_alt_or_super;
            let is_enter = matches!(key.code, KeyCode::Enter);

            if !is_plain_char
                && !is_enter
                && let Some(pending) = app.paste_burst.flush_before_modified_input()
            {
                app.insert_str(&pending);
            }

            if (is_plain_char || is_enter) && handle_paste_burst_key(app, &key, now) {
                continue;
            }

            let slash_menu_entries = visible_slash_menu_entries(app, 6);
            let slash_menu_open = !slash_menu_entries.is_empty();
            if slash_menu_open && app.slash_menu_selected >= slash_menu_entries.len() {
                app.slash_menu_selected = slash_menu_entries.len().saturating_sub(1);
            }

            // Global keybindings
            match key.code {
                KeyCode::Enter if app.input.is_empty() && app.transcript_selection.is_active() => {
                    if open_pager_for_selection(app) {
                        continue;
                    }
                }
                KeyCode::Char('l') if key.modifiers.is_empty() && app.input.is_empty() => {
                    if open_pager_for_last_message(app) {
                        continue;
                    }
                }
                KeyCode::Char('v') if key.modifiers.is_empty() && app.input.is_empty() => {
                    if open_tool_details_pager(app) {
                        continue;
                    }
                }
                KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::ALT) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.set_sidebar_focus(SidebarFocus::Plan);
                        app.status_message = Some("Sidebar focus: plan".to_string());
                    } else {
                        app.set_mode(AppMode::Normal);
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
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.set_sidebar_focus(SidebarFocus::Agents);
                        app.status_message = Some("Sidebar focus: agents".to_string());
                    } else {
                        app.set_mode(AppMode::Plan);
                    }
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
                    // Cancel current request or exit
                    if app.is_loading {
                        engine_handle.cancel();
                        app.is_loading = false;
                        app.status_message = Some("Request cancelled".to_string());
                    } else {
                        let _ = engine_handle.send(Op::Shutdown).await;
                        return Ok(());
                    }
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if app.input.is_empty() {
                        let _ = engine_handle.send(Op::Shutdown).await;
                        return Ok(());
                    }
                }
                KeyCode::Esc => {
                    if slash_menu_open {
                        app.close_slash_menu();
                    } else if app.is_loading {
                        engine_handle.cancel();
                        app.is_loading = false;
                        app.status_message = Some("Request cancelled".to_string());
                    } else if !app.input.is_empty() {
                        app.clear_input();
                    } else {
                        app.set_mode(AppMode::Normal);
                    }
                }
                KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.scroll_up(3);
                }
                KeyCode::Up if key.modifiers.is_empty() && slash_menu_open => {
                    if app.slash_menu_selected > 0 {
                        app.slash_menu_selected = app.slash_menu_selected.saturating_sub(1);
                    }
                }
                KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.scroll_down(3);
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
                    if slash_menu_open && apply_slash_menu_selection(app, &slash_menu_entries, true)
                    {
                        continue;
                    }
                    if try_autocomplete_slash_command(app) {
                        continue;
                    }
                    app.cycle_mode();
                }
                KeyCode::BackTab => {
                    app.cycle_mode_reverse();
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
                    if key.modifiers.is_empty() && app.input.is_empty() && !slash_menu_open =>
                {
                    if !jump_to_adjacent_tool_cell(app, SearchDirection::Backward) {
                        app.status_message = Some("No previous tool output".to_string());
                    }
                }
                KeyCode::Char(']')
                    if key.modifiers.is_empty() && app.input.is_empty() && !slash_menu_open =>
                {
                    if !jump_to_adjacent_tool_cell(app, SearchDirection::Forward) {
                        app.status_message = Some("No next tool output".to_string());
                    }
                }
                // Input handling
                KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.insert_char('\n');
                }
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.insert_char('\n');
                }
                KeyCode::Enter => {
                    if let Some(input) = app.submit_input() {
                        if handle_plan_choice(app, &engine_handle, &input).await? {
                            continue;
                        }
                        if input.starts_with('/') {
                            // Use the commands module for slash commands
                            let result = commands::execute(&input, app);

                            // Handle command result
                            if let Some(msg) = result.message {
                                app.add_message(HistoryCell::System { content: msg });
                            }

                            if let Some(action) = result.action {
                                match action {
                                    AppAction::Quit => {
                                        let _ = engine_handle.send(Op::Shutdown).await;
                                        return Ok(());
                                    }
                                    AppAction::SaveSession(path) => {
                                        app.status_message =
                                            Some(format!("Session saved to {}", path.display()));
                                    }
                                    AppAction::LoadSession(path) => {
                                        app.status_message =
                                            Some(format!("Session loaded from {}", path.display()));
                                    }
                                    AppAction::SyncSession {
                                        messages,
                                        system_prompt,
                                        model,
                                        workspace,
                                    } => {
                                        let is_full_reset =
                                            messages.is_empty() && system_prompt.is_none();
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
                                        submit_or_steer_message(app, &engine_handle, queued)
                                            .await?;
                                    }
                                    AppAction::ListSubAgents => {
                                        let _ = engine_handle.send(Op::ListSubAgents).await;
                                    }
                                    AppAction::FetchModels => {
                                        app.status_message = Some("Fetching models...".to_string());
                                        match fetch_available_models(config).await {
                                            Ok(models) => {
                                                app.add_message(HistoryCell::System {
                                                    content: format_available_models_message(
                                                        &app.model, &models,
                                                    ),
                                                });
                                                app.status_message = Some(format!(
                                                    "Found {} model(s)",
                                                    models.len()
                                                ));
                                            }
                                            Err(error) => {
                                                app.add_message(HistoryCell::System {
                                                    content: format!(
                                                        "Failed to fetch models: {error}"
                                                    ),
                                                });
                                            }
                                        }
                                    }
                                    AppAction::UpdateCompaction(compaction) => {
                                        let _ = engine_handle
                                            .send(Op::SetCompaction { config: compaction })
                                            .await;
                                    }
                                    AppAction::CompactContext => {
                                        app.status_message =
                                            Some("Compacting context...".to_string());
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
                                            auto_approve: Some(true),
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
                                                app.status_message =
                                                    Some(format!("Queued {}", task.id));
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
                                    AppAction::TaskShow { id } => {
                                        match task_manager.get_task(&id).await {
                                            Ok(task) => open_task_pager(app, &task),
                                            Err(err) => {
                                                app.add_message(HistoryCell::System {
                                                    content: format!("Task lookup failed: {err}"),
                                                });
                                            }
                                        }
                                    }
                                    AppAction::TaskCancel { id } => {
                                        match task_manager.cancel_task(&id).await {
                                            Ok(task) => {
                                                app.add_message(HistoryCell::System {
                                                    content: format!(
                                                        "Task {} status: {:?}",
                                                        task.id, task.status
                                                    ),
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
                        } else {
                            // Global @ file completion - works in any mode
                            if let Some(path) = input.trim().strip_prefix('@') {
                                let command = format!("/load @{path}");
                                let result = commands::execute(&command, app);
                                if let Some(msg) = result.message {
                                    app.add_message(HistoryCell::System { content: msg });
                                }
                                continue;
                            }

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
                KeyCode::End | KeyCode::Char('e')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    app.move_cursor_end();
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
                KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let new_mode = match app.mode {
                        AppMode::Agent => AppMode::Normal,
                        _ => AppMode::Agent,
                    };
                    app.set_mode(new_mode);
                }
                KeyCode::Char('v') if is_paste_shortcut(&key) => {
                    app.paste_from_clipboard();
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_mode(AppMode::Normal);
                    continue;
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
                KeyCode::Char('N') if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.set_mode(AppMode::Normal);
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

fn handle_paste_burst_key(app: &mut App, key: &KeyEvent, now: Instant) -> bool {
    let has_ctrl_alt_or_super = key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::ALT)
        || key.modifiers.contains(KeyModifiers::SUPER);

    match key.code {
        KeyCode::Enter => {
            if !in_command_context(app) && app.paste_burst.append_newline_if_active(now) {
                return true;
            }
            if !in_command_context(app)
                && app.paste_burst.newline_should_insert_instead_of_submit(now)
            {
                app.insert_char('\n');
                app.paste_burst.extend_window(now);
                return true;
            }
        }
        KeyCode::Char(c) if !has_ctrl_alt_or_super => {
            if !c.is_ascii() {
                if let Some(pending) = app.paste_burst.flush_before_modified_input() {
                    app.insert_str(&pending);
                }
                if app.paste_burst.try_append_char_if_active(c, now) {
                    return true;
                }
                if let Some(decision) = app.paste_burst.on_plain_char_no_hold(now) {
                    return handle_paste_burst_decision(app, decision, c, now);
                }
                app.insert_char(c);
                return true;
            }

            let decision = app.paste_burst.on_plain_char(c, now);
            return handle_paste_burst_decision(app, decision, c, now);
        }
        _ => {}
    }

    false
}

fn handle_paste_burst_decision(
    app: &mut App,
    decision: CharDecision,
    c: char,
    now: Instant,
) -> bool {
    match decision {
        CharDecision::RetainFirstChar => true,
        CharDecision::BeginBufferFromPending | CharDecision::BufferAppend => {
            app.paste_burst.append_char_to_buffer(c, now);
            true
        }
        CharDecision::BeginBuffer { retro_chars } => {
            if apply_paste_burst_retro_capture(app, retro_chars as usize, c, now) {
                return true;
            }
            app.insert_char(c);
            true
        }
    }
}

fn apply_paste_burst_retro_capture(
    app: &mut App,
    retro_chars: usize,
    c: char,
    now: Instant,
) -> bool {
    let cursor_byte = app.cursor_byte_index();
    let before = &app.input[..cursor_byte];
    let Some(grab) = app
        .paste_burst
        .decide_begin_buffer(now, before, retro_chars)
    else {
        return false;
    };
    if !grab.grabbed.is_empty() {
        app.input.replace_range(grab.start_byte..cursor_byte, "");
        let removed = grab.grabbed.chars().count();
        app.cursor_position = app.cursor_position.saturating_sub(removed);
    }
    app.paste_burst.append_char_to_buffer(c, now);
    true
}

fn in_command_context(app: &App) -> bool {
    app.input.starts_with('/')
}

fn visible_slash_menu_entries(app: &App, limit: usize) -> Vec<String> {
    if app.slash_menu_hidden {
        return Vec::new();
    }
    slash_completion_hints(&app.input, limit)
}

fn apply_slash_menu_selection(app: &mut App, entries: &[String], append_space: bool) -> bool {
    if entries.is_empty() {
        return false;
    }

    let selected_idx = app.slash_menu_selected.min(entries.len().saturating_sub(1));
    let mut command = entries[selected_idx].clone();

    if append_space && !command.ends_with(' ') && !command.contains(char::is_whitespace) {
        if let Some(info) = commands::get_command_info(command.trim_start_matches('/'))
            && (info.usage.contains('<') || info.usage.contains('['))
        {
            command.push(' ');
        }
    }

    app.input = command;
    app.cursor_position = app.input.chars().count();
    app.slash_menu_hidden = false;
    app.status_message = Some(format!("Command selected: {}", app.input.trim_end()));
    true
}

fn try_autocomplete_slash_command(app: &mut App) -> bool {
    if !app.input.starts_with('/') || app.input.contains(char::is_whitespace) {
        return false;
    }

    let prefix = app.input.trim_start_matches('/');
    let matches = commands::commands_matching(prefix);
    if matches.is_empty() {
        return false;
    }

    let names = matches.iter().map(|info| info.name).collect::<Vec<_>>();
    let shared = longest_common_prefix(&names);

    if !shared.is_empty() && shared.len() > prefix.len() {
        app.input = format!("/{shared}");
        app.cursor_position = app.input.chars().count();
        app.slash_menu_hidden = false;
        app.status_message = Some(format!("Autocomplete: /{shared}"));
        return true;
    }

    if matches.len() == 1 {
        let completed = format!("/{} ", matches[0].name);
        app.input = completed.clone();
        app.cursor_position = completed.chars().count();
        app.slash_menu_hidden = false;
        app.status_message = Some(format!("Command completed: {}", completed.trim_end()));
        return true;
    }

    let preview = matches
        .iter()
        .take(5)
        .map(|info| format!("/{}", info.name))
        .collect::<Vec<_>>()
        .join(", ");
    app.status_message = Some(format!("Suggestions: {preview}"));
    true
}

fn longest_common_prefix<'a>(values: &[&'a str]) -> &'a str {
    let Some(first) = values.first().copied() else {
        return "";
    };
    let mut end = first.len();

    for value in values.iter().skip(1) {
        while end > 0 && !value.starts_with(&first[..end]) {
            end -= 1;
        }
        if end == 0 {
            return "";
        }
    }

    &first[..end]
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
        updated.metadata.mode = Some(app.mode.label().to_string());
        updated
    } else {
        create_saved_session_with_mode(
            &app.api_messages,
            &app.model,
            &app.workspace,
            u64::from(app.total_tokens),
            app.system_prompt.as_ref(),
            Some(app.mode.label()),
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

fn validate_api_key_for_onboarding(api_key: &str) -> Option<String> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Some("API key cannot be empty.".to_string());
    }
    if trimmed.contains(char::is_whitespace) {
        return Some("API key appears malformed (contains whitespace).".to_string());
    }
    if trimmed.len() < 16 {
        return Some("API key appears too short. Please paste the full key.".to_string());
    }
    if !trimmed.contains('-') {
        return Some(
            "API key format looks unusual. Check that the full key was copied.".to_string(),
        );
    }
    None
}

fn build_queued_message(app: &mut App, input: String) -> QueuedMessage {
    let skill_instruction = app.active_skill.take();
    QueuedMessage::new(input, skill_instruction)
}

async fn dispatch_user_message(
    app: &mut App,
    engine_handle: &EngineHandle,
    message: QueuedMessage,
) -> Result<()> {
    // Set immediately to prevent double-dispatch before TurnStarted event arrives.
    app.is_loading = true;

    let content = message.content();
    app.system_prompt = Some(prompts::system_prompt_for_mode_with_context(
        app.mode,
        &app.workspace,
        None,
    ));
    app.add_message(HistoryCell::User {
        content: message.display.clone(),
    });
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
    // Persist immediately so abrupt termination can recover this in-flight turn.
    persist_checkpoint(app);

    engine_handle
        .send(Op::SendMessage {
            content,
            mode: app.mode,
            model: app.model.clone(),
            allow_shell: app.allow_shell,
            trust_mode: app.trust_mode,
        })
        .await?;

    Ok(())
}

async fn steer_user_message(
    app: &mut App,
    engine_handle: &EngineHandle,
    message: QueuedMessage,
) -> Result<()> {
    let content = message.content();

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
        "Plan ready. Review and choose:",
        "  1) Accept + implement in Agent mode",
        "  2) Accept + implement in YOLO mode",
        "  3) Revise the plan / ask follow-ups",
        "  4) Exit Plan mode",
        "",
        "Use the plan confirmation popup or type 1-4 and press Enter.",
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
    let trimmed = input.trim().to_lowercase();
    let first = trimmed.chars().next()?;
    if let Some(digit) = first.to_digit(10)
        && let Some(choice) = plan_choice_from_option(usize::try_from(digit).unwrap_or(0))
    {
        return Some(choice);
    }

    match trimmed.as_str() {
        "accept" | "approve" | "agent" | "a" => Some(PlanChoice::AcceptAgent),
        "accept-yolo" | "yolo" | "y" => Some(PlanChoice::AcceptYolo),
        "revise" | "edit" | "plan" | "stay" => Some(PlanChoice::RevisePlan),
        "normal" | "exit" | "cancel" | "back" => Some(PlanChoice::ExitPlan),
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
    let queued_preview = app.queued_message_previews(MAX_QUEUED_PREVIEW);
    let queued_lines = if queued_preview.is_empty() {
        0
    } else {
        queued_preview.len() + 1
    };
    let editing_lines = usize::from(app.queued_draft.is_some());
    let status_lines = usize::from(app.is_loading);
    let status_height =
        u16::try_from(status_lines + queued_lines + editing_lines).unwrap_or(u16::MAX);
    let prompt = prompt_for_mode(app.mode);
    let available_height = size
        .height
        .saturating_sub(header_height + footer_height + status_height);
    let composer_height = {
        let composer_widget = ComposerWidget::new(app, prompt, available_height);
        composer_widget.desired_height(size.width)
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),   // Header
            Constraint::Min(1),                  // Chat area
            Constraint::Length(status_height),   // Status indicator
            Constraint::Length(composer_height), // Composer
            Constraint::Length(footer_height),   // Footer
        ])
        .split(size);

    // Render header
    {
        let context_window = crate::models::context_window_for_model(&app.model);
        let header_data =
            HeaderData::new(app.mode, &app.model, app.is_loading, app.ui_theme.header_bg)
                .with_usage(
                    app.total_conversation_tokens,
                    context_window,
                    app.session_cost,
                    app.last_prompt_tokens,
                );
        let header_widget = HeaderWidget::new(header_data);
        let buf = f.buffer_mut();
        header_widget.render(chunks[0], buf);
    }

    // Render chat + sidebar
    {
        let mut chat_area = chunks[1];
        let mut sidebar_area = None;

        if chunks[1].width >= 100 {
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
            render_sidebar(f, sidebar_area, app);
        }
    }

    // Render status
    if status_height > 0 {
        render_status_indicator(f, chunks[2], app, &queued_preview);
    }

    // Render composer
    let cursor_pos = {
        let composer_widget = ComposerWidget::new(app, prompt, available_height);
        let buf = f.buffer_mut();
        composer_widget.render(chunks[3], buf);
        composer_widget.cursor_pos(chunks[3])
    };
    if let Some(cursor_pos) = cursor_pos {
        f.set_cursor_position(cursor_pos);
    }

    // Render footer
    render_footer(f, chunks[4], app);

    if !app.view_stack.is_empty() {
        let buf = f.buffer_mut();
        app.view_stack.render(size, buf);
    }
}

fn render_sidebar(f: &mut Frame, area: Rect, app: &App) {
    if area.width < 24 || area.height < 8 {
        return;
    }

    match app.sidebar_focus {
        SidebarFocus::Auto => {
            let sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Min(6),
                ])
                .split(area);

            render_sidebar_plan(f, sections[0], app);
            render_sidebar_todos(f, sections[1], app);
            render_sidebar_tasks(f, sections[2], app);
            render_sidebar_subagents(f, sections[3], app);
        }
        SidebarFocus::Plan => render_sidebar_plan(f, area, app),
        SidebarFocus::Todos => render_sidebar_todos(f, area, app),
        SidebarFocus::Tasks => render_sidebar_tasks(f, area, app),
        SidebarFocus::Agents => render_sidebar_subagents(f, area, app),
    }
}

fn render_sidebar_plan(f: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();

    match app.plan_state.try_lock() {
        Ok(plan) => {
            if plan.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No active plan",
                    Style::default().fg(palette::TEXT_MUTED),
                )));
            } else {
                let (pending, in_progress, completed) = plan.counts();
                let total = pending + in_progress + completed;
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{}%", plan.progress_percent()),
                        Style::default().fg(palette::STATUS_SUCCESS).bold(),
                    ),
                    Span::styled(
                        format!(" complete ({completed}/{total})"),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                ]));

                if let Some(explanation) = plan.explanation() {
                    lines.push(Line::from(Span::styled(
                        truncate_line_to_width(explanation, content_width.max(1)),
                        Style::default().fg(palette::TEXT_DIM),
                    )));
                }

                let usable_rows = area.height.saturating_sub(3) as usize;
                let max_steps = usable_rows.saturating_sub(lines.len());
                for step in plan.steps().iter().take(max_steps) {
                    let (prefix, color) = match &step.status {
                        StepStatus::Pending => ("[ ]", palette::TEXT_MUTED),
                        StepStatus::InProgress => ("[~]", palette::STATUS_WARNING),
                        StepStatus::Completed => ("[x]", palette::STATUS_SUCCESS),
                    };
                    let mut text = format!("{prefix} {}", step.text);
                    let elapsed = step.elapsed_str();
                    if !elapsed.is_empty() {
                        let _ = write!(text, " ({elapsed})");
                    }
                    lines.push(Line::from(Span::styled(
                        truncate_line_to_width(&text, content_width.max(1)),
                        Style::default().fg(color),
                    )));
                }

                let remaining = plan.steps().len().saturating_sub(max_steps);
                if remaining > 0 {
                    lines.push(Line::from(Span::styled(
                        format!("+{remaining} more steps"),
                        Style::default().fg(palette::TEXT_MUTED),
                    )));
                }
            }
        }
        Err(_) => {
            lines.push(Line::from(Span::styled(
                "Plan state updating...",
                Style::default().fg(palette::TEXT_MUTED),
            )));
        }
    }

    render_sidebar_section(f, area, "Plan", lines);
}

fn render_sidebar_todos(f: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();

    match app.todos.try_lock() {
        Ok(todos) => {
            let snapshot = todos.snapshot();
            if snapshot.items.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No todos",
                    Style::default().fg(palette::TEXT_MUTED),
                )));
            } else {
                let total = snapshot.items.len();
                let completed = snapshot
                    .items
                    .iter()
                    .filter(|item| item.status == TodoStatus::Completed)
                    .count();
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{}%", snapshot.completion_pct),
                        Style::default().fg(palette::STATUS_SUCCESS).bold(),
                    ),
                    Span::styled(
                        format!(" complete ({completed}/{total})"),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                ]));

                let usable_rows = area.height.saturating_sub(3) as usize;
                let max_items = usable_rows.saturating_sub(lines.len());
                for item in snapshot.items.iter().take(max_items) {
                    let (prefix, color) = match item.status {
                        TodoStatus::Pending => ("[ ]", palette::TEXT_MUTED),
                        TodoStatus::InProgress => ("[~]", palette::STATUS_WARNING),
                        TodoStatus::Completed => ("[x]", palette::STATUS_SUCCESS),
                    };
                    let text = format!("{prefix} #{} {}", item.id, item.content);
                    lines.push(Line::from(Span::styled(
                        truncate_line_to_width(&text, content_width.max(1)),
                        Style::default().fg(color),
                    )));
                }

                let remaining = snapshot.items.len().saturating_sub(max_items);
                if remaining > 0 {
                    lines.push(Line::from(Span::styled(
                        format!("+{remaining} more todos"),
                        Style::default().fg(palette::TEXT_MUTED),
                    )));
                }
            }
        }
        Err(_) => {
            lines.push(Line::from(Span::styled(
                "Todo list updating...",
                Style::default().fg(palette::TEXT_MUTED),
            )));
        }
    }

    render_sidebar_section(f, area, "Todos", lines);
}

fn render_sidebar_tasks(f: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();

    if let Some(turn_id) = app.runtime_turn_id.as_ref() {
        let status = app
            .runtime_turn_status
            .as_deref()
            .unwrap_or("unknown")
            .to_string();
        lines.push(Line::from(Span::styled(
            truncate_line_to_width(
                &format!("turn {} ({status})", truncate_line_to_width(turn_id, 12)),
                content_width.max(1),
            ),
            Style::default().fg(palette::DEEPSEEK_SKY),
        )));
    }

    if app.task_panel.is_empty() {
        lines.push(Line::from(Span::styled(
            "No tasks",
            Style::default().fg(palette::TEXT_MUTED),
        )));
    } else {
        let running = app
            .task_panel
            .iter()
            .filter(|task| task.status == "running")
            .count();
        lines.push(Line::from(vec![
            Span::styled(
                format!("{running} running"),
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            ),
            Span::styled(
                format!(" / {}", app.task_panel.len()),
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]));

        let usable_rows = area.height.saturating_sub(3) as usize;
        let max_items = usable_rows.saturating_sub(lines.len());
        for task in app.task_panel.iter().take(max_items) {
            let color = match task.status.as_str() {
                "queued" => palette::TEXT_MUTED,
                "running" => palette::STATUS_WARNING,
                "completed" => palette::STATUS_SUCCESS,
                "failed" => palette::STATUS_ERROR,
                "canceled" => palette::TEXT_DIM,
                _ => palette::TEXT_MUTED,
            };
            let duration = task
                .duration_ms
                .map(|ms| format!("{:.1}s", ms as f64 / 1000.0))
                .unwrap_or_else(|| "-".to_string());
            let label = format!(
                "{} {} {}",
                truncate_line_to_width(&task.id, 10),
                task.status,
                duration
            );
            lines.push(Line::from(Span::styled(
                truncate_line_to_width(&label, content_width.max(1)),
                Style::default().fg(color),
            )));
            lines.push(Line::from(Span::styled(
                format!(
                    "  {}",
                    truncate_line_to_width(
                        &task.prompt_summary,
                        content_width.saturating_sub(2).max(1)
                    )
                ),
                Style::default().fg(palette::TEXT_DIM),
            )));
        }
    }

    render_sidebar_section(f, area, "Tasks", lines);
}

fn render_sidebar_subagents(f: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }

    let content_width = area.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();

    if app.subagent_cache.is_empty() {
        lines.push(Line::from(Span::styled(
            "No sub-agents",
            Style::default().fg(palette::TEXT_MUTED),
        )));
    } else {
        let running = app
            .subagent_cache
            .iter()
            .filter(|agent| matches!(agent.status, SubAgentStatus::Running))
            .count();
        lines.push(Line::from(vec![
            Span::styled(
                format!("{running} running"),
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            ),
            Span::styled(
                format!(" / {}", app.subagent_cache.len()),
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]));

        let usable_rows = area.height.saturating_sub(3) as usize;
        let max_agents = usable_rows.saturating_sub(lines.len());
        for agent in app.subagent_cache.iter().take(max_agents) {
            let (status_label, status_color) = match &agent.status {
                SubAgentStatus::Running => ("running", palette::STATUS_WARNING),
                SubAgentStatus::Completed => ("done", palette::STATUS_SUCCESS),
                SubAgentStatus::Failed(_) => ("failed", palette::STATUS_ERROR),
                SubAgentStatus::Cancelled => ("cancelled", palette::TEXT_MUTED),
            };
            let agent_type = match agent.agent_type {
                SubAgentType::General => "general",
                SubAgentType::Explore => "explore",
                SubAgentType::Plan => "plan",
                SubAgentType::Review => "review",
                SubAgentType::Custom => "custom",
            };
            let summary = format!(
                "{} {agent_type} {status_label} ({} steps)",
                truncate_line_to_width(&agent.agent_id, 10),
                agent.steps_taken
            );
            lines.push(Line::from(Span::styled(
                truncate_line_to_width(&summary, content_width.max(1)),
                Style::default().fg(status_color),
            )));
        }

        let remaining = app.subagent_cache.len().saturating_sub(max_agents);
        if remaining > 0 {
            lines.push(Line::from(Span::styled(
                format!("+{remaining} more agents"),
                Style::default().fg(palette::TEXT_MUTED),
            )));
        }
    }

    render_sidebar_section(f, area, "Sub-Agents", lines);
}

fn render_sidebar_section(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>) {
    if area.width < 4 || area.height < 3 {
        return;
    }

    let section = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::default()
            .title(Line::from(vec![Span::styled(
                format!(" {title} "),
                Style::default().fg(palette::DEEPSEEK_BLUE).bold(),
            )]))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::DEEPSEEK_INK))
            .padding(Padding::uniform(1)),
    );

    f.render_widget(section, area);
}

async fn handle_view_events(app: &mut App, engine_handle: &EngineHandle, events: Vec<ViewEvent>) {
    for event in events {
        match event {
            ViewEvent::CommandPaletteSelected { command } => {
                app.input = command;
                app.cursor_position = app.input.chars().count();
                app.status_message = Some("Command inserted. Press Enter to run.".to_string());
            }
            ViewEvent::OpenTextPager { title, content } => {
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
            ViewEvent::SubAgentsRefresh => {
                app.status_message = Some("Refreshing sub-agents...".to_string());
                let _ = engine_handle.send(Op::ListSubAgents).await;
            }
        }
    }
}

fn apply_loaded_session(app: &mut App, session: &SavedSession) {
    app.api_messages.clone_from(&session.messages);
    app.history.clear();
    app.tool_cells.clear();
    app.tool_details_by_cell.clear();
    app.exploring_entries.clear();
    app.ignored_tool_calls.clear();
    app.pending_tool_uses.clear();
    app.last_exec_wait_command = None;

    for msg in &app.api_messages {
        app.history.extend(history_cells_from_message(msg));
    }
    app.mark_history_updated();
    app.transcript_selection.clear();
    app.model.clone_from(&session.metadata.model);
    app.update_model_compaction_budget();
    app.workspace.clone_from(&session.metadata.workspace);
    app.total_tokens = u32::try_from(session.metadata.total_tokens).unwrap_or(u32::MAX);
    app.total_conversation_tokens = app.total_tokens;
    app.last_prompt_tokens = None;
    app.last_completion_tokens = None;
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

fn refresh_workspace_context_if_needed(app: &mut App, now: Instant) {
    if app
        .workspace_context_refreshed_at
        .is_some_and(|refreshed_at| {
            now.duration_since(refreshed_at) < Duration::from_secs(WORKSPACE_CONTEXT_REFRESH_SECS)
        })
    {
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
) -> Result<()> {
    disable_raw_mode()?;
    if use_alt_screen {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    Ok(())
}

fn resume_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    use_alt_screen: bool,
) -> Result<()> {
    enable_raw_mode()?;
    if use_alt_screen {
        execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    }
    execute!(
        terminal.backend_mut(),
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    terminal.clear()?;
    Ok(())
}

fn render_status_indicator(f: &mut Frame, area: Rect, app: &App, queued: &[String]) {
    let mut lines = Vec::new();

    if app.is_loading {
        let header = if app.show_thinking {
            app.reasoning_header.clone()
        } else {
            None
        };
        let elapsed = app.turn_started_at.map(format_elapsed);
        // Use typing indicator when streaming content, otherwise use a subtle status glyph.
        let has_streaming_content = app.streaming_message_index.is_some();
        let spinner = if has_streaming_content {
            typing_indicator(app.turn_started_at)
        } else {
            deepseek_squiggle(app.turn_started_at)
        };
        let (label, label_color) = if has_streaming_content {
            ("ANSWER", palette::DEEPSEEK_SKY)
        } else if app.show_thinking {
            ("THINKING", palette::STATUS_WARNING)
        } else {
            ("WORKING", palette::STATUS_WARNING)
        };
        let mut spans = vec![
            Span::styled(spinner, Style::default().fg(palette::DEEPSEEK_SKY).bold()),
            Span::raw(" "),
            Span::styled(label, Style::default().fg(label_color).bold()),
        ];
        if !has_streaming_content && let Some(header) = header {
            spans.push(Span::raw(": "));
            spans.push(Span::styled(
                header,
                Style::default().fg(palette::STATUS_WARNING),
            ));
        }

        if let Some(elapsed) = elapsed {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled(
                elapsed,
                Style::default().fg(palette::TEXT_MUTED),
            ));
        }

        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            "Esc/Ctrl+C to interrupt",
            Style::default().fg(palette::TEXT_MUTED),
        ));

        lines.push(Line::from(spans));
    }

    if let Some(draft) = app.queued_draft.as_ref() {
        let available = area.width as usize;
        let prefix = "Editing queued:";
        let prefix_width = prefix.width() + 1;
        let max_len = available.saturating_sub(prefix_width).max(1);
        let preview = truncate_line_to_width(&draft.display, max_len);
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(palette::TEXT_MUTED)),
            Span::raw(" "),
            Span::styled(preview, Style::default().fg(palette::DEEPSEEK_SKY)),
        ]));
    }

    if !queued.is_empty() {
        let available = area.width as usize;
        let queued_count = app.queued_message_count();
        let header = format!("Queued ({queued_count}) - /queue edit <n>");
        let header = truncate_line_to_width(&header, available.max(1));
        lines.push(Line::from(vec![Span::styled(
            header,
            Style::default().fg(palette::TEXT_MUTED),
        )]));

        for (idx, message) in queued.iter().enumerate() {
            let label = if message.starts_with('+') {
                message.to_string()
            } else {
                format!("{}. {message}", idx + 1)
            };
            let preview = truncate_line_to_width(&label, available.max(1));
            lines.push(Line::from(vec![Span::styled(
                preview,
                Style::default().fg(palette::TEXT_DIM),
            )]));
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
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
    let width = area.width;
    let available_width = width as usize;

    // 1. Context Progress Bar (Right)
    let context_snapshot = context_usage_snapshot(app);
    let percent = context_snapshot
        .map(|(_, _, pct)| pct as f32)
        .unwrap_or(0.0);
    let bar_width = 10; // Width of the progress bar
    let filled = ((percent / 100.0) * bar_width as f32).round() as usize;
    let filled = filled.min(bar_width);
    let empty = bar_width - filled;

    let bar_color = context_color_for_percent(f64::from(percent));

    let bar_filled = "█".repeat(filled);
    let bar_empty = "░".repeat(empty);
    let context_text = format!("[{}{}] {:.0}%", bar_filled, bar_empty, percent);
    let context_span = Span::styled(context_text, Style::default().fg(bar_color));
    let budget_span = context_snapshot.map(|(used, max, _)| {
        Span::styled(
            format_context_budget(used, max),
            Style::default().fg(bar_color),
        )
    });

    // 2. Right side extras (Scroll, Selection) - Minimalist
    let mut right_extras = Vec::new();

    // Scroll %
    if !matches!(app.transcript_scroll, TranscriptScroll::ToBottom)
        && let Some(scroll_pct) = transcript_scroll_percent(
            app.last_transcript_top,
            app.last_transcript_visible,
            app.last_transcript_total,
        )
    {
        right_extras.push(Span::styled(
            format!(" {scroll_pct}% "),
            Style::default().fg(palette::TEXT_DIM),
        ));
    }

    // Selection
    if app.transcript_selection.is_active() {
        right_extras.push(Span::styled(
            " [SEL] ",
            Style::default().fg(palette::TEXT_DIM),
        ));
    }

    // Assemble Right Side
    // context_span is always last
    let mut right_spans = right_extras;
    right_spans.push(Span::raw("   ")); // Space before context
    if let Some(budget) = budget_span {
        right_spans.push(budget);
        right_spans.push(Span::raw(" "));
    }
    right_spans.push(context_span);

    let right_width: usize = right_spans.iter().map(|s| s.content.width()).sum();

    // 3. Left side content (status toast or standard footer)
    let active_status = app.active_status_toast();
    let left_spans = if let Some(toast) = active_status {
        let max_left = available_width
            .saturating_sub(right_width)
            .saturating_sub(1)
            .max(1);
        let truncated = truncate_line_to_width(&toast.text, max_left);
        vec![Span::styled(
            truncated,
            Style::default().fg(status_color(toast.level)),
        )]
    } else {
        // Compact footer: mode + workspace + session + token cost + help hint
        let mut spans = Vec::new();

        // Mode indicator
        let (mode_label, mode_color) = match app.mode {
            crate::tui::app::AppMode::Normal => ("[Normal]", palette::MODE_NORMAL),
            crate::tui::app::AppMode::Agent => ("[Agent]", palette::MODE_AGENT),
            crate::tui::app::AppMode::Yolo => ("[YOLO]", palette::MODE_YOLO),
            crate::tui::app::AppMode::Plan => ("[Plan]", palette::MODE_PLAN),
        };
        spans.push(Span::styled(
            format!("{} ", mode_label),
            Style::default().fg(mode_color),
        ));

        // Workspace (directory name)
        if let Some(workspace_name) = app.workspace.file_name() {
            if let Some(name) = workspace_name.to_str() {
                let ws = format!("{} ", name);
                spans.push(Span::styled(ws, Style::default().fg(palette::TEXT_DIM)));
            }
        }

        if let Some(workspace_context) = app.workspace_context.as_deref() {
            let context =
                truncate_line_to_width(&format!("ctx: {workspace_context}"), available_width / 2);
            if !context.is_empty() {
                spans.push(Span::styled(
                    format!("{context} "),
                    Style::default().fg(palette::TEXT_DIM),
                ));
            }
        }

        // Session ID
        if let Some(ref sid) = app.current_session_id {
            spans.push(Span::styled(
                format!("session:{}  ", &sid[..8.min(sid.len())]),
                Style::default().fg(palette::TEXT_DIM),
            ));
        }

        // Token cost
        if app.total_conversation_tokens > 0 {
            let tokens_k = app.total_conversation_tokens as f64 / 1000.0;
            spans.push(Span::styled(
                format!("{tokens_k:.1}k tokens  "),
                Style::default().fg(palette::TEXT_DIM),
            ));
        }

        // Help hint
        spans.push(Span::styled(
            "F1 help",
            Style::default().fg(palette::TEXT_DIM),
        ));

        spans
    };

    // Calculate Widths
    let left_width: usize = left_spans.iter().map(|s| s.content.width()).sum();

    // Spacer
    let spacer_width = available_width.saturating_sub(left_width + right_width);

    let mut all_spans = left_spans;
    if spacer_width > 0 {
        all_spans.push(Span::raw(" ".repeat(spacer_width)));
        all_spans.extend(right_spans);
    } else {
        // Fallback for narrow screens
        let simple_left = if let Some(toast) = app.active_status_toast() {
            let max_left = available_width.saturating_sub(10).saturating_sub(1).max(1);
            let truncated = truncate_line_to_width(&toast.text, max_left);
            vec![Span::styled(
                truncated,
                Style::default().fg(status_color(toast.level)),
            )]
        } else {
            vec![Span::styled(
                "F1 help",
                Style::default().fg(palette::TEXT_DIM),
            )]
        };
        let bar_filled_narrow = "█".repeat(filled.min(5));
        let bar_empty_narrow = "░".repeat(5 - filled.min(5));
        let budget_prefix = context_snapshot
            .map(|(used, max, _)| format!("{} ", format_context_budget(used, max)))
            .unwrap_or_default();
        let simple_right = vec![Span::styled(
            format!(
                "{}[{}{}] {:.0}%",
                budget_prefix, bar_filled_narrow, bar_empty_narrow, percent
            ),
            Style::default().fg(bar_color),
        )];

        let sl_width: usize = simple_left.iter().map(|s| s.content.width()).sum();
        let sr_width: usize = simple_right.iter().map(|s| s.content.width()).sum();
        let sp_width = available_width.saturating_sub(sl_width + sr_width);

        all_spans = simple_left;
        all_spans.push(Span::raw(" ".repeat(sp_width)));
        all_spans.extend(simple_right);
    }

    let footer = Paragraph::new(Line::from(all_spans));
    f.render_widget(footer, area);
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

fn context_color_for_percent(percent: f64) -> ratatui::style::Color {
    if percent >= CONTEXT_CRITICAL_THRESHOLD_PERCENT {
        palette::STATUS_ERROR
    } else if percent >= CONTEXT_WARNING_THRESHOLD_PERCENT {
        palette::STATUS_WARNING
    } else {
        palette::DEEPSEEK_SKY
    }
}

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

fn prompt_for_mode(mode: AppMode) -> &'static str {
    match mode {
        AppMode::Normal => "> ",
        AppMode::Agent => "agent> ",
        AppMode::Yolo => "yolo> ",
        AppMode::Plan => "plan> ",
    }
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

    let used = match (reported, estimated) {
        (Some(reported), Some(estimated))
            if reported > max_i64 && estimated > 0 && estimated <= max_i64 =>
        {
            estimated
        }
        (Some(reported), _) => reported,
        (None, Some(estimated)) => estimated,
        (None, None) => return None,
    };

    let max_f64 = f64::from(max);
    let used_f64 = used as f64;
    let percent = ((used_f64 / max_f64) * 100.0).clamp(0.0, 100.0);
    Some((used, max, percent))
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

fn format_elapsed(start: Instant) -> String {
    let elapsed = start.elapsed().as_secs();
    if elapsed >= 60 {
        format!("{}m{:02}s", elapsed / 60, elapsed % 60)
    } else {
        format!("{elapsed}s")
    }
}

fn deepseek_squiggle(start: Option<Instant>) -> &'static str {
    const FRAMES: [&str; 6] = ["◍", "◉", "◌", "◌", "◉", "◍"];
    let elapsed_ms = start.map_or(0, |t| t.elapsed().as_millis());
    let idx = ((elapsed_ms / u128::from(UI_DEEPSEEK_SQUIGGLE_MS)) as usize) % FRAMES.len();
    FRAMES[idx]
}

/// Braille pattern frames for typing/thinking indicator animation.
const TYPING_FRAMES: &[&str] = &["⠁", "⠂", "⠄", "⠂"];

/// Returns the typing indicator frame based on elapsed time.
fn typing_indicator(start: Option<Instant>) -> &'static str {
    let elapsed_ms = start.map_or(0, |t| t.elapsed().as_millis());
    let idx = ((elapsed_ms / u128::from(UI_TYPING_INDICATOR_MS)) as usize) % TYPING_FRAMES.len();
    TYPING_FRAMES[idx]
}

fn truncate_line_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 3 {
        return text.chars().take(max_width).collect();
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
                    && matches!(app.transcript_scroll, TranscriptScroll::ToBottom)
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
        MouseEventKind::Up(MouseButton::Left) => {
            if app.transcript_selection.dragging {
                app.transcript_selection.dragging = false;
                if selection_has_content(app) {
                    copy_active_selection(app);
                }
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
            slice_text(&line_text, start.column, line_text.chars().count())
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

fn open_tool_details_pager(app: &mut App) -> bool {
    let target_cell = if let Some((start, _)) = app.transcript_selection.ordered_endpoints() {
        app.transcript_cache
            .line_meta()
            .get(start.line_index)
            .and_then(|meta| meta.cell_line())
            .map(|(cell_index, _)| cell_index)
    } else {
        app.history
            .len()
            .checked_sub(1)
            .filter(|idx| app.tool_details_by_cell.contains_key(idx))
            .or_else(|| app.tool_details_by_cell.keys().copied().max())
    };

    let Some(cell_index) = target_cell else {
        return false;
    };
    let Some(detail) = app.tool_details_by_cell.get(&cell_index) else {
        app.status_message = Some("No tool details for selected line".to_string());
        return false;
    };

    let input =
        serde_json::to_string_pretty(&detail.input).unwrap_or_else(|_| detail.input.to_string());
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

fn format_subagent_list(agents: &[SubAgentResult]) -> String {
    if agents.is_empty() {
        return "No sub-agents running.".to_string();
    }

    let mut lines = Vec::new();
    lines.push("Sub-agents:".to_string());
    lines.push("----------------------------------------".to_string());

    for agent in agents {
        let status = format_subagent_status(&agent.status);
        let mut line = format!(
            "  {} ({:?}) - {} | steps: {} | {}ms",
            agent.agent_id, agent.agent_type, status, agent.steps_taken, agent.duration_ms
        );
        if matches!(agent.status, SubAgentStatus::Completed)
            && let Some(result) = agent.result.as_ref()
        {
            let _ = write!(line, "\n    Result: {}", summarize_tool_output(result));
        }
        lines.push(line);
    }

    lines.join("\n")
}

fn task_mode_label(mode: AppMode) -> &'static str {
    match mode {
        AppMode::Normal => "normal",
        AppMode::Agent => "agent",
        AppMode::Yolo => "yolo",
        AppMode::Plan => "plan",
    }
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

fn format_subagent_status(status: &SubAgentStatus) -> String {
    match status {
        SubAgentStatus::Running => "running".to_string(),
        SubAgentStatus::Completed => "completed".to_string(),
        SubAgentStatus::Cancelled => "cancelled".to_string(),
        SubAgentStatus::Failed(err) => format!("failed: {}", summarize_tool_output(err)),
    }
}

#[allow(clippy::too_many_lines)]
fn handle_tool_call_started(app: &mut App, id: &str, name: &str, input: &serde_json::Value) {
    let id = id.to_string();
    if is_exploring_tool(name) {
        let label = exploring_label(name, input);
        let cell_index = if let Some(idx) = app.exploring_cell {
            idx
        } else {
            app.add_message(HistoryCell::Tool(ToolCell::Exploring(ExploringCell {
                entries: Vec::new(),
            })));
            let idx = app.history.len().saturating_sub(1);
            app.exploring_cell = Some(idx);
            idx
        };

        if let Some(HistoryCell::Tool(ToolCell::Exploring(cell))) = app.history.get_mut(cell_index)
        {
            let entry_index = cell.insert_entry(ExploringEntry {
                label,
                status: ToolStatus::Running,
            });
            app.mark_history_updated();
            app.exploring_entries
                .insert(id.clone(), (cell_index, entry_index));
        }
        register_tool_cell(app, &id, name, input, cell_index);
        return;
    }

    app.exploring_cell = None;

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

            app.add_message(HistoryCell::Tool(ToolCell::Exec(ExecCell {
                command,
                status: ToolStatus::Running,
                output: None,
                started_at: Some(Instant::now()),
                duration_ms: None,
                source,
                interaction: Some(summary.clone()),
            })));
            let cell_index = app.history.len().saturating_sub(1);
            register_tool_cell(app, &id, name, input, cell_index);
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

        app.add_message(HistoryCell::Tool(ToolCell::Exec(ExecCell {
            command,
            status: ToolStatus::Running,
            output: None,
            started_at: Some(Instant::now()),
            duration_ms: None,
            source,
            interaction: None,
        })));
        let cell_index = app.history.len().saturating_sub(1);
        register_tool_cell(app, &id, name, input, cell_index);
        return;
    }

    if name == "update_plan" {
        let (explanation, steps) = parse_plan_input(input);
        app.add_message(HistoryCell::Tool(ToolCell::PlanUpdate(PlanUpdateCell {
            explanation,
            steps,
            status: ToolStatus::Running,
        })));
        let cell_index = app.history.len().saturating_sub(1);
        register_tool_cell(app, &id, name, input, cell_index);
        return;
    }

    if name == "apply_patch" {
        let (path, summary) = parse_patch_summary(input);
        app.add_message(HistoryCell::Tool(ToolCell::PatchSummary(
            PatchSummaryCell {
                path,
                summary,
                status: ToolStatus::Running,
                error: None,
            },
        )));
        let cell_index = app.history.len().saturating_sub(1);
        register_tool_cell(app, &id, name, input, cell_index);
        return;
    }

    if name == "review" {
        let target = review_target_label(input);
        app.add_message(HistoryCell::Tool(ToolCell::Review(ReviewCell {
            target,
            status: ToolStatus::Running,
            output: None,
            error: None,
        })));
        let cell_index = app.history.len().saturating_sub(1);
        register_tool_cell(app, &id, name, input, cell_index);
        return;
    }

    if is_mcp_tool(name) {
        app.add_message(HistoryCell::Tool(ToolCell::Mcp(McpToolCell {
            tool: name.to_string(),
            status: ToolStatus::Running,
            content: None,
            is_image: false,
        })));
        let cell_index = app.history.len().saturating_sub(1);
        register_tool_cell(app, &id, name, input, cell_index);
        return;
    }

    if is_view_image_tool(name) {
        if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
            let raw_path = PathBuf::from(path);
            let display_path = raw_path
                .strip_prefix(&app.workspace)
                .unwrap_or(&raw_path)
                .to_path_buf();
            app.add_message(HistoryCell::Tool(ToolCell::ViewImage(ViewImageCell {
                path: display_path,
            })));
            let cell_index = app.history.len().saturating_sub(1);
            register_tool_cell(app, &id, name, input, cell_index);
        }
        return;
    }

    if is_web_search_tool(name) {
        let query = web_search_query(input);
        app.add_message(HistoryCell::Tool(ToolCell::WebSearch(WebSearchCell {
            query,
            status: ToolStatus::Running,
            summary: None,
        })));
        let cell_index = app.history.len().saturating_sub(1);
        register_tool_cell(app, &id, name, input, cell_index);
        return;
    }

    let input_summary = summarize_tool_args(input);
    app.add_message(HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: name.to_string(),
        status: ToolStatus::Running,
        input_summary,
        output: None,
    })));
    let cell_index = app.history.len().saturating_sub(1);
    register_tool_cell(app, &id, name, input, cell_index);
}

fn register_tool_cell(
    app: &mut App,
    tool_id: &str,
    tool_name: &str,
    input: &serde_json::Value,
    cell_index: usize,
) {
    app.tool_cells.insert(tool_id.to_string(), cell_index);
    app.tool_details_by_cell.insert(
        cell_index,
        ToolDetailRecord {
            tool_id: tool_id.to_string(),
            tool_name: tool_name.to_string(),
            input: input.clone(),
            output: None,
        },
    );
}

fn store_tool_detail_output(
    app: &mut App,
    cell_index: usize,
    result: &Result<ToolResult, ToolError>,
) {
    if let Some(detail) = app.tool_details_by_cell.get_mut(&cell_index) {
        detail.output = Some(match result {
            Ok(tool_result) => tool_result.content.clone(),
            Err(err) => err.to_string(),
        });
    }
}

#[allow(clippy::too_many_lines)]
fn handle_tool_call_complete(
    app: &mut App,
    id: &str,
    _name: &str,
    result: &Result<ToolResult, ToolError>,
) {
    if app.ignored_tool_calls.remove(id) {
        return;
    }

    if let Some((cell_index, entry_index)) = app.exploring_entries.remove(id) {
        app.tool_cells.remove(id);
        store_tool_detail_output(app, cell_index, result);

        if let Some(HistoryCell::Tool(ToolCell::Exploring(cell))) = app.history.get_mut(cell_index)
            && let Some(entry) = cell.entries.get_mut(entry_index)
        {
            entry.status = match result.as_ref() {
                Ok(tool_result) if tool_result.success => ToolStatus::Success,
                Ok(_) | Err(_) => ToolStatus::Failed,
            };
            app.mark_history_updated();
        }
        return;
    }

    let Some(cell_index) = app.tool_cells.remove(id) else {
        return;
    };

    store_tool_detail_output(app, cell_index, result);

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

    if let Some(cell) = app.history.get_mut(cell_index) {
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
            .map_or(fallback, |path| format!("Read {path}")),
        "list_dir" => obj
            .and_then(|o| o.get("path"))
            .and_then(|v| v.as_str())
            .map_or("List directory".to_string(), |path| format!("List {path}")),
        "grep_files" => {
            let pattern = obj
                .and_then(|o| o.get("pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("pattern");
            format!("Search {pattern}")
        }
        "list_files" => "List files".to_string(),
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
    if let Some(searches) = input.get("search_query").and_then(|v| v.as_array()) {
        if let Some(first) = searches.first() {
            if let Some(q) = first.get("q").and_then(|v| v.as_str()) {
                return q.to_string();
            }
        }
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
            .get(0)
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
            if let Some(path) = parts.get(1).or_else(|| parts.get(0)) {
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
