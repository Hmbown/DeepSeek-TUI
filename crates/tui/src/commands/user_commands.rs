//! User-defined slash commands from `~/.deepseek/commands/<name>.md` and
//! project-local `.deepseek/commands/<name>.md` / `.cursor/commands/<name>.md`.
//!
//! Users drop `.md` files into any of these directories and the filename
//! (without `.md` extension) becomes a slash command. When invoked via
//! `/name`, the file contents are sent as a user message.
//!
//! Discovery order (later entries override earlier ones):
//! 1. `~/.deepseek/commands/`  — global user commands
//! 2. `<cwd>/.deepseek/commands/` — project-local deepseek commands
//! 3. `<cwd>/.cursor/commands/`   — project-local cursor commands

use std::path::{Path, PathBuf};

use crate::tui::app::{App, AppAction};

use super::CommandResult;

/// Path to the global user commands directory: `~/.deepseek/commands/`.
fn global_commands_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    home.join(".deepseek").join("commands")
}

/// Scan a single directory for `.md` command files.
///
/// Returns `(name, content)` pairs where name is the stem lowercased.
/// Non-`.md` files and unreadable files are silently skipped.
fn scan_commands_dir(dir: &Path) -> Vec<(String, String)> {
    let mut commands = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return commands,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_lowercase(),
            None => continue,
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        commands.push((stem, content));
    }
    commands
}

/// Collect user-defined commands from all command directories.
///
/// Scans the global directory first, then project-local directories.
/// Project-local commands override global ones with the same name.
/// The directory is re-scanned on every call so newly-added commands
/// show up immediately without requiring a restart.
pub fn load_user_commands() -> Vec<(String, String)> {
    // Use an ordered map so later (higher-priority) entries overwrite earlier ones.
    let mut by_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for (name, content) in scan_commands_dir(&global_commands_dir()) {
        by_name.insert(name, content);
    }

    if let Ok(cwd) = std::env::current_dir() {
        for sub in &[".deepseek/commands", ".cursor/commands"] {
            for (name, content) in scan_commands_dir(&cwd.join(sub)) {
                by_name.insert(name, content);
            }
        }
    }

    let mut commands: Vec<(String, String)> = by_name.into_iter().collect();
    commands.sort_by(|a, b| a.0.cmp(&b.0));
    commands
}

/// Check if the input matches a user-defined command and return the
/// content as a `SendMessage` action.
///
/// The `input` should be the full command string including the `/`
/// prefix (e.g. `/mycmd` or `/mycmd with args`). Only exact matches
/// on the command name are considered (no partial/alias matching).
/// Substitute $1, $2, $ARGUMENTS placeholders in a command template.
fn apply_template(template: &str, args: &str) -> String {
    let positional: Vec<&str> = args.split_whitespace().collect();
    let mut result = template.replace("$ARGUMENTS", args);
    for (i, arg) in positional.iter().enumerate() {
        result = result.replace(&format!("${}", i + 1), arg);
    }
    result
}

pub fn try_dispatch_user_command(_app: &mut App, input: &str) -> Option<CommandResult> {
    let parts: Vec<&str> = input.trim().splitn(2, ' ').collect();
    let command = parts[0].to_lowercase();
    let command = command.strip_prefix('/').unwrap_or(&command);
    let args = parts.get(1).copied().unwrap_or("").trim();

    let user_commands = load_user_commands();

    for (name, content) in &user_commands {
        if name == command {
            let message = apply_template(content, args);
            return Some(CommandResult::action(AppAction::SendMessage(message)));
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
    use std::fs;

    #[test]
    fn test_global_commands_dir_contains_deepseek_commands() {
        let dir = global_commands_dir();
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
    fn test_scan_commands_dir_absent() {
        let tmp = std::env::temp_dir().join("deepseek-scan-absent-12345");
        let cmds = scan_commands_dir(&tmp);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_scan_commands_dir_reads_md_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(tmp.path().join("hello.md"), "Hello world!").unwrap();
        fs::write(tmp.path().join("other.txt"), "ignored").unwrap();
        let cmds = scan_commands_dir(tmp.path());
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].0, "hello");
        assert_eq!(cmds[0].1, "Hello world!");
    }

    #[test]
    fn test_project_local_commands_override_global() {
        // Simulate global and project-local dirs.
        let global_dir = tempfile::tempdir().expect("global dir");
        let project_dir = tempfile::tempdir().expect("project dir");

        fs::write(global_dir.path().join("shared.md"), "global version").unwrap();
        fs::write(global_dir.path().join("only-global.md"), "only in global").unwrap();
        fs::write(project_dir.path().join("shared.md"), "project version").unwrap();

        // Merge: global first, then project-local overrides.
        let mut by_name: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (name, content) in scan_commands_dir(global_dir.path()) {
            by_name.insert(name, content);
        }
        for (name, content) in scan_commands_dir(project_dir.path()) {
            by_name.insert(name, content);
        }

        assert_eq!(
            by_name.get("shared").map(String::as_str),
            Some("project version")
        );
        assert_eq!(
            by_name.get("only-global").map(String::as_str),
            Some("only in global")
        );
    }

    #[test]
    fn test_load_user_commands_when_dir_absent() {
        let cmds = load_user_commands();
        // Should not panic; returns whatever exists (may be empty or not).
        let _ = cmds;
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
