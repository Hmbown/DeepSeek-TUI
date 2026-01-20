//! Config commands: config, set, settings, yolo, trust, logout

use super::CommandResult;
use crate::compaction::CompactionConfig;
use crate::config::clear_api_key;
use crate::palette;
use crate::settings::Settings;
use crate::tui::app::{App, AppAction, AppMode, OnboardingState};
use crate::tui::approval::ApprovalMode;

/// Display current configuration
pub fn show_config(app: &mut App) -> CommandResult {
    let has_project_doc = app.project_doc.is_some();
    let config_info = format!(
        "Session Configuration:\n\
         ─────────────────────────────\n\
         Mode:           {}\n\
         Model:          {}\n\
         Workspace:      {}\n\
         Shell enabled:  {}\n\
         Approval mode:  {}\n\
         Max sub-agents: {}\n\
         Trust mode:     {}\n\
         Auto-compact:   {}\n\
         Total tokens:   {}\n\
         Project doc:    {}",
        app.mode.label(),
        app.model,
        app.workspace.display(),
        if app.allow_shell { "yes" } else { "no" },
        app.approval_mode.label(),
        app.max_subagents,
        if app.trust_mode { "yes" } else { "no" },
        if app.auto_compact { "yes" } else { "no" },
        app.total_tokens,
        if has_project_doc {
            "loaded"
        } else {
            "not found"
        },
    );
    CommandResult::message(config_info)
}

/// Show persistent settings
pub fn show_settings(_app: &mut App) -> CommandResult {
    match Settings::load() {
        Ok(settings) => CommandResult::message(settings.display()),
        Err(e) => CommandResult::error(format!("Failed to load settings: {e}")),
    }
}

/// Modify a setting at runtime
pub fn set_config(app: &mut App, args: Option<&str>) -> CommandResult {
    let Some(args) = args else {
        let available = Settings::available_settings()
            .iter()
            .map(|(k, d)| format!("  {k}: {d}"))
            .collect::<Vec<_>>()
            .join("\n");
        return CommandResult::message(format!(
            "Usage: /set <key> <value>\n\n\
             Available settings:\n{available}\n\n\
             Session-only settings:\n  \
             model: Current model\n  \
             approval_mode: auto | suggest | never\n\n\
             Add --save to persist to settings file."
        ));
    };

    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return CommandResult::error("Usage: /set <key> <value>");
    }

    let key = parts[0].to_lowercase();
    let (value, should_save) = if parts[1].ends_with(" --save") {
        (parts[1].trim_end_matches(" --save").trim(), true)
    } else {
        (parts[1].trim(), false)
    };

    // Handle session-only settings first
    match key.as_str() {
        "model" => {
            app.model = value.to_string();
            return CommandResult::message(format!("model = {value}"));
        }
        "approval_mode" | "approval" => {
            let mode = match value.to_lowercase().as_str() {
                "auto" => Some(ApprovalMode::Auto),
                "suggest" | "suggested" => Some(ApprovalMode::Suggest),
                "never" => Some(ApprovalMode::Never),
                _ => None,
            };
            return match mode {
                Some(m) => {
                    app.approval_mode = m;
                    CommandResult::message(format!("approval_mode = {}", m.label()))
                }
                None => CommandResult::error("Invalid approval_mode. Use: auto, suggest, never"),
            };
        }
        _ => {}
    }

    // Load and update persistent settings
    let mut settings = match Settings::load() {
        Ok(s) => s,
        Err(e) => return CommandResult::error(format!("Failed to load settings: {e}")),
    };

    if let Err(e) = settings.set(&key, value) {
        return CommandResult::error(format!("{e}"));
    }

    // Apply to current session
    let mut action = None;
    match key.as_str() {
        "auto_compact" | "compact" => {
            app.auto_compact = settings.auto_compact;
            let mut compaction = CompactionConfig::default();
            compaction.enabled = app.auto_compact;
            compaction.token_threshold = app.compact_threshold;
            compaction.model = app.model.clone();
            action = Some(AppAction::UpdateCompaction(compaction));
        }
        "auto_rlm" => {
            app.auto_rlm = settings.auto_rlm;
        }
        "show_thinking" | "thinking" => {
            app.show_thinking = settings.show_thinking;
            app.mark_history_updated();
        }
        "show_tool_details" | "tool_details" => {
            app.show_tool_details = settings.show_tool_details;
            app.mark_history_updated();
        }
        "default_mode" | "mode" => {
            let mode = match settings.default_mode.as_str() {
                "agent" | "normal" => AppMode::Agent,
                "plan" => AppMode::Plan,
                "yolo" => AppMode::Yolo,
                "rlm" => AppMode::Rlm,
                "duo" => AppMode::Duo,
                _ => AppMode::Agent,
            };
            app.set_mode(mode);
        }
        "max_history" | "history" => {
            app.max_input_history = settings.max_input_history;
        }
        "default_model" => {
            if let Some(ref model) = settings.default_model {
                app.model.clone_from(model);
            }
        }
        "theme" => {
            app.ui_theme = palette::ui_theme(&settings.theme);
            app.mark_history_updated();
        }
        _ => {}
    }

    // Save if requested
    let message = if should_save {
        if let Err(e) = settings.save() {
            return CommandResult::error(format!("Failed to save: {e}"));
        }
        format!("{key} = {value} (saved)")
    } else {
        format!("{key} = {value} (session only, add --save to persist)")
    };

    CommandResult {
        message: Some(message),
        action,
    }
}

/// Enable YOLO mode (shell + trust + auto-approve)
pub fn yolo(app: &mut App) -> CommandResult {
    app.set_mode(AppMode::Yolo);
    CommandResult::message("YOLO mode enabled - shell + trust + auto-approve!")
}

/// Enable trust mode (file access outside workspace)
pub fn trust(app: &mut App) -> CommandResult {
    app.trust_mode = true;
    CommandResult::message("Trust mode enabled - can access files outside workspace")
}

/// Logout - clear API key and return to onboarding
pub fn logout(app: &mut App) -> CommandResult {
    match clear_api_key() {
        Ok(()) => {
            app.onboarding = OnboardingState::Welcome;
            app.api_key_input.clear();
            app.api_key_cursor = 0;
            CommandResult::message("Logged out. Enter a new API key to continue.")
        }
        Err(e) => CommandResult::error(format!("Failed to clear API key: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::{App, TuiOptions};
    use crate::tui::approval::ApprovalMode;
    use std::path::PathBuf;

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "test-model".to_string(),
            workspace: PathBuf::from("."),
            allow_shell: false,
            use_alt_screen: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            yolo: false,
            resume_session_id: None,
        };
        App::new(options, &Config::default())
    }

    #[test]
    fn test_yolo_command_sets_all_flags() {
        let mut app = create_test_app();
        let _ = yolo(&mut app);
        assert!(app.allow_shell);
        assert!(app.trust_mode);
        assert!(app.yolo);
        assert_eq!(app.approval_mode, ApprovalMode::Auto);
        assert_eq!(app.mode, AppMode::Yolo);
    }
}
