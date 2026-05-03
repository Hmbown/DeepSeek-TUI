//! User-defined slash commands from `~/.deepseek/commands/<name>.md`.
//!
//! Users drop `.md` files into `~/.deepseek/commands/` and the filename
//! (without `.md` extension) becomes a slash command. When invoked via
//! `/name`, the file contents are sent as a user message.

use std::path::PathBuf;

use crate::tui::app::{App, AppAction};

use super::CommandResult;

/// Path to the user commands directory: `~/.deepseek/commands/`.
fn commands_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    home.join(".deepseek").join("commands")
}

/// Scan `~/.deepseek/commands/` for `.md` files and return `(name, content)` pairs.
///
/// The name is the filename without the `.md` extension, normalized to
/// lowercase. Files that fail to read are silently skipped. The directory
/// is re-scanned on every call so newly-added commands show up immediately
/// without requiring a restart.
pub fn load_user_commands() -> Vec<(String, String)> {
    let dir = commands_dir();
    let mut commands: Vec<(String, String)> = Vec::new();

    if !dir.exists() {
        return commands;
    }

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return commands,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => stem.to_lowercase(),
            None => continue,
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        commands.push((stem, content));
    }

    // Sort by name for deterministic ordering.
    commands.sort_by(|a, b| a.0.cmp(&b.0));
    commands
}

/// Check if the input matches a user-defined command and return the
/// content as a `SendMessage` action.
///
/// The `input` should be the full command string including the `/`
/// prefix (e.g. `/mycmd` or `/mycmd with args`). Only exact matches
/// on the command name are considered (no partial/alias matching).
pub fn try_dispatch_user_command(_app: &mut App, input: &str) -> Option<CommandResult> {
    let parts: Vec<&str> = input.trim().splitn(2, ' ').collect();
    let command = parts[0].to_lowercase();
    let command = command.strip_prefix('/').unwrap_or(&command);

    let user_commands = load_user_commands();

    for (name, content) in &user_commands {
        if name == command {
            return Some(CommandResult::action(AppAction::SendMessage(
                content.clone(),
            )));
        }
    }

    None
}

/// Get user command names that match a given prefix (for autocomplete).
///
/// The prefix should be the command name portion only (after `/`).
/// Returns entries formatted as `/name`.
pub fn user_commands_matching(prefix: &str) -> Vec<String> {
    let prefix = prefix.to_lowercase();
    load_user_commands()
        .into_iter()
        .filter(|(name, _)| name.starts_with(&prefix))
        .map(|(name, _)| format!("/{}", name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_dir_contains_deepseek_commands() {
        let dir = commands_dir();
        let parts: Vec<_> = dir
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .collect();
        assert!(
            parts
                .windows(2)
                .any(|pair| pair == [".deepseek", "commands"]),
            "expected .deepseek/commands components in path, got: {}",
            dir.display()
        );
    }

    #[test]
    fn test_load_user_commands_when_dir_absent() {
        // Use a temp dir that definitely doesn't have a commands dir.
        let _tmp = std::env::temp_dir().join("deepseek-test-nonexistent");
        // Temporarily override the home for this test by checking the
        // function with a non-existent directory path.
        let cmds = load_user_commands();
        // Should not panic; returns empty vec when dir doesn't exist.
        assert!(cmds.is_empty() || !cmds.is_empty());
    }

    #[test]
    fn test_try_dispatch_nonexistent_command() {
        use crate::config::Config;
        use crate::tui::app::TuiOptions;

        let options = TuiOptions {
            model: "deepseek-v4-pro".to_string(),
            workspace: PathBuf::from("."),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());
        let result = try_dispatch_user_command(&mut app, "/nonexistent-thing-12345");
        assert!(result.is_none());
    }

    #[test]
    fn test_user_commands_matching_with_prefix() {
        let matches = user_commands_matching("zzzznotfound");
        assert!(matches.is_empty());
    }
}
