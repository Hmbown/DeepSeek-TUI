//! Core commands: help, clear, exit, model

use std::fmt::Write;

use crate::config::{COMMON_DEEPSEEK_MODELS, canonical_model_name};
use crate::tui::app::{App, AppAction, AppMode};
use crate::tui::views::{HelpView, ModalKind, SubAgentsView};

use super::CommandResult;

/// Show help information
pub fn help(app: &mut App, topic: Option<&str>) -> CommandResult {
    if let Some(topic) = topic {
        // Show help for specific command
        if let Some(cmd) = super::get_command_info(topic) {
            let mut help = format!(
                "{}\n\n  {}\n\n  Usage: {}",
                cmd.name, cmd.description, cmd.usage
            );
            if !cmd.aliases.is_empty() {
                let _ = write!(help, "\n  Aliases: {}", cmd.aliases.join(", "));
            }
            return CommandResult::message(help);
        }
        return CommandResult::error(format!("Unknown command: {topic}"));
    }

    // Show help overlay
    if app.view_stack.top_kind() != Some(ModalKind::Help) {
        app.view_stack.push(HelpView::new());
    }
    CommandResult::ok()
}

/// Clear conversation history
pub fn clear(app: &mut App) -> CommandResult {
    app.history.clear();
    app.mark_history_updated();
    app.api_messages.clear();
    app.system_prompt = None;
    app.transcript_selection.clear();
    app.queued_messages.clear();
    app.queued_draft = None;
    app.total_conversation_tokens = 0;
    let todos_cleared = app.clear_todos();
    app.tool_log.clear();
    app.tool_cells.clear();
    app.tool_details_by_cell.clear();
    app.exploring_entries.clear();
    app.ignored_tool_calls.clear();
    app.pending_tool_uses.clear();
    app.last_exec_wait_command = None;
    app.last_prompt_tokens = None;
    app.last_completion_tokens = None;
    app.current_session_id = None;
    let message = if todos_cleared {
        "Conversation cleared".to_string()
    } else {
        "Conversation cleared (plan state busy; run /clear again if needed)".to_string()
    };
    CommandResult::with_message_and_action(
        message,
        AppAction::SyncSession {
            messages: Vec::new(),
            system_prompt: None,
            model: app.model.clone(),
            workspace: app.workspace.clone(),
        },
    )
}

/// Exit the application
pub fn exit() -> CommandResult {
    CommandResult::action(AppAction::Quit)
}

/// Switch or view current model
pub fn model(app: &mut App, model_name: Option<&str>) -> CommandResult {
    if let Some(name) = model_name {
        let Some(canonical) = canonical_model_name(name) else {
            return CommandResult::error(format!(
                "Invalid model '{name}'. Supported models: {}",
                COMMON_DEEPSEEK_MODELS.join(", ")
            ));
        };
        let old_model = app.model.clone();
        app.model = canonical.to_string();
        app.update_model_compaction_budget();
        app.last_prompt_tokens = None;
        app.last_completion_tokens = None;
        CommandResult::with_message_and_action(
            format!("Model changed: {old_model} → {canonical}"),
            AppAction::UpdateCompaction(app.compaction_config()),
        )
    } else {
        let supported = COMMON_DEEPSEEK_MODELS.join(", ");
        CommandResult::message(format!(
            "Current model: {}\nSupported models: {}",
            app.model, supported
        ))
    }
}

/// Fetch and list available models from the configured API endpoint.
pub fn models(_app: &mut App) -> CommandResult {
    CommandResult::action(AppAction::FetchModels)
}

/// List sub-agent status from the engine
pub fn subagents(app: &mut App) -> CommandResult {
    if app.view_stack.top_kind() != Some(ModalKind::SubAgents) {
        app.view_stack
            .push(SubAgentsView::new(app.subagent_cache.clone()));
    }
    app.status_message = Some("Fetching sub-agent status...".to_string());
    CommandResult::action(AppAction::ListSubAgents)
}

/// Show `DeepSeek` dashboard and docs links
pub fn deepseek_links() -> CommandResult {
    CommandResult::message(
        "DeepSeek Links:\n\
─────────────────────────────\n\
Dashboard: https://platform.deepseek.com\n\
Docs:      https://platform.deepseek.com/docs\n\n\
Tip: API keys are available in the dashboard console.",
    )
}

/// Show home dashboard with stats and quick actions
pub fn home_dashboard(app: &mut App) -> CommandResult {
    let mut stats = String::new();

    // Basic info
    let _ = writeln!(stats, "DeepSeek CLI Home Dashboard");
    let _ = writeln!(stats, "============================================");

    // Model & mode
    let _ = writeln!(stats, "Model:      {}", app.model);
    let _ = writeln!(stats, "Mode:       {}", app.mode.label());
    let _ = writeln!(stats, "Workspace:  {}", app.workspace.display());

    // Session stats
    let history_count = app.history.len();
    let total_tokens = app.total_conversation_tokens;
    let queued_messages = app.queued_messages.len();
    let _ = writeln!(stats, "History:    {} messages", history_count);
    let _ = writeln!(stats, "Tokens:     {} (session)", total_tokens);
    if queued_messages > 0 {
        let _ = writeln!(stats, "Queued:     {} messages", queued_messages);
    }

    // Sub-agents
    let subagent_count = app.subagent_cache.len();
    if subagent_count > 0 {
        let _ = writeln!(stats, "Sub-agents: {} active", subagent_count);
    }

    // Active skill
    if let Some(skill) = &app.active_skill {
        let _ = writeln!(stats, "Skill:      {} (active)", skill);
    }

    // Quick actions section
    let _ = writeln!(stats, "\nQuick Actions");
    let _ = writeln!(stats, "--------------------------------------------");
    let _ = writeln!(stats, "/deepseek    - Dashboard & API links");
    let _ = writeln!(stats, "/skills      - List available skills");
    let _ = writeln!(stats, "/config      - Show current configuration");
    let _ = writeln!(stats, "/settings    - Show persistent settings");
    let _ = writeln!(stats, "/model       - Switch or view model");
    let _ = writeln!(stats, "/subagents   - List sub-agent status");
    let _ = writeln!(stats, "/task list   - Show background task queue");
    let _ = writeln!(stats, "/help        - Show help");

    // Mode-specific tips
    let _ = writeln!(stats, "\nMode Tips");
    let _ = writeln!(stats, "--------------------------------------------");
    match app.mode {
        AppMode::Normal => {
            let _ = writeln!(stats, "Normal mode - Chat with the assistant");
        }
        AppMode::Agent => {
            let _ = writeln!(stats, "Agent mode - Use tools for autonomous tasks");
            let _ = writeln!(stats, "  Type /yolo to enable full tool access");
        }
        AppMode::Yolo => {
            let _ = writeln!(stats, "YOLO mode - Full tool access, no approvals");
            let _ = writeln!(stats, "  Be careful with destructive operations!");
        }
        AppMode::Plan => {
            let _ = writeln!(stats, "Plan mode - Design before implementing");
            let _ = writeln!(stats, "  Use /plan to create structured checklists");
        }
    }

    CommandResult::message(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::models::Message;
    use crate::tui::app::{App, AppMode, TuiOptions};
    use crate::tui::history::HistoryCell;
    use std::path::PathBuf;

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "deepseek-reasoner".to_string(),
            workspace: PathBuf::from("/tmp/test-workspace"),
            allow_shell: false,
            use_alt_screen: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("/tmp/test-skills"),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
        };
        App::new(options, &Config::default())
    }

    #[test]
    fn test_help_unknown_command() {
        let mut app = create_test_app();
        let result = help(&mut app, Some("nonexistent"));
        assert!(result.message.is_some());
        assert!(result.message.unwrap().contains("Unknown command"));
        assert!(result.action.is_none());
    }

    #[test]
    fn test_help_known_command() {
        let mut app = create_test_app();
        let result = help(&mut app, Some("clear"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("clear"));
        assert!(msg.contains("Clear conversation history"));
        assert!(msg.contains("Usage: /clear"));
    }

    #[test]
    fn test_help_pushes_overlay() {
        let mut app = create_test_app();
        assert_ne!(app.view_stack.top_kind(), Some(ModalKind::Help));
        let result = help(&mut app, None);
        assert_eq!(result.message, None);
        assert_eq!(result.action, None);
        assert_eq!(app.view_stack.top_kind(), Some(ModalKind::Help));
    }

    #[test]
    fn test_help_does_not_duplicate_overlay() {
        let mut app = create_test_app();
        help(&mut app, None);
        let initial_kind = app.view_stack.top_kind();
        help(&mut app, None);
        assert_eq!(app.view_stack.top_kind(), initial_kind);
    }

    #[test]
    fn test_clear_resets_all_state() {
        let mut app = create_test_app();
        // Set up some state
        app.history.push(HistoryCell::User {
            content: "test".to_string(),
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![],
        });
        app.total_conversation_tokens = 100;
        app.tool_log.push("test".to_string());
        app.current_session_id = Some("existing-session".to_string());

        let result = clear(&mut app);
        assert!(result.message.is_some());
        assert!(app.history.is_empty());
        assert!(app.api_messages.is_empty());
        assert_eq!(app.total_conversation_tokens, 0);
        assert!(app.tool_log.is_empty());
        assert!(app.tool_cells.is_empty());
        assert!(app.tool_details_by_cell.is_empty());
        assert!(app.current_session_id.is_none());
        assert!(matches!(result.action, Some(AppAction::SyncSession { .. })));
    }

    #[test]
    fn test_exit_returns_quit_action() {
        let result = exit();
        assert!(result.message.is_none());
        assert!(matches!(result.action, Some(AppAction::Quit)));
    }

    #[test]
    fn test_model_change_updates_state() {
        let mut app = create_test_app();
        let old_model = app.model.clone();
        let result = model(&mut app, Some("deepseek-reasoner"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains(&old_model));
        assert!(msg.contains("deepseek-reasoner"));
        assert!(matches!(
            result.action,
            Some(AppAction::UpdateCompaction(_))
        ));
        assert_eq!(app.model, "deepseek-reasoner");
        assert_eq!(app.last_prompt_tokens, None);
        assert_eq!(app.last_completion_tokens, None);
    }

    #[test]
    fn test_model_change_rejects_invalid_model() {
        let mut app = create_test_app();
        let result = model(&mut app, Some("gpt-4"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Invalid model"));
        assert!(msg.contains("deepseek-chat"));
        assert!(msg.contains("deepseek-reasoner"));
        assert!(result.action.is_none());
    }

    #[test]
    fn test_model_without_args_shows_info() {
        let mut app = create_test_app();
        let result = model(&mut app, None);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Current model:"));
        assert!(msg.contains("Supported models:"));
        assert!(result.action.is_none());
    }

    #[test]
    fn test_models_triggers_fetch_action() {
        let mut app = create_test_app();
        let result = models(&mut app);
        assert!(result.message.is_none());
        assert!(matches!(result.action, Some(AppAction::FetchModels)));
    }

    #[test]
    fn test_subagents_pushes_view_and_sets_status() {
        let mut app = create_test_app();
        let result = subagents(&mut app);
        assert!(result.message.is_none());
        assert!(matches!(result.action, Some(AppAction::ListSubAgents)));
        assert_eq!(app.view_stack.top_kind(), Some(ModalKind::SubAgents));
        assert_eq!(
            app.status_message,
            Some("Fetching sub-agent status...".to_string())
        );
    }

    #[test]
    fn test_deepseek_links() {
        let result = deepseek_links();
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("DeepSeek Links"));
        assert!(msg.contains("https://platform.deepseek.com"));
        assert!(result.action.is_none());
    }

    #[test]
    fn test_home_dashboard_includes_all_sections() {
        let mut app = create_test_app();
        app.total_conversation_tokens = 1234;
        let result = home_dashboard(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("DeepSeek CLI Home Dashboard"));
        assert!(msg.contains("Model:"));
        assert!(msg.contains("Mode:"));
        assert!(msg.contains("Workspace:"));
        assert!(msg.contains("History:"));
        assert!(msg.contains("Tokens:"));
        assert!(msg.contains("Quick Actions"));
        assert!(msg.contains("Mode Tips"));
        assert!(result.action.is_none());
    }

    #[test]
    fn test_home_dashboard_shows_queued_when_present() {
        let mut app = create_test_app();
        app.queued_messages
            .push_back(crate::tui::app::QueuedMessage::new(
                "test".to_string(),
                None,
            ));
        let result = home_dashboard(&mut app);
        let msg = result.message.unwrap();
        assert!(msg.contains("Queued:"));
    }

    #[test]
    fn test_home_dashboard_mode_tips_for_each_mode() {
        let modes = [
            AppMode::Normal,
            AppMode::Agent,
            AppMode::Yolo,
            AppMode::Plan,
        ];
        for mode in modes {
            let mut app = create_test_app();
            app.mode = mode;
            let result = home_dashboard(&mut app);
            let msg = result.message.unwrap();
            assert!(msg.contains("Mode Tips"), "Missing tips for mode {mode:?}");
        }
    }
}
