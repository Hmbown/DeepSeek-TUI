//! Agent profile data types and built-in predefined profiles.
//!
//! NOTE: This module is not yet wired into TUI dispatch paths.
//! All types and functions are kept public for follow-up integration.
#![allow(dead_code)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Permission action for a tool × path combination.
///
/// Mirrors the same pattern used by the execpolicy layer for
/// command-prefix rules, applied here per-profile so each agent
/// flavour can ship with different posture (e.g. reviewer denies
/// all writes, implementer auto-approves them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathRuleAction {
    /// Always allow the tool on matching paths.
    Allow,
    /// Always deny — stronger than Ask, overrides Allow at same specificity.
    Deny,
    /// Prompt the user (or the session approval mode) for a one-shot decision.
    Ask,
}

impl PathRuleAction {
    /// Parse from a TOML/serpent string. Case-insensitive.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "allow" => Some(Self::Allow),
            "deny" => Some(Self::Deny),
            "ask" | "prompt" => Some(Self::Ask),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Ask => "ask",
        }
    }
}

/// Named agent profile.
///
/// Profiles are resolved by [`super::manager::AgentProfileManager`] at
/// spawn time (for sub-agents via `agent_spawn`) or session time (via
/// `/agent <name>` slash command).  When a field is `None` the resolved
/// value falls back to the session default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    /// Canonical lower-case name: `"general"`, `"explore"`, `"plan"`,
    /// `"implementer"`, `"reviewer"`, `"builder"`, or a custom user key.
    pub name: String,

    /// Human-readable one-liner shown in `/agent list` and picker UIs.
    #[serde(default)]
    pub description: String,

    /// Override default model for this profile.  `None` = inherit from session.
    #[serde(default)]
    pub model: Option<String>,

    /// Override reasoning/thinking effort tier.  `None` = inherit from session.
    #[serde(default)]
    pub reasoning_effort: Option<String>,

    /// Tool allowlist.  `None` = all tools available (full registry).
    /// `Some(vec)` = only these tool names are visible and callable.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,

    /// Tool denylist.  `None` = nothing denied.
    /// `Some(vec)` = these tool names are removed from the effective set.
    ///
    /// Denial happens *after* the allowlist is applied, so a tool in both
    /// lists is denied.
    #[serde(default)]
    pub denied_tools: Option<Vec<String>>,

    /// Per-tool default path-permission posture.
    ///
    /// Keys are tool names (`"edit_file"`, `"write_file"`, `"exec_shell"`,
    /// `"*"` for catch-all).  When a profile carries `auto_approve: true`
    /// but a tool's permission is `Deny`, the deny wins — the point is
    /// that the implementer can auto-approve most writes while still
    /// refusing to touch `*.env` or `~/.ssh/*`.
    #[serde(default)]
    pub permissions: HashMap<String, PathRuleAction>,

    /// Extra text appended to the system prompt when this profile is active.
    /// Merged after the per-type prompt (`SubAgentType::system_prompt`).
    #[serde(default)]
    pub system_prompt_extension: Option<String>,

    /// When `true`, tool calls that would normally require approval are
    /// auto-approved (YOLO-lite).  Individual `permissions` entries can
    /// still deny specific tool × path combinations.
    #[serde(default)]
    pub auto_approve: bool,
}

// ---------------------------------------------------------------------------
// Predefined built-in profiles
// ---------------------------------------------------------------------------

/// Return every built-in profile in declaration order.
///
/// These are the base layer; user/project config merges on top.
/// Custom profiles live only in config and are added by the manager.
#[must_use]
pub fn builtin_profiles() -> Vec<AgentProfile> {
    vec![
        general_profile(),
        explore_profile(),
        plan_profile(),
        implementer_profile(),
        reviewer_profile(),
        builder_profile(),
    ]
}

/// Look up a single built-in profile by canonical name.
#[must_use]
pub fn builtin_profile(name: &str) -> Option<AgentProfile> {
    match name {
        "general" => Some(general_profile()),
        "explore" => Some(explore_profile()),
        "plan" => Some(plan_profile()),
        "implementer" => Some(implementer_profile()),
        "reviewer" => Some(reviewer_profile()),
        "builder" => Some(builder_profile()),
        _ => None,
    }
}

// --- general ---

fn general_profile() -> AgentProfile {
    AgentProfile {
        name: "general".to_string(),
        description: "All tools, ask for writes (default)".to_string(),
        model: None,
        reasoning_effort: None,
        allowed_tools: None, // full registry
        denied_tools: None,
        permissions: HashMap::new(),
        system_prompt_extension: None,
        auto_approve: false,
    }
}

// --- explore ---

fn explore_tools() -> Vec<String> {
    vec![
        "read_file",
        "list_dir",
        "grep_files",
        "file_search",
        "project_map",
        "diagnostics",
        "web_search",
        "web.run",
        "fetch_url",
        "finance",
        "validate_data",
        "load_skill",
        "recall_archive",
        "note",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn explore_profile() -> AgentProfile {
    AgentProfile {
        name: "explore".to_string(),
        description: "Read-only exploration — read_file, grep_files, file_search, \
                      web_search, fetch_url, list_dir, plus diagnostics and recall"
            .to_string(),
        model: None,
        reasoning_effort: None,
        allowed_tools: Some(explore_tools()),
        denied_tools: None,
        permissions: HashMap::new(),
        system_prompt_extension: Some(
            "You are an explorer. You are read-only — do not write, patch, or \
             run side-effectful commands. If the task seems to require a write, \
             stop and list it under BLOCKERS."
                .to_string(),
        ),
        auto_approve: false,
    }
}

// --- plan ---

fn plan_tools() -> Vec<String> {
    let mut tools = explore_tools();
    tools.extend(
        vec![
            "update_plan",
            "checklist_write",
            "checklist_add",
            "checklist_update",
            "checklist_list",
            "todo_write",
            "todo_add",
            "todo_update",
            "todo_list",
            "agent_spawn",
            "spawn_agent",
            "delegate_to_agent",
            "agent_result",
            "agent_list",
            "agent_wait",
            "wait",
            "agent_send_input",
            "send_input",
            "agent_assign",
            "assign_agent",
            "agent_cancel",
            "close_agent",
            "agent_resume",
            "resume_agent",
        ]
        .into_iter()
        .map(String::from),
    );
    tools
}

fn plan_profile() -> AgentProfile {
    AgentProfile {
        name: "plan".to_string(),
        description:
            "Read-only + plan tools — same as explore + update_plan, checklist_*, \
             agent_spawn (read-only children)"
                .to_string(),
        model: None,
        reasoning_effort: None,
        allowed_tools: Some(plan_tools()),
        denied_tools: None,
        permissions: HashMap::new(),
        system_prompt_extension: Some(
            "You are a planner. Keep writes to a minimum (notes and plan \
             artifacts only); avoid patches and shell side effects."
                .to_string(),
        ),
        auto_approve: false,
    }
}

// --- implementer ---

fn implementer_profile() -> AgentProfile {
    let mut permissions = HashMap::new();
    // Auto-approve common write tools — the implementer is trusted to
    // land changes.  Path-level deny rules (e.g. *.env) can still be
    // layered on top via user config.
    permissions.insert("edit_file".to_string(), PathRuleAction::Allow);
    permissions.insert("write_file".to_string(), PathRuleAction::Allow);
    permissions.insert("apply_patch".to_string(), PathRuleAction::Allow);
    // Shell is auto-approved for build/test/lint; dangerous commands are
    // gated by the execpolicy engine, not the profile.
    permissions.insert("exec_shell".to_string(), PathRuleAction::Allow);
    permissions.insert("task_shell_start".to_string(), PathRuleAction::Allow);

    AgentProfile {
        name: "implementer".to_string(),
        description:
            "Full tools, auto-approve writes with path permission checks".to_string(),
        model: None,
        reasoning_effort: None,
        allowed_tools: None, // full registry
        denied_tools: None,
        permissions,
        system_prompt_extension: Some(
            "You are an implementation sub-agent. Your job is to land the change \
             the parent assigned to you — write the code, modify the files, satisfy \
             the contract — with the *minimum* surrounding edit."
                .to_string(),
        ),
        auto_approve: true,
    }
}

// --- reviewer ---

fn reviewer_tools() -> Vec<String> {
    vec![
        "read_file",
        "list_dir",
        "grep_files",
        "file_search",
        "project_map",
        "diagnostics",
        "review",
        "github_issue_context",
        "github_pr_context",
        "github_comment",
        "github_close_issue",
        "validate_data",
        "load_skill",
        "recall_archive",
        "note",
        "web_search",
        "web.run",
        "fetch_url",
        "finance",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn reviewer_profile() -> AgentProfile {
    AgentProfile {
        name: "reviewer".to_string(),
        description: "Read-only + review + GitHub issue/PR/comment tools".to_string(),
        model: None,
        reasoning_effort: None,
        allowed_tools: Some(reviewer_tools()),
        denied_tools: Some(vec![
            "edit_file".to_string(),
            "write_file".to_string(),
            "apply_patch".to_string(),
            "exec_shell".to_string(),
            "exec_shell_wait".to_string(),
            "exec_shell_interact".to_string(),
            "exec_wait".to_string(),
            "exec_interact".to_string(),
            "task_shell_start".to_string(),
            "task_shell_wait".to_string(),
            "run_tests".to_string(),
        ]),
        permissions: HashMap::new(),
        system_prompt_extension: Some(
            "You are a code reviewer. Focus on correctness, security, and style. \
             Do not patch the code under review even if a fix is obvious; describe \
             the fix in the finding so the parent can apply it."
                .to_string(),
        ),
        auto_approve: false,
    }
}

// --- builder ---

fn builder_tools() -> Vec<String> {
    vec![
        "read_file",
        "list_dir",
        "grep_files",
        "file_search",
        "diagnostics",
        "exec_shell",
        "exec_shell_wait",
        "exec_shell_interact",
        "exec_wait",
        "exec_interact",
        "run_tests",
        "task_shell_start",
        "task_shell_wait",
        "note",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn builder_profile() -> AgentProfile {
    AgentProfile {
        name: "builder".to_string(),
        description: "exec_shell (build/test/lint commands only), read_file, \
                      task_shell_start"
            .to_string(),
        model: None,
        reasoning_effort: None,
        allowed_tools: Some(builder_tools()),
        denied_tools: None,
        permissions: HashMap::new(),
        system_prompt_extension: Some(
            "You are a build-and-test agent. Run the project's build, test, and \
             lint commands and report pass/fail with evidence. Do not modify \
             source files."
                .to_string(),
        ),
        auto_approve: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_are_all_present() {
        let profiles = builtin_profiles();
        assert_eq!(profiles.len(), 6);
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"general"));
        assert!(names.contains(&"explore"));
        assert!(names.contains(&"plan"));
        assert!(names.contains(&"implementer"));
        assert!(names.contains(&"reviewer"));
        assert!(names.contains(&"builder"));
    }

    #[test]
    fn path_rule_action_parse_round_trips() {
        assert_eq!(PathRuleAction::parse("allow"), Some(PathRuleAction::Allow));
        assert_eq!(PathRuleAction::parse("ALLOW"), Some(PathRuleAction::Allow));
        assert_eq!(PathRuleAction::parse("deny"), Some(PathRuleAction::Deny));
        assert_eq!(PathRuleAction::parse("ask"), Some(PathRuleAction::Ask));
        assert_eq!(PathRuleAction::parse("prompt"), Some(PathRuleAction::Ask));
        assert_eq!(PathRuleAction::parse("unknown"), None);
    }

    #[test]
    fn implementer_auto_approves_writes() {
        let p = implementer_profile();
        assert!(p.auto_approve);
        assert_eq!(p.permissions.get("edit_file"), Some(&PathRuleAction::Allow));
        assert_eq!(p.permissions.get("write_file"), Some(&PathRuleAction::Allow));
    }

    #[test]
    fn explorer_is_read_only() {
        let p = explore_profile();
        assert!(!p.auto_approve);
        let tools = p.allowed_tools.as_ref().unwrap();
        assert!(tools.contains(&"read_file".to_string()));
        assert!(!tools.contains(&"edit_file".to_string()));
        assert!(!tools.contains(&"exec_shell".to_string()));
    }

    #[test]
    fn builder_allows_shell_and_test() {
        let p = builder_profile();
        assert!(p.auto_approve);
        let tools = p.allowed_tools.as_ref().unwrap();
        assert!(tools.contains(&"exec_shell".to_string()));
        assert!(tools.contains(&"run_tests".to_string()));
        assert!(!tools.contains(&"edit_file".to_string()));
    }
}
