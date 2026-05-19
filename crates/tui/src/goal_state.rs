//! On-disk persistence for `/goal` state (#891).
//!
//! When the user sets `/goal <objective>` we now keep the goal alive
//! across restarts so a long-running session can resume after a crash
//! or intentional quit. The on-disk format mirrors
//! [`composer_stash`](crate::composer_stash) — a small JSON file under
//! `~/.deepseek/`, self-healing parser, never propagates write errors
//! (goal persistence is a UX nicety, not a correctness concern).
//!
//! Schema is versioned (`schema_version: 1`) and every field after
//! `objective` is `#[serde(default)]` so future additions can land
//! without breaking older saved files.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const GOAL_FILE_NAME: &str = "goals.v1.json";
const CURRENT_SCHEMA_VERSION: u32 = 1;

/// The full on-disk envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub current: Option<PersistedGoal>,
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

impl Default for GoalFile {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            current: None,
        }
    }
}

/// Persisted shape of a single active goal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedGoal {
    pub objective: String,
    #[serde(default)]
    pub token_budget: Option<u32>,
    #[serde(default = "default_auto_continue")]
    pub auto_continue_enabled: bool,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub iterations: u32,
    #[serde(default)]
    pub session_id: Option<String>,
}

fn default_auto_continue() -> bool {
    true
}

/// Resolve the on-disk goal-state path. Honours the
/// `DEEPSEEK_TUI_GOAL_PATH` env var so unit / integration tests can
/// redirect writes away from the real `~/.deepseek/` and keep test
/// runs hermetic.
fn default_goal_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("DEEPSEEK_TUI_GOAL_PATH")
        && !p.is_empty()
    {
        return Some(PathBuf::from(p));
    }
    dirs::home_dir().map(|home| home.join(".deepseek").join(GOAL_FILE_NAME))
}

/// Load the persisted goal envelope. Returns `GoalFile::default()`
/// when the file doesn't exist or is corrupt — corruption never
/// stops the TUI from booting.
#[must_use]
pub fn load_goal() -> GoalFile {
    match default_goal_path() {
        Some(path) => load_goal_from(&path),
        None => GoalFile::default(),
    }
}

fn load_goal_from(path: &Path) -> GoalFile {
    let Ok(text) = fs::read_to_string(path) else {
        return GoalFile::default();
    };
    serde_json::from_str::<GoalFile>(&text).unwrap_or_default()
}

/// Save the given envelope. Failures are logged but never
/// propagated.
pub fn save_goal(file: &GoalFile) {
    let Some(path) = default_goal_path() else {
        return;
    };
    save_goal_to(&path, file);
}

fn save_goal_to(path: &Path, file: &GoalFile) {
    if let Some(parent) = path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        tracing::warn!(
            "Failed to create goal state dir {}: {err}",
            parent.display()
        );
        return;
    }
    let Ok(text) = serde_json::to_string_pretty(file) else {
        return;
    };
    if let Err(err) = fs::write(path, text) {
        tracing::warn!("Failed to write goal state {}: {err}", path.display());
    }
}

/// Wipe the persisted goal. Idempotent — no error when the file
/// is already absent.
pub fn clear_goal() {
    let Some(path) = default_goal_path() else {
        return;
    };
    clear_goal_at(&path);
}

fn clear_goal_at(path: &Path) {
    if !path.exists() {
        return;
    }
    if let Err(err) = fs::remove_file(path) {
        tracing::warn!("Failed to remove goal state {}: {err}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "deepseek-tui-goal-test-{name}-{}.json",
            std::process::id()
        ));
        p
    }

    #[test]
    fn roundtrip_persists_all_fields() {
        let path = tmp_path("roundtrip");
        let _ = fs::remove_file(&path);
        let file = GoalFile {
            schema_version: 1,
            current: Some(PersistedGoal {
                objective: "Refactor auth".into(),
                token_budget: Some(50_000),
                auto_continue_enabled: true,
                started_at: Some(Utc::now()),
                iterations: 7,
                session_id: Some("sess-xyz".into()),
            }),
        };
        save_goal_to(&path, &file);
        let loaded = load_goal_from(&path);
        let g = loaded.current.expect("current goal present");
        assert_eq!(g.objective, "Refactor auth");
        assert_eq!(g.token_budget, Some(50_000));
        assert!(g.auto_continue_enabled);
        assert_eq!(g.iterations, 7);
        assert_eq!(g.session_id.as_deref(), Some("sess-xyz"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_file_yields_empty() {
        let path = tmp_path("missing");
        let _ = fs::remove_file(&path);
        let loaded = load_goal_from(&path);
        assert!(loaded.current.is_none());
        assert_eq!(loaded.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn corrupt_file_falls_back_to_default() {
        let path = tmp_path("corrupt");
        fs::write(&path, "{not valid json").unwrap();
        let loaded = load_goal_from(&path);
        assert!(loaded.current.is_none());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn forward_compat_missing_fields_default() {
        let path = tmp_path("forward");
        fs::write(
            &path,
            r#"{"schema_version":1,"current":{"objective":"minimal"}}"#,
        )
        .unwrap();
        let loaded = load_goal_from(&path);
        let g = loaded.current.expect("current goal present");
        assert_eq!(g.objective, "minimal");
        assert_eq!(g.token_budget, None);
        assert!(g.auto_continue_enabled, "missing field defaults to true");
        assert_eq!(g.iterations, 0);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn clear_removes_file() {
        let path = tmp_path("clear");
        save_goal_to(
            &path,
            &GoalFile {
                schema_version: 1,
                current: Some(PersistedGoal {
                    objective: "X".into(),
                    token_budget: None,
                    auto_continue_enabled: true,
                    started_at: None,
                    iterations: 0,
                    session_id: None,
                }),
            },
        );
        assert!(path.exists());
        clear_goal_at(&path);
        assert!(!path.exists());
    }
}
