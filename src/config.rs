//! Configuration loading and defaults for deepseek-cli.

use std::collections::HashMap;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::features::{Features, FeaturesToml, is_known_feature_key};
use crate::hooks::HooksConfig;

pub const DEFAULT_MAX_SUBAGENTS: usize = 5;
pub const MAX_SUBAGENTS: usize = 20;

// === Types ===

/// Raw retry configuration loaded from config files.
#[derive(Debug, Clone, Deserialize)]
pub struct RetryConfig {
    pub enabled: Option<bool>,
    pub max_retries: Option<u32>,
    pub initial_delay: Option<f64>,
    pub max_delay: Option<f64>,
    pub exponential_base: Option<f64>,
}

/// UI configuration loaded from config files.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TuiConfig {
    pub alternate_screen: Option<String>,
}

/// Resolved retry policy with defaults applied.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_retries: u32,
    pub initial_delay: f64,
    pub max_delay: f64,
    pub exponential_base: f64,
}

impl RetryPolicy {
    /// Compute the backoff delay for a retry attempt.
    #[must_use]
    pub fn delay_for_attempt(&self, attempt: u32) -> std::time::Duration {
        let exponent = i32::try_from(attempt).unwrap_or(i32::MAX);
        let delay = self.initial_delay * self.exponential_base.powi(exponent);
        let delay = delay.min(self.max_delay);
        std::time::Duration::from_secs_f64(delay)
    }
}

/// Resolved CLI configuration, including defaults and environment overrides.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub default_text_model: Option<String>,
    pub tools_file: Option<String>,
    pub skills_dir: Option<String>,
    pub mcp_config_path: Option<String>,
    pub notes_path: Option<String>,
    pub memory_path: Option<String>,
    pub allow_shell: Option<bool>,
    pub max_subagents: Option<usize>,
    pub retry: Option<RetryConfig>,
    pub features: Option<FeaturesToml>,

    /// TUI configuration (alternate screen, etc.)
    pub tui: Option<TuiConfig>,

    /// Lifecycle hooks configuration
    #[serde(default)]
    pub hooks: Option<HooksConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ConfigFile {
    #[serde(flatten)]
    base: Config,
    profiles: Option<HashMap<String, Config>>,
}

// === Config Loading ===

impl Config {
    /// Load configuration from disk and merge with environment overrides.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use crate::config::Config;
    /// let config = Config::load(None, None)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn load(path: Option<PathBuf>, profile: Option<&str>) -> Result<Self> {
        let path = path.or_else(default_config_path);
        let mut config = if let Some(path) = path.as_ref() {
            if path.exists() {
                let contents = fs::read_to_string(path)
                    .with_context(|| format!("Failed to read config file: {}", path.display()))?;
                let parsed: ConfigFile = toml::from_str(&contents)
                    .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
                apply_profile(parsed, profile)?
            } else {
                Config::default()
            }
        } else {
            Config::default()
        };

        apply_env_overrides(&mut config);
        config.validate()?;
        Ok(config)
    }

    /// Validate that critical config fields are present.
    pub fn validate(&self) -> Result<()> {
        if let Some(ref key) = self.api_key
            && key.trim().is_empty()
        {
            anyhow::bail!("api_key cannot be empty string");
        }
        if let Some(features) = &self.features {
            for key in features.entries.keys() {
                if !is_known_feature_key(key) {
                    anyhow::bail!("Unknown feature flag: {key}");
                }
            }
        }
        if let Some(tui) = &self.tui
            && let Some(mode) = tui.alternate_screen.as_deref()
        {
            let mode = mode.to_ascii_lowercase();
            if !matches!(mode.as_str(), "auto" | "always" | "never") {
                anyhow::bail!(
                    "Invalid tui.alternate_screen '{mode}': expected auto, always, or never."
                );
            }
        }
        Ok(())
    }

    /// Return the `DeepSeek` base URL (normalized).
    #[must_use]
    pub fn deepseek_base_url(&self) -> String {
        let base = self
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.deepseek.com".to_string());
        normalize_base_url(&base)
    }

    /// Read the `DeepSeek` API key from config/environment.
    pub fn deepseek_api_key(&self) -> Result<String> {
        self.api_key
            .clone()
            .context(
                "Failed to load DeepSeek API key: DEEPSEEK_API_KEY missing. Set it in config.toml or environment.",
            )
    }

    /// Resolve the skills directory path.
    #[must_use]
    pub fn skills_dir(&self) -> PathBuf {
        self.skills_dir
            .as_deref()
            .map(expand_path)
            .or_else(default_skills_dir)
            .unwrap_or_else(|| PathBuf::from("./skills"))
    }

    /// Resolve the MCP config path.
    #[must_use]
    pub fn mcp_config_path(&self) -> PathBuf {
        self.mcp_config_path
            .as_deref()
            .map(expand_path)
            .or_else(default_mcp_config_path)
            .unwrap_or_else(|| PathBuf::from("./mcp.json"))
    }

    /// Resolve the notes file path.
    #[must_use]
    pub fn notes_path(&self) -> PathBuf {
        self.notes_path
            .as_deref()
            .map(expand_path)
            .or_else(default_notes_path)
            .unwrap_or_else(|| PathBuf::from("./notes.txt"))
    }

    /// Resolve the memory file path.
    #[must_use]
    pub fn memory_path(&self) -> PathBuf {
        self.memory_path
            .as_deref()
            .map(expand_path)
            .or_else(default_memory_path)
            .unwrap_or_else(|| PathBuf::from("./memory.md"))
    }

    /// Return whether shell execution is allowed.
    #[must_use]
    pub fn allow_shell(&self) -> bool {
        self.allow_shell.unwrap_or(false)
    }

    /// Return the maximum number of concurrent sub-agents.
    #[must_use]
    pub fn max_subagents(&self) -> usize {
        self.max_subagents
            .unwrap_or(DEFAULT_MAX_SUBAGENTS)
            .clamp(1, MAX_SUBAGENTS)
    }

    /// Get hooks configuration, returning default if not configured.
    pub fn hooks_config(&self) -> HooksConfig {
        self.hooks.clone().unwrap_or_default()
    }

    /// Resolve enabled features from defaults and config entries.
    #[must_use]
    pub fn features(&self) -> Features {
        let mut features = Features::with_defaults();
        if let Some(table) = &self.features {
            features.apply_map(&table.entries);
        }
        features
    }

    /// Override a feature flag in memory (used by CLI overrides).
    pub fn set_feature(&mut self, key: &str, enabled: bool) -> Result<()> {
        if !is_known_feature_key(key) {
            anyhow::bail!("Unknown feature flag: {key}");
        }
        let table = self.features.get_or_insert_with(FeaturesToml::default);
        table.entries.insert(key.to_string(), enabled);
        Ok(())
    }

    /// Resolve the effective retry policy with defaults applied.
    #[must_use]
    pub fn retry_policy(&self) -> RetryPolicy {
        let defaults = RetryPolicy {
            enabled: true,
            max_retries: 3,
            initial_delay: 1.0,
            max_delay: 60.0,
            exponential_base: 2.0,
        };

        let Some(cfg) = &self.retry else {
            return defaults;
        };

        RetryPolicy {
            enabled: cfg.enabled.unwrap_or(defaults.enabled),
            max_retries: cfg.max_retries.unwrap_or(defaults.max_retries),
            initial_delay: cfg.initial_delay.unwrap_or(defaults.initial_delay),
            max_delay: cfg.max_delay.unwrap_or(defaults.max_delay),
            exponential_base: cfg.exponential_base.unwrap_or(defaults.exponential_base),
        }
    }
}

// === Defaults ===

fn default_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DEEPSEEK_CONFIG_PATH")
        && !path.trim().is_empty()
    {
        return Some(PathBuf::from(path));
    }
    dirs::home_dir().map(|home| home.join(".deepseek").join("config.toml"))
}

fn expand_path(path: &str) -> PathBuf {
    let expanded = shellexpand::tilde(path);
    PathBuf::from(expanded.as_ref())
}

fn default_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".deepseek").join("skills"))
}

fn default_mcp_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".deepseek").join("mcp.json"))
}

fn default_notes_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".deepseek").join("notes.txt"))
}

fn default_memory_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".deepseek").join("memory.md"))
}

// === Environment Overrides ===

fn apply_env_overrides(config: &mut Config) {
    if let Ok(value) = std::env::var("DEEPSEEK_API_KEY") {
        config.api_key = Some(value);
    }
    if let Ok(value) = std::env::var("DEEPSEEK_BASE_URL") {
        config.base_url = Some(value);
    }
    if let Ok(value) = std::env::var("DEEPSEEK_SKILLS_DIR") {
        config.skills_dir = Some(value);
    }
    if let Ok(value) = std::env::var("DEEPSEEK_MCP_CONFIG") {
        config.mcp_config_path = Some(value);
    }
    if let Ok(value) = std::env::var("DEEPSEEK_NOTES_PATH") {
        config.notes_path = Some(value);
    }
    if let Ok(value) = std::env::var("DEEPSEEK_MEMORY_PATH") {
        config.memory_path = Some(value);
    }
    if let Ok(value) = std::env::var("DEEPSEEK_ALLOW_SHELL") {
        config.allow_shell = Some(value == "1" || value.eq_ignore_ascii_case("true"));
    }
    if let Ok(value) = std::env::var("DEEPSEEK_MAX_SUBAGENTS")
        && let Ok(parsed) = value.parse::<usize>()
    {
        config.max_subagents = Some(parsed.clamp(1, MAX_SUBAGENTS));
    }
}

fn normalize_base_url(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    let deepseek_domains = ["api.deepseek.com", "api.deepseeki.com"];
    if deepseek_domains
        .iter()
        .any(|domain| trimmed.contains(domain))
    {
        return trimmed.trim_end_matches("/v1").to_string();
    }
    trimmed.to_string()
}

fn apply_profile(config: ConfigFile, profile: Option<&str>) -> Result<Config> {
    if let Some(profile_name) = profile {
        let profiles = config.profiles.as_ref();
        match profiles.and_then(|profiles| profiles.get(profile_name)) {
            Some(override_cfg) => Ok(merge_config(config.base, override_cfg.clone())),
            None => {
                let available = profiles
                    .map(|profiles| {
                        let mut keys = profiles.keys().cloned().collect::<Vec<_>>();
                        keys.sort();
                        if keys.is_empty() {
                            "none".to_string()
                        } else {
                            keys.join(", ")
                        }
                    })
                    .unwrap_or_else(|| "none".to_string());
                anyhow::bail!(
                    "Profile '{}' not found. Available profiles: {}",
                    profile_name,
                    available
                )
            }
        }
    } else {
        Ok(config.base)
    }
}

fn merge_config(base: Config, override_cfg: Config) -> Config {
    Config {
        api_key: override_cfg.api_key.or(base.api_key),
        base_url: override_cfg.base_url.or(base.base_url),
        default_text_model: override_cfg.default_text_model.or(base.default_text_model),
        tools_file: override_cfg.tools_file.or(base.tools_file),
        skills_dir: override_cfg.skills_dir.or(base.skills_dir),
        mcp_config_path: override_cfg.mcp_config_path.or(base.mcp_config_path),
        notes_path: override_cfg.notes_path.or(base.notes_path),
        memory_path: override_cfg.memory_path.or(base.memory_path),
        allow_shell: override_cfg.allow_shell.or(base.allow_shell),
        max_subagents: override_cfg.max_subagents.or(base.max_subagents),
        retry: override_cfg.retry.or(base.retry),
        tui: override_cfg.tui.or(base.tui),
        hooks: override_cfg.hooks.or(base.hooks),
        features: merge_features(base.features, override_cfg.features),
    }
}

fn merge_features(
    base: Option<FeaturesToml>,
    override_cfg: Option<FeaturesToml>,
) -> Option<FeaturesToml> {
    match (base, override_cfg) {
        (None, None) => None,
        (Some(mut base), Some(override_cfg)) => {
            for (key, value) in override_cfg.entries {
                base.entries.insert(key, value);
            }
            Some(base)
        }
        (Some(base), None) => Some(base),
        (None, Some(override_cfg)) => Some(override_cfg),
    }
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }
    Ok(())
}

/// Save an API key to the config file. Creates the file if it doesn't exist.
pub fn save_api_key(api_key: &str) -> Result<PathBuf> {
    fn is_api_key_assignment(line: &str) -> bool {
        let trimmed = line.trim_start();
        trimmed
            .strip_prefix("api_key")
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    }

    let config_path = default_config_path()
        .context("Failed to resolve config path: home directory not found.")?;

    ensure_parent_dir(&config_path)?;

    let content = if config_path.exists() {
        // Read existing config and update the api_key line
        let existing = fs::read_to_string(&config_path)?;
        if existing.contains("api_key") {
            // Replace existing api_key line
            let mut result = String::new();
            for line in existing.lines() {
                if is_api_key_assignment(line) {
                    let _ = writeln!(result, "api_key = \"{api_key}\"");
                } else {
                    result.push_str(line);
                    result.push('\n');
                }
            }
            result
        } else {
            // Prepend api_key to existing config
            format!("api_key = \"{api_key}\"\n{existing}")
        }
    } else {
        // Create new minimal config
        format!(
            r#"# DeepSeek CLI Configuration
# Get your API key from https://platform.deepseek.com

api_key = "{api_key}"

# Base URL (default: https://api.deepseek.com)
# base_url = "https://api.deepseek.com"

# Default model
default_text_model = "deepseek-reasoner"
"#
        )
    };

    fs::write(&config_path, content)
        .with_context(|| format!("Failed to write config to {}", config_path.display()))?;

    Ok(config_path)
}

/// Check if an API key is configured (either in config or environment)
pub fn has_api_key(config: &Config) -> bool {
    config.api_key.is_some()
}

/// Clear the API key from the config file
pub fn clear_api_key() -> Result<()> {
    let config_path = default_config_path()
        .context("Failed to resolve config path: home directory not found.")?;

    if !config_path.exists() {
        return Ok(());
    }

    let existing = fs::read_to_string(&config_path)?;
    let mut result = String::new();

    for line in existing.lines() {
        if !line.trim_start().starts_with("api_key") {
            result.push_str(line);
            result.push('\n');
        }
    }

    fs::write(&config_path, result)
        .with_context(|| format!("Failed to write config to {}", config_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::env;
    use std::ffi::OsString;
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

    #[test]
    fn save_api_key_writes_config() -> Result<()> {
        let _lock = env_lock().lock().unwrap();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = env::temp_dir().join(format!(
            "deepseek-cli-test-{}-{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&temp_root)?;
        let _guard = EnvGuard::new(&temp_root);

        let path = save_api_key("test-key")?;
        let expected = temp_root.join(".deepseek").join("config.toml");
        assert_eq!(path, expected);

        let contents = fs::read_to_string(&path)?;
        assert!(contents.contains("api_key = \"test-key\""));
        Ok(())
    }

    #[test]
    fn test_tilde_expansion_in_paths() -> Result<()> {
        let _lock = env_lock().lock().unwrap();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = env::temp_dir().join(format!(
            "deepseek-cli-tilde-test-{}-{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&temp_root)?;
        let _guard = EnvGuard::new(&temp_root);

        let config = Config {
            skills_dir: Some("~/.deepseek/skills".to_string()),
            ..Default::default()
        };
        let expected_home = dirs::home_dir().expect("home dir not found");
        let expected_skills = expected_home.join(".deepseek").join("skills");
        let actual_skills = config.skills_dir();
        assert_eq!(
            actual_skills.components().collect::<Vec<_>>(),
            expected_skills.components().collect::<Vec<_>>()
        );

        Ok(())
    }

    #[test]
    fn test_nonexistent_profile_error() {
        let mut profiles = HashMap::new();
        profiles.insert("work".to_string(), Config::default());
        let config = ConfigFile {
            base: Config::default(),
            profiles: Some(profiles),
        };

        let err = apply_profile(config, Some("nonexistent")).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Profile 'nonexistent' not found"));
        assert!(message.contains("Available profiles"));
        assert!(message.contains("work"));
    }

    #[test]
    fn test_profile_with_no_profiles_section() {
        let config = ConfigFile {
            base: Config::default(),
            profiles: None,
        };

        let err = apply_profile(config, Some("missing")).unwrap_err();
        assert!(err.to_string().contains("Available profiles: none"));
    }

    #[test]
    fn test_save_api_key_doesnt_match_similar_keys() -> Result<()> {
        let _lock = env_lock().lock().unwrap();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = env::temp_dir().join(format!(
            "deepseek-cli-api-key-test-{}-{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&temp_root)?;
        let _guard = EnvGuard::new(&temp_root);

        let config_path = temp_root.join(".deepseek").join("config.toml");
        ensure_parent_dir(&config_path)?;
        fs::write(
            &config_path,
            "api_key_backup = \"old\"\napi_key = \"current\"\n",
        )?;

        let path = save_api_key("new-key")?;
        assert_eq!(path, config_path);

        let contents = fs::read_to_string(&config_path)?;
        assert!(contents.contains("api_key_backup = \"old\""));
        assert!(contents.contains("api_key = \"new-key\""));
        Ok(())
    }

    #[test]
    fn test_empty_api_key_rejected() {
        let config = Config {
            api_key: Some("   ".to_string()),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_missing_api_key_allowed() -> Result<()> {
        let config = Config::default();
        config.validate()?;
        Ok(())
    }
}
