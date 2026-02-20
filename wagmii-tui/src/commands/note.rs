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

    // Determine notes path: workspace/.wagmii/notes.md
    let notes_path = app.workspace.join(".wagmii").join("notes.md");

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::{App, TuiOptions};
    use tempfile::TempDir;

    fn create_test_app_with_tmpdir(tmpdir: &TempDir) -> App {
        let options = TuiOptions {
            model: "wagmii-v3.2".to_string(),
            workspace: tmpdir.path().to_path_buf(),
            allow_shell: false,
            use_alt_screen: true,
            max_subagents: 1,
            skills_dir: tmpdir.path().join("skills"),
            memory_path: tmpdir.path().join("memory.md"),
            notes_path: tmpdir.path().join("notes.txt"),
            mcp_config_path: tmpdir.path().join("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
        };
        App::new(options, &Config::default())
    }

    #[test]
    fn test_note_without_content_returns_error() {
        let tmpdir = TempDir::new().unwrap();
        let mut app = create_test_app_with_tmpdir(&tmpdir);
        let result = note(&mut app, None);
        assert!(result.message.is_some());
        assert!(result.message.unwrap().contains("Usage: /note"));
    }

    #[test]
    fn test_note_with_empty_content_returns_error() {
        let tmpdir = TempDir::new().unwrap();
        let mut app = create_test_app_with_tmpdir(&tmpdir);
        let result = note(&mut app, Some("   "));
        assert!(result.message.is_some());
        assert!(result.message.unwrap().contains("cannot be empty"));
    }

    #[test]
    fn test_note_appends_to_file() {
        let tmpdir = TempDir::new().unwrap();
        let mut app = create_test_app_with_tmpdir(&tmpdir);
        let result = note(&mut app, Some("Test note content"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Note appended to"));

        let notes_path = tmpdir.path().join(".wagmii").join("notes.md");
        assert!(notes_path.exists());
        let content = std::fs::read_to_string(&notes_path).unwrap();
        assert!(content.contains("Test note content"));
    }

    #[test]
    fn test_note_multiple_appends() {
        let tmpdir = TempDir::new().unwrap();
        let mut app = create_test_app_with_tmpdir(&tmpdir);
        note(&mut app, Some("First note"));
        note(&mut app, Some("Second note"));

        let notes_path = tmpdir.path().join(".wagmii").join("notes.md");
        let content = std::fs::read_to_string(&notes_path).unwrap();
        assert!(content.contains("First note"));
        assert!(content.contains("Second note"));
        // Should have two separators
        assert_eq!(content.matches("---").count(), 2);
    }
}
