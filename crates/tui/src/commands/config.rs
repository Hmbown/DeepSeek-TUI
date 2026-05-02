//! Config commands: config, settings, mode switches, trust, logout

use std::path::{Path, PathBuf};

use super::CommandResult;
use crate::config::{COMMON_DEEPSEEK_MODELS, clear_api_key, normalize_model_name};
use crate::localization::resolve_locale;
use crate::settings::Settings;
use crate::tui::app::{App, AppAction, AppMode, OnboardingState, SidebarFocus};
use crate::tui::approval::ApprovalMode;

/// Open the interactive config editor modal, or handle `/config <key> <value>`.
pub fn show_config(_app: &mut App) -> CommandResult {
    CommandResult::action(AppAction::OpenConfigView)
}

/// Dispatch `/config` with optional args.
///
/// - `/config` (no args) — opens the interactive editor modal.
/// - `/config <key>` — shows the current value of a setting.
/// - `/config <key> <value>` — sets a runtime value (session only, no --save).
pub fn config_command(app: &mut App, arg: Option<&str>) -> CommandResult {
    let raw = arg.map(str::trim).unwrap_or("");
    if raw.is_empty() {
        return show_config(app);
    }
    let parts: Vec<&str> = raw.splitn(2, ' ').collect();
    if parts.len() == 1 {
        // `/config <key>` — show current value
        show_single_setting(app, parts[0])
    } else {
        // `/config <key> <value>` — set value
        set_config_value(app, parts[0], parts[1], false)
    }
}

/// Show the current value of a single setting.
fn show_single_setting(app: &App, key: &str) -> CommandResult {
    let key = key.to_lowercase();
    fn locale_display(l: crate::localization::Locale) -> &'static str {
        match l {
            crate::localization::Locale::En => "en",
            crate::localization::Locale::ZhHans => "zh-Hans",
            crate::localization::Locale::Ja => "ja",
            crate::localization::Locale::PtBr => "pt-BR",
        }
    }
    fn density_display(d: crate::tui::app::ComposerDensity) -> &'static str {
        match d {
            crate::tui::app::ComposerDensity::Compact => "compact",
            crate::tui::app::ComposerDensity::Comfortable => "comfortable",
            crate::tui::app::ComposerDensity::Spacious => "spacious",
        }
    }
    fn spacing_display(s: crate::tui::app::TranscriptSpacing) -> &'static str {
        match s {
            crate::tui::app::TranscriptSpacing::Compact => "compact",
            crate::tui::app::TranscriptSpacing::Comfortable => "comfortable",
            crate::tui::app::TranscriptSpacing::Spacious => "spacious",
        }
    }
    let value = match key.as_str() {
        "model" => Some(app.model.clone()),
        "approval_mode" | "approval" => Some(app.approval_mode.label().to_string()),
        "locale" | "language" => Some(locale_display(app.ui_locale).to_string()),
        "auto_compact" | "compact" => {
            Some(if app.auto_compact { "true" } else { "false" }.to_string())
        }
        "calm_mode" | "calm" => Some(if app.calm_mode { "true" } else { "false" }.to_string()),
        "show_thinking" | "thinking" => {
            Some(if app.show_thinking { "true" } else { "false" }.to_string())
        }
        "mode" | "default_mode" => Some(app.mode.as_setting().to_string()),
        "max_history" | "history" => Some(app.max_input_history.to_string()),
        "sidebar_width" | "sidebar" => Some(app.sidebar_width_percent.to_string()),
        "sidebar_focus" | "focus" => Some(app.sidebar_focus.as_setting().to_string()),
        "composer_density" | "composer" => Some(density_display(app.composer_density).to_string()),
        "composer_border" | "border" => {
            Some(if app.composer_border { "true" } else { "false" }.to_string())
        }
        "transcript_spacing" | "spacing" => {
            Some(spacing_display(app.transcript_spacing).to_string())
        }
        _ => {
            let known = Settings::available_settings()
                .iter()
                .any(|(k, _)| k == &key);
            if known {
                Some("(see /settings for current value)".to_string())
            } else {
                None
            }
        }
    };
    match value {
        Some(v) => CommandResult::message(format!("{key} = {v}")),
        None => CommandResult::error(format!(
            "Unknown setting '{key}'. See `/help config` for available settings."
        )),
    }
}

/// Show persistent settings
pub fn show_settings(app: &mut App) -> CommandResult {
    match Settings::load() {
        Ok(settings) => CommandResult::message(settings.display(app.ui_locale)),
        Err(e) => CommandResult::error(format!("Failed to load settings: {e}")),
    }
}

/// Open the `/statusline` multi-select picker for configuring footer items.
pub fn status_line(_app: &mut App) -> CommandResult {
    CommandResult::action(AppAction::OpenStatusPicker)
}

/// Persist `tui.status_items` to `~/.deepseek/config.toml` without disturbing
/// the rest of the file. We round-trip through `toml::Value` so any keys we
/// don't know about (provider blocks, MCP, etc.) survive the write
/// untouched.
///
/// Returns the path written so the caller can surface it in a status toast.
pub fn persist_status_items(items: &[crate::config::StatusItem]) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    use std::fs;

    let path = config_toml_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let mut doc: toml::Value = if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse config at {}", path.display()))?
    } else {
        toml::Value::Table(toml::value::Table::new())
    };

    let table = doc
        .as_table_mut()
        .context("config.toml root must be a table")?;
    let tui_entry = table
        .entry("tui".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let tui_table = tui_entry
        .as_table_mut()
        .context("`tui` section in config.toml must be a table")?;
    let array = items
        .iter()
        .map(|item| toml::Value::String(item.key().to_string()))
        .collect::<Vec<_>>();
    tui_table.insert("status_items".to_string(), toml::Value::Array(array));

    let body = toml::to_string_pretty(&doc).context("failed to serialize config.toml")?;
    fs::write(&path, body)
        .with_context(|| format!("failed to write config at {}", path.display()))?;
    Ok(path)
}

fn persist_root_string_key(key: &str, value: &str) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    use std::fs;

    let path = config_toml_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let mut doc: toml::Value = if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse config at {}", path.display()))?
    } else {
        toml::Value::Table(toml::value::Table::new())
    };
    let table = doc
        .as_table_mut()
        .context("config.toml root must be a table")?;
    table.insert(key.to_string(), toml::Value::String(value.to_string()));
    let body = toml::to_string_pretty(&doc).context("failed to serialize config.toml")?;
    fs::write(&path, body)
        .with_context(|| format!("failed to write config at {}", path.display()))?;
    Ok(path)
}

/// Resolve the path to `~/.deepseek/config.toml` (or
/// `$DEEPSEEK_CONFIG_PATH`). Mirrors what `Config::load` accepts so we
/// never write to a different file than the one we read.
fn config_toml_path() -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    if let Ok(env) = std::env::var("DEEPSEEK_CONFIG_PATH") {
        let trimmed = env.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let home = dirs::home_dir().context("failed to resolve home directory for config.toml path")?;
    Ok(home.join(".deepseek").join("config.toml"))
}

/// Modify a setting at runtime
pub fn set_config_value(app: &mut App, key: &str, value: &str, persist: bool) -> CommandResult {
    let key = key.to_lowercase();

    match key.as_str() {
        "model" => {
            let Some(model) = normalize_model_name(value) else {
                return CommandResult::error(format!(
                    "Invalid model '{value}'. Expected a DeepSeek model ID. Common models: {}",
                    COMMON_DEEPSEEK_MODELS.join(", ")
                ));
            };
            app.model = model.clone();
            app.update_model_compaction_budget();
            app.last_prompt_tokens = None;
            app.last_completion_tokens = None;
            return CommandResult::with_message_and_action(
                format!("model = {model}"),
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
        "mcp_config_path" | "mcp" => {
            if value.trim().is_empty() {
                return CommandResult::error("mcp_config_path cannot be empty");
            }
            app.mcp_config_path = PathBuf::from(expand_tilde(value));
            app.mcp_restart_required = true;
            let message = if persist {
                match persist_root_string_key("mcp_config_path", value) {
                    Ok(path) => format!(
                        "mcp_config_path = {} (saved to {}; restart required for MCP tool pool)",
                        app.mcp_config_path.display(),
                        path.display()
                    ),
                    Err(err) => return CommandResult::error(format!("Failed to save: {err}")),
                }
            } else {
                format!(
                    "mcp_config_path = {} (session only; restart required for MCP tool pool)",
                    app.mcp_config_path.display()
                )
            };
            return CommandResult::message(message);
        }
        _ => {}
    }

    let mut settings = match Settings::load() {
        Ok(s) => s,
        Err(e) if !persist => {
            app.status_message = Some(format!(
                "Settings unavailable; applying session-only override ({e})"
            ));
            Settings::default()
        }
        Err(e) => return CommandResult::error(format!("Failed to load settings: {e}")),
    };

    if let Err(e) = settings.set(&key, value) {
        return CommandResult::error(format!("{e}"));
    }

    let mut action = None;
    match key.as_str() {
        "auto_compact" | "compact" => {
            app.auto_compact = settings.auto_compact;
            action = Some(AppAction::UpdateCompaction(app.compaction_config()));
        }
        "calm_mode" | "calm" => {
            app.calm_mode = settings.calm_mode;
            app.mark_history_updated();
        }
        "low_motion" | "motion" => {
            app.low_motion = settings.low_motion;
            app.needs_redraw = true;
        }
        "show_thinking" | "thinking" => {
            app.show_thinking = settings.show_thinking;
            app.mark_history_updated();
        }
        "show_tool_details" | "tool_details" => {
            app.show_tool_details = settings.show_tool_details;
            app.mark_history_updated();
        }
        "locale" | "language" => {
            app.ui_locale = resolve_locale(&settings.locale);
            app.needs_redraw = true;
        }
        "composer_density" | "composer" => {
            app.composer_density =
                crate::tui::app::ComposerDensity::from_setting(&settings.composer_density);
            app.needs_redraw = true;
        }
        "composer_border" | "border" => {
            app.composer_border = settings.composer_border;
            app.needs_redraw = true;
        }
        "paste_burst_detection" | "paste_burst" => {
            app.use_paste_burst_detection = settings.paste_burst_detection;
            if !app.use_paste_burst_detection {
                app.paste_burst.clear_after_explicit_paste();
            }
        }
        "transcript_spacing" | "spacing" => {
            app.transcript_spacing =
                crate::tui::app::TranscriptSpacing::from_setting(&settings.transcript_spacing);
            app.mark_history_updated();
        }
        "default_mode" | "mode" => {
            let mode = AppMode::from_setting(&settings.default_mode);
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
        "sidebar_width" | "sidebar" => {
            app.sidebar_width_percent = settings.sidebar_width_percent;
            app.mark_history_updated();
        }
        "sidebar_focus" | "focus" => {
            app.set_sidebar_focus(SidebarFocus::from_setting(&settings.sidebar_focus));
        }
        _ => {}
    }

    let display_value = match key.as_str() {
        "default_mode" | "mode" => settings.default_mode.clone(),
        _ => value.to_string(),
    };

    let message = if persist {
        if let Err(e) = settings.save() {
            return CommandResult::error(format!("Failed to save: {e}"));
        }
        format!("{key} = {display_value} (saved)")
    } else {
        format!("{key} = {display_value} (session only, add --save to persist)")
    };

    CommandResult {
        message: Some(message),
        action,
    }
}

/// Modify a setting at runtime
#[allow(dead_code)]
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

    set_config_value(app, &key, value, should_save)
}

/// Enable YOLO mode (shell + trust + auto-approve)
pub fn yolo(app: &mut App) -> CommandResult {
    app.set_mode(AppMode::Yolo);
    CommandResult::message("YOLO mode enabled - shell + trust + auto-approve!")
}

/// Legacy alias for the removed normal mode.
pub fn normal_mode(app: &mut App) -> CommandResult {
    app.set_mode(AppMode::Agent);
    CommandResult::message("Normal mode was removed. Switched to Agent mode.")
}

/// Enable agent mode (autonomous tool use with approvals)
pub fn agent_mode(app: &mut App) -> CommandResult {
    app.set_mode(AppMode::Agent);
    CommandResult::message("Agent mode enabled.")
}

/// Enable plan mode (tool planning, then choose execution route)
pub fn plan_mode(app: &mut App) -> CommandResult {
    app.set_mode(AppMode::Plan);
    CommandResult::message(
        "Plan mode enabled. Describe your goal and I will create a plan before execution.",
    )
}

/// Manage workspace-level trust and the per-path allowlist.
///
/// Subcommands:
/// - `/trust`            – show current state and trusted external paths
/// - `/trust on`         – legacy: trust the entire workspace (turn off all path checks)
/// - `/trust off`        – disable workspace-level trust mode
/// - `/trust add <path>` – add a directory to the allowlist (#29)
/// - `/trust remove <path>` (alias `rm`) – remove a path from the allowlist
/// - `/trust list`       – list trusted external paths for this workspace
pub fn trust(app: &mut App, arg: Option<&str>) -> CommandResult {
    let raw = arg.map(str::trim).unwrap_or("");
    let mut parts = raw.splitn(2, char::is_whitespace);
    let sub = parts.next().unwrap_or("").to_lowercase();
    let rest = parts.next().map(str::trim).unwrap_or("");
    let workspace = app.workspace.clone();

    match sub.as_str() {
        "" | "status" | "list" => trust_status(&workspace, app, sub == "list"),
        "on" | "enable" | "yes" | "y" => {
            app.trust_mode = true;
            CommandResult::message(
                "Workspace trust mode enabled — agent file tools can now read/write any path. \
                 Use `/trust off` to revert; prefer `/trust add <path>` for a narrower opt-in.",
            )
        }
        "off" | "disable" | "no" | "n" => {
            app.trust_mode = false;
            CommandResult::message("Workspace trust mode disabled.")
        }
        "add" => trust_add(&workspace, rest),
        "remove" | "rm" | "del" | "delete" => trust_remove(&workspace, rest),
        other => CommandResult::error(format!(
            "Unknown /trust action `{other}`. Use `/trust`, `/trust on|off`, `/trust add <path>`, or `/trust remove <path>`."
        )),
    }
}

fn trust_status(workspace: &Path, app: &App, force_paths: bool) -> CommandResult {
    let trust = crate::workspace_trust::WorkspaceTrust::load_for(workspace);
    let mut lines = Vec::new();
    lines.push(format!(
        "Workspace trust mode: {}",
        if app.trust_mode {
            "enabled"
        } else {
            "disabled"
        }
    ));
    if trust.paths().is_empty() {
        if force_paths {
            lines.push("No external paths trusted from this workspace.".to_string());
        } else {
            lines.push(
                "No external paths trusted yet. Use `/trust add <path>` to allow a directory."
                    .to_string(),
            );
        }
    } else {
        lines.push(format!("Trusted external paths ({}):", trust.paths().len()));
        for path in trust.paths() {
            lines.push(format!("  • {}", path.display()));
        }
    }
    CommandResult::message(lines.join("\n"))
}

fn trust_add(workspace: &Path, raw: &str) -> CommandResult {
    if raw.is_empty() {
        return CommandResult::error(
            "Usage: /trust add <path>. Supply an absolute path or a path relative to the workspace.",
        );
    }
    let path = PathBuf::from(expand_tilde(raw));
    if !path.exists() {
        return CommandResult::error(format!(
            "Path not found: {} — supply an existing directory or file.",
            path.display()
        ));
    }
    match crate::workspace_trust::add(workspace, &path) {
        Ok(stored) => CommandResult::message(format!(
            "Added to trust list for this workspace: {}",
            stored.display()
        )),
        Err(err) => CommandResult::error(format!("Failed to update trust list: {err}")),
    }
}

fn trust_remove(workspace: &Path, raw: &str) -> CommandResult {
    if raw.is_empty() {
        return CommandResult::error("Usage: /trust remove <path>");
    }
    let path = PathBuf::from(expand_tilde(raw));
    match crate::workspace_trust::remove(workspace, &path) {
        Ok(true) => CommandResult::message(format!("Removed from trust list: {}", path.display())),
        Ok(false) => CommandResult::message(format!("Not in trust list: {}", path.display())),
        Err(err) => CommandResult::error(format!("Failed to update trust list: {err}")),
    }
}

fn expand_tilde(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().into_owned();
    } else if raw == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home.to_string_lossy().into_owned();
    }
    raw.to_string()
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
    use crate::test_support::lock_test_env;
    use crate::tui::app::{App, TuiOptions};
    use crate::tui::approval::ApprovalMode;
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
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

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "test-model".to_string(),
            workspace: PathBuf::from("."),
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
    fn test_mode_switch_commands() {
        let mut app = create_test_app();
        let _ = normal_mode(&mut app);
        assert_eq!(app.mode, AppMode::Agent);
        let _ = agent_mode(&mut app);
        assert_eq!(app.mode, AppMode::Agent);
        let _ = plan_mode(&mut app);
        assert_eq!(app.mode, AppMode::Plan);
    }

    #[test]
    fn test_show_config_opens_config_editor() {
        let mut app = create_test_app();
        app.total_tokens = 1234;
        let result = show_config(&mut app);
        assert!(result.message.is_none());
        assert!(matches!(result.action, Some(AppAction::OpenConfigView)));
    }

    #[test]
    fn test_show_settings_loads_from_file() {
        let _lock = lock_test_env();
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
        let result = set_config(&mut app, Some("model deepseek-v4-flash"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("model = deepseek-v4-flash"));
        assert_eq!(app.model, "deepseek-v4-flash");
        assert!(matches!(
            result.action,
            Some(AppAction::UpdateCompaction(_))
        ));
    }

    #[test]
    fn test_set_model_accepts_future_deepseek_model_id() {
        let mut app = create_test_app();
        let result = set_config(&mut app, Some("model deepseek-v4"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("model = deepseek-v4"));
        assert_eq!(app.model, "deepseek-v4");
    }

    #[test]
    fn test_set_model_with_save_flag() {
        let mut app = create_test_app();
        let _result = set_config(&mut app, Some("model deepseek-v4-flash --save"));
        // Note: This test may fail in environments where settings can't be saved
        // The important thing is that the model is updated
        assert_eq!(app.model, "deepseek-v4-flash");
    }

    #[test]
    fn test_set_default_mode_normal_save_reports_normalized_value() {
        let _lock = lock_test_env();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = env::temp_dir().join(format!(
            "deepseek-tui-default-mode-test-{}-{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&temp_root).unwrap();
        let _guard = EnvGuard::new(&temp_root);

        let mut app = create_test_app();
        let result = set_config(&mut app, Some("default_mode normal --save"));
        let msg = result.message.unwrap();
        assert_eq!(msg, "default_mode = agent (saved)");
        assert_eq!(app.mode, AppMode::Agent);

        let settings_path = Settings::path().unwrap();
        let saved = fs::read_to_string(settings_path).unwrap();
        assert!(saved.contains("default_mode = \"agent\""));
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
        let _lock = lock_test_env();
        let mut app = create_test_app();
        let result = set_config(&mut app, Some("auto_compact true"));
        assert!(result.message.is_some());
        let msg = result.message.unwrap();
        assert!(msg.contains("(session only"));
    }

    #[test]
    fn test_set_composer_border_updates_live_app() {
        let _lock = lock_test_env();
        let mut app = create_test_app();
        app.composer_border = true;

        let result = set_config(&mut app, Some("composer_border false"));

        assert!(result.message.is_some());
        assert!(!app.composer_border);
        assert!(app.needs_redraw);
    }

    #[test]
    fn test_trust_on_enables_flag() {
        let mut app = create_test_app();
        assert!(!app.trust_mode);
        let result = trust(&mut app, Some("on"));
        let msg = result.message.expect("message");
        assert!(msg.contains("Workspace trust mode enabled"));
        assert!(app.trust_mode);
    }

    #[test]
    fn test_trust_status_default_lists_state() {
        let mut app = create_test_app();
        let result = trust(&mut app, None);
        let msg = result.message.expect("status message");
        assert!(msg.contains("Workspace trust mode"));
    }

    #[test]
    fn test_trust_add_requires_path() {
        let mut app = create_test_app();
        let result = trust(&mut app, Some("add"));
        let msg = result.message.expect("error message");
        assert!(msg.starts_with("Error:"), "got {msg:?}");
    }

    #[test]
    fn test_logout_clears_api_key_state() {
        let _lock = lock_test_env();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = env::temp_dir().join(format!(
            "deepseek-tui-logout-test-{}-{}",
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
        let _lock = lock_test_env();
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

    #[test]
    fn persist_status_items_writes_tui_section_to_config_toml() {
        let _lock = lock_test_env();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = env::temp_dir().join(format!(
            "deepseek-statusline-persist-{}-{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&temp_root).unwrap();
        let _guard = EnvGuard::new(&temp_root);

        let items = vec![
            crate::config::StatusItem::Mode,
            crate::config::StatusItem::Model,
            crate::config::StatusItem::Cost,
        ];

        let path = persist_status_items(&items).expect("persist should succeed");
        let body = fs::read_to_string(&path).expect("written file should be readable");
        assert!(body.contains("[tui]"), "expected [tui] section in {body}");
        assert!(
            body.contains("status_items"),
            "expected status_items key in {body}"
        );
        assert!(body.contains("\"mode\""), "expected mode key in {body}");
        assert!(body.contains("\"cost\""), "expected cost key in {body}");
    }

    #[test]
    fn persist_status_items_preserves_existing_unrelated_keys() {
        let _lock = lock_test_env();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = env::temp_dir().join(format!(
            "deepseek-statusline-preserve-{}-{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&temp_root).unwrap();
        let _guard = EnvGuard::new(&temp_root);

        let path = temp_root.join(".deepseek").join("config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Seed the config with a sentinel key the picker MUST NOT clobber.
        fs::write(
            &path,
            "api_key = \"sentinel-key\"\nmodel = \"deepseek-v4-pro\"\n",
        )
        .unwrap();

        let written = persist_status_items(&[crate::config::StatusItem::Mode])
            .expect("persist should succeed");
        let body = fs::read_to_string(&written).expect("written file should be readable");
        assert!(
            body.contains("api_key = \"sentinel-key\""),
            "round-trip lost api_key: {body}"
        );
        assert!(
            body.contains("model = \"deepseek-v4-pro\""),
            "round-trip lost model: {body}"
        );
        assert!(
            body.contains("status_items"),
            "expected status_items in {body}"
        );
    }
}
