//! Note command: append to persistent notes file

use crate::tui::app::App;
use std::fs;
use std::io::Write;

use super::CommandResult;

/// Append a note to the persistent notes file
pub fn note(app: &mut App, content: Option<&str>) -> CommandResult {
    let note_content = match content {
        Some(c) => c.trim(),
        None => {
            return CommandResult::error("Usage: /note <text>");
        }
    };

    if note_content.is_empty() {
        return CommandResult::error("Note content cannot be empty");
    }

    // Determine notes path: workspace/.deepseek/notes.md
    let notes_path = app.workspace.join(".deepseek").join("notes.md");

    // Ensure parent directory exists
    if let Some(parent) = notes_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return CommandResult::error(format!("Failed to create notes directory: {e}"));
        }
    }

    // Append to notes file
    let mut file = match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&notes_path)
    {
        Ok(f) => f,
        Err(e) => {
            return CommandResult::error(format!("Failed to open notes file: {e}"));
        }
    };

    // Write separator and note content
    if let Err(e) = writeln!(file, "\n---\n{}", note_content) {
        return CommandResult::error(format!("Failed to write note: {e}"));
    }

    CommandResult::message(format!("Note appended to {}", notes_path.display()))
}
