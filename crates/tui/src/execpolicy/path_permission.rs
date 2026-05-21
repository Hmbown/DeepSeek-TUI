//! Path-based permission rules for tools.
//!
//! Extends the command-prefix ExecPolicyEngine with file-path-level rules
//! that control which tools can access which paths. Rules support wildcard
//! patterns (*, **) and home-directory expansion.
//!
//! ## Rule ordering
//!
//! Deny rules always take precedence regardless of position — all Deny
//! rules are checked before any Allow/Ask rules (deny-always-wins
//! semantics).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    /// Ordered rules — deny checked first, then allow/ask in order.
    rules: Vec<PathPermissionRule>,
    /// Pending approvals that were granted for this session (tool, path).
    session_allow_all: HashSet<(String, String)>,
    /// Cached home directory for pattern expansion.
    home_dir: Option<PathBuf>,
}

impl PathPermissionEngine {
    /// Build an engine from a list of rules.
    pub fn new(rules: Vec<PathPermissionRule>) -> Self {
        Self {
            rules,
            session_allow_all: HashSet::new(),
            home_dir: dirs::home_dir(),
        }
    }

    /// Create an empty engine (no rules — everything needs approval).
    pub fn empty() -> Self {
        Self {
            rules: Vec::new(),
            session_allow_all: HashSet::new(),
            home_dir: dirs::home_dir(),
        }
    }

    /// Check whether `tool_name` is allowed to operate on `file_path`.
    ///
    /// Deny rules are always checked first (deny-always-wins), then
    /// Allow/Ask rules are checked in order (first match wins). If no rule
    /// matches, returns `NeedsApproval`.
    pub fn check(
        &self,
        tool_name: &str,
        file_path: &Path,
    ) -> PathPermissionResult {
        // Session approvals: shortcuts past all rules.
        let path_str = file_path.to_string_lossy().to_string();
        if self.session_allow_all.contains(&(tool_name.to_string(), path_str.clone())) {
            return PathPermissionResult::Allowed;
        }

        let normalized = normalize_path(&path_str);

        // Pass 1: check all Deny rules first (deny-always-wins).
        for rule in &self.rules {
            if rule.action != PathRuleAction::Deny {
                continue;
            }
            if !self.tool_matches(&rule.tool, tool_name) {
                continue;
            }
            if !self.path_matches(&rule.pattern, &normalized) {
                continue;
            }
            return PathPermissionResult::Denied {
                reason: format!(
                    "path '{}' denied for tool '{}' by rule '{}'",
                    path_str, tool_name, rule.pattern
                ),
            };
        }

        // Pass 2: check Allow/Ask rules (first match wins).
        for rule in &self.rules {
            if !self.tool_matches(&rule.tool, tool_name) {
                continue;
            }
            if !self.path_matches(&rule.pattern, &normalized) {
                continue;
            }
            match rule.action {
                PathRuleAction::Allow => {
                    return PathPermissionResult::Allowed;
                }
                PathRuleAction::Ask => {
                    return PathPermissionResult::NeedsApproval;
                }
                PathRuleAction::Deny => {
                    unreachable!("Deny rules handled in pass 1")
                }
            }
        }

        // No rule matched — needs approval.
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
    /// New rules are inserted at the front so user-granted permissions
    /// take precedence over existing broader rules.
    pub fn add_rule(&mut self, rule: PathPermissionRule) {
        self.rules.retain(|r| r.tool != rule.tool || r.pattern != rule.pattern);
        self.rules.insert(0, rule);
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
            if let Some(ref home) = self.home_dir {
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

// ── Path normalization ────────────────────────────────────────────────

/// Normalize a path string: collapse redundant separators, resolve `.` and
/// `..` components so matching cannot be bypassed with `./foo` or `a//b`.
fn normalize_path(path: &str) -> String {
    let p = Path::new(path);
    let mut out = String::with_capacity(path.len());
    for component in p.components() {
        use std::path::Component;
        match component {
            Component::RootDir => out.push('/'),
            Component::CurDir => {}
            Component::ParentDir => {
                // Pop last segment
                if let Some(pos) = out.rfind('/') {
                    out.truncate(pos);
                } else {
                    out.clear();
                }
            }
            Component::Normal(s) => {
                if !out.is_empty() && !out.ends_with('/') {
                    out.push('/');
                }
                out.push_str(&s.to_string_lossy());
            }
            Component::Prefix(_) => {
                out.push_str(&component.as_os_str().to_string_lossy());
            }
        }
    }
    if out.is_empty() && path.starts_with('/') {
        out.push('/');
    }
    out
}

// ── Glob matching ─────────────────────────────────────────────────────

/// Simple glob matching. Supports:
/// - `**` — matches zero or more path segments (must be at a segment boundary).
/// - `*` — matches any chars within a single path segment (does NOT cross `/`).
/// - No wildcards — exact path match, or trailing filename match.
fn simple_glob_match(pattern: &str, path: &str) -> bool {
    if pattern.contains("**") {
        let prefix = pattern.trim_end_matches("**").trim_end_matches('/');
        if prefix.is_empty() {
            return true;
        }
        // Require segment boundary: path == prefix OR path starts with prefix/
        path == prefix || path.starts_with(&format!("{}/", prefix))
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
                // Final part: must match the end without crossing a separator.
                if remaining.ends_with(part) {
                    // The matched region (between previous part and this one)
                    // must not contain '/'. remaining ends with `part`, so the
                    // matched region is remaining[..remaining.len()-part.len()].
                    if remaining.len() >= part.len() {
                        let matched = &remaining[..remaining.len() - part.len()];
                        if matched.contains('/') {
                            return false;
                        }
                    }
                    return true;
                }
                return false;
            } else {
                // Middle part: find within the current segment (no '/' crossing).
                if let Some(pos) = remaining.find(part) {
                    let matched = &remaining[..pos];
                    if matched.contains('/') {
                        return false;
                    }
                    remaining = &remaining[pos + part.len()..];
                } else {
                    return false;
                }
            }
        }
        true
    } else {
        // Non-glob: exact path match, or matches the trailing filename portion.
        path == pattern || path.ends_with(&format!("/{}", pattern))
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
    /// Logs parse errors so users can diagnose misconfigured rules.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str::<PermissionsConfig>(&content) {
                Ok(config) => config,
                Err(e) => {
                    tracing::warn!(
                        "failed to parse path-permission config {}: {e}",
                        path.display()
                    );
                    Self::default()
                }
            },
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        "failed to read path-permission config {}: {e}",
                        path.display()
                    );
                }
                Self::default()
            }
        }
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
    fn deny_takes_precedence_over_allow() {
        // Deny is checked before Allow regardless of order.
        let rules = vec![
            PathPermissionRule {
                tool: "*".into(),
                pattern: "*.env".into(),
                action: PathRuleAction::Allow,
            },
            PathPermissionRule {
                tool: "edit_file".into(),
                pattern: "*.env".into(),
                action: PathRuleAction::Deny,
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
        assert_eq!(
            engine.check("edit_file", Path::new("other.toml")),
            PathPermissionResult::NeedsApproval
        );
    }

    // ── Glob matching tests ──────────────────────────────────────────

    #[test]
    fn simple_glob_star() {
        assert!(simple_glob_match("*.rs", "main.rs"));
        assert!(!simple_glob_match("*.rs", "main.py"));
    }

    #[test]
    fn star_does_not_cross_separator() {
        // * matches chars within one segment — must not cross '/'.
        assert!(!simple_glob_match("*.rs", "src/main.rs"));
        assert!(!simple_glob_match("src/*", "src/sub/main.rs"));
    }

    #[test]
    fn simple_glob_double_star() {
        assert!(simple_glob_match("src/**", "src/main.rs"));
        assert!(simple_glob_match("src/**", "src/sub/deep/mod.rs"));
        assert!(!simple_glob_match("src/**", "tests/main.rs"));
    }

    #[test]
    fn double_star_respects_segment_boundary() {
        // src/** should NOT match src_backup/file.rs
        assert!(!simple_glob_match("src/**", "src_backup/main.rs"));
        assert!(simple_glob_match("src/**", "src/main.rs"));
        assert!(simple_glob_match("src/**", "src/x"));
        // Edge: prefix-only with trailing /
        assert!(simple_glob_match("src/**", "src"));
    }

    #[test]
    fn non_glob_exact_or_trailing_filename() {
        // Exact match.
        assert!(simple_glob_match(".env", ".env"));
        // Trailing filename match.
        assert!(simple_glob_match(".env", "/home/user/project/.env"));
        // Does NOT substring-match in the middle.
        assert!(!simple_glob_match(".env", "/home/user/.env.example"));
        assert!(!simple_glob_match("passwd", "/tmp/passwd/copy"));
    }

    // ── Normalization tests ──────────────────────────────────────────

    #[test]
    fn normalization_collapses_redundant_separators() {
        assert_eq!(normalize_path("a//b"), "a/b");
    }

    #[test]
    fn normalization_resolves_current_dir() {
        assert_eq!(normalize_path("./secret.txt"), "secret.txt");
    }

    #[test]
    fn normalization_prevents_dot_slash_bypass() {
        let rules = vec![PathPermissionRule {
            tool: "*".into(),
            pattern: "secret.txt".into(),
            action: PathRuleAction::Deny,
        }];
        let engine = PathPermissionEngine::new(rules);
        assert!(matches!(
            engine.check("read_file", Path::new("./secret.txt")),
            PathPermissionResult::Denied { .. }
        ));
    }

    #[test]
    fn new_rules_prepend_for_user_override() {
        let mut engine = PathPermissionEngine::new(vec![PathPermissionRule {
            tool: "edit_file".into(),
            pattern: "*.env".into(),
            action: PathRuleAction::Deny,
        }]);
        // User explicitly allows a specific .env file.
        engine.add_rule(PathPermissionRule {
            tool: "edit_file".into(),
            pattern: ".env.local".into(),
            action: PathRuleAction::Allow,
        });
        // The new Allow rule is at the front, so it matches first in pass 2.
        assert!(matches!(
            engine.check("edit_file", Path::new(".env.local")),
            PathPermissionResult::Allowed
        ));
        // But a Deny for the same tool+pattern still wins because deny is
        // checked in pass 1.
        assert!(matches!(
            engine.check("edit_file", Path::new(".env.production")),
            PathPermissionResult::Denied { .. }
        ));
    }

    #[test]
    fn add_rule_replaces_existing_same_tool_and_pattern() {
        let mut engine = PathPermissionEngine::new(vec![PathPermissionRule {
            tool: "edit_file".into(),
            pattern: "*.env".into(),
            action: PathRuleAction::Ask,
        }]);
        // Change Ask to Allow for the same tool+pattern.
        engine.add_rule(PathPermissionRule {
            tool: "edit_file".into(),
            pattern: "*.env".into(),
            action: PathRuleAction::Allow,
        });
        assert_eq!(engine.len(), 1);
        assert!(matches!(
            engine.check("edit_file", Path::new(".env")),
            PathPermissionResult::Allowed
        ));
    }
}
