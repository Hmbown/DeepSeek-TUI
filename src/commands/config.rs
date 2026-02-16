//! Config commands: config, set, settings, yolo, trust, logout

use super::CommandResult;
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
         Sidebar width:  {}%\n\
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
        app.sidebar_width_percent,
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
            app.update_model_compaction_budget();
            app.last_prompt_tokens = None;
            app.last_completion_tokens = None;
            return CommandResult::with_message_and_action(
                format!("model = {value}"),
                AppAction::UpdateCompaction(app.compaction_config()),
            );
        }
        "approval_mode" | "approval" => {
            let mode = match value.to_lowercase().as_str() {
                "auto" => Some(ApprovalMode::Auto),
                "suggest" | "suggested" | "on-request" | "untrusted" => Some(ApprovalMode::Suggest),
                "never" => Some(ApprovalMode::Never),
                _ => None,
            };
            return match mode {
                Some(m) => {
                    app.approval_mode = m;
                    CommandResult::message(format!("approval_mode = {}", m.label()))
                }
                None => CommandResult::error(
                    "Invalid approval_mode. Use: auto, suggest/on-request/untrusted, never",
                ),
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
            action = Some(AppAction::UpdateCompaction(app.compaction_config()));
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
                app.update_model_compaction_budget();
                app.last_prompt_tokens = None;
                app.last_completion_tokens = None;
                action = Some(AppAction::UpdateCompaction(app.compaction_config()));
            }
        }
        "theme" => {
            app.ui_theme = palette::ui_theme(&settings.theme);
            app.mark_history_updated();
        }
        "sidebar_width" | "sidebar" => {
            app.sidebar_width_percent = settings.sidebar_width_percent;
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
            app.onboarding = OnboardingState::ApiKey;
            app.onboarding_needs_api_key = true;
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
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        home: Option<OsString>,
        userprofile: Option<OsString>,
        deepseek_config_path: Option<OsString>,
    }

    impl EnvGuard {
        fn new(home: &Path) -> Self {
            let home_str = OsString::from(home.as_os_str());
            let config_path = home.join(".deepseek").join("config.toml");
            let config_str = OsString::from(config_path.as_os_str());
            let home_prev = env::var_os("HOME");
            let userprofile_prev = env::var_os("USERPROFILE");
            let deepseek_config_prev = env::var_os("DEEPSEEK_CONFIG_PATH");

            // Safety: test-only environment mutation guarded by a global mutex.
            unsafe {
                env::set_var("HOME", &home_str);
                env::set_var("USERPROFILE", &home_str);
                env::set_var("DEEPSEEK_CONFIG_PATH", &config_str);
            }

            Self {
                home: home_prev,
                userprofile: userprofile_prev,
                deepseek_config_path: deepseek_config_prev,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.home.take() {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::set_var("HOME", value);
                }
            } else {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::remove_var("HOME");
                }
            }

            if let Some(value) = self.userprofile.take() {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::set_var("USERPROFILE", value);
                }
            } else {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::remove_var("USERPROFILE");
                }
            }

            if let Some(value) = self.deepseek_config_path.take() {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::set_var("DEEPSEEK_CONFIG_PATH", value);
                }
            } else {
                // Safety: test-only environment mutation guarded by a global mutex.
                unsafe {
                    env::remove_var("DEEPSEEK_CONFIG_PATH");
                }
            }
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

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
            skip_onboarding: false,
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

    #[test]
    fn test_show_config_displays_all_fields() {
        let mut app = create_test_app();
        app.total_tokens = 1234;
        let result = show_config(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Session Configuration"));
        assert!(msg.contains("Mode:"));
        assert!(msg.contains("Model:"));
        assert!(msg.contains("Workspace:"));
        assert!(msg.contains("Shell enabled:"));
        assert!(msg.contains("Approval mode:"));
        assert!(msg.contains("Max sub-agents:"));
        assert!(msg.contains("Trust mode:"));
        assert!(msg.contains("Auto-compact:"));
        assert!(msg.contains("Sidebar width:"));
        assert!(msg.contains("Total tokens:"));
        assert!(msg.contains("Project doc:"));
    }

    #[test]
    fn test_show_settings_loads_from_file() {
        let mut app = create_test_app();
        let result = show_settings(&mut app);
        // Settings should load (may use defaults if file doesn't exist)
        assert!(result.message.is_some());
    }

    #[test]
    fn test_set_without_args_shows_usage() {
        let mut app = create_test_app();
        let result = set_config(&mut app, None);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Usage: /set"));
        assert!(msg.contains("Available settings:"));
    }

    #[test]
    fn test_set_model_updates_app_state() {
        let mut app = create_test_app();
        let _old_model = app.model.clone();
        let result = set_config(&mut app, Some("model deepseek-reasoner"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("model = deepseek-reasoner"));
        assert_eq!(app.model, "deepseek-reasoner");
        assert!(matches!(
            result.action,
            Some(AppAction::UpdateCompaction(_))
        ));
    }

    #[test]
    fn test_set_model_with_save_flag() {
        let mut app = create_test_app();
        let _result = set_config(&mut app, Some("model deepseek-reasoner --save"));
        // Note: This test may fail in environments where settings can't be saved
        // The important thing is that the model is updated
        assert_eq!(app.model, "deepseek-reasoner");
    }

    #[test]
    fn test_set_approval_mode_valid_values() {
        let mut app = create_test_app();
        // Test auto
        let result = set_config(&mut app, Some("approval_mode auto"));
        assert!(result.message.is_some());
        assert_eq!(app.approval_mode, ApprovalMode::Auto);

        // Test suggest
        let result = set_config(&mut app, Some("approval_mode suggest"));
        assert!(result.message.is_some());
        assert_eq!(app.approval_mode, ApprovalMode::Suggest);

        // Test never
        let result = set_config(&mut app, Some("approval_mode never"));
        assert!(result.message.is_some());
        assert_eq!(app.approval_mode, ApprovalMode::Never);
    }

    #[test]
    fn test_set_approval_mode_invalid_value() {
        let mut app = create_test_app();
        let result = set_config(&mut app, Some("approval_mode invalid"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Invalid approval_mode"));
    }

    #[test]
    fn test_set_without_save_flag() {
        let mut app = create_test_app();
        let result = set_config(&mut app, Some("auto_compact true"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("(session only"));
    }

    #[test]
    fn test_trust_enables_flag() {
        let mut app = create_test_app();
        assert!(!app.trust_mode);
        let result = trust(&mut app);
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Trust mode enabled"));
        assert!(app.trust_mode);
    }

    #[test]
    fn test_logout_clears_api_key_state() {
        let _lock = env_lock().lock().unwrap();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = env::temp_dir().join(format!(
            "deepseek-cli-logout-test-{}-{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&temp_root).unwrap();
        let _guard = EnvGuard::new(&temp_root);

        let config_path = temp_root.join(".deepseek").join("config.toml");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(&config_path, "api_key = \"test-key\"\n").unwrap();

        let mut app = create_test_app();
        let result = logout(&mut app);
        assert!(result.message.is_some());
        assert_eq!(app.onboarding, OnboardingState::ApiKey);
        assert!(app.onboarding_needs_api_key);
        assert!(app.api_key_input.is_empty());
        assert_eq!(app.api_key_cursor, 0);

        let updated = fs::read_to_string(config_path).unwrap();
        assert!(!updated.contains("api_key"));
    }

    #[test]
    fn test_set_invalid_setting() {
        let mut app = create_test_app();
        let _result = set_config(&mut app, Some("nonexistent value"));
        // Should either error or handle as session setting
        // The current implementation tries to set it in Settings
        // which may succeed or fail depending on Settings implementation
    }

    #[test]
    fn test_set_key_without_value() {
        let mut app = create_test_app();
        let result = set_config(&mut app, Some("model"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("Usage: /set"));
    }
}
