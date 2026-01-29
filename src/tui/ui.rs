//! TUI event loop and rendering logic for `DeepSeek` CLI.

use std::fmt::Write;
use std::fs;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use chrono::Local;
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
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::commands;
use crate::compaction::CompactionConfig;
use crate::config::Config;
use crate::core::engine::{EngineConfig, EngineHandle, spawn_engine};
use crate::core::events::Event as EngineEvent;
use crate::core::ops::Op;
use crate::hooks::HookEvent;
use crate::models::{ContentBlock, Message, SystemPrompt, context_window_for_model};
use crate::palette;
use crate::prompts;
use crate::rlm;
use crate::session_manager::{
    SavedSession, SessionManager, create_saved_session_with_mode, update_session,
};
use crate::tools::ReviewOutput;
use crate::tools::spec::{ToolError, ToolResult};
use crate::tools::subagent::{SubAgentResult, SubAgentStatus};
use crate::tui::event_broker::EventBroker;
use crate::tui::onboarding;
use crate::tui::pager::PagerView;
use crate::tui::paste_burst::CharDecision;
use crate::tui::scrolling::{ScrollDirection, TranscriptScroll};
use crate::tui::selection::TranscriptSelectionPoint;
use crate::tui::session_picker::SessionPickerView;
use crate::utils::estimate_message_chars;

use super::app::{App, AppAction, AppMode, OnboardingState, QueuedMessage, TuiOptions};
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
use super::widgets::{ChatWidget, ComposerWidget, HeaderData, HeaderWidget, Renderable};

// === Constants ===

const MAX_QUEUED_PREVIEW: usize = 3;
const AUTO_RLM_MIN_FILE_BYTES: u64 = 200_000;
const AUTO_RLM_HINT_FILE_BYTES: u64 = 50_000;
const AUTO_RLM_PASTE_MIN_CHARS: usize = 15_000;
const AUTO_RLM_PASTE_HINT_CHARS: usize = 5_000;
const AUTO_RLM_PASTE_QUERY_MAX_CHARS: usize = 800;
const AUTO_RLM_PASTE_FIRST_LINE_MAX_CHARS: usize = 200;
const RLM_BUDGET_WARN_QUERIES: u32 = 8;
const RLM_BUDGET_WARN_INPUT_TOKENS: u64 = 60_000;
const RLM_BUDGET_WARN_OUTPUT_TOKENS: u64 = 20_000;
const RLM_BUDGET_HARD_QUERIES: u32 = 16;
const RLM_BUDGET_HARD_INPUT_TOKENS: u64 = 120_000;
const RLM_BUDGET_HARD_OUTPUT_TOKENS: u64 = 40_000;
const AUTO_RLM_MAX_SCAN_ENTRIES: usize = 50_000;
const AUTO_RLM_EXCLUDED_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".codex",
    ".aleph",
    "dist",
    "build",
];

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

    // Load existing session if resuming
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
                app.workspace.clone_from(&saved.metadata.workspace);
                app.current_session_id = Some(saved.metadata.id.clone());
                app.total_tokens = u32::try_from(saved.metadata.total_tokens).unwrap_or(u32::MAX);
                app.total_conversation_tokens = app.total_tokens;
                if let Some(prompt) = saved.system_prompt {
                    app.system_prompt = Some(SystemPrompt::Text(prompt));
                }
                // Convert saved messages to HistoryCell format for display
                app.history.clear();
                app.history.push(HistoryCell::System {
                    content: format!(
                        "Resumed session: {} ({})",
                        saved.metadata.title,
                        &saved.metadata.id[..8]
                    ),
                });
                for msg in &saved.messages {
                    app.history.extend(history_cells_from_message(msg));
                }
                app.mark_history_updated();
                app.status_message = Some(format!("Resumed session: {}", &saved.metadata.id[..8]));
            }
            Ok(None) => {
                app.status_message = Some("No sessions found to resume".to_string());
            }
            Err(e) => {
                app.status_message = Some(format!("Failed to load session: {e}"));
            }
        }
    }

    let mut compaction = CompactionConfig::default();
    compaction.enabled = app.auto_compact;
    compaction.token_threshold = app.compact_threshold;
    compaction.model = app.model.clone();

    // Create the Engine with configuration from TuiOptions
    let engine_config = EngineConfig {
        model: app.model.clone(),
        workspace: app.workspace.clone(),
        allow_shell: app.allow_shell,
        trust_mode: options.yolo,
        notes_path: config.notes_path(),
        mcp_config_path: config.mcp_config_path(),
        max_steps: 100,
        max_subagents: app.max_subagents,
        features: config.features(),
        rlm_session: app.rlm_session.clone(),
        duo_session: app.duo_session.clone(),
        compaction,
        todos: app.todos.clone(),
        plan_state: app.plan_state.clone(),
    };

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

    let result = run_event_loop(
        &mut terminal,
        &mut app,
        config,
        engine_handle,
        &event_broker,
    )
    .await;

    // Fire session end hook
    {
        let context = app.base_hook_context();
        let _ = app.execute_hooks(HookEvent::SessionEnd, &context);
    }

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

#[allow(clippy::too_many_lines)]
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    _config: &Config,
    engine_handle: EngineHandle,
    event_broker: &EventBroker,
) -> Result<()> {
    // Track streaming state
    let mut current_streaming_text = String::new();

    loop {
        // First, poll for engine events (non-blocking)
        let mut queued_to_send: Option<QueuedMessage> = None;
        {
            let mut rx = engine_handle.rx_event.write().await;
            while let Ok(event) = rx.try_recv() {
                match event {
                    EngineEvent::MessageStarted { .. } => {
                        current_streaming_text.clear();
                        app.streaming_message_index = None;
                    }
                    EngineEvent::MessageDelta { content, .. } => {
                        current_streaming_text.push_str(&content);
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

                        if let Some(HistoryCell::Assistant { content, .. }) =
                            app.history.get_mut(index)
                        {
                            content.clone_from(&current_streaming_text);
                            app.mark_history_updated();
                        }
                    }
                    EngineEvent::MessageComplete { .. } => {
                        if let Some(index) = app.streaming_message_index.take()
                            && let Some(HistoryCell::Assistant { streaming, .. }) =
                                app.history.get_mut(index)
                        {
                            *streaming = false;
                            app.mark_history_updated();
                        }

                        if !current_streaming_text.is_empty()
                            || app.last_reasoning.is_some()
                            || !app.pending_tool_uses.is_empty()
                        {
                            let mut blocks = Vec::new();
                            if let Some(thinking) = app.last_reasoning.take() {
                                blocks.push(ContentBlock::Thinking { thinking });
                            }
                            if !current_streaming_text.is_empty() {
                                blocks.push(ContentBlock::Text {
                                    text: current_streaming_text.clone(),
                                    cache_control: None,
                                });
                            }
                            for (id, name, input) in app.pending_tool_uses.drain(..) {
                                blocks.push(ContentBlock::ToolUse { id, name, input });
                            }
                            if !blocks.is_empty() {
                                app.api_messages.push(Message {
                                    role: "assistant".to_string(),
                                    content: blocks,
                                });
                            }
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
                        app.reasoning_buffer.push_str(&content);
                        if app.reasoning_header.is_none() {
                            app.reasoning_header = extract_reasoning_header(&app.reasoning_buffer);
                        }

                        if let Some(index) = app.streaming_message_index {
                            if let Some(HistoryCell::Thinking { content: c, .. }) =
                                app.history.get_mut(index)
                            {
                                c.push_str(&content);
                            }
                        }
                    }
                    EngineEvent::ThinkingComplete { .. } => {
                        if let Some(index) = app.streaming_message_index.take() {
                            if let Some(HistoryCell::Thinking { streaming, .. }) =
                                app.history.get_mut(index)
                            {
                                *streaming = false;
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
                            Ok(output) => output.content.clone(),
                            Err(err) => format!("Error: {err}"),
                        };
                        app.api_messages.push(Message {
                            role: "user".to_string(),
                            content: vec![ContentBlock::ToolResult {
                                tool_use_id: id.clone(),
                                content: tool_content,
                            }],
                        });
                        handle_tool_call_complete(app, &id, &name, &result);
                    }
                    EngineEvent::TurnStarted => {
                        app.is_loading = true;
                        current_streaming_text.clear();
                        app.turn_started_at = Some(Instant::now());
                        app.reasoning_buffer.clear();
                        app.reasoning_header = None;
                        app.last_reasoning = None;
                        app.pending_tool_uses.clear();
                        app.plan_tool_used_in_turn = false;
                    }
                    EngineEvent::TurnComplete { usage } => {
                        app.is_loading = false;
                        app.turn_started_at = None;
                        let turn_tokens = usage.input_tokens + usage.output_tokens;
                        app.total_tokens = app.total_tokens.saturating_add(turn_tokens);
                        app.total_conversation_tokens =
                            app.total_conversation_tokens.saturating_add(turn_tokens);
                        app.last_prompt_tokens = Some(usage.input_tokens);
                        app.last_completion_tokens = Some(usage.output_tokens);

                        // Auto-save session after each turn
                        if let Ok(manager) = SessionManager::default_location() {
                            let session = if let Some(ref existing_id) = app.current_session_id {
                                // Update existing session
                                if let Ok(existing) = manager.load_session(existing_id) {
                                    let mut updated = update_session(
                                        existing,
                                        &app.api_messages,
                                        u64::from(app.total_tokens),
                                        app.system_prompt.as_ref(),
                                    );
                                    updated.metadata.mode = Some(app.mode.label().to_string());
                                    updated
                                } else {
                                    // Session was deleted, create new
                                    create_saved_session_with_mode(
                                        &app.api_messages,
                                        &app.model,
                                        &app.workspace,
                                        u64::from(app.total_tokens),
                                        app.system_prompt.as_ref(),
                                        Some(app.mode.label()),
                                    )
                                }
                            } else {
                                // Create new session
                                create_saved_session_with_mode(
                                    &app.api_messages,
                                    &app.model,
                                    &app.workspace,
                                    u64::from(app.total_tokens),
                                    app.system_prompt.as_ref(),
                                    Some(app.mode.label()),
                                )
                            };

                            if let Err(e) = manager.save_session(&session) {
                                eprintln!("Failed to save session: {e}");
                            } else {
                                app.current_session_id = Some(session.metadata.id.clone());
                            }
                        }

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
                        app.is_loading = false;
                    }
                    EngineEvent::Status { message } => {
                        app.status_message = Some(message);
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
                            let _ = engine_handle.approve_tool_call(id.clone()).await;
                        } else if app.approval_mode == ApprovalMode::Never {
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
                            app.view_stack.push(ApprovalView::new(request));
                            app.add_message(HistoryCell::System {
                                content: format!(
                                    "Approval required for tool '{tool_name}': {description}"
                                ),
                            });
                        }
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
                            app.add_message(HistoryCell::System {
                                content: format!(
                                    "Sandbox denied {tool_name}: {denial_reason} - auto-elevating to full access"
                                ),
                            });
                            // Auto-elevate to full access (no sandbox)
                            let policy = crate::sandbox::SandboxPolicy::DangerFullAccess;
                            let _ = engine_handle.retry_tool_with_policy(tool_id, policy).await;
                        } else {
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

        if let Some(next) = queued_to_send {
            dispatch_user_message(app, &engine_handle, next).await?;
        }

        if !app.view_stack.is_empty() {
            let events = app.view_stack.tick();
            handle_view_events(app, &engine_handle, events).await;
        }

        if event_broker.is_paused() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            continue;
        }

        app.flush_paste_burst_if_due(Instant::now());

        terminal.draw(|f| render(f, app))?; // app is &mut

        if event::poll(std::time::Duration::from_millis(50))? {
            let evt = event::read()?;

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
                        OnboardingState::ApiKey => match app.submit_api_key() {
                            Ok(_) => {
                                advance_onboarding(app);
                            }
                            Err(e) => {
                                app.status_message = Some(e.to_string());
                            }
                        },
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
                    app.view_stack.push(HelpView::new());
                }
                continue;
            }

            if key.code == KeyCode::Char('/') && key.modifiers.contains(KeyModifiers::CONTROL) {
                if app.view_stack.top_kind() == Some(ModalKind::Help) {
                    app.view_stack.pop();
                } else {
                    app.view_stack.push(HelpView::new());
                }
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
                    if app.is_loading {
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
                KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                    app.scroll_down(3);
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
                    app.cycle_mode();
                    if app.mode == AppMode::Rlm {
                        app.rlm_repl_active = false;
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
                                        let _ = engine_handle
                                            .send(Op::SyncSession {
                                                messages,
                                                system_prompt,
                                                model,
                                                workspace,
                                            })
                                            .await;
                                    }
                                    AppAction::SendMessage(content) => {
                                        let queued = build_queued_message(app, content);
                                        dispatch_user_message(app, &engine_handle, queued).await?;
                                    }
                                    AppAction::ListSubAgents => {
                                        let _ = engine_handle.send(Op::ListSubAgents).await;
                                    }
                                    AppAction::UpdateCompaction(compaction) => {
                                        let _ = engine_handle
                                            .send(Op::SetCompaction { config: compaction })
                                            .await;
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

                            if app.mode == AppMode::Rlm && app.rlm_repl_active {
                                if rlm_repl_should_route_to_chat(app, &input) {
                                    app.rlm_repl_active = false;
                                    app.add_message(HistoryCell::System {
                                        content: "RLM REPL paused (no context loaded). Routing to chat so the model can call rlm_load. Use /repl to return.".to_string(),
                                    });
                                } else {
                                    handle_rlm_input(app, input);
                                    continue;
                                }
                            }

                            let queued = if let Some(mut draft) = app.queued_draft.take() {
                                draft.display = input;
                                draft
                            } else {
                                build_queued_message(app, input)
                            };
                            if app.is_loading {
                                app.queue_message(queued);
                                app.status_message = Some(format!(
                                    "Queued {} message(s) - /queue to view/edit",
                                    app.queued_message_count()
                                ));
                            } else {
                                dispatch_user_message(app, &engine_handle, queued).await?;
                            }
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

    let override_query = maybe_auto_switch_to_rlm(app, &message.display);
    let content = if let Some(query) = override_query.as_deref() {
        message.content_with_query(query)
    } else {
        message.content()
    };
    let rlm_summary = if app.mode == AppMode::Rlm {
        app.rlm_session
            .lock()
            .ok()
            .map(|session| rlm::session_summary(&session))
    } else {
        None
    };
    let duo_summary = if app.mode == AppMode::Duo {
        app.duo_session
            .lock()
            .ok()
            .map(|s| crate::duo::session_summary(&s))
    } else {
        None
    };
    app.system_prompt = Some(prompts::system_prompt_for_mode_with_context(
        app.mode,
        &app.workspace,
        None,
        rlm_summary.as_deref(),
        duo_summary.as_deref(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanChoice {
    ImplementAgent,
    ImplementYolo,
    RevisePlan,
    ExitPlan,
}

fn plan_next_step_prompt() -> String {
    [
        "Plan ready. Choose next step:",
        "  1) Implement in Agent mode (approvals on)",
        "  2) Implement in YOLO mode (auto-approve)",
        "  3) Revise the plan / ask follow-ups",
        "  4) Exit Plan mode",
        "",
        "Type 1-4 and press Enter.",
    ]
    .join("\n")
}

fn parse_plan_choice(input: &str) -> Option<PlanChoice> {
    let trimmed = input.trim().to_lowercase();
    let first = trimmed.chars().next()?;
    match first {
        '1' => return Some(PlanChoice::ImplementAgent),
        '2' => return Some(PlanChoice::ImplementYolo),
        '3' => return Some(PlanChoice::RevisePlan),
        '4' => return Some(PlanChoice::ExitPlan),
        _ => {}
    }

    match trimmed.as_str() {
        "agent" | "a" => Some(PlanChoice::ImplementAgent),
        "yolo" | "y" => Some(PlanChoice::ImplementYolo),
        "revise" | "edit" | "plan" | "stay" => Some(PlanChoice::RevisePlan),
        "normal" | "exit" | "cancel" | "back" => Some(PlanChoice::ExitPlan),
        _ => None,
    }
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

    match choice {
        PlanChoice::ImplementAgent => {
            app.set_mode(AppMode::Agent);
            app.add_message(HistoryCell::System {
                content: "Plan approved. Switching to Agent mode and starting implementation."
                    .to_string(),
            });
            let followup = QueuedMessage::new("Proceed with the plan.".to_string(), None);
            if app.is_loading {
                app.queue_message(followup);
                app.status_message = Some("Queued plan execution (agent mode).".to_string());
            } else {
                dispatch_user_message(app, engine_handle, followup).await?;
            }
        }
        PlanChoice::ImplementYolo => {
            app.set_mode(AppMode::Yolo);
            app.add_message(HistoryCell::System {
                content: "Plan approved. Switching to YOLO mode and starting implementation."
                    .to_string(),
            });
            let followup = QueuedMessage::new("Proceed with the plan.".to_string(), None);
            if app.is_loading {
                app.queue_message(followup);
                app.status_message = Some("Queued plan execution (YOLO mode).".to_string());
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

    Ok(true)
}

fn handle_rlm_input(app: &mut App, input: String) {
    if let Some(path) = input.trim().strip_prefix('@') {
        let command = format!("/load @{path}");
        let result = commands::execute(&command, app);
        if let Some(msg) = result.message {
            app.add_message(HistoryCell::System { content: msg });
        }
        return;
    }

    app.add_message(HistoryCell::User {
        content: input.clone(),
    });

    let content = match app.rlm_session.lock() {
        Ok(mut session) => match rlm::eval_in_session(&mut session, &input) {
            Ok(result) => {
                let trimmed = result.trim();
                if trimmed.is_empty() {
                    "RLM: (no output)".to_string()
                } else {
                    format!("RLM:\n{result}")
                }
            }
            Err(err) => format!("RLM error: {err}"),
        },
        Err(_) => "RLM error: failed to access session".to_string(),
    };

    app.add_message(HistoryCell::System { content });
}

struct AutoRlmDecision {
    source: AutoRlmSource,
    reason: String,
}

enum AutoRlmSource {
    File(PathBuf),
    Paste {
        content: String,
        query: Option<String>,
    },
    None,
}

struct AutoRlmLoaded {
    context_id: String,
    line_count: usize,
    char_count: usize,
}

fn maybe_auto_switch_to_rlm(app: &mut App, input: &str) -> Option<String> {
    if !app.auto_rlm {
        return None;
    }
    let already_rlm = app.mode == AppMode::Rlm;
    let decision = auto_rlm_decision(app, input, already_rlm)?;

    if !already_rlm {
        app.set_mode(AppMode::Rlm);
        app.rlm_repl_active = false;
    }

    let mut messages = vec![if already_rlm {
        format!("Auto-loaded RLM context ({})", decision.reason)
    } else {
        format!("Auto-switched to RLM mode ({})", decision.reason)
    }];
    let mut override_query = None;

    match decision.source {
        AutoRlmSource::File(path) => match load_file_into_rlm(app, &path) {
            Ok(loaded) => {
                messages.push(format!(
                    "Loaded {} as '{}' ({} lines, {} chars)",
                    format_load_path(app, &path),
                    loaded.context_id,
                    loaded.line_count,
                    loaded.char_count
                ));
                override_query = Some(format!(
                    "{}\n\nUse RLM context '{}' loaded from {}.",
                    input.trim(),
                    loaded.context_id,
                    format_load_path(app, &path)
                ));
            }
            Err(err) => {
                messages.push(format!("RLM auto-load failed: {err}"));
            }
        },
        AutoRlmSource::Paste { content, query } => match load_paste_into_rlm(app, content) {
            Ok(loaded) => {
                messages.push(format!(
                    "Loaded pasted content as '{}' ({} lines, {} chars)",
                    loaded.context_id, loaded.line_count, loaded.char_count
                ));
                let base_query = query.unwrap_or_else(|| {
                    "Analyze the pasted content and answer the user request.".to_string()
                });
                override_query = Some(format!(
                    "{base_query}\n\nRLM context: '{}'.",
                    loaded.context_id
                ));
            }
            Err(err) => {
                messages.push(format!("RLM auto-load failed: {err}"));
                override_query = Some(
                    "The user pasted a large block, but auto-loading failed. Ask them to retry /load or paste again."
                        .to_string(),
                );
            }
        },
        AutoRlmSource::None => {}
    }

    app.add_message(HistoryCell::System {
        content: messages.join("\n"),
    });

    override_query
}

fn auto_rlm_decision(app: &App, input: &str, already_rlm: bool) -> Option<AutoRlmDecision> {
    let input_lower = input.to_lowercase();
    let wants_largest_file = input_lower.contains("largest file")
        || input_lower.contains("biggest file")
        || input_lower.contains("largest files");
    let explicit_rlm_request = input_lower
        .split_whitespace()
        .any(|word| word.trim_matches(|c: char| !c.is_ascii_alphanumeric()) == "rlm")
        || input_lower.contains("rlm mode");
    let explicit_rlm = already_rlm || explicit_rlm_request;
    let has_hint = input_lower.contains("chunk")
        || input_lower.contains("chunking")
        || input_lower.contains("huge")
        || input_lower.contains("massive")
        || input_lower.contains("entire repo")
        || input_lower.contains("whole repo")
        || input_lower.contains("full repo")
        || input_lower.contains("whole project")
        || input_lower.contains("entire project")
        || input_lower.contains("full project")
        || explicit_rlm;

    if let Some(decision) = auto_rlm_paste_decision(input, explicit_rlm, has_hint) {
        return Some(decision);
    }

    if wants_largest_file && let Some((path, size)) = find_largest_file(&app.workspace) {
        return Some(AutoRlmDecision {
            source: AutoRlmSource::File(path),
            reason: format!("requested largest file ({} bytes)", size),
        });
    }

    let Some(candidate) = detect_requested_file(input, &app.workspace) else {
        if explicit_rlm_request && !already_rlm {
            return Some(AutoRlmDecision {
                source: AutoRlmSource::None,
                reason: "explicit RLM request".to_string(),
            });
        }
        return None;
    };
    if !app.trust_mode {
        let workspace_root = app
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| app.workspace.clone());
        let candidate_canonical = candidate
            .canonicalize()
            .unwrap_or_else(|_| candidate.clone());
        if !candidate_canonical.starts_with(&workspace_root) {
            return None;
        }
    }
    let metadata = fs::metadata(&candidate).ok()?;
    if !metadata.is_file() {
        return None;
    }

    let size = metadata.len();
    let min_bytes = if has_hint {
        AUTO_RLM_HINT_FILE_BYTES
    } else {
        AUTO_RLM_MIN_FILE_BYTES
    };
    if size < min_bytes && !explicit_rlm {
        return None;
    }

    let reason = if explicit_rlm_request && !already_rlm {
        format!("explicit RLM file request ({} bytes)", size)
    } else if already_rlm {
        format!("RLM file request ({} bytes)", size)
    } else {
        format!("large file ({} bytes)", size)
    };

    Some(AutoRlmDecision {
        source: AutoRlmSource::File(candidate),
        reason,
    })
}

fn auto_rlm_paste_decision(
    input: &str,
    explicit_rlm: bool,
    has_hint: bool,
) -> Option<AutoRlmDecision> {
    let min_chars = if explicit_rlm || has_hint {
        AUTO_RLM_PASTE_HINT_CHARS
    } else {
        AUTO_RLM_PASTE_MIN_CHARS
    };

    if input.len() < min_chars {
        return None;
    }

    let (query, content) = split_paste_input(input);
    if content.len() < min_chars {
        return None;
    }

    Some(AutoRlmDecision {
        source: AutoRlmSource::Paste { content, query },
        reason: format!("pasted content ({} chars)", input.len()),
    })
}

fn split_paste_input(input: &str) -> (Option<String>, String) {
    let trimmed = input.trim();

    if let Some(idx) = trimmed.find("```").or_else(|| trimmed.find("~~~")) {
        let (prefix, rest) = trimmed.split_at(idx);
        let query = clean_query_prefix(prefix);
        if !query.is_empty() && query.len() <= AUTO_RLM_PASTE_QUERY_MAX_CHARS {
            return (Some(query.to_string()), rest.trim_start().to_string());
        }
    }

    if let Some(idx) = trimmed.find("\n\n") {
        let (prefix, rest) = trimmed.split_at(idx);
        let query = clean_query_prefix(prefix);
        if !query.is_empty() && query.len() <= AUTO_RLM_PASTE_QUERY_MAX_CHARS {
            return (Some(query.to_string()), rest.trim_start().to_string());
        }
    }

    if let Some((first, rest)) = trimmed.split_once('\n') {
        let query = clean_query_prefix(first);
        if !query.is_empty() && query.len() <= AUTO_RLM_PASTE_FIRST_LINE_MAX_CHARS {
            return (Some(query.to_string()), rest.trim_start().to_string());
        }
    }

    (None, trimmed.to_string())
}

fn clean_query_prefix(prefix: &str) -> &str {
    prefix.trim().trim_end_matches(':')
}

fn load_file_into_rlm(app: &mut App, path: &Path) -> Result<AutoRlmLoaded, String> {
    let mut session = app
        .rlm_session
        .lock()
        .map_err(|_| "Failed to access RLM session".to_string())?;
    let base_id = rlm::context_id_from_path(path);
    let context_id = rlm::unique_context_id(&session, &base_id);
    let (line_count, char_count) = session
        .load_file(&context_id, path)
        .map_err(|err| err.to_string())?;
    Ok(AutoRlmLoaded {
        context_id,
        line_count,
        char_count,
    })
}

fn load_paste_into_rlm(app: &mut App, content: String) -> Result<AutoRlmLoaded, String> {
    let mut session = app
        .rlm_session
        .lock()
        .map_err(|_| "Failed to access RLM session".to_string())?;
    let context_id = rlm::unique_context_id(&session, "paste");
    let line_count = content.lines().count();
    let char_count = content.len();
    session.load_context(&context_id, content, Some("pasted input".to_string()));
    Ok(AutoRlmLoaded {
        context_id,
        line_count,
        char_count,
    })
}

fn detect_requested_file(input: &str, workspace: &Path) -> Option<PathBuf> {
    if input.to_lowercase().contains("readme") {
        let readme = ["README.md", "README", "README.txt"];
        for name in readme {
            let candidate = workspace.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    for token in input.split_whitespace() {
        let token = trim_token(token);
        if token.is_empty() || token.contains("://") {
            continue;
        }
        if !looks_like_path_token(token) {
            continue;
        }
        if let Some(path) = resolve_candidate_path(token, workspace) {
            return Some(path);
        }
    }

    None
}

fn trim_token(token: &str) -> &str {
    token
        .trim_start_matches(['(', '[', '{', '"', '\'', '`'])
        .trim_end_matches([')', ']', '}', ',', ';', ':', '"', '\'', '`', '.'])
}

fn looks_like_path_token(token: &str) -> bool {
    let lower = token.to_lowercase();
    if lower == "readme" || lower == "readme.md" {
        return true;
    }
    if token.starts_with('@') || token.contains('/') || token.contains('\\') {
        return true;
    }
    matches!(
        token.rsplit('.').next(),
        Some(
            "md" | "txt"
                | "rs"
                | "toml"
                | "json"
                | "yaml"
                | "yml"
                | "py"
                | "js"
                | "ts"
                | "tsx"
                | "jsx"
                | "go"
                | "java"
                | "c"
                | "h"
                | "cpp"
                | "log"
        )
    )
}

fn resolve_candidate_path(token: &str, workspace: &Path) -> Option<PathBuf> {
    let candidate = if let Some(stripped) = token.strip_prefix('@') {
        workspace.join(stripped.trim_start_matches(['/', '\\']))
    } else if Path::new(token).is_absolute() {
        PathBuf::from(token)
    } else {
        workspace.join(token)
    };

    if candidate.is_file() {
        return Some(candidate);
    }
    None
}

fn find_largest_file(workspace: &Path) -> Option<(PathBuf, u64)> {
    let mut stack = vec![workspace.to_path_buf()];
    let mut scanned = 0;
    let mut largest: Option<(PathBuf, u64)> = None;

    while let Some(dir) = stack.pop() {
        if scanned >= AUTO_RLM_MAX_SCAN_ENTRIES {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            scanned += 1;
            if scanned >= AUTO_RLM_MAX_SCAN_ENTRIES {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|s| s.to_str())
                    && AUTO_RLM_EXCLUDED_DIRS.contains(&name)
                {
                    continue;
                }
                stack.push(path);
            } else if path.is_file() {
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                let size = metadata.len();
                match largest {
                    Some((_, current)) if size <= current => {}
                    _ => largest = Some((path, size)),
                }
            }
        }
    }

    largest
}

fn format_load_path(app: &App, path: &Path) -> String {
    if let Ok(stripped) = path.strip_prefix(&app.workspace) {
        return format!("@{}", stripped.display());
    }
    path.display().to_string()
}

fn looks_like_rlm_expr(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with('/') {
        return true;
    }

    let token = trimmed
        .split(|c: char| c == '(' || c.is_whitespace())
        .next()
        .unwrap_or("");
    matches!(
        token,
        "len"
            | "line_count"
            | "lines"
            | "search"
            | "chunk"
            | "chunk_sections"
            | "chunk_lines"
            | "chunk_auto"
            | "vars"
            | "get"
            | "set"
            | "append"
            | "del"
            | "head"
            | "tail"
            | "peek"
    )
}

fn rlm_repl_should_route_to_chat(app: &App, input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() || looks_like_rlm_expr(trimmed) {
        return false;
    }

    let Ok(session) = app.rlm_session.lock() else {
        return false;
    };
    session.contexts.is_empty()
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
    let prompt = prompt_for_mode(app.mode, app.rlm_repl_active);
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
        let header_data = HeaderData::new(
            app.mode,
            &app.model,
            app.total_conversation_tokens,
            app.is_loading,
            app.ui_theme.header_bg,
        );
        let header_widget = HeaderWidget::new(header_data);
        let buf = f.buffer_mut();
        header_widget.render(chunks[0], buf);
    }

    // Render chat
    {
        let chat_widget = ChatWidget::new(app, chunks[1]);
        let buf = f.buffer_mut();
        chat_widget.render(chunks[1], buf);
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

async fn handle_view_events(app: &mut App, engine_handle: &EngineHandle, events: Vec<ViewEvent>) {
    for event in events {
        match event {
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
                        app.status_message =
                            Some(format!("Session loaded (ID: {})", &session_id[..8]));
                    }
                    Err(err) => {
                        app.status_message =
                            Some(format!("Failed to load session {session_id}: {err}"));
                    }
                }
            }
            ViewEvent::SessionDeleted { session_id, title } => {
                app.status_message =
                    Some(format!("Deleted session {} ({})", &session_id[..8], title));
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
    for msg in &app.api_messages {
        app.history.extend(history_cells_from_message(msg));
    }
    app.mark_history_updated();
    app.transcript_selection.clear();
    app.model.clone_from(&session.metadata.model);
    app.workspace.clone_from(&session.metadata.workspace);
    app.total_tokens = u32::try_from(session.metadata.total_tokens).unwrap_or(u32::MAX);
    app.total_conversation_tokens = app.total_tokens;
    app.current_session_id = Some(session.metadata.id.clone());
    if let Some(sp) = session.system_prompt.as_ref() {
        app.system_prompt = Some(SystemPrompt::Text(sp.clone()));
    } else {
        app.system_prompt = None;
    }
    app.scroll_to_bottom();
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
        // Use typing indicator when streaming content, otherwise use whale spinner
        let has_streaming_content = app.streaming_message_index.is_some();
        let spinner = if has_streaming_content {
            typing_indicator(app.turn_started_at)
        } else {
            deepseek_squiggle(app.turn_started_at)
        };
        let label = if app.show_thinking {
            deepseek_thinking_label(app.turn_started_at)
        } else {
            "Working"
        };
        let mut spans = vec![
            Span::styled(spinner, Style::default().fg(palette::DEEPSEEK_SKY).bold()),
            Span::raw(" "),
            Span::styled(label, Style::default().fg(palette::STATUS_WARNING).bold()),
        ];
        if let Some(header) = header {
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

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let width = area.width;
    let available_width = width as usize;

    // Status message override (Toast)
    if let Some(ref msg) = app.status_message {
        let status_span = Span::styled(msg, Style::default().fg(palette::DEEPSEEK_SKY));
        f.render_widget(Paragraph::new(Line::from(vec![status_span])), area);
        return;
    }

    // 1. Time (Left)
    let time_str = Local::now().format("%H:%M").to_string();
    let time_span = Span::styled(
        format!("{}  ", time_str),
        Style::default().fg(palette::TEXT_DIM),
    );

    // 2. Mode (Left) - Lowercase, colored
    let mode_str = app.mode.label().to_lowercase();
    let mode_style = mode_badge_style(app.mode);
    let mode_span = Span::styled(format!("{}  ", mode_str), mode_style);

    // 3. Agent Info (Left)
    let model = &app.model;
    let status_suffix = if app.is_loading { ", thinking" } else { "" };
    let agent_text = format!("agent ({}{})", model, status_suffix);
    let agent_span = Span::styled(agent_text, Style::default().fg(palette::TEXT_DIM));

    // Left side assembly
    let left_spans = vec![time_span, mode_span, agent_span];

    // 4. Context (Right)
    let percent = get_context_percent_decimal(app);
    let context_text = format!("context: {:.1}%", percent);
    let context_style = if percent > 90.0 {
        Style::default().fg(palette::STATUS_ERROR)
    } else if percent > 75.0 {
        Style::default().fg(palette::STATUS_WARNING)
    } else {
        Style::default().fg(palette::TEXT_DIM)
    };
    let context_span = Span::styled(context_text, context_style);

    // 5. Right side extras (Scroll, Selection, RLM) - Minimalist
    let mut right_extras = Vec::new();

    // Scroll %
    let can_scroll = app.last_transcript_total > app.last_transcript_visible;
    if can_scroll && !matches!(app.transcript_scroll, TranscriptScroll::ToBottom) {
        right_extras.push(Span::styled(
            format!(" {}% ", app.last_transcript_top + 1),
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

    // RLM Badge
    if app.mode == AppMode::Rlm {
        if let Some((badge, style)) = rlm_usage_badge(app) {
            right_extras.push(Span::styled(" ", Style::default()));
            right_extras.push(Span::styled(badge, style));
        }
    }

    // Assemble Right Side
    // context_span is always last
    let mut right_spans = right_extras;
    right_spans.push(Span::raw("   ")); // Space before context
    right_spans.push(context_span);

    // Calculate Widths
    let left_width: usize = left_spans.iter().map(|s| s.content.width()).sum();
    let right_width: usize = right_spans.iter().map(|s| s.content.width()).sum();

    // Spacer
    let spacer_width = available_width.saturating_sub(left_width + right_width);

    let mut all_spans = left_spans;
    if spacer_width > 0 {
        all_spans.push(Span::raw(" ".repeat(spacer_width)));
        all_spans.extend(right_spans);
    } else {
        // Fallback for narrow screens: Drop agent info
        let simple_left = vec![
            Span::styled(
                format!("{}  ", time_str),
                Style::default().fg(palette::TEXT_DIM),
            ),
            Span::styled(format!("{} ", mode_str), mode_style),
        ];
        let simple_right = vec![Span::styled(
            format!(" {:.0}%", percent),
            Style::default().fg(palette::TEXT_DIM),
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

fn get_context_percent_decimal(app: &App) -> f32 {
    let used = if app.total_conversation_tokens > 0 {
        Some(i64::from(app.total_conversation_tokens))
    } else {
        estimated_context_tokens(app)
    };

    if let Some(max) = context_window_for_model(&app.model) {
        if let Some(used) = used {
            let max_f64 = max as f64;
            let used_f64 = used as f64;
            let percent = (used_f64 / max_f64) * 100.0;
            percent.clamp(0.0, 100.0) as f32
        } else {
            0.0
        }
    } else {
        0.0
    }
}

fn rlm_usage_badge(app: &App) -> Option<(String, Style)> {
    let session = app.rlm_session.lock().ok()?;
    let usage = &session.usage;
    if usage.queries == 0 {
        return None;
    }

    let warn = usage.queries >= RLM_BUDGET_WARN_QUERIES
        || usage.input_tokens >= RLM_BUDGET_WARN_INPUT_TOKENS
        || usage.output_tokens >= RLM_BUDGET_WARN_OUTPUT_TOKENS;
    let hard = usage.queries >= RLM_BUDGET_HARD_QUERIES
        || usage.input_tokens >= RLM_BUDGET_HARD_INPUT_TOKENS
        || usage.output_tokens >= RLM_BUDGET_HARD_OUTPUT_TOKENS;

    let style = if hard {
        Style::default()
            .fg(palette::STATUS_ERROR)
            .add_modifier(Modifier::BOLD)
    } else if warn {
        Style::default().fg(palette::STATUS_WARNING)
    } else {
        Style::default().fg(palette::TEXT_MUTED)
    };

    Some((
        format!(
            "RLM q:{} in/out:{} /{}",
            usage.queries, usage.input_tokens, usage.output_tokens
        ),
        style,
    ))
}

fn mode_color(mode: AppMode) -> Color {
    match mode {
        AppMode::Normal => palette::MODE_NORMAL,
        AppMode::Agent => palette::MODE_AGENT,
        AppMode::Yolo => palette::MODE_YOLO,
        AppMode::Plan => palette::MODE_PLAN,
        AppMode::Rlm => palette::MODE_RLM,
        AppMode::Duo => palette::MODE_DUO,
    }
}

fn mode_badge_style(mode: AppMode) -> Style {
    Style::default()
        .fg(palette::TEXT_PRIMARY)
        .bg(mode_color(mode))
        .add_modifier(Modifier::BOLD)
}

fn prompt_for_mode(mode: AppMode, rlm_repl_active: bool) -> &'static str {
    match mode {
        AppMode::Normal => "> ",
        AppMode::Agent => "agent> ",
        AppMode::Yolo => "yolo> ",
        AppMode::Plan => "plan> ",
        AppMode::Rlm => {
            if rlm_repl_active {
                "rlm(repl)> "
            } else {
                "rlm> "
            }
        }
        AppMode::Duo => "duo> ",
    }
}

fn estimated_context_tokens(app: &App) -> Option<i64> {
    let mut total_chars = estimate_message_chars(&app.api_messages);

    match &app.system_prompt {
        Some(SystemPrompt::Text(text)) => total_chars = total_chars.saturating_add(text.len()),
        Some(SystemPrompt::Blocks(blocks)) => {
            for block in blocks {
                total_chars = total_chars.saturating_add(block.text.len());
            }
        }
        None => {}
    }

    let estimated_tokens = total_chars / 4;
    i64::try_from(estimated_tokens).ok()
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
    const FRAMES: [&str; 8] = ["🐳", "🐳·", "🐳··", "🐳···", "🐳··", "🐳·", "🐳", "🐳~"];
    let elapsed_ms = start.map_or(0, |t| t.elapsed().as_millis());
    let idx = ((elapsed_ms / 220) as usize) % FRAMES.len();
    FRAMES[idx]
}

/// Braille pattern frames for typing/thinking indicator animation.
const TYPING_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Returns the typing indicator frame based on elapsed time.
fn typing_indicator(start: Option<Instant>) -> &'static str {
    let elapsed_ms = start.map_or(0, |t| t.elapsed().as_millis());
    let idx = ((elapsed_ms / 80) as usize) % TYPING_FRAMES.len();
    TYPING_FRAMES[idx]
}

fn deepseek_thinking_label(start: Option<Instant>) -> &'static str {
    const TAGLINES: [&str; 5] = [
        "Thinking",
        "Plotting",
        "Drafting",
        "You're absolutely right! ... maybe.",
        "Working",
    ];
    const INITIAL_MS: u128 = 2400;
    let elapsed_ms = start.map_or(0, |t| t.elapsed().as_millis());
    if elapsed_ms < INITIAL_MS {
        return "Working";
    }
    let idx = (((elapsed_ms - INITIAL_MS) / 2400) as usize) % TAGLINES.len();
    TAGLINES[idx]
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
            if is_inside_scrollbar(app, mouse) {
                jump_scrollbar(app, mouse);
                return;
            }

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
            if is_inside_scrollbar(app, mouse) {
                jump_scrollbar(app, mouse);
                return;
            }

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

fn is_inside_scrollbar(app: &App, mouse: MouseEvent) -> bool {
    let Some(area) = app.last_scrollbar_area else {
        return false;
    };
    mouse.column >= area.x
        && mouse.column < area.x + area.width
        && mouse.row >= area.y
        && mouse.row < area.y + area.height
}

fn jump_scrollbar(app: &mut App, mouse: MouseEvent) {
    let Some(area) = app.last_scrollbar_area else {
        return;
    };
    if app.last_transcript_total <= app.last_transcript_visible {
        return;
    }

    let rel = usize::from(mouse.row.saturating_sub(area.y));
    let height = usize::from(area.height.max(1));
    let max_start = app
        .last_transcript_total
        .saturating_sub(app.last_transcript_visible)
        .max(1);
    let target = (rel.saturating_mul(max_start) + height / 2) / height;
    if let Some(anchor) = TranscriptScroll::anchor_for(app.transcript_cache.line_meta(), target) {
        app.transcript_scroll = anchor;
    }
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

fn history_cell_to_text(cell: &HistoryCell, width: u16) -> String {
    cell.lines(width)
        .into_iter()
        .map(line_to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_to_string(line: Line<'static>) -> String {
    line.spans
        .into_iter()
        .map(|span| span.content.to_string())
        .collect::<String>()
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

fn line_to_plain(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn slice_text(text: &str, start: usize, end: usize) -> String {
    let mut out = String::new();
    let mut idx = 0usize;
    for ch in text.chars() {
        if idx >= start && idx < end {
            out.push(ch);
        }
        idx += 1;
        if idx >= end {
            break;
        }
    }
    out
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
        app.tool_cells.insert(id, cell_index);
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
            app.tool_cells
                .insert(id, app.history.len().saturating_sub(1));
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
        app.tool_cells
            .insert(id, app.history.len().saturating_sub(1));
        return;
    }

    if name == "update_plan" {
        let (explanation, steps) = parse_plan_input(input);
        app.add_message(HistoryCell::Tool(ToolCell::PlanUpdate(PlanUpdateCell {
            explanation,
            steps,
            status: ToolStatus::Running,
        })));
        app.tool_cells
            .insert(id, app.history.len().saturating_sub(1));
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
        app.tool_cells
            .insert(id, app.history.len().saturating_sub(1));
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
        app.tool_cells
            .insert(id, app.history.len().saturating_sub(1));
        return;
    }

    if is_mcp_tool(name) {
        app.add_message(HistoryCell::Tool(ToolCell::Mcp(McpToolCell {
            tool: name.to_string(),
            status: ToolStatus::Running,
            content: None,
            is_image: false,
        })));
        app.tool_cells
            .insert(id, app.history.len().saturating_sub(1));
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
            app.tool_cells
                .insert(id, app.history.len().saturating_sub(1));
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
        app.tool_cells
            .insert(id, app.history.len().saturating_sub(1));
        return;
    }

    let input_summary = summarize_tool_args(input);
    app.add_message(HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
        name: name.to_string(),
        status: ToolStatus::Running,
        input_summary,
        output: None,
    })));
    app.tool_cells
        .insert(id, app.history.len().saturating_sub(1));
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
    matches!(name, "web_search" | "search_web" | "search") || name.ends_with("_web_search")
}

fn web_search_query(input: &serde_json::Value) -> String {
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
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::TuiOptions;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn selection_point_from_position_ignores_top_padding() {
        let area = Rect {
            x: 10,
            y: 20,
            width: 30,
            height: 5,
        };

        // Content is bottom-aligned: 2 transcript lines in a 5-row viewport.
        let padding_top = 3;
        let transcript_top = 0;
        let transcript_total = 2;

        // Click in padding area -> no selection
        assert!(
            selection_point_from_position(
                area,
                area.x + 1,
                area.y,
                transcript_top,
                transcript_total,
                padding_top,
            )
            .is_none()
        );

        // First transcript line is at row `padding_top`
        let p0 = selection_point_from_position(
            area,
            area.x + 2,
            area.y + u16::try_from(padding_top).unwrap(),
            transcript_top,
            transcript_total,
            padding_top,
        )
        .expect("point");
        assert_eq!(p0.line_index, 0);
        assert_eq!(p0.column, 2);

        // Second transcript line is one row below
        let p1 = selection_point_from_position(
            area,
            area.x,
            area.y + u16::try_from(padding_top + 1).unwrap(),
            transcript_top,
            transcript_total,
            padding_top,
        )
        .expect("point");
        assert_eq!(p1.line_index, 1);
        assert_eq!(p1.column, 0);
    }

    fn make_test_app_with_workspace(workspace: PathBuf) -> App {
        let options = TuiOptions {
            model: "test-model".to_string(),
            workspace,
            allow_shell: false,
            use_alt_screen: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: false,
            yolo: false,
            resume_session_id: None,
        };
        App::new(options, &Config::default())
    }

    #[test]
    fn looks_like_rlm_expr_detects_known_functions() {
        assert!(looks_like_rlm_expr("lines(1, 10)"));
        assert!(looks_like_rlm_expr("search(\"foo\")"));
        assert!(looks_like_rlm_expr("vars()"));
        assert!(!looks_like_rlm_expr("read the README"));
    }

    #[test]
    fn rlm_repl_routes_to_chat_when_no_context_loaded() {
        let app = make_test_app_with_workspace(PathBuf::from("."));
        assert!(rlm_repl_should_route_to_chat(
            &app,
            "Please read the README"
        ));
        assert!(!rlm_repl_should_route_to_chat(&app, "lines(1, 5)"));
    }

    #[test]
    fn rlm_repl_stays_in_repl_when_context_exists() {
        let app = make_test_app_with_workspace(PathBuf::from("."));
        {
            let mut session = app.rlm_session.lock().expect("lock session");
            session.load_context("ctx", "hello".to_string(), None);
        }
        assert!(!rlm_repl_should_route_to_chat(
            &app,
            "Please read the README"
        ));
    }

    #[test]
    fn auto_rlm_detects_large_file() {
        let tmp = tempdir().expect("tempdir");
        let big = tmp.path().join("big.txt");
        let content = vec![b'a'; (AUTO_RLM_MIN_FILE_BYTES + 1) as usize];
        fs::write(&big, content).expect("write");

        let app = make_test_app_with_workspace(tmp.path().to_path_buf());
        let decision = auto_rlm_decision(&app, "analyze big.txt", false).expect("decision");
        assert!(matches!(decision.source, AutoRlmSource::File(path) if path == big));
    }

    #[test]
    fn auto_rlm_uses_largest_file_hint() {
        let tmp = tempdir().expect("tempdir");
        let small = tmp.path().join("small.txt");
        let big = tmp.path().join("bigger.txt");
        fs::write(&small, b"tiny").expect("write");
        fs::write(&big, b"this is larger").expect("write");

        let app = make_test_app_with_workspace(tmp.path().to_path_buf());
        let decision =
            auto_rlm_decision(&app, "analyze the largest file", false).expect("decision");
        assert!(matches!(decision.source, AutoRlmSource::File(path) if path == big));
    }

    #[test]
    fn auto_rlm_triggers_on_explicit_request() {
        let tmp = tempdir().expect("tempdir");
        let app = make_test_app_with_workspace(tmp.path().to_path_buf());
        let decision = auto_rlm_decision(&app, "use rlm mode", false).expect("decision");
        assert!(matches!(decision.source, AutoRlmSource::None));
    }

    #[test]
    fn auto_rlm_triggers_on_large_paste() {
        let tmp = tempdir().expect("tempdir");
        let app = make_test_app_with_workspace(tmp.path().to_path_buf());
        let content = "a".repeat(AUTO_RLM_PASTE_MIN_CHARS + 5);
        let input = format!("Summarize this\n\n{content}");
        let decision = auto_rlm_decision(&app, &input, false).expect("decision");
        match decision.source {
            AutoRlmSource::Paste { content, query } => {
                assert!(content.len() >= AUTO_RLM_PASTE_MIN_CHARS);
                assert_eq!(query.as_deref(), Some("Summarize this"));
            }
            _ => panic!("expected paste decision"),
        }
    }

    #[test]
    fn auto_rlm_is_disabled_when_setting_off() {
        let tmp = tempdir().expect("tempdir");
        let mut app = make_test_app_with_workspace(tmp.path().to_path_buf());
        app.auto_rlm = false;

        let content = "a".repeat(AUTO_RLM_PASTE_MIN_CHARS + 5);
        let input = format!("Summarize this\n\n{content}");
        let override_query = maybe_auto_switch_to_rlm(&mut app, &input);

        assert!(override_query.is_none());
        assert_ne!(app.mode, AppMode::Rlm);
    }

    #[test]
    fn parse_plan_choice_accepts_numbers() {
        assert_eq!(parse_plan_choice("1"), Some(PlanChoice::ImplementAgent));
        assert_eq!(parse_plan_choice("2"), Some(PlanChoice::ImplementYolo));
        assert_eq!(parse_plan_choice("3"), Some(PlanChoice::RevisePlan));
        assert_eq!(parse_plan_choice("4"), Some(PlanChoice::ExitPlan));
    }

    #[test]
    fn parse_plan_choice_accepts_aliases() {
        assert_eq!(parse_plan_choice("agent"), Some(PlanChoice::ImplementAgent));
        assert_eq!(parse_plan_choice("yolo"), Some(PlanChoice::ImplementYolo));
        assert_eq!(parse_plan_choice("revise"), Some(PlanChoice::RevisePlan));
        assert_eq!(parse_plan_choice("exit"), Some(PlanChoice::ExitPlan));
        assert_eq!(parse_plan_choice("unknown"), None);
    }
}
