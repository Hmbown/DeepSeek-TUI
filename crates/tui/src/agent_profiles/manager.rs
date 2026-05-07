//! Agent profile manager — loads, resolves, and applies agent profiles.
//!
//! NOTE: This module is not yet wired into TUI dispatch paths.
//! All types and functions are kept public for follow-up integration.
#![allow(dead_code)]
//!
//! Profiles are loaded from two sources:
//! 1. **Built-in** predefined profiles (general, explore, plan, implementer, reviewer, builder)
//! 2. **User config** (`~/.deepseek/agents.toml`) and **project config** (`<workspace>/.deepseek/agents.toml`)
//!
//! User/project config overrides built-in profiles. Custom profiles (under `[agents.custom.*]`)
//! are also available.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::profile::{AgentProfile, builtin_profiles};

// ── Config types ──────────────────────────────────────────────────────

/// TOML wrapper for agents.toml.
#[derive(Debug, Default, Deserialize)]
pub struct AgentsConfig {
    #[serde(default)]
    pub agents: HashMap<String, AgentProfileConfig>,
    #[serde(default)]
    pub custom: HashMap<String, HashMap<String, AgentProfileConfig>>,
}

/// A user-defined agent profile (subset of AgentProfile fields configurable via TOML).
#[derive(Debug, Clone, Deserialize)]
pub struct AgentProfileConfig {
    pub description: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub denied_tools: Option<Vec<String>>,
    pub system_prompt_extension: Option<String>,
    pub auto_approve: Option<bool>,
}

// ── Manager ───────────────────────────────────────────────────────────

/// Manages agent profile resolution across built-in, user, and project configs.
#[derive(Debug, Clone)]
pub struct AgentProfileManager {
    /// Resolved profiles (built-in overridden by user, overridden by project).
    profiles: HashMap<String, AgentProfile>,
}

impl AgentProfileManager {
    /// Build a manager from config files.
    ///
    /// Loads built-in profiles first, then overlays user config, then project config.
    /// Custom profiles from `[agents.custom.*]` are also available.
    pub fn load(
        user_config_path: Option<&Path>,
        project_config_path: Option<&Path>,
    ) -> Self {
        let mut profiles: HashMap<String, AgentProfile> = HashMap::new();

        // Load built-in profiles
        for profile in builtin_profiles() {
            profiles.insert(profile.name.clone(), profile);
        }

        // Overlay user config
        if let Some(path) = user_config_path {
            Self::apply_config_file(&mut profiles, path);
        }

        // Overlay project config
        if let Some(path) = project_config_path {
            Self::apply_config_file(&mut profiles, path);
        }

        Self { profiles }
    }

    /// Create a manager with only built-in profiles (no config files).
    pub fn builtin_only() -> Self {
        let mut profiles = HashMap::new();
        for profile in builtin_profiles() {
            profiles.insert(profile.name.clone(), profile);
        }
        Self { profiles }
    }

    /// Create an empty manager (for tests).
    pub fn empty() -> Self {
        Self {
            profiles: HashMap::new(),
        }
    }

    /// Resolve a profile by name. Returns `None` if the profile doesn't exist.
    pub fn resolve(&self, name: &str) -> Option<&AgentProfile> {
        self.profiles.get(name)
    }

    /// List all available profile names.
    pub fn list_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.profiles.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// List all profiles.
    pub fn list_all(&self) -> Vec<&AgentProfile> {
        let mut profiles: Vec<&AgentProfile> = self.profiles.values().collect();
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        profiles
    }

    /// Register a profile programmatically (for tests or runtime overrides).
    pub fn register(&mut self, profile: AgentProfile) {
        self.profiles.insert(profile.name.clone(), profile);
    }

    /// Number of registered profiles.
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Whether the profile map is empty.
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    // ── Private helpers ──────────────────────────────────────────────

    fn apply_config_file(profiles: &mut HashMap<String, AgentProfile>, path: &Path) {
        let contents = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let config: AgentsConfig = match toml::from_str(&contents) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "failed to parse agents config");
                return;
            }
        };

        // Apply standard profiles
        for (name, cfg) in &config.agents {
            Self::overlay_profile(profiles, name, cfg);
        }

        // Apply custom profiles
        for (name, cfg) in &config.custom {
            if let Some(agent_cfg) = cfg.get(&name.clone()) {
                Self::overlay_profile(profiles, name, agent_cfg);
            }
        }
    }

    fn overlay_profile(
        profiles: &mut HashMap<String, AgentProfile>,
        name: &str,
        cfg: &AgentProfileConfig,
    ) {
        let mut profile = profiles
            .remove(name)
            .unwrap_or_else(|| AgentProfile {
                name: name.to_string(),
                description: String::new(),
                model: None,
                reasoning_effort: None,
                allowed_tools: None,
                denied_tools: None,
                permissions: HashMap::new(),
                system_prompt_extension: None,
                auto_approve: false,
            });

        if let Some(ref desc) = cfg.description {
            profile.description = desc.clone();
        }
        if let Some(ref model) = cfg.model {
            profile.model = Some(model.clone());
        }
        if let Some(ref re) = cfg.reasoning_effort {
            profile.reasoning_effort = Some(re.clone());
        }
        if let Some(ref allowed) = cfg.allowed_tools {
            profile.allowed_tools = Some(allowed.clone());
        }
        if let Some(ref denied) = cfg.denied_tools {
            profile.denied_tools = Some(denied.clone());
        }
        if let Some(ref ext) = cfg.system_prompt_extension {
            profile.system_prompt_extension = Some(ext.clone());
        }
        if let Some(auto) = cfg.auto_approve {
            profile.auto_approve = auto;
        }

        profiles.insert(name.to_string(), profile);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_only_has_profiles() {
        let mgr = AgentProfileManager::builtin_only();
        assert!(mgr.resolve("general").is_some());
        assert!(mgr.resolve("explore").is_some());
        assert!(mgr.resolve("plan").is_some());
        assert!(mgr.resolve("implementer").is_some());
        assert!(mgr.resolve("reviewer").is_some());
        assert!(mgr.resolve("builder").is_some());
    }

    #[test]
    fn resolve_missing_returns_none() {
        let mgr = AgentProfileManager::empty();
        assert!(mgr.resolve("nonexistent").is_none());
    }

    #[test]
    fn register_adds_profile() {
        let mut mgr = AgentProfileManager::empty();
        mgr.register(AgentProfile {
            name: "test-agent".into(),
            description: "test".into(),
            model: None,
            reasoning_effort: None,
            allowed_tools: None,
            denied_tools: None,
            permissions: HashMap::new(),
            system_prompt_extension: None,
            auto_approve: false,
        });
        assert!(mgr.resolve("test-agent").is_some());
    }

    #[test]
    fn list_names_sorted() {
        let mgr = AgentProfileManager::builtin_only();
        let names = mgr.list_names();
        assert_eq!(names, vec!["builder", "explore", "general", "implementer", "plan", "reviewer"]);
    }

    #[test]
    fn explore_profile_is_read_only() {
        let mgr = AgentProfileManager::builtin_only();
        let explore = mgr.resolve("explore").unwrap();
        assert!(!explore.auto_approve);
        let tools = explore.allowed_tools.as_ref().unwrap();
        assert!(!tools.contains(&"edit_file".to_string()));
        assert!(tools.contains(&"read_file".to_string()));
    }

    #[test]
    fn implementer_profile_auto_approves() {
        let mgr = AgentProfileManager::builtin_only();
        let imp = mgr.resolve("implementer").unwrap();
        assert!(imp.auto_approve);
    }
}
