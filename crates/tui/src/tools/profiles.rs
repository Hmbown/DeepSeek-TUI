//! Agent profile presets — pre-configured agent behaviors.
//!
//! Each profile bundles a role, model, thinking depth, tool set,
//! and posture prompt so agents can be spawned with a single
//! profile name instead of configuring each parameter individually.
//!
//! # Built-in profiles
//!
//! - **code-reviewer**: thorough code review with security focus
//! - **architect**: system design and architecture planning
//! - **debugger**: root-cause analysis and bug hunting
//! - **documenter**: writes clear, comprehensive documentation
//! - **security-auditor**: security-focused code audit
//! - **performance-engineer**: identifies and fixes performance issues

use serde::{Deserialize, Serialize};

use crate::llm::unified::ThinkingMode;
use crate::tools::subagent::SubAgentType;

// ── Profile ──────────────────────────────────────────────────────────────────

/// A predefined agent profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    /// Human-readable name (e.g., "code-reviewer").
    pub name: String,
    /// Short description for display.
    pub description: String,
    /// The sub-agent type this profile maps to.
    pub agent_type: SubAgentType,
    /// Recommended model.
    pub model: String,
    /// Thinking depth for the recommended model.
    pub thinking: ThinkingMode,
    /// Core instructions injected into the system prompt.
    pub posture_prompt: String,
    /// Additional tools beyond the agent type's defaults.
    #[serde(default)]
    pub extra_tools: Vec<String>,
}

impl AgentProfile {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        agent_type: SubAgentType,
        model: impl Into<String>,
        thinking: ThinkingMode,
        posture_prompt: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            agent_type,
            model: model.into(),
            thinking,
            posture_prompt: posture_prompt.into(),
            extra_tools: Vec::new(),
        }
    }
}

// ── Profile registry ─────────────────────────────────────────────────────────

/// Registry of built-in and user-defined agent profiles.
#[derive(Debug, Clone, Default)]
pub struct ProfileRegistry {
    profiles: Vec<AgentProfile>,
}

impl ProfileRegistry {
    /// Create a registry pre-populated with built-in profiles.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut reg = Self::default();
        reg.add(AgentProfile::new(
            "code-reviewer",
            "Thorough code review with security focus",
            SubAgentType::Review,
            "deepseek-v4-flash",
            ThinkingMode::Medium,
            "You are a meticulous code reviewer. For each file:\n\
             1. Identify bugs and logic errors\n\
             2. Flag security vulnerabilities\n\
             3. Suggest improvements for readability and maintainability\n\
             4. Check adherence to project conventions\n\
             Be specific — cite line numbers and suggest concrete fixes.",
        ));
        reg.add(AgentProfile::new(
            "architect",
            "System design and architecture planning",
            SubAgentType::Plan,
            "deepseek-v4-pro",
            ThinkingMode::Deep,
            "You are a systems architect. Think in terms of:\n\
             - Component boundaries and interfaces\n\
             - Data flow and state management\n\
             - Scalability and fault tolerance\n\
             - Trade-offs between simplicity and flexibility\n\
             Produce clear architecture diagrams (ASCII art) and document decisions.",
        ));
        reg.add(AgentProfile::new(
            "debugger",
            "Root-cause analysis and bug hunting",
            SubAgentType::Explore,
            "deepseek-v4-pro",
            ThinkingMode::Deep,
            "You are a senior debugger. Your process:\n\
             1. Reproduce the issue from the description\n\
             2. Trace the call path from entry to failure\n\
             3. Identify the root cause (not just the symptom)\n\
             4. Propose a minimal, safe fix\n\
             Use log analysis, stack traces, and git blame to narrow the search.",
        ));
        reg.add(AgentProfile::new(
            "documenter",
            "Writes clear, comprehensive documentation",
            SubAgentType::Implementer,
            "deepseek-v4-flash",
            ThinkingMode::Light,
            "You are a technical writer. Produce documentation that:\n\
             - Explains WHY, not just WHAT\n\
             - Includes concrete examples\n\
             - Uses consistent terminology\n\
             - Is structured for skimming (headings, bullets, code blocks)\n\
             Target audience: experienced developers new to this codebase.",
        ));
        reg.add(AgentProfile::new(
            "security-auditor",
            "Security-focused code audit",
            SubAgentType::Review,
            "deepseek-v4-pro",
            ThinkingMode::Deep,
            "You are a security auditor. Check for:\n\
             - OWASP Top 10 vulnerabilities\n\
             - Injection attacks (SQL, command, template)\n\
             - Authentication and authorization bypasses\n\
             - Secret leakage (API keys, tokens in code)\n\
             - Unsafe deserialization and input validation gaps\n\
             Rate each finding: Critical / High / Medium / Low.",
        ));
        reg.add(AgentProfile::new(
            "performance-engineer",
            "Identifies and fixes performance issues",
            SubAgentType::Explore,
            "deepseek-v4-pro",
            ThinkingMode::Medium,
            "You are a performance engineer. Analyze for:\n\
             - Algorithmic complexity hotspots\n\
             - Memory allocation patterns\n\
             - I/O bottlenecks (disk, network, database)\n\
             - Caching opportunities\n\
             - Concurrency and lock contention\n\
             Provide before/after benchmarks where possible.",
        ));
        reg
    }

    /// Add a profile.
    pub fn add(&mut self, profile: AgentProfile) {
        self.profiles.push(profile);
    }

    /// Find a profile by name (case-insensitive).
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&AgentProfile> {
        let lower = name.to_lowercase();
        self.profiles
            .iter()
            .find(|p| p.name.to_lowercase() == lower)
    }

    /// List all profile names.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.profiles.iter().map(|p| p.name.as_str()).collect()
    }

    /// Number of registered profiles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Whether no profiles are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtins_include_all_six() {
        let reg = ProfileRegistry::with_builtins();
        assert_eq!(reg.len(), 6);
        assert!(reg.find("code-reviewer").is_some());
        assert!(reg.find("architect").is_some());
        assert!(reg.find("debugger").is_some());
        assert!(reg.find("documenter").is_some());
        assert!(reg.find("security-auditor").is_some());
        assert!(reg.find("performance-engineer").is_some());
    }

    #[test]
    fn test_find_case_insensitive() {
        let reg = ProfileRegistry::with_builtins();
        assert!(reg.find("CODE-REVIEWER").is_some());
        assert!(reg.find("Architect").is_some());
    }

    #[test]
    fn test_find_unknown_returns_none() {
        let reg = ProfileRegistry::with_builtins();
        assert!(reg.find("nonexistent").is_none());
    }

    #[test]
    fn test_profiles_have_distinct_postures() {
        let reg = ProfileRegistry::with_builtins();
        let reviewer = reg.find("code-reviewer").unwrap();
        let debugger = reg.find("debugger").unwrap();
        assert_ne!(reviewer.posture_prompt, debugger.posture_prompt);
    }

    #[test]
    fn test_names_returns_all() {
        let reg = ProfileRegistry::with_builtins();
        let names = reg.names();
        assert_eq!(names.len(), 6);
    }

    #[test]
    fn test_custom_profile() {
        let mut reg = ProfileRegistry::default();
        reg.add(AgentProfile::new(
            "my-custom",
            "Custom profile for testing",
            SubAgentType::General,
            "deepseek-v4-flash",
            ThinkingMode::Light,
            "Be concise.",
        ));
        assert_eq!(reg.len(), 1);
        assert!(reg.find("my-custom").is_some());
    }
}
