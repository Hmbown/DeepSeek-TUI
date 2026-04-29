#![allow(clippy::items_after_test_module)]

//! Debug commands: tokens, cost, system, context, undo, retry

use super::CommandResult;
use crate::compaction::estimate_input_tokens_conservative;
use crate::models::{SystemPrompt, context_window_for_model};
use crate::tui::app::{App, AppAction};
use crate::tui::history::HistoryCell;

fn token_count(value: Option<u32>) -> String {
    value.map_or_else(|| "not reported".to_string(), |tokens| tokens.to_string())
}

fn active_context_summary(app: &App) -> String {
    let estimated =
        estimate_input_tokens_conservative(&app.api_messages, app.system_prompt.as_ref());
    match context_window_for_model(&app.model) {
        Some(window) => {
            let used = estimated.min(window as usize);
            let percent = (used as f64 / f64::from(window) * 100.0).clamp(0.0, 100.0);
            format!("~{used} / {window} ({percent:.1}%)")
        }
        None => format!("~{estimated} / unknown window"),
    }
}

fn cache_summary(app: &App) -> String {
    match (
        app.last_prompt_cache_hit_tokens,
        app.last_prompt_cache_miss_tokens,
    ) {
        (Some(hit), Some(miss)) => format!("{hit} hit / {miss} miss"),
        (Some(hit), None) => format!("{hit} hit / miss not reported"),
        (None, Some(miss)) => format!("hit not reported / {miss} miss"),
        (None, None) => "not reported".to_string(),
    }
}

/// Show token usage for session
pub fn tokens(app: &mut App) -> CommandResult {
    let message_count = app.api_messages.len();
    let chat_count = app.history.len();

    CommandResult::message(format!(
        "Token Usage:\n\
         ─────────────────────────────\n\
         Active context:        {}\n\
         Last API input:        {} (turn telemetry; may count repeated prefix across tool rounds)\n\
         Last API output:       {}\n\
         Cache hit/miss:        {} (telemetry/cost only)\n\
         Cumulative tokens:     {} (session usage telemetry)\n\
         Approx session cost:   ${:.4}\n\
         API messages:          {}\n\
         Chat messages:         {}\n\
         Model:                 {}",
        active_context_summary(app),
        token_count(app.last_prompt_tokens),
        token_count(app.last_completion_tokens),
        cache_summary(app),
        app.total_tokens,
        app.session_cost,
        message_count,
        chat_count,
        app.model,
    ))
}

/// Show session cost breakdown
pub fn cost(app: &mut App) -> CommandResult {
    CommandResult::message(format!(
        "Session Cost:\n\
         ─────────────────────────────\n\
         Approx total spent: ${:.4}\n\n\
         Cost estimates are approximate and use provider usage telemetry when available.\n\n\
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
pub fn context(_app: &mut App) -> CommandResult {
    CommandResult::action(AppAction::OpenContextInspector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::models::{ContentBlock, Message, SystemBlock};
    use crate::tui::app::{App, TuiOptions};
    use std::path::PathBuf;

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: PathBuf::from("/tmp/test-workspace"),
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
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
    fn test_tokens_shows_usage_info() {
        let mut app = create_test_app();
        app.total_tokens = 1234;
        app.session_cost = 0.05;
        app.last_prompt_tokens = Some(100);
        app.last_completion_tokens = Some(25);
        app.last_prompt_cache_hit_tokens = Some(70);
        app.last_prompt_cache_miss_tokens = Some(30);
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "test".to_string(),
                cache_control: None,
            }],
        });
        app.history.push(HistoryCell::User {
            content: "test".to_string(),
        });

        let result = tokens(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Token Usage"));
        assert!(msg.contains("Active context:"));
        assert!(msg.contains("Last API input:"));
        assert!(msg.contains("Last API output:"));
        assert!(msg.contains("Cache hit/miss:"));
        assert!(msg.contains("70 hit / 30 miss"));
        assert!(msg.contains("Cumulative tokens:"));
        assert!(msg.contains("Approx session cost:"));
        assert!(msg.contains("API messages:"));
        assert!(msg.contains("Chat messages:"));
        assert!(msg.contains("Model:"));
    }

    #[test]
    fn test_cost_shows_spending_info() {
        let mut app = create_test_app();
        app.session_cost = 0.1234;
        let result = cost(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Session Cost"));
        assert!(msg.contains("Approx total spent:"));
        assert!(msg.contains("approximate"));
        assert!(msg.contains("$0.1234"));
    }

    #[test]
    fn test_system_prompt_displays_text() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Text("Test system prompt".to_string()));
        let result = system_prompt(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("System Prompt"));
        assert!(msg.contains("Test system prompt"));
    }

    #[test]
    fn test_system_prompt_displays_blocks() {
        let mut app = create_test_app();
        app.system_prompt = Some(SystemPrompt::Blocks(vec![
            SystemBlock {
                block_type: "text".to_string(),
                text: "Block 1".to_string(),
                cache_control: None,
            },
            SystemBlock {
                block_type: "text".to_string(),
                text: "Block 2".to_string(),
                cache_control: None,
            },
        ]));
        let result = system_prompt(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("System Prompt"));
        assert!(msg.contains("Block 1"));
        assert!(msg.contains("Block 2"));
    }

    #[test]
    fn test_system_prompt_none() {
        let mut app = create_test_app();
        app.system_prompt = None;
        let result = system_prompt(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("(no system prompt)"));
    }

    #[test]
    fn test_system_prompt_truncates_long_text() {
        let mut app = create_test_app();
        let long_text = "x".repeat(600);
        app.system_prompt = Some(SystemPrompt::Text(long_text));
        let result = system_prompt(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("..."));
        assert!(msg.contains("chars total"));
    }

    #[test]
    fn test_context_shows_usage_stats() {
        let mut app = create_test_app();
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "Hello".to_string(),
                cache_control: None,
            }],
        });
        app.history.push(HistoryCell::User {
            content: "Hello".to_string(),
        });

        let result = context(&mut app);
        assert!(matches!(
            result.action,
            Some(AppAction::OpenContextInspector)
        ));
        assert!(result.message.is_none());
    }

    #[test]
    fn test_undo_removes_last_exchange() {
        let mut app = create_test_app();
        app.history.push(HistoryCell::User {
            content: "Hello".to_string(),
        });
        app.history.push(HistoryCell::Assistant {
            content: "Hi".to_string(),
            streaming: false,
        });
        app.api_messages.push(Message {
            role: "user".to_string(),
            content: vec![],
        });
        app.api_messages.push(Message {
            role: "assistant".to_string(),
            content: vec![],
        });

        let initial_history_len = app.history.len();
        let initial_api_len = app.api_messages.len();
        let result = undo(&mut app);

        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Removed"));
        assert!(app.history.len() < initial_history_len);
        assert!(app.api_messages.len() < initial_api_len);
    }

    #[test]
    fn test_undo_nothing_to_undo() {
        let mut app = create_test_app();
        // Clear any default history
        app.history.clear();
        app.api_messages.clear();
        let result = undo(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Nothing to undo") || msg.contains("Removed"));
    }

    #[test]
    fn test_retry_with_previous_message() {
        let mut app = create_test_app();
        app.history.push(HistoryCell::User {
            content: "Test message".to_string(),
        });
        app.history.push(HistoryCell::Assistant {
            content: "Response".to_string(),
            streaming: false,
        });

        let result = retry(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Retrying"));
        assert!(msg.contains("Test message"));
        assert!(matches!(result.action, Some(AppAction::SendMessage(_))));
    }

    #[test]
    fn test_retry_no_previous_message() {
        let mut app = create_test_app();
        let result = retry(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("No previous request to retry"));
        assert!(result.action.is_none());
    }

    #[test]
    fn test_retry_truncates_long_input() {
        let mut app = create_test_app();
        let long_input = "x".repeat(100);
        app.history.push(HistoryCell::User {
            content: long_input.clone(),
        });
        app.history.push(HistoryCell::Assistant {
            content: "Response".to_string(),
            streaming: false,
        });

        let result = retry(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Retrying"));
        assert!(msg.contains("..."));
    }
}

/// Remove last message pair (user + assistant)
pub fn undo(app: &mut App) -> CommandResult {
    // Remove from display history (up to the last user message)
    let mut removed_count = 0;
    while !app.history.is_empty() {
        let last_is_user = matches!(app.history.last(), Some(HistoryCell::User { .. }));
        app.pop_history();
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
        // Keep tool/index mappings consistent after truncation.
        app.tool_cells.clear();
        app.tool_details_by_cell.clear();
        app.exploring_entries.clear();
        app.ignored_tool_calls.clear();
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
