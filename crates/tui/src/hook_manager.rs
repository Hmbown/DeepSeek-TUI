//! Hook manager — loads and executes user-defined lifecycle hooks (Phase 5).
//!
//! Hooks are shell commands triggered by lifecycle events.
//! Users define them in `~/.deepseek/hooks.toml` and
//! `<workspace>/.deepseek/hooks.toml`.
#![allow(dead_code)]

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HookRule {
    pub event: String,
    pub command: String,
}

#[derive(Debug, Deserialize)]
struct HooksConfig {
    #[serde(default)]
    hooks: Vec<HookRule>,
}

/// Manages lifecycle hooks: loading, matching, and executing.
pub struct HookManager {
    #[allow(dead_code)]
    rules: Vec<HookRule>,
}

impl HookManager {
    pub fn load(
        user_config_path: Option<&Path>,
        _project_config_path: Option<&Path>,
    ) -> Self {
        let mut rules = Vec::new();
        if let Some(path) = user_config_path {
            match std::fs::read_to_string(path) {
                Ok(contents) => match toml::from_str::<HooksConfig>(&contents) {
                    Ok(config) => rules.extend(config.hooks),
                    Err(e) => {
                        tracing::error!(
                            "failed to parse hooks config {}: {e}",
                            path.display()
                        );
                    }
                },
                Err(e) => {
                    tracing::error!(
                        "failed to read hooks config {}: {e}",
                        path.display()
                    );
                }
            }
        }
        Self { rules }
    }

    /// Fire a hook event. Runs matching hook commands in order.
    /// Logs errors but never blocks the caller.
    pub fn fire(&self, event: &str, _context: &str) {
        for rule in &self.rules {
            if rule.event == event {
                // Best-effort: spawn shell command, don't block.
                let cmd = rule.command.clone();
                match std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .spawn()
                {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!(
                            "hook [{event}] failed to spawn command: {e}"
                        );
                    }
                }
            }
        }
    }

    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }
}
