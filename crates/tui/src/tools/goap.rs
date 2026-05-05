//! Goal-Oriented Action Planning (GOAP) with A* search.
//!
//! GOAP is an AI planning technique where agents satisfy goals by
//! chaining actions. Each action has preconditions and effects; the
//! planner uses A* search to find the lowest-cost action sequence
//! that satisfies the goal state from the current world state.
//!
//! # Example
//!
//! ```ignore
//! let mut planner = GoapPlanner::new();
//! planner.add_action("compile", vec!["src_exists"], vec!["binary_exists"], 1.0);
//! planner.add_action("write_code", vec![], vec!["src_exists"], 3.0);
//!
//! let plan = planner.plan(&["binary_exists"], &[]);
//! assert!(plan.is_some()); // [write_code, compile]
//! ```

use std::collections::{BinaryHeap, HashSet};

// ── Action ───────────────────────────────────────────────────────────────────

/// A single GOAP action with preconditions, effects, and cost.
#[derive(Debug, Clone)]
pub struct GoapAction {
    /// Human-readable name (e.g., "write_code", "compile").
    pub name: String,
    /// World facts that must be true for this action to execute.
    pub preconditions: Vec<String>,
    /// World facts that become true after this action executes.
    pub effects: Vec<String>,
    /// Cost of this action (lower = preferred). Default 1.0.
    pub cost: f64,
}

impl GoapAction {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        preconditions: Vec<impl Into<String>>,
        effects: Vec<impl Into<String>>,
        cost: f64,
    ) -> Self {
        Self {
            name: name.into(),
            preconditions: preconditions.into_iter().map(|s| s.into()).collect(),
            effects: effects.into_iter().map(|s| s.into()).collect(),
            cost,
        }
    }

    /// Whether this action is applicable given the current world state.
    #[must_use]
    pub fn is_applicable(&self, state: &HashSet<String>) -> bool {
        self.preconditions.iter().all(|p| state.contains(p))
    }
}

// ── Plan node (A* search state) ─────────────────────────────────────────────

#[derive(Debug, Clone)]
struct PlanNode {
    /// Accumulated actions so far.
    actions: Vec<String>,
    /// Current world state after applying accumulated actions.
    state: HashSet<String>,
    /// Total cost so far (g).
    cost: f64,
    /// Heuristic estimate to goal (h).
    heuristic: f64,
}

impl PartialEq for PlanNode {
    fn eq(&self, other: &Self) -> bool {
        self.actions == other.actions
    }
}

impl Eq for PlanNode {}

impl PartialOrd for PlanNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PlanNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap: lower f = g + h wins
        let f_self = self.cost + self.heuristic;
        let f_other = other.cost + other.heuristic;
        f_other
            .partial_cmp(&f_self)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

// ── Planner ──────────────────────────────────────────────────────────────────

/// GOAP planner using A* search.
#[derive(Debug, Clone, Default)]
pub struct GoapPlanner {
    actions: Vec<GoapAction>,
}

impl GoapPlanner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an action the planner can use.
    pub fn add_action(&mut self, action: GoapAction) {
        self.actions.push(action);
    }

    /// Plan a sequence of actions to satisfy `goals` from `initial_state`.
    ///
    /// Returns `None` if no plan exists. Returns `Some(Vec<action_name>)`
    /// with the lowest-cost action sequence.
    #[must_use]
    pub fn plan(
        &self,
        goals: &[impl AsRef<str>],
        initial_state: &[impl AsRef<str>],
    ) -> Option<Vec<String>> {
        let goals: HashSet<String> = goals.iter().map(|g| g.as_ref().to_string()).collect();
        let initial: HashSet<String> = initial_state
            .iter()
            .map(|s| s.as_ref().to_string())
            .collect();

        // If goals already satisfied, return empty plan.
        if goals.iter().all(|g| initial.contains(g)) {
            return Some(Vec::new());
        }

        let mut open = BinaryHeap::new();
        let h = heuristic(&self.actions, &goals, &initial);
        open.push(PlanNode {
            actions: Vec::new(),
            state: initial,
            cost: 0.0,
            heuristic: h,
        });

        let mut closed = HashSet::new();

        while let Some(node) = open.pop() {
            // Check if goals are satisfied.
            if goals.iter().all(|g| node.state.contains(g)) {
                return Some(node.actions);
            }

            let state_key = sorted_state_key(&node.state);
            if !closed.insert(state_key) {
                continue;
            }

            for action in &self.actions {
                if !action.is_applicable(&node.state) {
                    continue;
                }

                let mut new_state = node.state.clone();
                for effect in &action.effects {
                    new_state.insert(effect.clone());
                }

                let mut new_actions = node.actions.clone();
                new_actions.push(action.name.clone());

                let new_cost = node.cost + action.cost;
                let new_h = heuristic(&self.actions, &goals, &new_state);

                open.push(PlanNode {
                    actions: new_actions,
                    state: new_state,
                    cost: new_cost,
                    heuristic: new_h,
                });
            }
        }

        None
    }

    /// Number of registered actions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Whether no actions are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

/// Heuristic: count of unsatisfied goals. Admissible (never overestimates)
/// because each action can satisfy at most all remaining goals.
fn heuristic(actions: &[GoapAction], goals: &HashSet<String>, state: &HashSet<String>) -> f64 {
    let unsatisfied = goals.iter().filter(|g| !state.contains(*g)).count();
    // Find the action with the most goal-satisfying effects per cost unit.
    let best_ratio = actions
        .iter()
        .map(|a| {
            let satisfying = a
                .effects
                .iter()
                .filter(|e| goals.contains(*e) && !state.contains(*e))
                .count() as f64;
            if a.cost > 0.0 && satisfying > 0.0 {
                satisfying / a.cost
            } else {
                0.0
            }
        })
        .fold(0.0_f64, f64::max);

    if best_ratio > 0.0 {
        unsatisfied as f64 / best_ratio
    } else {
        unsatisfied as f64 * 100.0 // fallback: each goal costs ~100
    }
}

fn sorted_state_key(state: &HashSet<String>) -> Vec<String> {
    let mut sorted: Vec<String> = state.iter().cloned().collect();
    sorted.sort();
    sorted
}

// ── ToolSpec ──────────────────────────────────────────────────────────────

use async_trait::async_trait;
use serde_json::json;

use crate::tools::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Tool that exposes GOAP planning to the model.
///
/// The model can call `goap_plan` with a goal description, initial state
/// facts, and available actions. Returns the lowest-cost action plan or
/// an explanation of why no plan exists.
pub struct GoapPlanTool {
    planner: std::sync::Arc<tokio::sync::Mutex<GoapPlanner>>,
}

impl GoapPlanTool {
    pub fn new() -> Self {
        Self {
            planner: std::sync::Arc::new(tokio::sync::Mutex::new(GoapPlanner::default())),
        }
    }

    /// Register pre-built actions for a development workflow.
    pub fn with_default_actions() -> Self {
        let mut planner = GoapPlanner::default();
        // Core development actions
        planner.add_action(GoapAction::new(
            "read_code",
            Vec::<&str>::new(),
            vec!["code_understood"],
            1.0,
        ));
        planner.add_action(GoapAction::new(
            "write_tests",
            vec!["code_understood"],
            vec!["tests_exist"],
            2.0,
        ));
        planner.add_action(GoapAction::new(
            "write_code",
            vec!["code_understood"],
            vec!["code_written"],
            3.0,
        ));
        planner.add_action(GoapAction::new(
            "run_tests",
            vec!["tests_exist", "code_written"],
            vec!["tests_pass"],
            1.0,
        ));
        planner.add_action(GoapAction::new(
            "review_code",
            vec!["code_written"],
            vec!["code_reviewed"],
            1.0,
        ));
        planner.add_action(GoapAction::new(
            "fix_bugs",
            vec!["tests_fail"],
            vec!["tests_pass", "code_written"],
            2.0,
        ));
        planner.add_action(GoapAction::new(
            "create_pr",
            vec!["code_reviewed", "tests_pass"],
            vec!["pr_created", "done"],
            1.0,
        ));
        planner.add_action(GoapAction::new(
            "audit_security",
            vec!["code_written"],
            vec!["security_audited"],
            2.0,
        ));
        planner.add_action(GoapAction::new(
            "write_docs",
            vec!["code_written"],
            vec!["docs_written"],
            1.5,
        ));
        planner.add_action(GoapAction::new(
            "deploy",
            vec!["tests_pass", "code_reviewed"],
            vec!["deployed", "done"],
            2.0,
        ));
        planner.add_action(GoapAction::new(
            "format_code",
            vec!["code_written"],
            vec!["code_formatted"],
            0.5,
        ));
        planner.add_action(GoapAction::new(
            "run_lints",
            vec!["code_written"],
            vec!["lints_pass"],
            0.5,
        ));
        Self {
            planner: std::sync::Arc::new(tokio::sync::Mutex::new(planner)),
        }
    }
}

#[async_trait]
impl ToolSpec for GoapPlanTool {
    fn name(&self) -> &'static str {
        "goap_plan"
    }

    fn description(&self) -> &'static str {
        "Plan a sequence of actions to achieve a goal using GOAP (Goal-Oriented Action Planning) with A* search. Provide goals, initial_state facts, and optionally custom actions. Returns the lowest-cost action plan."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "High-level description of the goal (for logging/display)"
                },
                "goals": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "World-state facts that must be true for the plan to succeed (e.g. ['tests_pass', 'code_reviewed'])"
                },
                "initial_state": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "World-state facts that are currently true (e.g. ['code_understood', 'src_exists'])"
                },
                "actions": {
                    "type": "array",
                    "description": "Optional custom actions for this specific domain",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "Action name"},
                            "preconditions": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": "Facts that must be true"
                            },
                            "effects": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": "Facts that become true after the action"
                            },
                            "cost": {
                                "type": "number",
                                "description": "Relative cost (lower = preferred, default 1.0)"
                            }
                        },
                        "required": ["name", "effects"]
                    }
                }
            },
            "required": ["goal", "goals", "initial_state"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let goal_desc = input
            .get("goal")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::invalid_input("Missing 'goal'"))?;

        let goals: Vec<String> = input
            .get("goals")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolError::invalid_input("Missing or invalid 'goals' array"))?
            .iter()
            .filter_map(|v| v.as_str().map(std::string::ToString::to_string))
            .collect();

        let initial_state: Vec<String> = input
            .get("initial_state")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolError::invalid_input("Missing or invalid 'initial_state' array"))?
            .iter()
            .filter_map(|v| v.as_str().map(std::string::ToString::to_string))
            .collect();

        let mut planner = GoapPlanner::new();

        // Register default development actions
        planner.add_action(GoapAction::new(
            "read_code", Vec::<&str>::new(), vec!["code_understood"], 1.0,
        ));
        planner.add_action(GoapAction::new(
            "write_tests", vec!["code_understood"], vec!["tests_exist"], 2.0,
        ));
        planner.add_action(GoapAction::new(
            "write_code", vec!["code_understood"], vec!["code_written"], 3.0,
        ));
        planner.add_action(GoapAction::new(
            "run_tests", vec!["tests_exist", "code_written"], vec!["tests_pass"], 1.0,
        ));
        planner.add_action(GoapAction::new(
            "review_code", vec!["code_written"], vec!["code_reviewed"], 1.0,
        ));
        planner.add_action(GoapAction::new(
            "fix_bugs", vec!["tests_fail"], vec!["tests_pass", "code_written"], 2.0,
        ));
        planner.add_action(GoapAction::new(
            "create_pr", vec!["code_reviewed", "tests_pass"], vec!["pr_created", "done"], 1.0,
        ));
        planner.add_action(GoapAction::new(
            "audit_security", vec!["code_written"], vec!["security_audited"], 2.0,
        ));
        planner.add_action(GoapAction::new(
            "write_docs", vec!["code_written"], vec!["docs_written"], 1.5,
        ));
        planner.add_action(GoapAction::new(
            "deploy", vec!["tests_pass", "code_reviewed"], vec!["deployed", "done"], 2.0,
        ));
        planner.add_action(GoapAction::new(
            "format_code", vec!["code_written"], vec!["code_formatted"], 0.5,
        ));
        planner.add_action(GoapAction::new(
            "run_lints", vec!["code_written"], vec!["lints_pass"], 0.5,
        ));

        // Register any custom actions provided by the caller
        if let Some(custom_actions) = input.get("actions").and_then(|v| v.as_array()) {
            for entry in custom_actions {
                let name = entry
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unnamed");
                let preconditions: Vec<String> = entry
                    .get("preconditions")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(std::string::ToString::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let effects: Vec<String> = entry
                    .get("effects")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        ToolError::invalid_input(format!(
                            "Custom action '{name}' missing 'effects' array"
                        ))
                    })?
                    .iter()
                    .filter_map(|v| v.as_str().map(std::string::ToString::to_string))
                    .collect();
                let cost = entry
                    .get("cost")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0);

                planner.add_action(GoapAction::new(name, preconditions, effects, cost));
            }
        }

        let plan = planner.plan(&goals, &initial_state);

        let result = match plan {
            Some(actions) => {
                let _steps = actions
                    .iter()
                    .enumerate()
                    .map(|(i, a)| format!("{}. {}", i + 1, a))
                    .collect::<Vec<_>>()
                    .join("\n");

                let total_cost: f64 = actions.len() as f64; // approximate

                serde_json::json!({
                    "goal": goal_desc,
                    "found": true,
                    "steps": actions,
                    "total_steps": actions.len(),
                    "estimated_cost": total_cost,
                })
            }
            None => {
                serde_json::json!({
                    "goal": goal_desc,
                    "found": false,
                    "reason": format!(
                        "No plan found to achieve goals {goals:?} from state {initial_state:?}",
                    ),
                })
            }
        };

        let summary = if result["found"].as_bool().unwrap_or(false) {
            format!(
                "GOAP plan found for '{}': {} steps",
                goal_desc,
                result["total_steps"].as_u64().unwrap_or(0)
            )
        } else {
            format!("No GOAP plan found for '{}'", goal_desc)
        };

        Ok(ToolResult::success(format!(
            "{}\n{}",
            summary,
            serde_json::to_string_pretty(&result).unwrap_or_default()
        )))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_plan() {
        let mut planner = GoapPlanner::new();
        planner.add_action(GoapAction::new(
            "compile",
            vec!["src_exists"],
            vec!["binary_exists"],
            1.0,
        ));
        planner.add_action(GoapAction::new(
            "write_code",
            Vec::<&str>::new(),
            vec!["src_exists"],
            3.0,
        ));

        let initial: &[&str] = &[];
        let plan = planner.plan(&["binary_exists"], initial).unwrap();
        assert_eq!(plan, vec!["write_code", "compile"]);
    }

    #[test]
    fn test_goals_already_satisfied() {
        let planner = GoapPlanner::new();
        let plan = planner
            .plan(&["binary_exists"], &["binary_exists"])
            .unwrap();
        assert!(plan.is_empty());
    }

    #[test]
    fn test_impossible_plan() {
        let mut planner = GoapPlanner::new();
        planner.add_action(GoapAction::new(
            "compile",
            vec!["src_exists"],
            vec!["binary_exists"],
            1.0,
        ));

        // No action produces "src_exists" → impossible.
        let initial: &[&str] = &[];
        let plan = planner.plan(&["binary_exists"], initial);
        assert!(plan.is_none());
    }

    #[test]
    fn test_cheapest_plan_chosen() {
        let mut planner = GoapPlanner::new();
        // Expensive path
        planner.add_action(GoapAction::new(
            "expensive_build",
            Vec::<&str>::new(),
            vec!["binary_exists"],
            100.0,
        ));
        // Cheap path
        planner.add_action(GoapAction::new(
            "write_code",
            Vec::<&str>::new(),
            vec!["src_exists"],
            1.0,
        ));
        planner.add_action(GoapAction::new(
            "compile",
            vec!["src_exists"],
            vec!["binary_exists"],
            1.0,
        ));

        let initial: &[&str] = &[];
        let plan = planner.plan(&["binary_exists"], initial).unwrap();
        // Should prefer write_code(1) + compile(1) = 2 over expensive_build(100)
        assert_eq!(plan, vec!["write_code", "compile"]);
    }
}
