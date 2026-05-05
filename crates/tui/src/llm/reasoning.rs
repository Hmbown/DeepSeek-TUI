//! Auto reasoning-effort selector — #663.
//!
//! Resolves `reasoning_effort = "auto"` at turn-build time by evaluating
//! four signals:
//!
//! 1. **Task complexity** — Lookup / CodeGen / Architecture / Debug
//! 2. **Cache pressure** — hit ratio over recent turns
//! 3. **Cost budget** — session cost vs configured ceiling
//! 4. **Fanout** — sub-agent vs main-loop turn
//!
//! The selector emits an effort tier (`Off`/`Low`/`Medium`/`High`/`Max`)
//! plus a recommended model id.

use std::fmt;

use crate::llm::unified::ThinkingMode;

// ── Effort tier ─────────────────────────────────────────────────────────────

/// Explicit reasoning-effort tier passed to the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReasoningEffort {
    /// No reasoning tokens.
    Off,
    /// Budget-limited (fast, cheap).
    Low,
    /// Balanced depth.
    Medium,
    /// Extended reasoning.
    High,
    /// Maximum depth (most expensive).
    Max,
}

impl ReasoningEffort {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Max => "max",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "off" | "none" => Some(Self::Off),
            "low" => Some(Self::Low),
            "medium" | "normal" | "balanced" => Some(Self::Medium),
            "high" => Some(Self::High),
            "max" | "maximum" => Some(Self::Max),
            "auto" => None, // Handled by caller — resolves via selector
            _ => None,
        }
    }

    /// Map to the corresponding ThinkingMode.
    #[must_use]
    pub fn to_thinking_mode(self) -> ThinkingMode {
        match self {
            Self::Off => ThinkingMode::None,
            Self::Low => ThinkingMode::Light,
            Self::Medium => ThinkingMode::Medium,
            Self::High | Self::Max => ThinkingMode::Deep,
        }
    }
}

impl fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Task complexity ─────────────────────────────────────────────────────────

/// Task complexity classification used by the auto selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskComplexity {
    /// Fast lookup / search / grep — minimal reasoning needed.
    Lookup,
    /// Code generation / editing / refactoring — medium reasoning.
    CodeGen,
    /// System design / architecture planning — deep reasoning.
    Architecture,
    /// Root-cause analysis / debugging — deepest reasoning.
    Debug,
}

impl TaskComplexity {
    /// Classify a task description heuristically.
    #[must_use]
    pub fn classify(task: &str) -> Self {
        let lower = task.to_lowercase();

        // Debug signals (strongest)
        if lower.contains("debug")
            || lower.contains("root cause")
            || lower.contains("trace")
            || lower.contains("stack trace")
            || lower.contains("why is")
            || lower.contains("what caused")
        {
            return Self::Debug;
        }

        // Architecture signals
        if lower.contains("architect")
            || lower.contains("design")
            || lower.contains("system")
            || lower.contains("migrat")
            || lower.contains("roadmap")
            || lower.contains("proposal")
        {
            return Self::Architecture;
        }

        // Code generation signals
        if lower.contains("implement")
            || lower.contains("write")
            || lower.contains("build")
            || lower.contains("refactor")
            || lower.contains("create")
            || lower.contains("generate")
            || lower.contains("code")
            || lower.contains("patch")
            || lower.contains("edit")
        {
            return Self::CodeGen;
        }

        // Default: lookup
        Self::Lookup
    }

    /// Recommended effort for this complexity level.
    #[must_use]
    pub fn recommended_effort(self) -> ReasoningEffort {
        match self {
            Self::Lookup => ReasoningEffort::Off,
            Self::CodeGen => ReasoningEffort::Medium,
            Self::Architecture => ReasoningEffort::High,
            Self::Debug => ReasoningEffort::Max,
        }
    }

    /// Recommended model for this complexity level.
    #[must_use]
    pub fn recommended_model(self) -> &'static str {
        match self {
            Self::Lookup | Self::CodeGen => "deepseek-v4-flash",
            Self::Architecture | Self::Debug => "deepseek-v4-pro",
        }
    }
}

// ── Selector inputs ─────────────────────────────────────────────────────────

/// Inputs to the auto reasoning-effort selector.
#[derive(Debug, Clone)]
pub struct SelectorInputs {
    /// Task description / prompt text.
    pub task: String,
    /// Cache-hit ratio over recent turns (0.0–1.0). None if unavailable.
    pub cache_hit_ratio: Option<f64>,
    /// Session cost so far in USD. None if unavailable.
    pub session_cost: Option<f64>,
    /// Configured cost budget ceiling in USD. None = no budget.
    pub cost_budget: Option<f64>,
    /// Whether this is a sub-agent fanout (vs main-loop reasoning).
    pub is_subagent: bool,
    /// Explicit user override — when set, auto stays out of the way.
    pub explicit_effort: Option<ReasoningEffort>,
}

// ── Selector ─────────────────────────────────────────────────────────────────

/// Auto reasoning-effort selector.
///
/// When `reasoning_effort = "auto"`, this selector evaluates the inputs
/// and returns the optimal effort tier + model id. When an explicit
/// override is set, the selector returns the override unchanged.
#[derive(Debug, Clone, Default)]
pub struct AutoReasoningSelector {
    /// Cache-hit ratio threshold below which we drop effort (cache rebuild).
    pub cache_pressure_threshold: f64,
    /// Cost ratio (session / budget) above which we throttle effort.
    pub cost_throttle_ratio: f64,
}

impl AutoReasoningSelector {
    /// Create with sensible defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache_pressure_threshold: 0.30, // below 30% cache hit → throttle
            cost_throttle_ratio: 0.80,      // above 80% budget → throttle
        }
    }

    /// Select the optimal effort and model.
    ///
    /// Returns `(effort, model_id, reason)` where `reason` is a
    /// human-readable explanation for audit logging.
    #[must_use]
    pub fn select(&self, inputs: &SelectorInputs) -> (ReasoningEffort, String, String) {
        // 1. Explicit override always wins.
        if let Some(override_effort) = inputs.explicit_effort {
            let reason = format!("explicit override: {}", override_effort);
            let model = if override_effort >= ReasoningEffort::High {
                "deepseek-v4-pro"
            } else {
                "deepseek-v4-flash"
            };
            return (override_effort, model.to_string(), reason);
        }

        // 2. Classify task complexity.
        let complexity = TaskComplexity::classify(&inputs.task);
        let mut effort = complexity.recommended_effort();
        let mut model = complexity.recommended_model().to_string();
        let mut reasons: Vec<String> = vec![format!("task={:?}", complexity)];

        // 3. Cache pressure: if hit ratio is low, drop one tier to rebuild cache.
        if let Some(hit_ratio) = inputs.cache_hit_ratio {
            if hit_ratio < self.cache_pressure_threshold && effort > ReasoningEffort::Off {
                let old = effort;
                effort = match effort {
                    ReasoningEffort::Off => ReasoningEffort::Off,
                    ReasoningEffort::Low => ReasoningEffort::Off,
                    ReasoningEffort::Medium => ReasoningEffort::Low,
                    ReasoningEffort::High => ReasoningEffort::Medium,
                    ReasoningEffort::Max => ReasoningEffort::High,
                };
                if effort != old {
                    let hr_pct = hit_ratio * 100.0;
                    let thr_pct = self.cache_pressure_threshold * 100.0;
                    reasons.push(format!(
                        "cache_pressure: hit_ratio={hr_pct:.0}% < {thr_pct:.0}%, dropped from {old} to {effort}",
                    ));
                }
            }
        }

        // 4. Cost budget: throttle if approaching ceiling.
        if let (Some(cost), Some(budget)) = (inputs.session_cost, inputs.cost_budget) {
            if budget > 0.0 {
                let ratio = cost / budget;
                if ratio >= self.cost_throttle_ratio && effort > ReasoningEffort::Low {
                    let old = effort;
                    effort = ReasoningEffort::Low;
                    model = "deepseek-v4-flash".to_string();
                    let ratio_pct = ratio * 100.0;
                    let ctr_pct = self.cost_throttle_ratio * 100.0;
                    reasons.push(format!(
                        "cost_throttle: ${cost:.2} / ${budget:.2} = {ratio_pct:.0}% ≥ {ctr_pct:.0}%, throttled from {old} to low",
                    ));
                }
            }
        }

        // 5. Sub-agent fanout: always route to flash.
        if inputs.is_subagent {
            model = "deepseek-v4-flash".to_string();
            if effort > ReasoningEffort::Medium {
                effort = ReasoningEffort::Medium;
            }
            reasons.push("subagent=flash".to_string());
        }

        let reason = reasons.join("; ");
        (effort, model, reason)
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn basic_inputs(task: &str) -> SelectorInputs {
        SelectorInputs {
            task: task.to_string(),
            cache_hit_ratio: None,
            session_cost: None,
            cost_budget: None,
            is_subagent: false,
            explicit_effort: None,
        }
    }

    #[test]
    fn test_task_classify_lookup() {
        assert_eq!(
            TaskComplexity::classify("search for TODO comments"),
            TaskComplexity::Lookup
        );
        assert_eq!(
            TaskComplexity::classify("list all files in src/"),
            TaskComplexity::Lookup
        );
    }

    #[test]
    fn test_task_classify_codegen() {
        assert_eq!(
            TaskComplexity::classify("implement the login endpoint"),
            TaskComplexity::CodeGen
        );
        assert_eq!(
            TaskComplexity::classify("refactor the auth module"),
            TaskComplexity::CodeGen
        );
    }

    #[test]
    fn test_task_classify_architecture() {
        assert_eq!(
            TaskComplexity::classify("design the API surface"),
            TaskComplexity::Architecture
        );
        assert_eq!(
            TaskComplexity::classify("architect the plugin system"),
            TaskComplexity::Architecture
        );
    }

    #[test]
    fn test_task_classify_debug() {
        assert_eq!(
            TaskComplexity::classify("debug the null pointer exception"),
            TaskComplexity::Debug
        );
        assert_eq!(
            TaskComplexity::classify("trace the call path to find root cause"),
            TaskComplexity::Debug
        );
    }

    #[test]
    fn test_selector_basic_classification() {
        let selector = AutoReasoningSelector::new();
        let (effort, model, reason) = selector.select(&basic_inputs("search for errors"));

        assert!(reason.contains("task=Lookup"));
        assert_eq!(effort, ReasoningEffort::Off);
        assert_eq!(model, "deepseek-v4-flash");
    }

    #[test]
    fn test_selector_debug_gets_max() {
        let selector = AutoReasoningSelector::new();
        let (effort, model, reason) =
            selector.select(&basic_inputs("debug why the server crashes"));

        assert!(reason.contains("task=Debug"));
        assert_eq!(effort, ReasoningEffort::Max);
        assert_eq!(model, "deepseek-v4-pro");
    }

    #[test]
    fn test_selector_explicit_override_wins() {
        let selector = AutoReasoningSelector::new();
        let inputs = SelectorInputs {
            explicit_effort: Some(ReasoningEffort::Max),
            ..basic_inputs("search for errors")
        };

        let (effort, model, reason) = selector.select(&inputs);

        assert!(reason.contains("explicit override"));
        assert_eq!(effort, ReasoningEffort::Max);
        assert_eq!(model, "deepseek-v4-pro");
    }

    #[test]
    fn test_selector_cache_pressure_throttles() {
        let selector = AutoReasoningSelector::new();
        let inputs = SelectorInputs {
            cache_hit_ratio: Some(0.10), // 10% cache hit — very low
            ..basic_inputs("implement a parser")
        };

        let (effort, _, reason) = selector.select(&inputs);

        // CodeGen normally gets Medium; cache pressure drops to Low
        assert_eq!(effort, ReasoningEffort::Low);
        assert!(reason.contains("cache_pressure"));
    }

    #[test]
    fn test_selector_cost_budget_throttles() {
        let selector = AutoReasoningSelector::new();
        let inputs = SelectorInputs {
            task: "architect the database schema".to_string(),
            session_cost: Some(90.0),
            cost_budget: Some(100.0),
            ..basic_inputs("")
        };

        let (effort, model, reason) = selector.select(&inputs);

        // Architecture normally gets High; cost at 90% budget throttles to Low
        assert_eq!(effort, ReasoningEffort::Low);
        assert_eq!(model, "deepseek-v4-flash");
        assert!(reason.contains("cost_throttle"));
    }

    #[test]
    fn test_selector_subagent_routes_to_flash() {
        let selector = AutoReasoningSelector::new();
        let inputs = SelectorInputs {
            is_subagent: true,
            ..basic_inputs("debug the crash")
        };

        let (effort, model, _) = selector.select(&inputs);

        assert_eq!(model, "deepseek-v4-flash");
        assert!(
            effort <= ReasoningEffort::Medium,
            "sub-agent should cap at Medium, got {effort}"
        );
    }

    #[test]
    fn test_effort_to_thinking_mode() {
        assert_eq!(ReasoningEffort::Off.to_thinking_mode(), ThinkingMode::None);
        assert_eq!(ReasoningEffort::Low.to_thinking_mode(), ThinkingMode::Light);
        assert_eq!(
            ReasoningEffort::Medium.to_thinking_mode(),
            ThinkingMode::Medium
        );
        assert_eq!(ReasoningEffort::High.to_thinking_mode(), ThinkingMode::Deep);
        assert_eq!(ReasoningEffort::Max.to_thinking_mode(), ThinkingMode::Deep);
    }

    #[test]
    fn test_effort_display() {
        assert_eq!(ReasoningEffort::Off.to_string(), "off");
        assert_eq!(ReasoningEffort::High.to_string(), "high");
    }
}
