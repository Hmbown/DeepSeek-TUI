//! Settings system - Persistent user preferences
//!
//! Settings are stored at ~/.config/deepseek/settings.toml

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{expand_path, normalize_model_name};
use crate::localization::normalize_configured_locale;

/// User settings with defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Auto-compact conversations when they approach the model limit.
    pub auto_compact: bool,
    /// Reduce status noise and collapse details more aggressively
    pub calm_mode: bool,
    /// Reduce animation and redraw churn
    pub low_motion: bool,
    /// Enable fancy footer animations (water-spout strip, pulsing text)
    pub fancy_animations: bool,
    /// Enable terminal bracketed-paste mode. Default true. Disable if your
    /// terminal mishandles the `\e[?2004h` escape (rare; some legacy
    /// terminals over SSH+screen multiplex without the cap).
    pub bracketed_paste: bool,
    /// Enable rapid-key paste-burst detection for terminals that do not emit
    /// bracketed-paste events. Independent from `bracketed_paste`.
    pub paste_burst_detection: bool,
    /// Show thinking blocks from the model
    pub show_thinking: bool,
    /// Show detailed tool output
    pub show_tool_details: bool,
    /// UI locale: auto, en, ja, zh-Hans, pt-BR
    pub locale: String,
    /// Composer layout density: compact, comfortable, spacious
    pub composer_density: String,
    /// Show a border around the composer input area
    pub composer_border: bool,
    /// Composer editing mode: "normal" (default) or "vim" for modal editing.
    /// When set to "vim" the composer starts in Normal mode; press i/a/o to
    /// enter Insert mode and Esc to return to Normal.
    pub composer_vim_mode: String,
    /// Transcript spacing rhythm: compact, comfortable, spacious
    pub transcript_spacing: String,
    /// Default mode: "agent", "plan", "yolo"
    pub default_mode: String,
    /// Sidebar width as percentage of terminal width
    pub sidebar_width_percent: u16,
    /// Sidebar focus mode: auto, plan, todos, tasks, agents, context
    pub sidebar_focus: String,
    /// Enable the session-context panel (#504). Shows working set, tokens,
    /// cost, MCP/LSP status, cycle count, and memory info.
    pub context_panel: bool,
    /// Maximum number of input history entries to save
    pub max_input_history: usize,
    /// Default model to use
    pub default_model: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // v0.8.11: default flipped to `false` to stop the engine from
            // routinely rewriting the prompt prefix, which breaks DeepSeek
            // V4's prefix cache (~90% discount on cached prefix tokens) and
            // ends up costing more than the compaction itself saves. With
            // V4's 1M-token window the user has plenty of headroom to run
            // long sessions without auto-trimming, and the explicit
            // `/compact` slash command + `auto_compact = on` opt-in remain
            // available for users / agents that decide compaction is
            // worth the cache hit on their workload (#664).
            auto_compact: false,
            calm_mode: false,
            low_motion: false,
            fancy_animations: false,
            bracketed_paste: true,
            paste_burst_detection: true,
            show_thinking: true,
            show_tool_details: true,
            locale: "auto".to_string(),
            composer_density: "comfortable".to_string(),
            composer_border: true,
            composer_vim_mode: "normal".to_string(),
            transcript_spacing: "comfortable".to_string(),
            default_mode: "agent".to_string(),
            sidebar_width_percent: 28,
            sidebar_focus: "auto".to_string(),
            context_panel: false,
            max_input_history: 100,
            default_model: None,
        }
    }
}

impl Settings {
    /// Get the settings file path
    pub fn path() -> Result<PathBuf> {
        // Allow tests to override the settings directory via the same env var
        // used for config (DEEPSEEK_CONFIG_PATH points at config.toml; the
        // settings file lives as a sibling in the same directory).
        if let Ok(config_path) = std::env::var("DEEPSEEK_CONFIG_PATH") {
            let config_path = config_path.trim();
            if !config_path.is_empty() {
                let p = expand_path(config_path);
                if let Some(parent) = p.parent() {
                    return Ok(parent.join("settings.toml"));
                }
            }
        }

        let config_dir = dirs::config_dir()
            .context("Failed to resolve config directory: not found.")?
            .join("deepseek");
        Ok(config_dir.join("settings.toml"))
    }

    /// Load settings from disk, or return defaults if not found
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        let mut settings = if !path.exists() {
            Self::default()
        } else {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read settings from {}", path.display()))?;
            let mut s: Settings = toml::from_str(&content)
                .with_context(|| format!("Failed to parse settings from {}", path.display()))?;
            s.default_mode = normalize_mode(&s.default_mode).to_string();
            s.composer_density = normalize_composer_density(&s.composer_density).to_string();
            s.transcript_spacing = normalize_transcript_spacing(&s.transcript_spacing).to_string();
            s.sidebar_focus = normalize_sidebar_focus(&s.sidebar_focus).to_string();
            s.locale = normalize_configured_locale(&s.locale)
                .unwrap_or("en")
                .to_string();
            s.default_model = s.default_model.as_deref().and_then(normalize_model_name);
            s
        };
        settings.apply_env_overrides();
        Ok(settings)
    }

    /// Apply environment-driven overlays after disk load. Used for
    /// platform a11y signals that should ignore the user's saved
    /// preference (#450). The env values are consulted at startup;
    /// changing them mid-session has no effect because settings are
    /// only re-read on `Settings::load()`.
    pub fn apply_env_overrides(&mut self) {
        if env_truthy("NO_ANIMATIONS") {
            self.low_motion = true;
            self.fancy_animations = false;
        }
    }

    /// Save settings to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }

        let content = toml::to_string_pretty(self).context("Failed to serialize settings")?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write settings to {}", path.display()))?;
        Ok(())
    }

    /// Set a single setting by key
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "auto_compact" | "compact" => {
                self.auto_compact = parse_bool(value)?;
            }
            "calm_mode" | "calm" => {
                self.calm_mode = parse_bool(value)?;
            }
            "low_motion" | "motion" => {
                self.low_motion = parse_bool(value)?;
            }
            "fancy_animations" | "fancy" | "animations" => {
                self.fancy_animations = parse_bool(value)?;
            }
            "bracketed_paste" | "paste" => {
                self.bracketed_paste = parse_bool(value)?;
            }
            "paste_burst_detection" | "paste_burst" => {
                self.paste_burst_detection = parse_bool(value)?;
            }
            "show_thinking" | "thinking" => {
                self.show_thinking = parse_bool(value)?;
            }
            "show_tool_details" | "tool_details" => {
                self.show_tool_details = parse_bool(value)?;
            }
            "locale" | "language" => {
                let Some(locale) = normalize_configured_locale(value) else {
                    anyhow::bail!(
                        "Failed to update setting: invalid locale '{value}'. Expected: auto, en, ja, zh-Hans, pt-BR."
                    );
                };
                self.locale = locale.to_string();
            }
            "composer_density" | "composer" => {
                let normalized = normalize_composer_density(value);
                if !["compact", "comfortable", "spacious"].contains(&normalized) {
                    anyhow::bail!(
                        "Failed to update setting: invalid composer density '{value}'. Expected: compact, comfortable, spacious."
                    );
                }
                self.composer_density = normalized.to_string();
            }
            "composer_border" | "border" => {
                self.composer_border = parse_bool(value)?;
            }
            "composer_vim_mode" | "vim_mode" | "vim" => {
                let normalized = value.trim().to_ascii_lowercase();
                if !["vim", "normal"].contains(&normalized.as_str()) {
                    anyhow::bail!(
                        "Failed to update setting: invalid composer vim mode '{value}'. Expected: normal, vim."
                    );
                }
                self.composer_vim_mode = normalized;
            }
            "transcript_spacing" | "spacing" => {
                let normalized = normalize_transcript_spacing(value);
                if !["compact", "comfortable", "spacious"].contains(&normalized) {
                    anyhow::bail!(
                        "Failed to update setting: invalid transcript spacing '{value}'. Expected: compact, comfortable, spacious."
                    );
                }
                self.transcript_spacing = normalized.to_string();
            }
            "default_mode" | "mode" => {
                let normalized = normalize_mode(value);
                if !["agent", "plan", "yolo"].contains(&normalized) {
                    anyhow::bail!(
                        "Failed to update setting: invalid mode '{value}'. Expected: agent, plan, yolo."
                    );
                }
                self.default_mode = normalized.to_string();
            }
            "sidebar_width" | "sidebar" => {
                let width: u16 = value
                    .parse()
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "Failed to update setting: invalid width '{value}'. Expected a number between 10-50."
                        )
                    })?;
                if !(10..=50).contains(&width) {
                    anyhow::bail!(
                        "Failed to update setting: width must be between 10 and 50 percent."
                    );
                }
                self.sidebar_width_percent = width;
            }
            "sidebar_focus" | "focus" => {
                let normalized = match value.trim().to_ascii_lowercase().as_str() {
                    "auto" => "auto",
                    "plan" => "plan",
                    "todos" => "todos",
                    "tasks" => "tasks",
                    "agents" | "subagents" | "sub-agents" => "agents",
                    _ => {
                        anyhow::bail!(
                            "Failed to update setting: invalid sidebar focus '{value}'. Expected: auto, plan, todos, tasks, agents."
                        )
                    }
                };
                self.sidebar_focus = normalized.to_string();
            }
            "max_history" | "history" => {
                let max: usize = value.parse().map_err(|_| {
                    anyhow::anyhow!(
                        "Failed to update setting: invalid max history '{value}'. Expected a positive number."
                    )
                })?;
                self.max_input_history = max;
            }
            "default_model" | "model" => {
                let trimmed = value.trim();
                if trimmed.is_empty()
                    || matches!(
                        trimmed.to_ascii_lowercase().as_str(),
                        "none" | "default" | "(default)"
                    )
                {
                    self.default_model = None;
                    return Ok(());
                }

                let Some(model) = normalize_model_name(trimmed) else {
                    anyhow::bail!(
                        "Failed to update setting: invalid model '{value}'. Expected: a DeepSeek model ID (for example deepseek-v4-pro, deepseek-v4-flash), or none/default."
                    );
                };
                self.default_model = Some(model);
            }
            _ => {
                anyhow::bail!("Failed to update setting: unknown setting '{key}'.");
            }
        }
        Ok(())
    }

    /// Get all settings as a displayable string
    pub fn display(&self, locale: crate::localization::Locale) -> String {
        use crate::localization::{MessageId, tr};
        let mut lines = Vec::new();
        lines.push(tr(locale, MessageId::SettingsTitle).to_string());
        lines.push("─────────────────────────────".to_string());
        lines.push(format!("  auto_compact:       {}", self.auto_compact));
        lines.push(format!("  calm_mode:          {}", self.calm_mode));
        lines.push(format!("  low_motion:         {}", self.low_motion));
        lines.push(format!("  fancy_animations:   {}", self.fancy_animations));
        lines.push(format!("  bracketed_paste:    {}", self.bracketed_paste));
        lines.push(format!(
            "  paste_burst_detect: {}",
            self.paste_burst_detection
        ));
        lines.push(format!("  show_thinking:      {}", self.show_thinking));
        lines.push(format!("  show_tool_details:  {}", self.show_tool_details));
        lines.push(format!("  locale:            {}", self.locale));
        lines.push(format!("  composer_density:   {}", self.composer_density));
        lines.push(format!("  composer_border:    {}", self.composer_border));
        lines.push(format!("  composer_vim_mode:  {}", self.composer_vim_mode));
        lines.push(format!("  transcript_spacing: {}", self.transcript_spacing));
        lines.push(format!("  default_mode:       {}", self.default_mode));
        lines.push(format!(
            "  sidebar_width:      {}%",
            self.sidebar_width_percent
        ));
        lines.push(format!("  sidebar_focus:      {}", self.sidebar_focus));
        lines.push(format!("  max_history:        {}", self.max_input_history));
        lines.push(format!(
            "  default_model:      {}",
            self.default_model.as_deref().unwrap_or("(default)")
        ));
        lines.push(String::new());
        lines.push(format!(
            "{} {}",
            tr(locale, MessageId::SettingsConfigFile),
            Self::path().map_or_else(|_| "(unknown)".to_string(), |p| p.display().to_string())
        ));
        lines.join("\n")
    }

    /// Get available setting keys and their descriptions
    #[allow(dead_code)]
    pub fn available_settings() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "auto_compact",
                "Auto-compact near context limit: on/off (default on)",
            ),
            ("calm_mode", "Calmer UI defaults: on/off"),
            ("low_motion", "Reduce animation and redraw churn: on/off"),
            (
                "fancy_animations",
                "Fancy footer animations (water-spout strip): on/off",
            ),
            (
                "bracketed_paste",
                "Terminal bracketed-paste mode: on/off (rare to disable)",
            ),
            (
                "paste_burst_detection",
                "Fallback rapid-key paste detection: on/off",
            ),
            ("show_thinking", "Show model thinking: on/off"),
            ("show_tool_details", "Show detailed tool output: on/off"),
            (
                "locale",
                "UI locale: auto, en, ja, zh-Hans, pt-BR (model output is unchanged)",
            ),
            (
                "composer_density",
                "Composer density: compact, comfortable, spacious",
            ),
            (
                "composer_border",
                "Show a border around the composer input area: on/off",
            ),
            (
                "transcript_spacing",
                "Transcript spacing: compact, comfortable, spacious",
            ),
            ("default_mode", "Default mode: agent, plan, yolo"),
            ("sidebar_width", "Sidebar width percentage: 10-50"),
            (
                "sidebar_focus",
                "Sidebar focus: auto, plan, todos, tasks, agents",
            ),
            ("max_history", "Max input history entries"),
            (
                "default_model",
                "Default model: any DeepSeek model ID (e.g. deepseek-v4-pro)",
            ),
        ]
    }
}

/// Parse a boolean value from various formats
fn parse_bool(value: &str) -> Result<bool> {
    match value.to_lowercase().as_str() {
        "on" | "true" | "yes" | "1" | "enabled" => Ok(true),
        "off" | "false" | "no" | "0" | "disabled" => Ok(false),
        _ => {
            anyhow::bail!("Failed to parse boolean '{value}': expected on/off, true/false, yes/no.")
        }
    }
}

fn normalize_mode(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "edit" => "agent",
        "normal" => "agent",
        "agent" => "agent",
        "plan" => "plan",
        "yolo" => "yolo",
        _ => value,
    }
}

fn normalize_composer_density(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "compact" | "tight" => "compact",
        "comfortable" | "default" | "normal" => "comfortable",
        "spacious" | "loose" => "spacious",
        _ => value,
    }
}

fn normalize_transcript_spacing(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "compact" | "tight" => "compact",
        "comfortable" | "default" | "normal" => "comfortable",
        "spacious" | "loose" => "spacious",
        _ => value,
    }
}

fn normalize_sidebar_focus(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "plan" => "plan",
        "todos" => "todos",
        "tasks" => "tasks",
        "agents" | "subagents" | "sub-agents" => "agents",
        "context" | "session" => "context",
        _ => "auto",
    }
}

/// Resolve an environment variable as a boolean. Recognises the
/// common truthy spellings (`1`, `true`, `yes`, `on`) case-
/// insensitively. Used by [`Settings::apply_env_overrides`] for
/// platform a11y signals like `NO_ANIMATIONS`.
fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_disable_auto_compact_to_protect_v4_prefix_cache() {
        let settings = Settings::default();
        // v0.8.11: default is `false` to stop the engine from routinely
        // rewriting the prompt prefix, which breaks V4's prefix-cache
        // discount. The explicit `/compact` command and the
        // `auto_compact = on` opt-in stay available; the default is
        // flipped so the cache-friendly path is the one users get
        // without configuring anything (#664).
        assert!(!settings.auto_compact);
    }

    #[test]
    fn auto_compact_remains_explicitly_configurable() {
        let mut settings = Settings::default();
        settings.set("auto_compact", "on").expect("enable");
        assert!(settings.auto_compact);
        settings.set("auto_compact", "off").expect("disable");
        assert!(!settings.auto_compact);
    }

    #[test]
    fn paste_burst_detection_is_configurable_independent_of_bracketed_paste() {
        let mut settings = Settings::default();
        assert!(settings.bracketed_paste);
        assert!(settings.paste_burst_detection);

        settings
            .set("paste_burst_detection", "off")
            .expect("disable paste burst fallback");
        assert!(settings.bracketed_paste);
        assert!(!settings.paste_burst_detection);

        settings
            .set("bracketed_paste", "off")
            .expect("disable bracketed paste");
        assert!(!settings.bracketed_paste);
        assert!(!settings.paste_burst_detection);
    }

    #[test]
    fn locale_normalizes_supported_values_and_rejects_unknowns() {
        let mut settings = Settings::default();
        settings.set("locale", "ja_JP.UTF-8").expect("set ja");
        assert_eq!(settings.locale, "ja");

        settings.set("language", "pt-PT").expect("set pt fallback");
        assert_eq!(settings.locale, "pt-BR");

        let err = settings
            .set("locale", "ar")
            .expect_err("Arabic is planned, not shipped");
        assert!(err.to_string().contains("invalid locale"));
    }

    #[test]
    fn display_localizes_header_and_config_file_label() {
        let settings = Settings::default();
        let en = settings.display(crate::localization::Locale::En);
        assert!(en.contains("Settings:"), "english header missing:\n{en}");
        assert!(
            en.contains("Config file:"),
            "english config label missing:\n{en}"
        );

        let zh = settings.display(crate::localization::Locale::ZhHans);
        assert!(zh.contains("设置"), "chinese header missing:\n{zh}");
        assert!(
            zh.contains("配置文件"),
            "chinese config label missing:\n{zh}"
        );
    }

    /// Tests that mutate process-global `NO_ANIMATIONS` serialise
    /// through this guard so the cargo parallel runner doesn't
    /// observe interleaved overrides.
    fn no_animations_test_guard() -> std::sync::MutexGuard<'static, ()> {
        static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
        GUARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn no_animations_env_forces_low_motion_on() {
        let _g = no_animations_test_guard();
        // SAFETY: tests in this group serialise through the guard.
        unsafe {
            std::env::set_var("NO_ANIMATIONS", "1");
        }
        let mut settings = Settings::default();
        assert!(!settings.low_motion, "default is animated");
        assert!(!settings.fancy_animations, "default is animated");
        settings.apply_env_overrides();
        assert!(settings.low_motion, "NO_ANIMATIONS=1 forces low_motion");
        assert!(
            !settings.fancy_animations,
            "NO_ANIMATIONS=1 keeps fancy off"
        );
        // SAFETY: cleanup under the guard.
        unsafe {
            std::env::remove_var("NO_ANIMATIONS");
        }
    }

    #[test]
    fn no_animations_env_overrides_user_opt_in() {
        let _g = no_animations_test_guard();
        // SAFETY: serialised by the guard.
        unsafe {
            std::env::set_var("NO_ANIMATIONS", "true");
        }
        // User had explicitly opted into fancy animations on disk.
        let mut settings = Settings {
            fancy_animations: true,
            ..Settings::default()
        };
        settings.apply_env_overrides();
        assert!(
            !settings.fancy_animations,
            "platform NO_ANIMATIONS overrides user-opt-in fancy_animations"
        );
        assert!(settings.low_motion);
        // SAFETY: cleanup under the guard.
        unsafe {
            std::env::remove_var("NO_ANIMATIONS");
        }
    }

    #[test]
    fn no_animations_env_recognises_truthy_spellings_only() {
        let _g = no_animations_test_guard();
        for truthy in ["1", "true", "True", "YES", "on"] {
            // SAFETY: serialised by the guard.
            unsafe {
                std::env::set_var("NO_ANIMATIONS", truthy);
            }
            let mut s = Settings::default();
            s.apply_env_overrides();
            assert!(s.low_motion, "{truthy:?} should be truthy");
        }
        for falsy in ["0", "false", "no", "off", ""] {
            // SAFETY: serialised by the guard.
            unsafe {
                std::env::set_var("NO_ANIMATIONS", falsy);
            }
            let mut s = Settings::default();
            s.apply_env_overrides();
            assert!(!s.low_motion, "{falsy:?} should be falsy");
        }
        // SAFETY: cleanup under the guard.
        unsafe {
            std::env::remove_var("NO_ANIMATIONS");
        }
    }
}
