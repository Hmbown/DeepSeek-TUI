//! Core commands: help, clear, exit, model

use std::fmt::Write;

use crate::tools::plan::PlanState;
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
    app.transcript_selection.clear();
    app.total_conversation_tokens = 0;
    app.clear_todos();
    if let Ok(mut plan) = app.plan_state.lock() {
        *plan = PlanState::default();
    }
    app.tool_log.clear();
    CommandResult::message("Conversation cleared")
}

/// Exit the application
pub fn exit() -> CommandResult {
    CommandResult::action(AppAction::Quit)
}

/// Available DeepSeek models
const AVAILABLE_MODELS: &[&str] = &[
    "deepseek-reasoner",
    "deepseek-chat",
    "deepseek-r1",
    "deepseek-v3",
    "deepseek-v3.2",
];

/// Switch or view current model
pub fn model(app: &mut App, model_name: Option<&str>) -> CommandResult {
    if let Some(name) = model_name {
        let old_model = app.model.clone();
        app.model = name.to_string();
        CommandResult::message(format!("Model changed: {old_model} → {name}"))
    } else {
        let available = AVAILABLE_MODELS.join(", ");
        CommandResult::message(format!(
            "Current model: {}\nAvailable: {}",
            app.model, available
        ))
    }
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
        AppMode::Rlm => {
            let _ = writeln!(stats, "RLM mode - Recursive language model sandbox");
            let _ = writeln!(stats, "  Use /repl to toggle REPL input");
        }
        AppMode::Duo => {
            let _ = writeln!(stats, "Duo mode - Dialectical autocoding");
            let _ = writeln!(stats, "  Player-coach loop for complex tasks");
        }
    }

    CommandResult::message(stats)
}
