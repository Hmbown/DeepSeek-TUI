//! Settings system - Persistent user preferences
//!
//! Settings are stored at ~/.config/deepseek/settings.toml

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// User settings with defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Color theme: "default", "dark", "light"
    pub theme: String,
    /// Auto-compact conversations when they get long
    pub auto_compact: bool,
    /// Auto-switch to RLM mode when large inputs are detected
    pub auto_rlm: bool,
    /// Show thinking blocks from the model
    pub show_thinking: bool,
    /// Show detailed tool output
    pub show_tool_details: bool,
    /// Default mode: "agent", "plan", "yolo", "rlm", "duo"
    pub default_mode: String,
    /// Sidebar width as percentage of terminal width
    pub sidebar_width_percent: u16,
    /// Maximum number of input history entries to save
    pub max_input_history: usize,
    /// Default model to use
    pub default_model: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "default".to_string(),
            auto_compact: true,
            auto_rlm: false,
            show_thinking: true,
            show_tool_details: true,
            default_mode: "agent".to_string(),
            sidebar_width_percent: 28,
            max_input_history: 100,
            default_model: None,
        }
    }
}

impl Settings {
    /// Get the settings file path
    pub fn path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Failed to resolve config directory: not found.")?
            .join("deepseek");
        Ok(config_dir.join("settings.toml"))
    }

    /// Load settings from disk, or return defaults if not found
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read settings from {}", path.display()))?;
        let mut settings: Settings = toml::from_str(&content)
            .with_context(|| format!("Failed to parse settings from {}", path.display()))?;
        settings.default_mode = normalize_mode(&settings.default_mode).to_string();
        Ok(settings)
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
            "theme" => {
                if !["default", "dark", "light"].contains(&value) {
                    anyhow::bail!(
                        "Failed to update setting: invalid theme '{value}'. Expected: default, dark, light."
                    );
                }
                self.theme = value.to_string();
            }
            "auto_compact" | "compact" => {
                self.auto_compact = parse_bool(value)?;
            }
            "auto_rlm" => {
                self.auto_rlm = parse_bool(value)?;
            }
            "show_thinking" | "thinking" => {
                self.show_thinking = parse_bool(value)?;
            }
            "show_tool_details" | "tool_details" => {
                self.show_tool_details = parse_bool(value)?;
            }
            "default_mode" | "mode" => {
                let normalized = normalize_mode(value);
                if !["agent", "plan", "yolo", "rlm", "duo"].contains(&normalized) {
                    anyhow::bail!(
                        "Failed to update setting: invalid mode '{value}'. Expected: agent, plan, yolo, rlm, duo."
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
            "max_history" | "history" => {
                let max: usize = value.parse().map_err(|_| {
                    anyhow::anyhow!(
                        "Failed to update setting: invalid max history '{value}'. Expected a positive number."
                    )
                })?;
                self.max_input_history = max;
            }
            "default_model" | "model" => {
                self.default_model = Some(value.to_string());
            }
            _ => {
                anyhow::bail!("Failed to update setting: unknown setting '{key}'.");
            }
        }
        Ok(())
    }

    /// Get all settings as a displayable string
    pub fn display(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Settings:".to_string());
        lines.push("─────────────────────────────".to_string());
        lines.push(format!("  theme:              {}", self.theme));
        lines.push(format!("  auto_compact:       {}", self.auto_compact));
        lines.push(format!("  auto_rlm:           {}", self.auto_rlm));
        lines.push(format!("  show_thinking:      {}", self.show_thinking));
        lines.push(format!("  show_tool_details:  {}", self.show_tool_details));
        lines.push(format!("  default_mode:       {}", self.default_mode));
        lines.push(format!(
            "  sidebar_width:      {}%",
            self.sidebar_width_percent
        ));
        lines.push(format!("  max_history:        {}", self.max_input_history));
        lines.push(format!(
            "  default_model:      {}",
            self.default_model.as_deref().unwrap_or("(default)")
        ));
        lines.push(String::new());
        lines.push(format!(
            "Config file: {}",
            Self::path().map_or_else(|_| "(unknown)".to_string(), |p| p.display().to_string())
        ));
        lines.join("\n")
    }

    /// Get available setting keys and their descriptions
    pub fn available_settings() -> Vec<(&'static str, &'static str)> {
        vec![
            ("theme", "Color theme: default, dark, light"),
            ("auto_compact", "Auto-compact conversations: on/off"),
            (
                "auto_rlm",
                "Auto-switch to RLM mode for large inputs: on/off",
            ),
            ("show_thinking", "Show model thinking: on/off"),
            ("show_tool_details", "Show detailed tool output: on/off"),
            ("default_mode", "Default mode: agent, plan, yolo, rlm, duo"),
            ("sidebar_width", "Sidebar width percentage: 10-50"),
            ("max_history", "Max input history entries"),
            ("default_model", "Default model name"),
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
    match value {
        "edit" | "normal" => "agent",
        _ => value,
    }
}
