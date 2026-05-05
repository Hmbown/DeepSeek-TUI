//! Model router — heuristically selects the optimal model for a given
//! task type and complexity.
//!
//! # Routing rules
//!
//! - **Implementation tasks** → `deepseek-v4-flash` (fast, cost-effective)
//! - **Review / verification** → `deepseek-v4-flash`
//! - **Explore** → `deepseek-v4-flash`
//! - **Plan / architecture** → `deepseek-v4-pro` (deeper reasoning)
//! - **General / unclassified** → `deepseek-v4-pro` (safe default)
//! - **Complex (multi-file / debate / swarm)** → `deepseek-v4-pro`
//!
//! Callers can override the mapping per type via `set_model()`.

use super::SubAgentType;

/// Maps task types to optimal models.
#[derive(Debug, Clone)]
pub struct ModelRouter {
    /// Model for implementation tasks (write, edit, patch, refactor).
    implementer_model: String,
    /// Model for exploration tasks (search, read, inspect).
    explorer_model: String,
    /// Model for review tasks (audit, assess, grade).
    reviewer_model: String,
    /// Model for verification tasks (test, validate).
    verifier_model: String,
    /// Model for planning tasks (design, architect).
    planner_model: String,
    /// Default model for unclassified or general tasks.
    default_model: String,
}

impl Default for ModelRouter {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl ModelRouter {
    /// Create a router with sensible defaults:
    /// - flash for execution roles
    /// - pro for reasoning roles
    #[must_use]
    pub fn with_defaults() -> Self {
        Self {
            implementer_model: "deepseek-v4-flash".to_string(),
            explorer_model: "deepseek-v4-flash".to_string(),
            reviewer_model: "deepseek-v4-flash".to_string(),
            verifier_model: "deepseek-v4-flash".to_string(),
            planner_model: "deepseek-v4-pro".to_string(),
            default_model: "deepseek-v4-pro".to_string(),
        }
    }

    /// Override the model for a specific agent type.
    pub fn set_model(&mut self, agent_type: SubAgentType, model: impl Into<String>) {
        let model = model.into();
        match agent_type {
            SubAgentType::Implementer => self.implementer_model = model,
            SubAgentType::Explore => self.explorer_model = model,
            SubAgentType::Review => self.reviewer_model = model,
            SubAgentType::Verifier => self.verifier_model = model,
            SubAgentType::Plan => self.planner_model = model,
            SubAgentType::General | SubAgentType::Custom => self.default_model = model,
        }
    }

    /// Route an agent type to its recommended model.
    #[must_use]
    pub fn route(&self, agent_type: &SubAgentType) -> &str {
        match agent_type {
            SubAgentType::Implementer => &self.implementer_model,
            SubAgentType::Explore => &self.explorer_model,
            SubAgentType::Review => &self.reviewer_model,
            SubAgentType::Verifier => &self.verifier_model,
            SubAgentType::Plan => &self.planner_model,
            SubAgentType::General | SubAgentType::Custom => &self.default_model,
        }
    }

    /// Get the default model (used for unclassified tasks).
    #[must_use]
    pub fn default_model(&self) -> &str {
        &self.default_model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_route_execution_to_flash() {
        let router = ModelRouter::with_defaults();
        assert_eq!(
            router.route(&SubAgentType::Implementer),
            "deepseek-v4-flash"
        );
        assert_eq!(router.route(&SubAgentType::Explore), "deepseek-v4-flash");
        assert_eq!(router.route(&SubAgentType::Review), "deepseek-v4-flash");
        assert_eq!(router.route(&SubAgentType::Verifier), "deepseek-v4-flash");
    }

    #[test]
    fn test_defaults_route_reasoning_to_pro() {
        let router = ModelRouter::with_defaults();
        assert_eq!(router.route(&SubAgentType::Plan), "deepseek-v4-pro");
        assert_eq!(router.route(&SubAgentType::General), "deepseek-v4-pro");
    }

    #[test]
    fn test_can_override_per_type() {
        let mut router = ModelRouter::with_defaults();
        router.set_model(SubAgentType::Implementer, "custom-model");
        assert_eq!(router.route(&SubAgentType::Implementer), "custom-model");
        // Others unchanged
        assert_eq!(router.route(&SubAgentType::Explore), "deepseek-v4-flash");
    }

    #[test]
    fn test_default_model() {
        let router = ModelRouter::with_defaults();
        assert_eq!(router.default_model(), "deepseek-v4-pro");
    }
}
