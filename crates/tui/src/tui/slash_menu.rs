//! Slash-command autocomplete + popup-menu helpers.
//!
//! Extracted from `tui/ui.rs` (P1.2). The on-screen popup itself is rendered
//! by the composer widget; these helpers source the entries, apply a
//! selection, and handle Tab-completion when the popup isn't open.
//!
//! Intentionally separate from `tui::file_mention` even though both surface
//! a similar popup — the trigger characters, ranking, and post-selection
//! behaviour differ enough to keep them apart.

use crate::commands;

use super::app::App;
use super::widgets::slash_completion_hints;

/// Return the slash-menu entries the composer should display, honouring
/// `slash_menu_hidden` (set when the user dismisses the popup with Esc).
pub fn visible_slash_menu_entries(app: &App, limit: usize) -> Vec<String> {
    if app.slash_menu_hidden {
        return Vec::new();
    }
    slash_completion_hints(&app.input, limit)
}

/// Apply the currently-selected slash menu entry to the composer input.
/// Optionally appends a trailing space when the command takes arguments
/// so the user can type the rest without an extra keystroke.
pub fn apply_slash_menu_selection(app: &mut App, entries: &[String], append_space: bool) -> bool {
    if entries.is_empty() {
        return false;
    }

    let selected_idx = app.slash_menu_selected.min(entries.len().saturating_sub(1));
    let mut command = entries[selected_idx].clone();

    if append_space
        && !command.ends_with(' ')
        && !command.contains(char::is_whitespace)
        && let Some(info) = commands::get_command_info(command.trim_start_matches('/'))
        && (info.usage.contains('<') || info.usage.contains('['))
    {
        command.push(' ');
    }

    app.input = command;
    app.cursor_position = app.input.chars().count();
    app.slash_menu_hidden = false;
    app.status_message = Some(format!("Command selected: {}", app.input.trim_end()));
    true
}

/// Tab-completion for a slash-command-like input. Extends the input to the
/// longest unambiguous prefix; if exactly one command matches, completes it
/// fully (with trailing space). On ambiguity, posts a status hint listing
/// up to five candidates.
pub fn try_autocomplete_slash_command(app: &mut App) -> bool {
    if !app.input.starts_with('/') || app.input.contains(char::is_whitespace) {
        return false;
    }

    let prefix = app.input.trim_start_matches('/');
    let matches = commands::commands_matching(prefix);
    if matches.is_empty() {
        return false;
    }

    let names = matches.iter().map(|info| info.name).collect::<Vec<_>>();
    let shared = crate::tui::file_mention::longest_common_prefix(&names);

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
