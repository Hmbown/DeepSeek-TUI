//! Debug commands: tokens, cost, system, context, undo, retry

use super::CommandResult;
use crate::models::{SystemPrompt, context_window_for_model};
use crate::tui::app::{App, AppAction};
use crate::tui::history::HistoryCell;
use crate::utils::estimate_message_chars;

/// Show token usage for session
pub fn tokens(app: &mut App) -> CommandResult {
    let message_count = app.api_messages.len();
    let chat_count = app.history.len();

    CommandResult::message(format!(
        "Token Usage:\n\
         ─────────────────────────────\n\
         Total tokens:     {}\n\
         Session cost:     ${:.4}\n\
         API messages:     {}\n\
         Chat messages:    {}\n\
         Model:            {}",
        app.total_tokens, app.session_cost, message_count, chat_count, app.model,
    ))
}

/// Show session cost breakdown
pub fn cost(app: &mut App) -> CommandResult {
    CommandResult::message(format!(
        "Session Cost:\n\
         ─────────────────────────────\n\
         Total spent:      ${:.4}\n\n\
         DeepSeek API Pricing:\n\
         ─────────────────────────────\n\
         Pricing details are not configured in this CLI.",
        app.session_cost,
    ))
}

/// Show current system prompt
pub fn system_prompt(app: &mut App) -> CommandResult {
    let prompt_text = match &app.system_prompt {
        Some(SystemPrompt::Text(text)) => text.clone(),
        Some(SystemPrompt::Blocks(blocks)) => blocks
            .iter()
            .map(|b| b.text.clone())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n"),
        None => "(no system prompt)".to_string(),
    };

    // Truncate if too long
    let display = if prompt_text.len() > 500 {
        // Find a valid UTF-8 char boundary at or before byte 500
        let truncate_at = prompt_text
            .char_indices()
            .take_while(|(i, _)| *i <= 500)
            .last()
            .map_or(0, |(i, _)| i);
        format!(
            "{}...\n\n(truncated, {} chars total)",
            &prompt_text[..truncate_at],
            prompt_text.len()
        )
    } else {
        prompt_text
    };

    CommandResult::message(format!(
        "System Prompt ({} mode):\n─────────────────────────────\n{}",
        app.mode.label(),
        display
    ))
}

/// Show context window usage
pub fn context(app: &mut App) -> CommandResult {
    let mut total_chars = estimate_message_chars(&app.api_messages);

    // System prompt
    if let Some(SystemPrompt::Text(text)) = &app.system_prompt {
        total_chars += text.len();
    } else if let Some(SystemPrompt::Blocks(blocks)) = &app.system_prompt {
        for block in blocks {
            total_chars += block.text.len();
        }
    }

    // Rough token estimate (4 chars per token on average)
    let estimated_tokens = total_chars / 4;

    let context_size = context_window_for_model(&app.model).unwrap_or(128_000);
    let estimated_tokens_u32 = u32::try_from(estimated_tokens).unwrap_or(u32::MAX);
    let usage_pct = (f64::from(estimated_tokens_u32) / f64::from(context_size) * 100.0).min(100.0);

    CommandResult::message(format!(
        "Context Usage:\n\
         ─────────────────────────────\n\
         Characters:       {}\n\
         Estimated tokens: ~{}\n\
         Context window:   {}\n\
         Usage:            {:.1}%\n\n\
         Messages:         {}\n\
         API messages:     {}",
        total_chars,
        estimated_tokens,
        context_size,
        usage_pct,
        app.history.len(),
        app.api_messages.len(),
    ))
}

/// Remove last message pair (user + assistant)
pub fn undo(app: &mut App) -> CommandResult {
    // Remove from display history (up to the last user message)
    let mut removed_count = 0;
    while !app.history.is_empty() {
        let last_is_user = matches!(app.history.last(), Some(HistoryCell::User { .. }));
        app.history.pop();
        removed_count += 1;
        if last_is_user {
            break;
        }
    }

    // Remove from API messages
    while let Some(last) = app.api_messages.last() {
        if last.role == "user" {
            app.api_messages.pop();
            break;
        }
        app.api_messages.pop();
    }

    if removed_count > 0 {
        app.mark_history_updated();
        CommandResult::message(format!("Removed {removed_count} message(s)"))
    } else {
        CommandResult::message("Nothing to undo")
    }
}

/// Retry last request - remove last exchange and re-send the user's message
pub fn retry(app: &mut App) -> CommandResult {
    let last_user_input = app.history.iter().rev().find_map(|cell| match cell {
        HistoryCell::User { content } => Some(content.clone()),
        _ => None,
    });

    match last_user_input {
        Some(input) => {
            undo(app);
            let display_input = if input.len() > 50 {
                let truncate_at = input
                    .char_indices()
                    .take_while(|(i, _)| *i <= 50)
                    .last()
                    .map_or(0, |(i, _)| i);
                format!("{}...", &input[..truncate_at])
            } else {
                input.clone()
            };
            CommandResult::with_message_and_action(
                format!("Retrying: {display_input}"),
                AppAction::SendMessage(input),
            )
        }
        None => CommandResult::error("No previous request to retry"),
    }
}
