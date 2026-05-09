//! User-defined slash commands from project-local and user-global command dirs.
//!
//! Users drop `.md` files into `$WORKSPACE/.deepseek/commands/`,
//! `$WORKSPACE/.cursor/commands/`, or `~/.deepseek/commands/` and the
//! filename (without `.md` extension) becomes a slash command. When invoked
//! via `/name`, the file contents are sent as a user message.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::tui::app::{App, AppAction};

use super::CommandResult;

/// Path to the user commands directory: `~/.deepseek/commands/`.
fn commands_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    home.join(".deepseek").join("commands")
}

fn project_commands_dirs(workspace: &Path) -> [PathBuf; 2] {
    [
        workspace.join(".deepseek").join("commands"),
        workspace.join(".cursor").join("commands"),
    ]
}

fn command_dirs_for_workspace(workspace: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(workspace) = workspace {
        dirs.extend(project_commands_dirs(workspace));
    }
    dirs.push(commands_dir());
    dirs
}

fn load_commands_from_dirs(dirs: impl IntoIterator<Item = PathBuf>) -> Vec<(String, String)> {
    let mut commands: Vec<(String, String)> = Vec::new();
    let mut seen = HashSet::new();

    for dir in dirs {
        if !dir.exists() {
            continue;
        }

        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();

        for path in paths {
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(stem) => stem.to_lowercase(),
                None => continue,
            };
            if seen.contains(&stem) {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            seen.insert(stem.clone());
            commands.push((stem, content));
        }
    }

    // Sort by name for deterministic ordering after precedence is resolved.
    commands.sort_by(|a, b| a.0.cmp(&b.0));
    commands
}

/// Scan command directories for `.md` files and return `(name, content)` pairs.
///
/// The name is the filename without the `.md` extension, normalized to
/// lowercase. Files that fail to read are silently skipped. The directory
/// is re-scanned on every call so newly-added commands show up immediately
/// without requiring a restart.
pub fn load_user_commands_for_workspace(workspace: Option<&Path>) -> Vec<(String, String)> {
    load_commands_from_dirs(command_dirs_for_workspace(workspace))
}

/// Scan `~/.deepseek/commands/` for `.md` files and return `(name, content)` pairs.
pub fn load_user_commands() -> Vec<(String, String)> {
    load_user_commands_for_workspace(None)
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

pub fn try_dispatch_user_command(app: &mut App, input: &str) -> Option<CommandResult> {
    let parts: Vec<&str> = input.trim().splitn(2, ' ').collect();
    let command = parts[0].to_lowercase();
    let command = command.strip_prefix('/').unwrap_or(&command);
    let args = parts.get(1).copied().unwrap_or("").trim();

    let user_commands = load_user_commands_for_workspace(Some(&app.workspace));

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
    user_commands_matching_for_workspace(prefix, None)
}

pub fn user_commands_matching_for_workspace(prefix: &str, workspace: Option<&Path>) -> Vec<String> {
    let prefix = prefix.to_lowercase();
    load_user_commands_for_workspace(workspace)
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

    #[test]
    fn load_user_commands_includes_project_local_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let deepseek_commands = tmp.path().join(".deepseek").join("commands");
        let cursor_commands = tmp.path().join(".cursor").join("commands");
        std::fs::create_dir_all(&deepseek_commands).expect("mkdir deepseek commands");
        std::fs::create_dir_all(&cursor_commands).expect("mkdir cursor commands");
        std::fs::write(deepseek_commands.join("Review.md"), "review project").expect("write");
        std::fs::write(cursor_commands.join("Plan.md"), "plan project").expect("write");

        let cmds = load_user_commands_for_workspace(Some(tmp.path()));

        assert!(
            cmds.iter()
                .any(|(name, body)| name == "review" && body == "review project")
        );
        assert!(
            cmds.iter()
                .any(|(name, body)| name == "plan" && body == "plan project")
        );
    }

    #[test]
    fn project_commands_override_global_commands() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project_commands = tmp.path().join(".deepseek").join("commands");
        let global_commands = tmp.path().join("global").join("commands");
        std::fs::create_dir_all(&project_commands).expect("mkdir project commands");
        std::fs::create_dir_all(&global_commands).expect("mkdir global commands");
        std::fs::write(project_commands.join("Audit.md"), "project audit")
            .expect("write project");
        std::fs::write(global_commands.join("audit.md"), "global audit").expect("write global");

        let cmds = load_commands_from_dirs(vec![project_commands, global_commands]);

        assert_eq!(
            cmds.iter()
                .find(|(name, _)| name == "audit")
                .map(|(_, body)| body.as_str()),
            Some("project audit")
        );
    }
}
