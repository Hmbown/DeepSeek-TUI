//! Queue commands: queue list/edit/drop/clear

use crate::tui::app::App;

use super::CommandResult;

const PREVIEW_LIMIT: usize = 120;

pub fn queue(app: &mut App, args: Option<&str>) -> CommandResult {
    let arg = args.unwrap_or("").trim();
    if arg.is_empty() || arg.eq_ignore_ascii_case("list") {
        return list_queue(app);
    }

    let mut parts = arg.split_whitespace();
    let action = parts.next().unwrap_or("").to_lowercase();

    match action.as_str() {
        "edit" => edit_queue(app, parts.next()),
        "drop" | "remove" | "rm" => drop_queue(app, parts.next()),
        "clear" => clear_queue(app),
        _ => CommandResult::error("Usage: /queue [list|edit <n>|drop <n>|clear]"),
    }
}

fn list_queue(app: &mut App) -> CommandResult {
    let mut lines = Vec::new();
    let queued = app.queued_message_count();

    if let Some(draft) = app.queued_draft.as_ref() {
        lines.push("Editing queued message:".to_string());
        lines.push(format!("- {}", truncate_preview(&draft.display)));
    }

    if queued == 0 {
        if lines.is_empty() {
            return CommandResult::message("No queued messages");
        }
        return CommandResult::message(lines.join("\n"));
    }

    lines.push(format!("Queued messages ({queued}):"));
    for (idx, message) in app.queued_messages.iter().enumerate() {
        lines.push(format!(
            "{}. {}",
            idx + 1,
            truncate_preview(&message.display)
        ));
    }

    lines.push("Tip: /queue edit <n> to edit, /queue drop <n> to remove".to_string());

    CommandResult::message(lines.join("\n"))
}

fn edit_queue(app: &mut App, index: Option<&str>) -> CommandResult {
    if app.queued_draft.is_some() {
        return CommandResult::error(
            "Already editing a queued message. Send it or /queue clear to discard.",
        );
    }
    let index = match parse_index(index) {
        Ok(index) => index,
        Err(err) => return CommandResult::error(err),
    };

    let Some(message) = app.remove_queued_message(index) else {
        return CommandResult::error("Queued message not found");
    };

    app.input = message.display.clone();
    app.cursor_position = app.input.len();
    app.queued_draft = Some(message);
    app.status_message = Some(format!("Editing queued message {}", index + 1));

    CommandResult::message(format!(
        "Editing queued message {} (press Enter to re-queue/send)",
        index + 1
    ))
}

fn drop_queue(app: &mut App, index: Option<&str>) -> CommandResult {
    let index = match parse_index(index) {
        Ok(index) => index,
        Err(err) => return CommandResult::error(err),
    };

    if app.remove_queued_message(index).is_none() {
        return CommandResult::error("Queued message not found");
    }

    CommandResult::message(format!("Dropped queued message {}", index + 1))
}

fn clear_queue(app: &mut App) -> CommandResult {
    let queued = app.queued_message_count();
    let had_draft = app.queued_draft.take().is_some();
    app.queued_messages.clear();
    if queued == 0 && !had_draft {
        return CommandResult::message("Queue already empty");
    }

    CommandResult::message("Queue cleared")
}

fn parse_index(input: Option<&str>) -> Result<usize, &'static str> {
    let Some(input) = input else {
        return Err("Missing index. Usage: /queue edit <n> or /queue drop <n>");
    };
    let raw = input
        .parse::<usize>()
        .map_err(|_| "Index must be a positive number")?;
    if raw == 0 {
        return Err("Index must be >= 1");
    }
    Ok(raw - 1)
}

fn truncate_preview(text: &str) -> String {
    if text.chars().count() <= PREVIEW_LIMIT {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars().take(PREVIEW_LIMIT.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}
