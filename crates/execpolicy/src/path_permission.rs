//! Path-based permission rules for tools.
//!
//! Extends the command-prefix ExecPolicyEngine with file-path-level rules
//! that control which tools can access which paths. Rules support wildcard
//! patterns (*, **) and home-directory expansion.
//!
//! ## Rule ordering
//!
//! Rules are ordered — first match wins. A `Deny` rule at any position
//! blocks the tool regardless of later `Allow` rules (deny-always-wins
//! semantics).

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

// ── Types ─────────────────────────────────────────────────────────────

/// The action to take for a matched rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathRuleAction {
    /// Always allow — skip approval.
    Allow,
    /// Always deny — block the tool call.
    Deny,
    /// Ask the user for approval.
    Ask,
}

/// A single permission rule: a tool name pattern × path pattern × action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathPermissionRule {
    /// Tool name to match (e.g. "edit_file", "exec_shell", or "*" for all).
    pub tool: String,
    /// Path glob pattern (e.g. "*.env", "src/**", "~/.ssh/*").
    pub pattern: String,
    /// What to do when this rule matches.
    pub action: PathRuleAction,
}

/// Result of a permission check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathPermissionResult {
    /// Tool is allowed on this path — skip approval.
    Allowed,
    /// Tool is denied on this path — block execution.
    Denied { reason: String },
    /// User must approve this tool on this path.
    NeedsApproval,
}

// ── Engine ────────────────────────────────────────────────────────────

/// Manages path-based permission rules for tool execution.
pub struct PathPermissionEngine {
    /// Ordered rules — first match wins, deny takes precedence.
    rules: Vec<PathPermissionRule>,
    /// Pending approvals that were granted for this session (tool, path).
    session_allow_all: HashSet<(String, String)>,
}

impl PathPermissionEngine {
    /// Build an engine from a list of rules.
    pub fn new(rules: Vec<PathPermissionRule>) -> Self {
        Self {
            rules,
            session_allow_all: HashSet::new(),
        }
    }

    /// Create an empty engine (no rules — everything needs approval).
    pub fn empty() -> Self {
        Self {
            rules: Vec::new(),
            session_allow_all: HashSet::new(),
        }
    }

    /// Check whether `tool_name` is allowed to operate on `file_path`.
    ///
    /// The check runs through the ordered rules list. First matching rule
    /// determines the result. If no rule matches, returns `NeedsApproval`.
    pub fn check(
        &self,
        tool_name: &str,
        file_path: &Path,
    ) -> PathPermissionResult {
        // Check session approvals first
        let path_str = file_path.to_string_lossy().to_string();
        if self.session_allow_all.contains(&(tool_name.to_string(), path_str.clone())) {
            return PathPermissionResult::Allowed;
        }

        // Check deny rules first (deny-always-wins)
        for rule in &self.rules {
            if !self.tool_matches(&rule.tool, tool_name) {
                continue;
            }
            if !self.path_matches(&rule.pattern, &path_str) {
                continue;
            }
            match rule.action {
                PathRuleAction::Deny => {
                    return PathPermissionResult::Denied {
                        reason: format!(
                            "path '{}' denied for tool '{}' by rule '{}'",
                            path_str, tool_name, rule.pattern
                        ),
                    };
                }
                PathRuleAction::Allow => {
                    return PathPermissionResult::Allowed;
                }
                PathRuleAction::Ask => {
                    return PathPermissionResult::NeedsApproval;
                }
            }
        }

        // No rule matched — needs approval
        PathPermissionResult::NeedsApproval
    }

    /// Remember that the user allowed this tool on this path for the session.
    /// Cascades: matching pending requests auto-resolve.
    pub fn remember_allow(&mut self, tool_name: &str, file_path: &Path) {
        let path_str = file_path.to_string_lossy().to_string();
        self.session_allow_all
            .insert((tool_name.to_string(), path_str));
    }

    /// Add a rule at runtime (from user approval dialog).
    pub fn add_rule(&mut self, rule: PathPermissionRule) {
        // Avoid duplicates
        if !self.rules.iter().any(|r| r.tool == rule.tool && r.pattern == rule.pattern) {
            self.rules.push(rule);
        }
    }

    /// Number of rules.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the rule list is empty.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    // ── Private helpers ──────────────────────────────────────────────

    fn tool_matches(&self, pattern: &str, tool_name: &str) -> bool {
        pattern == "*" || pattern == tool_name
    }

    fn path_matches(&self, pattern: &str, path_str: &str) -> bool {
        let expanded = self.expand_home(pattern);
        simple_glob_match(&expanded, path_str)
    }

    fn expand_home(&self, pattern: &str) -> String {
        if pattern.starts_with("~/") || pattern == "~" {
            if let Some(home) = dirs::home_dir() {
                let home_str = home.to_string_lossy().to_string();
                pattern.replacen("~", &home_str, 1)
            } else {
                pattern.to_string()
            }
        } else {
            pattern.to_string()
        }
    }
}

// ── Glob matching ─────────────────────────────────────────────────────

/// Simple glob matching. Supports `*` (any chars in one path segment) and
/// `**` (any chars across path segments).
fn simple_glob_match(pattern: &str, path: &str) -> bool {
    if pattern.contains("**") {
        let prefix = pattern.trim_end_matches("**").trim_end_matches('/');
        if prefix.is_empty() {
            return true;
        }
        path.starts_with(prefix)
    } else if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        let mut remaining = path;
        for (i, part) in parts.iter().enumerate() {
            if i == 0 {
                if !remaining.starts_with(part) {
                    return false;
                }
                remaining = &remaining[part.len()..];
            } else if i == parts.len() - 1 {
                return remaining.ends_with(part);
            } else if let Some(pos) = remaining.find(part) {
                remaining = &remaining[pos + part.len()..];
            } else {
                return false;
            }
        }
        true
    } else {
        path.contains(pattern)
    }
}

// ── Config serialization ──────────────────────────────────────────────

/// TOML configuration for path permissions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionsConfig {
    #[serde(default)]
    pub path_rules: Vec<PathPermissionRule>,
}

impl PermissionsConfig {
    /// Load from user config file, falling back to empty.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save to a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_takes_precedence() {
        let rules = vec![
            PathPermissionRule {
                tool: "edit_file".into(),
                pattern: "*.env".into(),
                action: PathRuleAction::Deny,
            },
            PathPermissionRule {
                tool: "*".into(),
                pattern: "*.env".into(),
                action: PathRuleAction::Allow,
            },
        ];
        let engine = PathPermissionEngine::new(rules);
        let result = engine.check("edit_file", Path::new("/app/.env"));
        assert!(matches!(result, PathPermissionResult::Denied { .. }));
    }

    #[test]
    fn allow_matches_specific_tool() {
        let rules = vec![PathPermissionRule {
            tool: "read_file".into(),
            pattern: "src/**".into(),
            action: PathRuleAction::Allow,
        }];
        let engine = PathPermissionEngine::new(rules);
        assert!(matches!(
            engine.check("read_file", Path::new("src/main.rs")),
            PathPermissionResult::Allowed
        ));
        assert!(matches!(
            engine.check("edit_file", Path::new("src/main.rs")),
            PathPermissionResult::NeedsApproval
        ));
    }

    #[test]
    fn wildcard_tool_matches_all() {
        let rules = vec![PathPermissionRule {
            tool: "*".into(),
            pattern: "*.lock".into(),
            action: PathRuleAction::Deny,
        }];
        let engine = PathPermissionEngine::new(rules);
        assert!(matches!(
            engine.check("edit_file", Path::new("Cargo.lock")),
            PathPermissionResult::Denied { .. }
        ));
        assert!(matches!(
            engine.check("write_file", Path::new("yarn.lock")),
            PathPermissionResult::Denied { .. }
        ));
    }

    #[test]
    fn no_rules_means_needs_approval() {
        let engine = PathPermissionEngine::empty();
        assert_eq!(
            engine.check("read_file", Path::new("anything.rs")),
            PathPermissionResult::NeedsApproval
        );
    }

    #[test]
    fn session_allow_remembered() {
        let mut engine = PathPermissionEngine::empty();
        engine.remember_allow("edit_file", Path::new("config.toml"));
        assert_eq!(
            engine.check("edit_file", Path::new("config.toml")),
            PathPermissionResult::Allowed
        );
        // Different file still needs approval
        assert_eq!(
            engine.check("edit_file", Path::new("other.toml")),
            PathPermissionResult::NeedsApproval
        );
    }

    #[test]
    fn simple_glob_star() {
        assert!(simple_glob_match("*.rs", "main.rs"));
        assert!(!simple_glob_match("*.rs", "main.py"));
    }

    #[test]
    fn simple_glob_double_star() {
        assert!(simple_glob_match("src/**", "src/main.rs"));
        assert!(simple_glob_match("src/**", "src/sub/deep/mod.rs"));
        assert!(!simple_glob_match("src/**", "tests/main.rs"));
    }

    #[test]
    fn simple_glob_contains() {
        assert!(simple_glob_match(".env", "/home/user/project/.env"));
        assert!(!simple_glob_match(".env", "/home/user/config.yml"));
    }
}
