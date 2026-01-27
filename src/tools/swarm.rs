//! Swarm orchestration for spawning multiple sub-agents with dependencies.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_bool, optional_str, optional_u64,
};
use crate::tools::subagent::{
    SharedSubAgentManager, SubAgentResult, SubAgentRuntime, SubAgentStatus, SubAgentType,
};

const SWARM_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_SWARM_TIMEOUT_MS: u64 = 600_000;
const DEFAULT_SWARM_TIMEOUT_NONBLOCK_MS: u64 = 15_000;
const MAX_SWARM_TIMEOUT_MS: u64 = 3_600_000;

#[derive(Debug, Clone, Deserialize)]
struct SwarmTaskSpec {
    id: String,
    prompt: String,
    #[serde(default, rename = "type")]
    agent_type: Option<SubAgentType>,
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    depends_on: Vec<String>,
}

#[derive(Debug, Clone)]
enum SwarmTaskState {
    Pending,
    Running { agent_id: String },
    Done(SubAgentResult),
    Failed(String),
    Skipped(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum SwarmTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
struct SwarmTaskOutcome {
    task_id: String,
    agent_id: Option<String>,
    status: SwarmTaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    steps_taken: u32,
    duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum SwarmStatus {
    Completed,
    Partial,
    Timeout,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
struct SwarmCounts {
    total: usize,
    completed: usize,
    failed: usize,
    cancelled: usize,
    skipped: usize,
    running: usize,
    pending: usize,
}

#[derive(Debug, Clone, Serialize)]
struct SwarmOutcome {
    swarm_id: String,
    status: SwarmStatus,
    duration_ms: u64,
    counts: SwarmCounts,
    tasks: Vec<SwarmTaskOutcome>,
}

/// Tool to launch a swarm of sub-agents with dependency-aware scheduling.
pub struct AgentSwarmTool {
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
}

impl AgentSwarmTool {
    /// Create a new swarm tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: SubAgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[async_trait]
impl ToolSpec for AgentSwarmTool {
    fn name(&self) -> &'static str {
        "agent_swarm"
    }

    fn description(&self) -> &'static str {
        "Spawn multiple sub-agents with optional dependencies and aggregate their results."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "description": "List of swarm tasks to execute.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Unique task id." },
                            "prompt": { "type": "string", "description": "Task prompt for the sub-agent." },
                            "type": { "type": "string", "description": "Sub-agent type: general, explore, plan, review, custom." },
                            "allowed_tools": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Explicit tool allowlist (required for custom type)."
                            },
                            "depends_on": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "List of task ids that must complete successfully first."
                            }
                        },
                        "required": ["id", "prompt"]
                    }
                },
                "shared_context": {
                    "type": "string",
                    "description": "Optional shared context prepended to each task prompt."
                },
                "block": {
                    "type": "boolean",
                    "description": "Whether to wait for tasks to finish (default: true)."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Max wall time in milliseconds before returning partial results."
                },
                "max_parallel": {
                    "type": "integer",
                    "description": "Max concurrent swarm agents (defaults to max_subagents)."
                },
                "fail_fast": {
                    "type": "boolean",
                    "description": "Cancel remaining work on first failure (default: false)."
                }
            },
            "required": ["tasks"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let tasks_value = input
            .get("tasks")
            .cloned()
            .ok_or_else(|| ToolError::missing_field("tasks"))?;
        let tasks: Vec<SwarmTaskSpec> = serde_json::from_value(tasks_value)
            .map_err(|err| ToolError::invalid_input(format!("Invalid tasks payload: {err}")))?;

        validate_swarm_tasks(&tasks)?;

        let block = optional_bool(&input, "block", true);
        let default_timeout = if block {
            DEFAULT_SWARM_TIMEOUT_MS
        } else {
            DEFAULT_SWARM_TIMEOUT_NONBLOCK_MS
        };
        let timeout_ms =
            optional_u64(&input, "timeout_ms", default_timeout).clamp(1_000, MAX_SWARM_TIMEOUT_MS);
        let fail_fast = optional_bool(&input, "fail_fast", false);
        let shared_context = optional_str(&input, "shared_context")
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string);

        let max_parallel = {
            let manager = self.manager.lock().await;
            let max_agents = manager.max_agents();
            let requested = optional_u64(&input, "max_parallel", max_agents as u64);
            requested.clamp(1, max_agents as u64) as usize
        };

        let outcome = run_swarm(
            &self.manager,
            &self.runtime,
            tasks,
            shared_context,
            Duration::from_millis(timeout_ms),
            max_parallel,
            fail_fast,
            block,
        )
        .await?;

        ToolResult::json(&outcome).map_err(|err| ToolError::execution_failed(err.to_string()))
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_swarm(
    shared_manager: &SharedSubAgentManager,
    runtime: &SubAgentRuntime,
    tasks: Vec<SwarmTaskSpec>,
    shared_context: Option<String>,
    timeout: Duration,
    max_parallel: usize,
    fail_fast: bool,
    block: bool,
) -> Result<SwarmOutcome, ToolError> {
    let swarm_id = format!("swarm_{}", &Uuid::new_v4().to_string()[..8]);
    let start = Instant::now();
    let deadline = start + timeout;
    let task_order = tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>();

    let mut task_map = HashMap::new();
    let mut states = HashMap::new();
    let mut pending = HashSet::new();
    for task in tasks {
        pending.insert(task.id.clone());
        states.insert(task.id.clone(), SwarmTaskState::Pending);
        task_map.insert(task.id.clone(), task);
    }

    let mut running: HashMap<String, String> = HashMap::new();
    let mut fail_fast_triggered = false;
    let mut timed_out = false;

    loop {
        let mut changed = false;

        if !running.is_empty() {
            let snapshots = {
                let manager = shared_manager.lock().await;
                manager.list()
            };
            let snapshot_map: HashMap<String, SubAgentResult> = snapshots
                .into_iter()
                .map(|snapshot| (snapshot.agent_id.clone(), snapshot))
                .collect();

            let running_ids = running.clone();
            for (task_id, agent_id) in running_ids {
                match snapshot_map.get(&agent_id) {
                    Some(snapshot) => {
                        if snapshot.status != SubAgentStatus::Running {
                            states.insert(task_id.clone(), SwarmTaskState::Done(snapshot.clone()));
                            running.remove(&task_id);
                            changed = true;
                            if fail_fast
                                && matches!(
                                    snapshot.status,
                                    SubAgentStatus::Failed(_) | SubAgentStatus::Cancelled
                                )
                            {
                                fail_fast_triggered = true;
                            }
                        }
                    }
                    None => {
                        states.insert(
                            task_id.clone(),
                            SwarmTaskState::Failed("Agent result not found".to_string()),
                        );
                        running.remove(&task_id);
                        changed = true;
                        if fail_fast {
                            fail_fast_triggered = true;
                        }
                    }
                }
            }
        }

        if fail_fast_triggered {
            apply_fail_fast(shared_manager, &mut states, &mut pending, &mut running).await?;
            break;
        }

        let mut newly_skipped = Vec::new();
        for task_id in pending.iter() {
            if let Some(task) = task_map.get(task_id)
                && dependencies_failed(task, &states)
            {
                newly_skipped.push(task_id.clone());
            }
        }
        for task_id in newly_skipped {
            pending.remove(&task_id);
            states.insert(
                task_id,
                SwarmTaskState::Skipped("Dependency failed".to_string()),
            );
            changed = true;
        }

        let mut ready = Vec::new();
        for task_id in pending.iter() {
            if let Some(task) = task_map.get(task_id)
                && dependencies_satisfied(task, &states)
            {
                ready.push(task_id.clone());
            }
        }

        if !ready.is_empty() {
            let available_slots = {
                let manager = shared_manager.lock().await;
                let global_slots = manager.available_slots();
                let swarm_slots = max_parallel.saturating_sub(running.len());
                global_slots.min(swarm_slots)
            };

            if available_slots > 0 {
                for task_id in ready.into_iter().take(available_slots) {
                    let task = task_map
                        .get(&task_id)
                        .ok_or_else(|| ToolError::execution_failed("Missing swarm task"))?;
                    let agent_type = task.agent_type.clone().unwrap_or_default();
                    let prompt = format_prompt(shared_context.as_deref(), &task.prompt);

                    let spawn_result = {
                        let mut manager = shared_manager.lock().await;
                        manager.spawn_background(
                            Arc::clone(shared_manager),
                            runtime.clone(),
                            agent_type,
                            prompt,
                            task.allowed_tools.clone(),
                        )
                    };

                    match spawn_result {
                        Ok(snapshot) => {
                            states.insert(
                                task_id.clone(),
                                SwarmTaskState::Running {
                                    agent_id: snapshot.agent_id.clone(),
                                },
                            );
                            running.insert(task_id.clone(), snapshot.agent_id);
                            pending.remove(&task_id);
                            changed = true;
                        }
                        Err(err) => {
                            let message = err.to_string();
                            if message.contains("Sub-agent limit reached") {
                                break;
                            }
                            states.insert(task_id.clone(), SwarmTaskState::Failed(message));
                            pending.remove(&task_id);
                            changed = true;
                            if fail_fast {
                                fail_fast_triggered = true;
                            }
                        }
                    }
                }
            }
        }

        if fail_fast_triggered {
            apply_fail_fast(shared_manager, &mut states, &mut pending, &mut running).await?;
            break;
        }

        if pending.is_empty() && running.is_empty() {
            break;
        }
        if !block {
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            break;
        }

        if !changed {
            tokio::time::sleep(SWARM_POLL_INTERVAL).await;
        }
    }

    let outcomes = build_task_outcomes(&task_order, &states);
    let counts = build_counts(&outcomes);
    let status = if fail_fast_triggered {
        SwarmStatus::Failed
    } else if timed_out {
        SwarmStatus::Timeout
    } else if counts.failed > 0
        || counts.cancelled > 0
        || counts.skipped > 0
        || counts.pending > 0
        || counts.running > 0
    {
        SwarmStatus::Partial
    } else {
        SwarmStatus::Completed
    };

    Ok(SwarmOutcome {
        swarm_id,
        status,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        counts,
        tasks: outcomes,
    })
}

fn format_prompt(shared_context: Option<&str>, prompt: &str) -> String {
    if let Some(context) = shared_context {
        format!("Shared context:\n{context}\n\nTask:\n{prompt}")
    } else {
        prompt.to_string()
    }
}

fn dependencies_satisfied(task: &SwarmTaskSpec, states: &HashMap<String, SwarmTaskState>) -> bool {
    task.depends_on.iter().all(|dep| {
        matches!(
            states.get(dep),
            Some(SwarmTaskState::Done(result))
                if matches!(result.status, SubAgentStatus::Completed)
        )
    })
}

fn dependencies_failed(task: &SwarmTaskSpec, states: &HashMap<String, SwarmTaskState>) -> bool {
    task.depends_on.iter().any(|dep| match states.get(dep) {
        Some(SwarmTaskState::Done(result)) => matches!(
            result.status,
            SubAgentStatus::Failed(_) | SubAgentStatus::Cancelled
        ),
        Some(SwarmTaskState::Failed(_)) | Some(SwarmTaskState::Skipped(_)) => true,
        _ => false,
    })
}

async fn cancel_running_tasks(
    manager: &SharedSubAgentManager,
    running: &HashMap<String, String>,
    states: &mut HashMap<String, SwarmTaskState>,
) -> Result<(), ToolError> {
    let mut manager = manager.lock().await;
    for (task_id, agent_id) in running {
        match manager.cancel(agent_id) {
            Ok(snapshot) => {
                states.insert(task_id.clone(), SwarmTaskState::Done(snapshot));
            }
            Err(err) => {
                states.insert(
                    task_id.clone(),
                    SwarmTaskState::Failed(format!("Failed to cancel agent: {err}")),
                );
            }
        }
    }
    Ok(())
}

async fn apply_fail_fast(
    manager: &SharedSubAgentManager,
    states: &mut HashMap<String, SwarmTaskState>,
    pending: &mut HashSet<String>,
    running: &mut HashMap<String, String>,
) -> Result<(), ToolError> {
    cancel_running_tasks(manager, running, states).await?;
    for task_id in pending.drain() {
        states.insert(
            task_id,
            SwarmTaskState::Skipped("Skipped due to fail_fast".to_string()),
        );
    }
    running.clear();
    Ok(())
}

fn build_task_outcomes(
    order: &[String],
    states: &HashMap<String, SwarmTaskState>,
) -> Vec<SwarmTaskOutcome> {
    order
        .iter()
        .map(|task_id| match states.get(task_id) {
            Some(SwarmTaskState::Running { agent_id }) => SwarmTaskOutcome {
                task_id: task_id.clone(),
                agent_id: Some(agent_id.clone()),
                status: SwarmTaskStatus::Running,
                result: None,
                error: None,
                steps_taken: 0,
                duration_ms: 0,
            },
            Some(SwarmTaskState::Done(result)) => match &result.status {
                SubAgentStatus::Completed => SwarmTaskOutcome {
                    task_id: task_id.clone(),
                    agent_id: Some(result.agent_id.clone()),
                    status: SwarmTaskStatus::Completed,
                    result: result.result.clone(),
                    error: None,
                    steps_taken: result.steps_taken,
                    duration_ms: result.duration_ms,
                },
                SubAgentStatus::Failed(err) => SwarmTaskOutcome {
                    task_id: task_id.clone(),
                    agent_id: Some(result.agent_id.clone()),
                    status: SwarmTaskStatus::Failed,
                    result: result.result.clone(),
                    error: Some(err.clone()),
                    steps_taken: result.steps_taken,
                    duration_ms: result.duration_ms,
                },
                SubAgentStatus::Cancelled => SwarmTaskOutcome {
                    task_id: task_id.clone(),
                    agent_id: Some(result.agent_id.clone()),
                    status: SwarmTaskStatus::Cancelled,
                    result: result.result.clone(),
                    error: Some("Cancelled".to_string()),
                    steps_taken: result.steps_taken,
                    duration_ms: result.duration_ms,
                },
                SubAgentStatus::Running => SwarmTaskOutcome {
                    task_id: task_id.clone(),
                    agent_id: Some(result.agent_id.clone()),
                    status: SwarmTaskStatus::Running,
                    result: result.result.clone(),
                    error: None,
                    steps_taken: result.steps_taken,
                    duration_ms: result.duration_ms,
                },
            },
            Some(SwarmTaskState::Failed(message)) => SwarmTaskOutcome {
                task_id: task_id.clone(),
                agent_id: None,
                status: SwarmTaskStatus::Failed,
                result: None,
                error: Some(message.clone()),
                steps_taken: 0,
                duration_ms: 0,
            },
            Some(SwarmTaskState::Skipped(message)) => SwarmTaskOutcome {
                task_id: task_id.clone(),
                agent_id: None,
                status: SwarmTaskStatus::Skipped,
                result: None,
                error: Some(message.clone()),
                steps_taken: 0,
                duration_ms: 0,
            },
            _ => SwarmTaskOutcome {
                task_id: task_id.clone(),
                agent_id: None,
                status: SwarmTaskStatus::Pending,
                result: None,
                error: None,
                steps_taken: 0,
                duration_ms: 0,
            },
        })
        .collect()
}

fn build_counts(outcomes: &[SwarmTaskOutcome]) -> SwarmCounts {
    let mut counts = SwarmCounts {
        total: outcomes.len(),
        completed: 0,
        failed: 0,
        cancelled: 0,
        skipped: 0,
        running: 0,
        pending: 0,
    };

    for outcome in outcomes {
        match outcome.status {
            SwarmTaskStatus::Completed => counts.completed += 1,
            SwarmTaskStatus::Failed => counts.failed += 1,
            SwarmTaskStatus::Cancelled => counts.cancelled += 1,
            SwarmTaskStatus::Skipped => counts.skipped += 1,
            SwarmTaskStatus::Running => counts.running += 1,
            SwarmTaskStatus::Pending => counts.pending += 1,
        }
    }

    counts
}

fn validate_swarm_tasks(tasks: &[SwarmTaskSpec]) -> Result<(), ToolError> {
    if tasks.is_empty() {
        return Err(ToolError::invalid_input("tasks cannot be empty"));
    }

    let mut ids = HashSet::new();
    for task in tasks {
        let id = task.id.trim();
        if id.is_empty() {
            return Err(ToolError::invalid_input("task id cannot be empty"));
        }
        if task.prompt.trim().is_empty() {
            return Err(ToolError::invalid_input(format!(
                "task '{id}' prompt cannot be empty"
            )));
        }
        if matches!(task.agent_type, Some(SubAgentType::Custom)) {
            let tools = task
                .allowed_tools
                .as_ref()
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if tools.is_empty() {
                return Err(ToolError::invalid_input(format!(
                    "task '{id}' requires allowed_tools for custom type"
                )));
            }
        }
        if !ids.insert(task.id.clone()) {
            return Err(ToolError::invalid_input(format!(
                "duplicate task id '{id}'"
            )));
        }
        if task.depends_on.iter().any(|dep| dep == id) {
            return Err(ToolError::invalid_input(format!(
                "task '{id}' cannot depend on itself"
            )));
        }
    }

    for task in tasks {
        for dep in &task.depends_on {
            if !ids.contains(dep) {
                return Err(ToolError::invalid_input(format!(
                    "task '{}' depends on unknown task '{dep}'",
                    task.id
                )));
            }
        }
    }

    if has_dependency_cycle(tasks) {
        return Err(ToolError::invalid_input(
            "task dependencies contain a cycle",
        ));
    }

    Ok(())
}

fn has_dependency_cycle(tasks: &[SwarmTaskSpec]) -> bool {
    let mut deps = HashMap::new();
    for task in tasks {
        deps.insert(task.id.clone(), task.depends_on.clone());
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();

    for id in deps.keys() {
        if visit(id, &deps, &mut visiting, &mut visited) {
            return true;
        }
    }

    false
}

fn visit(
    id: &str,
    deps: &HashMap<String, Vec<String>>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) -> bool {
    if visited.contains(id) {
        return false;
    }
    if !visiting.insert(id.to_string()) {
        return true;
    }
    if let Some(children) = deps.get(id) {
        for child in children {
            if visit(child, deps, visiting, visited) {
                return true;
            }
        }
    }
    visiting.remove(id);
    visited.insert(id.to_string());
    false
}

#[cfg(test)]
mod tests {
    use super::{SwarmTaskSpec, validate_swarm_tasks};

    fn task(id: &str, deps: &[&str]) -> SwarmTaskSpec {
        SwarmTaskSpec {
            id: id.to_string(),
            prompt: "do work".to_string(),
            agent_type: None,
            allowed_tools: None,
            depends_on: deps.iter().map(|dep| dep.to_string()).collect(),
        }
    }

    #[test]
    fn validate_swarm_tasks_accepts_valid_graph() {
        let tasks = vec![task("a", &[]), task("b", &["a"])];
        assert!(validate_swarm_tasks(&tasks).is_ok());
    }

    #[test]
    fn validate_swarm_tasks_rejects_unknown_dependency() {
        let tasks = vec![task("a", &["missing"])];
        assert!(validate_swarm_tasks(&tasks).is_err());
    }

    #[test]
    fn validate_swarm_tasks_rejects_cycle() {
        let tasks = vec![task("a", &["b"]), task("b", &["a"])];
        assert!(validate_swarm_tasks(&tasks).is_err());
    }
}
