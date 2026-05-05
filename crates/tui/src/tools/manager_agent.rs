#![allow(dead_code)]
//! Manager Agent — hierarchical process coordinator.
//!
//! Inspired by CrewAI's hierarchical Process, where a manager agent
//! automatically:
//! 1. Decomposes high-level goals into subtasks (using GOAP)
//! 2. Delegates subtasks to specialized sub-agents
//! 3. Validates results from each delegated task
//! 4. Synthesizes final output from all agent contributions
//!
//! # Architecture
//!
//! ```text
//! Goal → Manager Agent → GOAP Plan → Delegate to agents → Validate → Synthesize
//!          ↑                                                          |
//!          └────────────────── Feedback loop ─────────────────────────┘
//! ```

use std::collections::HashMap;
use std::time::Instant;
use serde::{Deserialize, Serialize};

// ── Delegation ──────────────────────────────────────────────────────────────

/// A delegation is a single subtask dispatched to a sub-agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    pub id: String,
    pub agent_role: String,
    pub task_description: String,
    pub expected_output: String,
    pub context: Option<String>,
    pub status: DelegationStatus,
    pub result: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DelegationStatus {
    Pending,
    Delegated,
    InProgress,
    Completed,
    Failed(String),
}

// ── Manager agent ───────────────────────────────────────────────────────────

/// A manager agent that coordinates hierarchical task execution.
///
/// The manager uses GOAP to plan, then delegates to specialized agents
/// via AgentRouter/SubAgentManager. It validates each result before
/// proceeding to the next step.
#[derive(Debug, Clone)]
pub struct ManagerAgent {
    pub id: String,
    pub goal: String,
    pub plan: Vec<String>,
    pub delegations: Vec<Delegation>,
    status: ManagerStatus,
    started_at: Instant,
    max_retries: u32,
    current_step: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManagerStatus {
    Planning,
    Delegating,
    Validating,
    Synthesizing,
    Completed,
    Failed(String),
}

impl ManagerStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Delegating => "delegating",
            Self::Validating => "validating",
            Self::Synthesizing => "synthesizing",
            Self::Completed => "completed",
            Self::Failed(_) => "failed",
        }
    }
}

impl ManagerAgent {
    /// Create a new manager agent with a goal.
    #[must_use]
    pub fn new(goal: impl Into<String>, plan: Vec<String>) -> Self {
        let id = format!("mgr_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let mut delegations = Vec::new();
        for (i, step) in plan.iter().enumerate() {
            delegations.push(Delegation {
                id: format!("del_{}", i + 1),
                agent_role: String::new(),
                task_description: step.clone(),
                expected_output: String::new(),
                context: None,
                status: DelegationStatus::Pending,
                result: None,
                started_at: None,
                completed_at: None,
            });
        }
        Self {
            id,
            goal: goal.into(),
            plan,
            delegations,
            status: ManagerStatus::Planning,
            started_at: Instant::now(),
            max_retries: 3,
            current_step: 0,
        }
    }

    /// Assign an agent role to a delegation.
    pub fn assign_role(&mut self, delegation_id: &str, role: impl Into<String>) {
        let role = role.into();
        let plan_step = delegation_id
            .strip_prefix("del_")
            .and_then(|n| n.parse::<usize>().ok())
            .map(|n| n - 1);
        if let Some(idx) = plan_step
            && let Some(del) = self.delegations.get_mut(idx)
        {
            del.agent_role = role;
        }
    }

    /// Set expected output for a delegation.
    pub fn set_expected_output(&mut self, delegation_id: &str, output: impl Into<String>) {
        let plan_step = delegation_id
            .strip_prefix("del_")
            .and_then(|n| n.parse::<usize>().ok())
            .map(|n| n - 1);
        if let Some(idx) = plan_step
            && let Some(del) = self.delegations.get_mut(idx)
        {
            del.expected_output = output.into();
        }
    }

    /// Mark a delegation as in-progress (dispatched to a sub-agent).
    pub fn mark_delegated(&mut self, delegation_id: &str, context: Option<String>) {
        if let Some(del) = self
            .delegations
            .iter_mut()
            .find(|d| d.id == delegation_id)
        {
            del.status = DelegationStatus::Delegated;
            del.context = context;
            del.started_at = Some(chrono::Utc::now().to_rfc3339());
        }
        self.status = ManagerStatus::Delegating;
    }

    /// Record the result of a delegation and validate it.
    pub fn record_result(&mut self, delegation_id: &str, result: impl Into<String>) -> bool {
        let result = result.into();
        let success = !result.is_empty() && !result.contains("Error:") && !result.contains("FAILED");

        if let Some(del) = self
            .delegations
            .iter_mut()
            .find(|d| d.id == delegation_id)
        {
            if success {
                del.status = DelegationStatus::Completed;
            } else {
                del.status = DelegationStatus::Failed(result.clone());
            }
            del.result = Some(result);
            del.completed_at = Some(chrono::Utc::now().to_rfc3339());
        }

        self.status = ManagerStatus::Validating;
        success
    }

    /// Check if all delegations are complete.
    #[must_use]
    pub fn all_complete(&self) -> bool {
        self.delegations
            .iter()
            .all(|d| matches!(d.status, DelegationStatus::Completed))
    }

    /// Synthesize final output from all delegation results.
    #[must_use]
    pub fn synthesize(&self) -> String {
        let mut output = format!("# Manager Report: {}\n\n", self.goal);
        output.push_str(&format!(
            "**Status**: {} | **Duration**: {:?}\n\n",
            self.status.as_str(),
            self.started_at.elapsed()
        ));

        for del in &self.delegations {
            let status_icon = match del.status {
                DelegationStatus::Completed => "✅",
                DelegationStatus::Failed(_) => "❌",
                DelegationStatus::Delegated | DelegationStatus::InProgress => "🔄",
                DelegationStatus::Pending => "⏳",
            };
            output.push_str(&format!(
                "## {} {}. {}\n",
                status_icon, del.id, del.task_description
            ));
            output.push_str(&format!("**Agent**: {}\n", del.agent_role));
            if let Some(ref result) = del.result {
                let truncated = if result.len() > 2000 {
                    format!("{}... ({} chars)", &result[..2000], result.len())
                } else {
                    result.clone()
                };
                output.push_str(&format!("**Result**: {}\n", truncated));
            }
            output.push('\n');
        }
        output
    }

    /// Generate a snapshot for display.
    #[must_use]
    pub fn snapshot(&self) -> ManagerSnapshot {
        ManagerSnapshot {
            id: self.id.clone(),
            goal: self.goal.clone(),
            status: self.status.as_str().to_string(),
            plan_steps: self.plan.len(),
            completed_steps: self
                .delegations
                .iter()
                .filter(|d| matches!(d.status, DelegationStatus::Completed))
                .count(),
            delegations: self.delegations.clone(),
            elapsed: format!("{:?}", self.started_at.elapsed()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerSnapshot {
    pub id: String,
    pub goal: String,
    pub status: String,
    pub plan_steps: usize,
    pub completed_steps: usize,
    pub delegations: Vec<Delegation>,
    pub elapsed: String,
}

// ── ToolSpec ────────────────────────────────────────────────────────────────

use async_trait::async_trait;
use serde_json::json;
use crate::tools::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Tool for managing hierarchical task delegation.
///
/// The model can call `manager_delegate` to create a manager agent, assign
/// roles, record delegation results, and synthesize final output.
pub struct ManagerDelegateTool {
    managers: std::sync::Arc<tokio::sync::Mutex<HashMap<String, ManagerAgent>>>,
}

impl ManagerDelegateTool {
    pub fn new() -> Self {
        Self {
            managers: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ToolSpec for ManagerDelegateTool {
    fn name(&self) -> &'static str {
        "manager_delegate"
    }

    fn description(&self) -> &'static str {
        "Use a manager agent to decompose a goal, delegate subtasks to specialized agents, and synthesize results. Supports: create (create a manager with plan), assign (assign agent role to step), dispatch (mark step as delegated), record (record result), and synthesize (generate final report)."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "assign", "dispatch", "record", "synthesize", "status"],
                    "description": "Manager action to perform"
                },
                "manager_id": {
                    "type": "string",
                    "description": "Manager agent ID (required for actions other than 'create')"
                },
                "goal": {
                    "type": "string",
                    "description": "High-level goal (required for 'create')"
                },
                "steps": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Plan steps to delegate (required for 'create')"
                },
                "delegation_id": {
                    "type": "string",
                    "description": "Delegation ID e.g. 'del_1' (required for 'assign', 'dispatch', 'record')"
                },
                "agent_role": {
                    "type": "string",
                    "description": "Role to assign e.g. 'coder', 'tester', 'reviewer' (for 'assign')"
                },
                "expected_output": {
                    "type": "string",
                    "description": "Description of expected output (for 'assign')"
                },
                "context": {
                    "type": "string",
                    "description": "Context to pass to the delegated agent (for 'dispatch')"
                },
                "result": {
                    "type": "string",
                    "description": "Result from the delegated agent (for 'record')"
                }
            },
            "required": ["action"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::invalid_input("Missing 'action'"))?;

        let mut managers = self.managers.lock().await;

        match action {
            "create" => {
                let goal = input
                    .get("goal")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::invalid_input("Missing 'goal' for create"))?;
                let steps: Vec<String> = input
                    .get("steps")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| ToolError::invalid_input("Missing 'steps' array for create"))?
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();

                let manager = ManagerAgent::new(goal, steps);
                let snapshot = manager.snapshot();
                let id = snapshot.id.clone();
                managers.insert(id.clone(), manager);

                Ok(ToolResult::success(format!(
                    "Manager agent created: {} ({} steps)\n{}",
                    id,
                    snapshot.plan_steps,
                    serde_json::to_string_pretty(&snapshot).unwrap_or_default()
                )))
            }

            "assign" => {
                let (manager, delegation_id) = get_manager_and_del(&mut managers, &input)?;
                let role = input
                    .get("agent_role")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::invalid_input("Missing 'agent_role' for assign"))?;
                let output = input
                    .get("expected_output")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Complete the task as described");

                manager.assign_role(&delegation_id, role);
                manager.set_expected_output(&delegation_id, output);

                Ok(ToolResult::success(format!(
                    "Assigned role '{}' to delegation {}",
                    role, delegation_id
                )))
            }

            "dispatch" => {
                let (manager, delegation_id) = get_manager_and_del(&mut managers, &input)?;
                let context = input
                    .get("context")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                manager.mark_delegated(&delegation_id, context);

                Ok(ToolResult::success(format!(
                    "Delegation {} dispatched. Now spawn a sub-agent to execute this task.",
                    delegation_id
                )))
            }

            "record" => {
                let (manager, delegation_id) = get_manager_and_del(&mut managers, &input)?;
                let result = input
                    .get("result")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::invalid_input("Missing 'result' for record"))?;

                let success = manager.record_result(&delegation_id, result);

                Ok(ToolResult::success(format!(
                    "Result recorded for {}: {}",
                    delegation_id,
                    if success { "success" } else { "failure" }
                )))
            }

            "synthesize" => {
                let manager_id = input
                    .get("manager_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::invalid_input("Missing 'manager_id' for synthesize"))?;

                let manager = managers
                    .get(manager_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Manager '{manager_id}' not found")))?;

                let report = manager.synthesize();
                Ok(ToolResult::success(report))
            }

            "status" => {
                let manager_id = input
                    .get("manager_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::invalid_input("Missing 'manager_id' for status"))?;

                let manager = managers
                    .get(manager_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Manager '{manager_id}' not found")))?;

                let snapshot = manager.snapshot();
                Ok(ToolResult::success(format!(
                    "Manager {}: {} ({}/{})\n{}",
                    manager_id,
                    snapshot.status,
                    snapshot.completed_steps,
                    snapshot.plan_steps,
                    serde_json::to_string_pretty(&snapshot).unwrap_or_default()
                )))
            }

            _ => Err(ToolError::invalid_input(format!(
                "Unknown action '{}'. Use: create, assign, dispatch, record, synthesize, status",
                action
            ))),
        }
    }
}

fn get_manager_and_del<'a>(
    managers: &'a mut HashMap<String, ManagerAgent>,
    input: &serde_json::Value,
) -> Result<(&'a mut ManagerAgent, String), ToolError> {
    let manager_id = input
        .get("manager_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::invalid_input("Missing 'manager_id'"))?;
    let delegation_id = input
        .get("delegation_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::invalid_input("Missing 'delegation_id'"))?;

    let manager = managers
        .get_mut(manager_id)
        .ok_or_else(|| ToolError::invalid_input(format!("Manager '{manager_id}' not found")))?;

    Ok((manager, delegation_id.to_string()))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_agent_lifecycle() {
        let mut mgr = ManagerAgent::new(
            "Implement user authentication",
            vec![
                "Write auth module".to_string(),
                "Add tests".to_string(),
                "Review code".to_string(),
            ],
        );

        assert_eq!(mgr.delegations.len(), 3);
        assert!(mgr.delegations[0].task_description.contains("auth module"));

        mgr.assign_role("del_1", "coder");
        mgr.set_expected_output("del_1", "src/auth.rs with login/logout");

        assert_eq!(mgr.delegations[0].agent_role, "coder");

        mgr.mark_delegated("del_1", Some("Use bcrypt for password hashing".into()));
        assert!(matches!(mgr.delegations[0].status, DelegationStatus::Delegated));

        let ok = mgr.record_result("del_1", "Created src/auth.rs with login endpoint");
        assert!(ok);
        assert!(matches!(mgr.delegations[0].status, DelegationStatus::Completed));
    }

    #[test]
    fn test_synthesis() {
        let mut mgr = ManagerAgent::new("Test goal", vec!["Step 1".into()]);
        mgr.assign_role("del_1", "tester");
        mgr.mark_delegated("del_1", None);
        mgr.record_result("del_1", "All tests pass");

        let report = mgr.synthesize();
        assert!(report.contains("Test goal"));
        assert!(report.contains("Step 1"));
        assert!(report.contains("tester"));
        assert!(report.contains("All tests pass"));
    }
}
