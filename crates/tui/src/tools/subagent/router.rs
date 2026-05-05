//! Agent capability router — routes task descriptions to the optimal
//! sub-agent type using a keyword registry with LLM fallback.
//!
//! # Architecture
//!
//! 1. **Keyword registry** — fast, deterministic matching. Keywords map to
//!    agent types with priorities. Higher priority = checked first.
//! 2. **LLM fallback** — when no keyword matches, an optional LLM call
//!    classifies the task. Stubbed for now; wires into the existing
//!    `DeepSeekClient` when enabled.
//!
//! # Example
//!
//! ```ignore
//! let mut router = AgentRouter::with_defaults();
//! assert_eq!(router.route("write a parser"), Some(SubAgentType::Implementer));
//! assert_eq!(router.route("review this PR"), Some(SubAgentType::Review));
//! ```

use super::SubAgentType;

// ── Capability entry ────────────────────────────────────────────────────────

/// A single capability entry in the router registry.
#[derive(Debug, Clone)]
struct CapabilityEntry {
    /// Case-insensitive keywords that, when found in the task description,
    /// trigger this capability.
    keywords: Vec<String>,
    /// The agent type best suited for this capability.
    agent_type: SubAgentType,
    /// Priority (higher = checked first, up to 255).
    priority: u8,
}

// ── Router ───────────────────────────────────────────────────────────────────

/// Routes task descriptions to the optimal sub-agent type.
///
/// Built with a sensible default registry covering the common agent
/// types (implement, explore, review, verify, plan). Callers can
/// extend with `register()`.
#[derive(Debug, Clone)]
pub struct AgentRouter {
    registry: Vec<CapabilityEntry>,
    /// When true (default), `route()` returns `Some(SubAgentType::General)`
    /// for unmatched tasks instead of `None`. The LLM fallback is a stub
    /// for now — General agents inherit the full tool surface.
    llm_fallback: bool,
}

impl Default for AgentRouter {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl AgentRouter {
    /// Create a router with the default capability registry.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut router = Self {
            registry: Vec::new(),
            llm_fallback: true,
        };
        // Priority 2: write/edit/patch/implement/build/create (narrower than priority 1)
        router.register(
            vec![
                "write",
                "edit_file",
                "apply_patch",
                "implement",
                "implementation",
                "build",
                "modify",
                "refactor",
                "rewrite",
            ],
            SubAgentType::Implementer,
            2,
        );
        // Priority 2: explore/search/find/inspect
        router.register(
            vec![
                "explore",
                "search",
                "find",
                "inspect",
                "look",
                "grep",
                "locate",
                "trace",
                "scan",
                "survey",
                "investigate",
                "read",
                "browse",
            ],
            SubAgentType::Explore,
            2,
        );
        // Priority 1: review/audit/check (lowest — overridden by action keywords)
        router.register(
            vec![
                "review", "audit", "check", "assess", "grade", "critique", "analyze", "examine",
            ],
            SubAgentType::Review,
            1,
        );
        // Priority 1: test/verify/validate
        router.register(
            vec!["run tests", "test suite", "verify", "validate", "coverage"],
            SubAgentType::Verifier,
            1,
        );
        // Priority 1: plan/design/architect
        router.register(
            vec![
                "plan",
                "design",
                "architect",
                "architecture",
                "strategy",
                "proposal",
            ],
            SubAgentType::Plan,
            1,
        );

        // Sort by priority descending
        router.registry.sort_by(|a, b| b.priority.cmp(&a.priority));
        router
    }

    /// Create an empty router with no registered capabilities.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            registry: Vec::new(),
            llm_fallback: false,
        }
    }

    /// Register a set of case-insensitive keywords that map to an agent type.
    /// Keywords are lowercased on registration. Within the same priority,
    /// registration order determines check order.
    pub fn register(
        &mut self,
        keywords: Vec<impl Into<String>>,
        agent_type: SubAgentType,
        priority: u8,
    ) {
        let entry = CapabilityEntry {
            keywords: keywords
                .into_iter()
                .map(|k| k.into().to_lowercase())
                .collect(),
            agent_type,
            priority,
        };
        self.registry.push(entry);
        self.registry.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Enable or disable LLM fallback for unmatched tasks.
    pub fn set_llm_fallback(&mut self, enabled: bool) {
        self.llm_fallback = enabled;
    }

    /// Route a task description to the best agent type.
    ///
    /// Returns `None` when no keyword matches and LLM fallback is disabled.
    /// With fallback enabled (default), returns `Some(SubAgentType::General)`
    /// for unmatched tasks.
    #[must_use]
    pub fn route(&self, task: &str) -> Option<SubAgentType> {
        self.route_with_meta(task).0
    }

    /// Route with metadata: returns `(agent_type, used_fallback)`.
    /// `used_fallback` is true when no keyword matched and LLM fallback
    /// produced the result.
    #[must_use]
    pub fn route_with_meta(&self, task: &str) -> (Option<SubAgentType>, bool) {
        let task_lower = task.to_lowercase();

        // Collect all keyword matches with their priority and keyword length.
        // Within the same priority, prefer the longest matching keyword
        // (most specific). Across priorities, higher priority wins.
        let mut best: Option<(&CapabilityEntry, usize)> = None; // (entry, keyword_len)

        for entry in &self.registry {
            for keyword in &entry.keywords {
                if task_lower.contains(keyword.as_str()) {
                    let kw_len = keyword.len();
                    match best {
                        None => {
                            best = Some((entry, kw_len));
                        }
                        Some((ref best_entry, best_len)) => {
                            if entry.priority > best_entry.priority
                                || (entry.priority == best_entry.priority && kw_len > best_len)
                            {
                                best = Some((entry, kw_len));
                            }
                        }
                    }
                }
            }
        }

        if let Some((entry, _)) = best {
            (Some(entry.agent_type.clone()), false)
        } else if self.llm_fallback {
            (Some(SubAgentType::General), true)
        } else {
            (None, false)
        }
    }

    /// Number of registered capability entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.registry.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_routes_implement_tasks() {
        let router = AgentRouter::with_defaults();
        for task in [
            "write a JSON parser",
            "implement the login endpoint",
            "refactor the auth module",
            "build the feature flag system",
            "modify the config loader",
        ] {
            let (agent_type, fallback) = router.route_with_meta(task);
            assert!(
                !fallback,
                "task '{task}' should match a keyword, not fallback"
            );
            assert_eq!(
                agent_type,
                Some(SubAgentType::Implementer),
                "task '{task}' should route to Implementer"
            );
        }
    }

    #[test]
    fn test_default_registry_routes_explore_tasks() {
        let router = AgentRouter::with_defaults();
        for task in [
            "explore the module structure",
            "find all uses of this function",
            "search for TODO comments",
            "inspect the parser internals",
            "survey the error handling patterns",
            "trace the call path from entry point",
            "read the configuration files",
        ] {
            let (agent_type, fallback) = router.route_with_meta(task);
            assert!(!fallback, "task '{task}' should match a keyword");
            assert_eq!(
                agent_type,
                Some(SubAgentType::Explore),
                "task '{task}' should route to Explore"
            );
        }
    }

    #[test]
    fn test_default_registry_routes_review_tasks() {
        let router = AgentRouter::with_defaults();
        for task in [
            "review this pull request",
            "audit the security model",
            "assess code quality",
            "analyze the module dependencies",
        ] {
            let (agent_type, fallback) = router.route_with_meta(task);
            assert!(!fallback, "task '{task}' should match a keyword");
            assert_eq!(
                agent_type,
                Some(SubAgentType::Review),
                "task '{task}' should route to Review"
            );
        }
    }

    #[test]
    fn test_ambiguous_tasks_route_to_higher_priority() {
        let router = AgentRouter::with_defaults();
        // "implementation" keyword (Implementer, p2) beats "check" (Review, p1)
        assert_eq!(
            router.route("check the implementation"),
            Some(SubAgentType::Implementer)
        );
        // "grade" (Review, p1) vs "implementation" (Implementer, p2)
        assert_eq!(
            router.route("grade the implementation against spec"),
            Some(SubAgentType::Implementer)
        );
    }

    #[test]
    fn test_default_registry_routes_verify_tasks() {
        let router = AgentRouter::with_defaults();
        for task in [
            "run tests for the auth module",
            "verify the input parser",
            "validate the input parser",
            "check test coverage",
        ] {
            let (agent_type, fallback) = router.route_with_meta(task);
            assert!(!fallback, "task '{task}' should match a keyword");
            assert_eq!(
                agent_type,
                Some(SubAgentType::Verifier),
                "task '{task}' should route to Verifier"
            );
        }
    }

    #[test]
    fn test_default_registry_routes_plan_tasks() {
        let router = AgentRouter::with_defaults();
        for task in [
            "plan the database migration",
            "design the API surface",
            "architect the plugin system",
        ] {
            let (agent_type, fallback) = router.route_with_meta(task);
            assert!(!fallback, "task '{task}' should match a keyword");
            assert_eq!(
                agent_type,
                Some(SubAgentType::Plan),
                "task '{task}' should route to Plan"
            );
        }
    }

    #[test]
    fn test_priority_overrides_lower_matches() {
        // "implement" hits Implementer (priority 3), not Plan (priority 1)
        let router = AgentRouter::with_defaults();
        let (agent_type, _) = router.route_with_meta("plan and implement the feature");
        assert_eq!(agent_type, Some(SubAgentType::Implementer));
    }

    #[test]
    fn test_fallback_to_general_when_enabled() {
        let router = AgentRouter::with_defaults();
        let (agent_type, fallback) = router.route_with_meta("something completely unrelated xyzzy");
        assert!(fallback);
        assert_eq!(agent_type, Some(SubAgentType::General));
    }

    #[test]
    fn test_no_fallback_when_disabled() {
        let mut router = AgentRouter::with_defaults();
        router.set_llm_fallback(false);
        let (agent_type, fallback) = router.route_with_meta("something completely unrelated xyzzy");
        assert!(!fallback);
        assert!(agent_type.is_none());
    }

    #[test]
    fn test_empty_router_returns_none() {
        let router = AgentRouter::empty();
        assert_eq!(router.route("implement a feature"), None);
    }

    #[test]
    fn test_case_insensitive_matching() {
        let router = AgentRouter::with_defaults();
        assert_eq!(
            router.route("IMPLEMENT the feature"),
            Some(SubAgentType::Implementer)
        );
        assert_eq!(router.route("Explore MODULE"), Some(SubAgentType::Explore));
    }

    #[test]
    fn test_custom_registration() {
        let mut router = AgentRouter::empty();
        router.register(vec!["deploy", "release"], SubAgentType::General, 5);
        router.set_llm_fallback(false);

        assert_eq!(
            router.route("deploy to production"),
            Some(SubAgentType::General)
        );
        assert_eq!(router.route("unmatched task"), None);
    }

    #[test]
    fn test_register_maintains_priority_order() {
        let mut router = AgentRouter::empty();
        router.register(vec!["alpha"], SubAgentType::Explore, 1);
        router.register(vec!["beta"], SubAgentType::Implementer, 5);
        router.register(vec!["gamma"], SubAgentType::Review, 3);
        router.set_llm_fallback(false);

        // "alpha beta" should match Implementer (beta, priority 5) first
        assert_eq!(router.route("alpha beta"), Some(SubAgentType::Implementer));
    }

    #[test]
    fn test_len_and_is_empty() {
        let router = AgentRouter::empty();
        assert!(router.is_empty());
        assert_eq!(router.len(), 0);

        let default = AgentRouter::with_defaults();
        assert!(!default.is_empty());
        assert_eq!(default.len(), 5); // 5 default entries
    }
}
