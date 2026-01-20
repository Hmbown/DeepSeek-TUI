//! Core commands: help, clear, exit, model

use std::fmt::Write;

use crate::tools::plan::PlanState;
use crate::tui::app::{App, AppAction};
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
