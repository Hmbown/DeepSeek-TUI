//! Swarm orchestration for spawning multiple sub-agents with dependencies.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::core::events::Event;
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_bool, optional_str, optional_u64,
};
use crate::tools::subagent::{
    SharedSubAgentManager, SubAgentAssignment, SubAgentResult, SubAgentRuntime, SubAgentStatus,
    SubAgentType,
};

const SWARM_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_SWARM_TIMEOUT_MS: u64 = 600_000;
const DEFAULT_SWARM_TIMEOUT_NONBLOCK_MS: u64 = 600_000;
const MAX_SWARM_TIMEOUT_MS: u64 = 3_600_000;
const DEFAULT_SWARM_RESULT_TIMEOUT_MS: u64 = 30_000;
const MAX_SWARM_HISTORY: usize = 256;
const SWARM_STATE_SCHEMA_VERSION: u32 = 1;
const SWARM_STATE_FILE: &str = "swarm_outcomes.v1.json";
const DEFAULT_TASK_RETRY_DELAY_MS: u64 = 1_000;
const MAX_TASK_RETRY_DELAY_MS: u64 = 60_000;
const MAX_TASK_TIMEOUT_MS: u64 = 600_000;
const MAX_TASK_RETRIES: u32 = 10;

static SWARM_OUTCOMES: OnceLock<StdMutex<HashMap<String, SwarmOutcome>>> = OnceLock::new();
static SWARM_ORDER: OnceLock<StdMutex<VecDeque<String>>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
struct SwarmTaskSpec {
    id: String,
    prompt: String,
    #[serde(default, rename = "type")]
    agent_type: Option<SubAgentType>,
    #[serde(default, alias = "agent_role")]
    role: Option<String>,
    #[serde(default)]
    objective: Option<String>,
    #[serde(default)]
    retry_count: Option<u32>,
    #[serde(default)]
    retry_delay_ms: Option<u64>,
    #[serde(default)]
    task_timeout_ms: Option<u64>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SwarmTaskStatus {
    Pending,
    Running,
    Completed,
    Interrupted,
    Failed,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SwarmStatus {
    Running,
    Completed,
    Partial,
    Timeout,
    Failed,
}

impl SwarmStatus {
    fn is_terminal(&self) -> bool {
        !matches!(self, Self::Running)
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Partial => "partial",
            Self::Timeout => "timeout",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SwarmCounts {
    total: usize,
    completed: usize,
    interrupted: usize,
    failed: usize,
    cancelled: usize,
    skipped: usize,
    running: usize,
    pending: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SwarmOutcome {
    swarm_id: String,
    status: SwarmStatus,
    duration_ms: u64,
    counts: SwarmCounts,
    tasks: Vec<SwarmTaskOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSwarmStore {
    schema_version: u32,
    outcomes: HashMap<String, SwarmOutcome>,
    order: VecDeque<String>,
}

impl Default for PersistedSwarmStore {
    fn default() -> Self {
        Self {
            schema_version: SWARM_STATE_SCHEMA_VERSION,
            outcomes: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

fn swarm_outcomes_store() -> &'static StdMutex<HashMap<String, SwarmOutcome>> {
    SWARM_OUTCOMES.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn swarm_order_store() -> &'static StdMutex<VecDeque<String>> {
    SWARM_ORDER.get_or_init(|| StdMutex::new(VecDeque::new()))
}

fn swarm_state_path(workspace: &Path) -> PathBuf {
    workspace
        .join(".deepseek")
        .join("state")
        .join(SWARM_STATE_FILE)
}

fn load_swarm_store(path: &Path) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    let Ok(persisted) = serde_json::from_str::<PersistedSwarmStore>(&raw) else {
        return;
    };
    if persisted.schema_version != SWARM_STATE_SCHEMA_VERSION {
        return;
    }

    let mut outcomes = swarm_outcomes_store()
        .lock()
        .expect("swarm outcomes store lock poisoned");
    let mut order = swarm_order_store()
        .lock()
        .expect("swarm order store lock poisoned");
    for id in persisted.order {
        if let Some(outcome) = persisted.outcomes.get(&id)
            && !outcomes.contains_key(&id)
        {
            outcomes.insert(id.clone(), outcome.clone());
            order.push_back(id);
        }
    }
    while order.len() > MAX_SWARM_HISTORY {
        if let Some(oldest) = order.pop_front() {
            outcomes.remove(&oldest);
        }
    }
}

fn persist_swarm_store(path: &Path) {
    let outcomes = swarm_outcomes_store()
        .lock()
        .expect("swarm outcomes store lock poisoned");
    let order = swarm_order_store()
        .lock()
        .expect("swarm order store lock poisoned");
    let payload = PersistedSwarmStore {
        schema_version: SWARM_STATE_SCHEMA_VERSION,
        outcomes: outcomes.clone(),
        order: order.clone(),
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(&payload) {
        let tmp_path = path.with_extension("tmp");
        if fs::write(&tmp_path, raw).is_ok() {
            let _ = fs::rename(tmp_path, path);
        }
    }
}

fn store_swarm_outcome(outcome: &SwarmOutcome, persistence_path: Option<&Path>) {
    let mut outcomes = swarm_outcomes_store()
        .lock()
        .expect("swarm outcomes store lock poisoned");
    outcomes.insert(outcome.swarm_id.clone(), outcome.clone());

    let mut order = swarm_order_store()
        .lock()
        .expect("swarm order store lock poisoned");
    if let Some(idx) = order.iter().position(|id| id == &outcome.swarm_id) {
        let _ = order.remove(idx);
    }
    order.push_back(outcome.swarm_id.clone());

    while order.len() > MAX_SWARM_HISTORY {
        if let Some(oldest) = order.pop_front() {
            outcomes.remove(&oldest);
        }
    }

    if let Some(path) = persistence_path {
        persist_swarm_store(path);
    }
}

fn load_swarm_outcome(swarm_id: &str) -> Option<SwarmOutcome> {
    let outcomes = swarm_outcomes_store()
        .lock()
        .expect("swarm outcomes store lock poisoned");
    outcomes.get(swarm_id).cloned()
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
                            "objective": { "type": "string", "description": "Optional assignment objective shown in sub-agent views (defaults to prompt)." },
                            "type": { "type": "string", "description": "Sub-agent type: general, explore, plan, review, custom." },
                            "role": { "type": "string", "description": "Optional role alias: worker, explorer, awaiter, default." },
                            "agent_role": { "type": "string", "description": "Alias for role." },
                            "retry_count": { "type": "integer", "description": "Retries after the initial attempt (default: 0)." },
                            "retry_delay_ms": { "type": "integer", "description": "Base retry delay in milliseconds (default: 1000, exponential backoff)." },
                            "task_timeout_ms": { "type": "integer", "description": "Per-task timeout in milliseconds; cancels and optionally retries on timeout." },
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
        let persistence_path = swarm_state_path(&self.runtime.context.workspace);
        load_swarm_store(&persistence_path);

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

        let swarm_id = format!("swarm_{}", &Uuid::new_v4().to_string()[..8]);

        if block {
            let outcome = run_swarm(
                &self.manager,
                &self.runtime,
                swarm_id,
                tasks,
                shared_context,
                Duration::from_millis(timeout_ms),
                max_parallel,
                fail_fast,
                false,
                Some(persistence_path.clone()),
            )
            .await?;
            store_swarm_outcome(&outcome, Some(&persistence_path));
            return ToolResult::json(&outcome)
                .map_err(|err| ToolError::execution_failed(err.to_string()));
        }

        let initial = build_initial_outcome(&swarm_id, &tasks);
        store_swarm_outcome(&initial, Some(&persistence_path));

        let manager = self.manager.clone();
        let runtime = self.runtime.clone();
        let persistence_path_bg = persistence_path.clone();
        tokio::spawn(async move {
            let outcome = run_swarm(
                &manager,
                &runtime,
                swarm_id.clone(),
                tasks,
                shared_context,
                Duration::from_millis(timeout_ms),
                max_parallel,
                fail_fast,
                true,
                Some(persistence_path_bg.clone()),
            )
            .await
            .unwrap_or_else(|err| build_failed_outcome(&swarm_id, err.to_string()));
            store_swarm_outcome(&outcome, Some(&persistence_path_bg));
        });

        let mut result = ToolResult::json(&initial)
            .map_err(|err| ToolError::execution_failed(err.to_string()))?;
        result.metadata = Some(json!({
            "status": "Running",
            "swarm_id": initial.swarm_id,
        }));
        Ok(result)
    }
}

/// Tool to get lightweight swarm status.
pub struct SwarmStatusTool {
    persistence_path: PathBuf,
}

impl SwarmStatusTool {
    #[must_use]
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            persistence_path: swarm_state_path(&workspace),
        }
    }
}

#[async_trait]
impl ToolSpec for SwarmStatusTool {
    fn name(&self) -> &'static str {
        "swarm_status"
    }

    fn description(&self) -> &'static str {
        "Get the latest status for a previously spawned swarm."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "swarm_id": { "type": "string", "description": "Swarm id returned by agent_swarm." },
                "id": { "type": "string", "description": "Alias for swarm_id." }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        load_swarm_store(&self.persistence_path);
        let swarm_id = parse_swarm_id(&input)?;
        let outcome = load_swarm_outcome(swarm_id)
            .ok_or_else(|| ToolError::execution_failed(format!("Swarm '{swarm_id}' not found")))?;

        let snapshot = json!({
            "swarm_id": outcome.swarm_id,
            "status": outcome.status,
            "counts": outcome.counts,
            "duration_ms": outcome.duration_ms,
        });
        ToolResult::json(&snapshot).map_err(|err| ToolError::execution_failed(err.to_string()))
    }
}

/// Tool to fetch full swarm outcomes, optionally waiting for completion.
pub struct SwarmResultTool {
    persistence_path: PathBuf,
}

impl SwarmResultTool {
    #[must_use]
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            persistence_path: swarm_state_path(&workspace),
        }
    }
}

#[async_trait]
impl ToolSpec for SwarmResultTool {
    fn name(&self) -> &'static str {
        "swarm_result"
    }

    fn description(&self) -> &'static str {
        "Get full outcomes for a previously spawned swarm."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "swarm_id": { "type": "string", "description": "Swarm id returned by agent_swarm." },
                "id": { "type": "string", "description": "Alias for swarm_id." },
                "block": { "type": "boolean", "description": "Wait for terminal status (default: false)." },
                "timeout_ms": { "type": "integer", "description": "Max wait in milliseconds when block=true (default: 30000)." }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        load_swarm_store(&self.persistence_path);
        let swarm_id = parse_swarm_id(&input)?;
        let block = optional_bool(&input, "block", false);
        let timeout_ms = optional_u64(&input, "timeout_ms", DEFAULT_SWARM_RESULT_TIMEOUT_MS)
            .clamp(1_000, MAX_SWARM_TIMEOUT_MS);

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut timed_out = false;
        let outcome = loop {
            if let Some(outcome) = load_swarm_outcome(swarm_id) {
                if !block || outcome.status.is_terminal() {
                    break outcome;
                }
                if Instant::now() >= deadline {
                    timed_out = true;
                    break outcome;
                }
            } else if !block || Instant::now() >= deadline {
                return Err(ToolError::execution_failed(format!(
                    "Swarm '{swarm_id}' not found"
                )));
            }

            tokio::time::sleep(SWARM_POLL_INTERVAL).await;
        };

        let mut result = ToolResult::json(&outcome)
            .map_err(|err| ToolError::execution_failed(err.to_string()))?;
        if timed_out {
            result.metadata = Some(json!({
                "status": "TimedOut",
                "timed_out": true,
                "timeout_ms": timeout_ms,
            }));
        } else if !outcome.status.is_terminal() {
            result.metadata = Some(json!({ "status": "Running" }));
        }
        Ok(result)
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_swarm(
    shared_manager: &SharedSubAgentManager,
    runtime: &SubAgentRuntime,
    swarm_id: String,
    tasks: Vec<SwarmTaskSpec>,
    shared_context: Option<String>,
    timeout: Duration,
    max_parallel: usize,
    fail_fast: bool,
    publish_progress: bool,
    persistence_path: Option<PathBuf>,
) -> Result<SwarmOutcome, ToolError> {
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
    let mut running_started_at: HashMap<String, Instant> = HashMap::new();
    let mut attempts_made: HashMap<String, u32> = HashMap::new();
    let mut retry_ready_at: HashMap<String, Instant> = HashMap::new();
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
                let Some(task) = task_map.get(&task_id) else {
                    states.insert(
                        task_id.clone(),
                        SwarmTaskState::Failed("Missing swarm task".to_string()),
                    );
                    running.remove(&task_id);
                    running_started_at.remove(&task_id);
                    changed = true;
                    if fail_fast {
                        fail_fast_triggered = true;
                    }
                    continue;
                };

                if let Some(limit) = task_timeout(task) {
                    let started = running_started_at.get(&task_id).copied().unwrap_or(start);
                    if started.elapsed() >= limit {
                        let timeout_ms = u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
                        {
                            let mut manager = shared_manager.lock().await;
                            let _ = manager.cancel(&agent_id);
                        }

                        if schedule_retry_if_possible(
                            task,
                            &task_id,
                            &attempts_made,
                            fail_fast,
                            &mut pending,
                            &mut running,
                            &mut running_started_at,
                            &mut retry_ready_at,
                            &mut states,
                        ) {
                            changed = true;
                            continue;
                        }

                        states.insert(
                            task_id.clone(),
                            SwarmTaskState::Failed(format!("Timed out after {timeout_ms}ms")),
                        );
                        running.remove(&task_id);
                        running_started_at.remove(&task_id);
                        retry_ready_at.remove(&task_id);
                        changed = true;
                        if fail_fast {
                            fail_fast_triggered = true;
                        }
                        continue;
                    }
                }

                match snapshot_map.get(&agent_id) {
                    Some(snapshot) => {
                        if snapshot.status != SubAgentStatus::Running {
                            if matches!(
                                snapshot.status,
                                SubAgentStatus::Interrupted(_)
                                    | SubAgentStatus::Failed(_)
                                    | SubAgentStatus::Cancelled
                            ) && schedule_retry_if_possible(
                                task,
                                &task_id,
                                &attempts_made,
                                fail_fast,
                                &mut pending,
                                &mut running,
                                &mut running_started_at,
                                &mut retry_ready_at,
                                &mut states,
                            ) {
                                changed = true;
                                continue;
                            }

                            states.insert(task_id.clone(), SwarmTaskState::Done(snapshot.clone()));
                            running.remove(&task_id);
                            running_started_at.remove(&task_id);
                            retry_ready_at.remove(&task_id);
                            changed = true;
                            if fail_fast
                                && matches!(
                                    snapshot.status,
                                    SubAgentStatus::Interrupted(_)
                                        | SubAgentStatus::Failed(_)
                                        | SubAgentStatus::Cancelled
                                )
                            {
                                fail_fast_triggered = true;
                            }
                        }
                    }
                    None => {
                        if schedule_retry_if_possible(
                            task,
                            &task_id,
                            &attempts_made,
                            fail_fast,
                            &mut pending,
                            &mut running,
                            &mut running_started_at,
                            &mut retry_ready_at,
                            &mut states,
                        ) {
                            changed = true;
                            continue;
                        }

                        states.insert(
                            task_id.clone(),
                            SwarmTaskState::Failed("Agent result not found".to_string()),
                        );
                        running.remove(&task_id);
                        running_started_at.remove(&task_id);
                        changed = true;
                        if fail_fast {
                            fail_fast_triggered = true;
                        }
                    }
                }
            }
        }

        if fail_fast_triggered {
            apply_fail_fast(
                shared_manager,
                &mut states,
                &mut pending,
                &mut running,
                &mut running_started_at,
                &mut retry_ready_at,
            )
            .await?;
            if publish_progress {
                let progress = build_progress_outcome(
                    &swarm_id,
                    start,
                    &task_order,
                    &states,
                    SwarmStatus::Failed,
                );
                store_swarm_outcome(&progress, persistence_path.as_deref());
                emit_swarm_status(runtime.event_tx.as_ref(), &progress);
            }
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
        let now = Instant::now();
        for task_id in pending.iter() {
            if let Some(task) = task_map.get(task_id)
                && dependencies_satisfied(task, &states)
                && match retry_ready_at.get(task_id) {
                    Some(ready_at) => now >= *ready_at,
                    None => true,
                }
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
                    attempts_made
                        .entry(task_id.clone())
                        .and_modify(|count| *count = count.saturating_add(1))
                        .or_insert(1);
                    let (agent_type, role, objective) = resolve_task_assignment(task)?;
                    let prompt = format_prompt(shared_context.as_deref(), &task.prompt);
                    let assignment = SubAgentAssignment { objective, role };

                    let spawn_result = {
                        let mut manager = shared_manager.lock().await;
                        manager.spawn_background_with_assignment(
                            Arc::clone(shared_manager),
                            runtime.clone(),
                            agent_type,
                            prompt,
                            assignment,
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
                            running_started_at.insert(task_id.clone(), Instant::now());
                            retry_ready_at.remove(&task_id);
                            pending.remove(&task_id);
                            changed = true;
                        }
                        Err(err) => {
                            let message = err.to_string();
                            if message.contains("Sub-agent limit reached") {
                                if let Some(count) = attempts_made.get_mut(&task_id) {
                                    *count = count.saturating_sub(1);
                                }
                                break;
                            }
                            if schedule_retry_if_possible(
                                task,
                                &task_id,
                                &attempts_made,
                                fail_fast,
                                &mut pending,
                                &mut running,
                                &mut running_started_at,
                                &mut retry_ready_at,
                                &mut states,
                            ) {
                                changed = true;
                            } else {
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
        }

        if fail_fast_triggered {
            apply_fail_fast(
                shared_manager,
                &mut states,
                &mut pending,
                &mut running,
                &mut running_started_at,
                &mut retry_ready_at,
            )
            .await?;
            if publish_progress {
                let progress = build_progress_outcome(
                    &swarm_id,
                    start,
                    &task_order,
                    &states,
                    SwarmStatus::Failed,
                );
                store_swarm_outcome(&progress, persistence_path.as_deref());
                emit_swarm_status(runtime.event_tx.as_ref(), &progress);
            }
            break;
        }

        if pending.is_empty() && running.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            if !running.is_empty() {
                cancel_running_tasks(shared_manager, &running, &mut states).await?;
                running.clear();
                running_started_at.clear();
            }
            break;
        }

        if publish_progress && changed {
            let progress = build_progress_outcome(
                &swarm_id,
                start,
                &task_order,
                &states,
                SwarmStatus::Running,
            );
            store_swarm_outcome(&progress, persistence_path.as_deref());
            emit_swarm_status(runtime.event_tx.as_ref(), &progress);
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
        || counts.interrupted > 0
        || counts.cancelled > 0
        || counts.skipped > 0
        || counts.pending > 0
        || counts.running > 0
    {
        SwarmStatus::Partial
    } else {
        SwarmStatus::Completed
    };

    let outcome = SwarmOutcome {
        swarm_id,
        status,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        counts,
        tasks: outcomes,
    };
    emit_swarm_status(runtime.event_tx.as_ref(), &outcome);
    Ok(outcome)
}

fn build_initial_outcome(swarm_id: &str, tasks: &[SwarmTaskSpec]) -> SwarmOutcome {
    let task_ids = tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>();
    let states = tasks
        .iter()
        .map(|task| (task.id.clone(), SwarmTaskState::Pending))
        .collect::<HashMap<_, _>>();
    build_progress_outcome(
        swarm_id,
        Instant::now(),
        &task_ids,
        &states,
        SwarmStatus::Running,
    )
}

fn build_failed_outcome(swarm_id: &str, error: String) -> SwarmOutcome {
    SwarmOutcome {
        swarm_id: swarm_id.to_string(),
        status: SwarmStatus::Failed,
        duration_ms: 0,
        counts: SwarmCounts {
            total: 0,
            completed: 0,
            interrupted: 0,
            failed: 1,
            cancelled: 0,
            skipped: 0,
            running: 0,
            pending: 0,
        },
        tasks: vec![SwarmTaskOutcome {
            task_id: "swarm_runtime".to_string(),
            agent_id: None,
            status: SwarmTaskStatus::Failed,
            result: None,
            error: Some(error),
            steps_taken: 0,
            duration_ms: 0,
        }],
    }
}

fn build_progress_outcome(
    swarm_id: &str,
    start: Instant,
    order: &[String],
    states: &HashMap<String, SwarmTaskState>,
    status: SwarmStatus,
) -> SwarmOutcome {
    let tasks = build_task_outcomes(order, states);
    let counts = build_counts(&tasks);
    SwarmOutcome {
        swarm_id: swarm_id.to_string(),
        status,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        counts,
        tasks,
    }
}

fn emit_swarm_status(event_tx: Option<&tokio::sync::mpsc::Sender<Event>>, outcome: &SwarmOutcome) {
    let Some(event_tx) = event_tx else {
        return;
    };

    let message = format!(
        "Swarm {}: status={} completed={}/{} running={} interrupted={} failed={} skipped={} cancelled={}",
        outcome.swarm_id,
        outcome.status.as_str(),
        outcome.counts.completed,
        outcome.counts.total,
        outcome.counts.running,
        outcome.counts.interrupted,
        outcome.counts.failed,
        outcome.counts.skipped,
        outcome.counts.cancelled
    );
    let _ = event_tx.try_send(Event::Status { message });
}

fn parse_swarm_id(input: &Value) -> Result<&str, ToolError> {
    input
        .get("swarm_id")
        .or_else(|| input.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ToolError::missing_field("swarm_id"))
}

fn format_prompt(shared_context: Option<&str>, prompt: &str) -> String {
    if let Some(context) = shared_context {
        format!(
            "Shared context (authoritative):\n{context}\n\nTask:\n{prompt}\n\nReturn sections:\nSUMMARY\nEVIDENCE\nCHANGES\nRISKS"
        )
    } else {
        format!("{prompt}\n\nReturn sections:\nSUMMARY\nEVIDENCE\nCHANGES\nRISKS")
    }
}

fn normalize_role_alias(input: &str) -> Option<&'static str> {
    match input.to_ascii_lowercase().as_str() {
        "default" => Some("default"),
        "worker" | "general" => Some("worker"),
        "explorer" | "explore" => Some("explorer"),
        "awaiter" | "plan" | "planner" => Some("awaiter"),
        _ => None,
    }
}

fn default_role_for_type(agent_type: &SubAgentType) -> Option<&'static str> {
    match agent_type {
        SubAgentType::General => Some("worker"),
        SubAgentType::Explore => Some("explorer"),
        SubAgentType::Plan => Some("awaiter"),
        SubAgentType::Review | SubAgentType::Custom => None,
    }
}

fn resolve_task_assignment(
    task: &SwarmTaskSpec,
) -> Result<(SubAgentType, Option<String>, String), ToolError> {
    let prompt = task.prompt.trim();
    if prompt.is_empty() {
        return Err(ToolError::invalid_input(format!(
            "task '{}' prompt cannot be empty",
            task.id
        )));
    }

    let objective = task.objective.as_deref().unwrap_or(prompt).trim();
    if objective.is_empty() {
        return Err(ToolError::invalid_input(format!(
            "task '{}' objective cannot be empty",
            task.id
        )));
    }

    let normalized_role = task
        .role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(|role| {
            normalize_role_alias(role).ok_or_else(|| {
                ToolError::invalid_input(format!(
                    "task '{}' has invalid role '{}'. Use: worker, explorer, awaiter, default",
                    task.id, role
                ))
            })
        })
        .transpose()?;

    let role_type = task
        .role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .and_then(SubAgentType::from_str);

    if let (Some(explicit), Some(inferred)) = (&task.agent_type, &role_type)
        && explicit != inferred
    {
        return Err(ToolError::invalid_input(format!(
            "task '{}' has conflicting type and role values",
            task.id
        )));
    }

    let agent_type = task
        .agent_type
        .clone()
        .or(role_type)
        .unwrap_or(SubAgentType::General);

    let role = normalized_role
        .or_else(|| default_role_for_type(&agent_type))
        .map(str::to_string);

    Ok((agent_type, role, objective.to_string()))
}

fn task_retry_count(task: &SwarmTaskSpec) -> u32 {
    task.retry_count.unwrap_or(0).min(MAX_TASK_RETRIES)
}

fn task_retry_delay_ms(task: &SwarmTaskSpec) -> u64 {
    task.retry_delay_ms
        .unwrap_or(DEFAULT_TASK_RETRY_DELAY_MS)
        .clamp(1, MAX_TASK_RETRY_DELAY_MS)
}

fn task_timeout(task: &SwarmTaskSpec) -> Option<Duration> {
    task.task_timeout_ms
        .map(|timeout_ms| timeout_ms.clamp(1, MAX_TASK_TIMEOUT_MS))
        .map(Duration::from_millis)
}

fn retry_delay_for_attempt(task: &SwarmTaskSpec, attempts_made: u32) -> Duration {
    let base = task_retry_delay_ms(task);
    let exponent = attempts_made.saturating_sub(1).min(8);
    let factor = 1u64 << exponent;
    let delay = base.saturating_mul(factor).min(MAX_TASK_RETRY_DELAY_MS);
    Duration::from_millis(delay)
}

#[allow(clippy::too_many_arguments)]
fn schedule_retry_if_possible(
    task: &SwarmTaskSpec,
    task_id: &str,
    attempts_made: &HashMap<String, u32>,
    fail_fast: bool,
    pending: &mut HashSet<String>,
    running: &mut HashMap<String, String>,
    running_started_at: &mut HashMap<String, Instant>,
    retry_ready_at: &mut HashMap<String, Instant>,
    states: &mut HashMap<String, SwarmTaskState>,
) -> bool {
    if fail_fast {
        return false;
    }
    let attempts = attempts_made.get(task_id).copied().unwrap_or(0);
    if attempts == 0 || attempts > task_retry_count(task) {
        return false;
    }

    let delay = retry_delay_for_attempt(task, attempts);
    pending.insert(task_id.to_string());
    running.remove(task_id);
    running_started_at.remove(task_id);
    retry_ready_at.insert(task_id.to_string(), Instant::now() + delay);
    states.insert(task_id.to_string(), SwarmTaskState::Pending);
    true
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
            SubAgentStatus::Interrupted(_) | SubAgentStatus::Failed(_) | SubAgentStatus::Cancelled
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
    running_started_at: &mut HashMap<String, Instant>,
    retry_ready_at: &mut HashMap<String, Instant>,
) -> Result<(), ToolError> {
    cancel_running_tasks(manager, running, states).await?;
    for task_id in pending.drain() {
        states.insert(
            task_id,
            SwarmTaskState::Skipped("Skipped due to fail_fast".to_string()),
        );
    }
    running.clear();
    running_started_at.clear();
    retry_ready_at.clear();
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
                SubAgentStatus::Interrupted(err) => SwarmTaskOutcome {
                    task_id: task_id.clone(),
                    agent_id: Some(result.agent_id.clone()),
                    status: SwarmTaskStatus::Interrupted,
                    result: result.result.clone(),
                    error: Some(err.clone()),
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
        interrupted: 0,
        failed: 0,
        cancelled: 0,
        skipped: 0,
        running: 0,
        pending: 0,
    };

    for outcome in outcomes {
        match outcome.status {
            SwarmTaskStatus::Completed => counts.completed += 1,
            SwarmTaskStatus::Interrupted => counts.interrupted += 1,
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
        if let Some(retry_count) = task.retry_count
            && retry_count > MAX_TASK_RETRIES
        {
            return Err(ToolError::invalid_input(format!(
                "task '{id}' retry_count must be <= {MAX_TASK_RETRIES}"
            )));
        }
        if matches!(task.task_timeout_ms, Some(0)) {
            return Err(ToolError::invalid_input(format!(
                "task '{id}' task_timeout_ms must be > 0"
            )));
        }
        let (resolved_type, _, _) = resolve_task_assignment(task)?;
        if matches!(resolved_type, SubAgentType::Custom) {
            let tools = task.allowed_tools.as_deref().unwrap_or(&[]);
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
    use super::{
        SwarmStatus, SwarmTaskSpec, build_initial_outcome, parse_swarm_id, resolve_task_assignment,
        retry_delay_for_attempt, task_retry_count, task_timeout, validate_swarm_tasks,
    };
    use serde_json::json;
    use std::time::Duration;

    fn task(id: &str, deps: &[&str]) -> SwarmTaskSpec {
        SwarmTaskSpec {
            id: id.to_string(),
            prompt: "do work".to_string(),
            agent_type: None,
            role: None,
            objective: None,
            retry_count: None,
            retry_delay_ms: None,
            task_timeout_ms: None,
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

    #[test]
    fn validate_swarm_tasks_rejects_invalid_role_alias() {
        let mut tasks = vec![task("a", &[])];
        tasks[0].role = Some("invalid".to_string());
        assert!(validate_swarm_tasks(&tasks).is_err());
    }

    #[test]
    fn validate_swarm_tasks_rejects_conflicting_role_and_type() {
        let mut tasks = vec![task("a", &[])];
        tasks[0].agent_type = Some(crate::tools::subagent::SubAgentType::Explore);
        tasks[0].role = Some("worker".to_string());
        assert!(validate_swarm_tasks(&tasks).is_err());
    }

    #[test]
    fn validate_swarm_tasks_rejects_zero_task_timeout() {
        let mut tasks = vec![task("a", &[])];
        tasks[0].task_timeout_ms = Some(0);
        assert!(validate_swarm_tasks(&tasks).is_err());
    }

    #[test]
    fn retry_helpers_apply_caps_and_backoff() {
        let mut t = task("a", &[]);
        t.retry_count = Some(super::MAX_TASK_RETRIES + 5);
        t.retry_delay_ms = Some(250);
        t.task_timeout_ms = Some(super::MAX_TASK_TIMEOUT_MS + 5_000);

        assert_eq!(task_retry_count(&t), super::MAX_TASK_RETRIES);
        assert_eq!(
            task_timeout(&t).expect("timeout should exist"),
            Duration::from_millis(super::MAX_TASK_TIMEOUT_MS)
        );
        assert_eq!(retry_delay_for_attempt(&t, 1), Duration::from_millis(250));
        assert_eq!(retry_delay_for_attempt(&t, 2), Duration::from_millis(500));
    }

    #[test]
    fn resolve_task_assignment_infers_role_and_objective_defaults() {
        let mut task = task("a", &[]);
        task.prompt = "scan files".to_string();
        task.role = Some("explorer".to_string());
        let (agent_type, role, objective) =
            resolve_task_assignment(&task).expect("assignment should resolve");
        assert!(matches!(
            agent_type,
            crate::tools::subagent::SubAgentType::Explore
        ));
        assert_eq!(role.as_deref(), Some("explorer"));
        assert_eq!(objective, "scan files");
    }

    #[test]
    fn build_initial_outcome_marks_swarm_running() {
        let tasks = vec![task("a", &[]), task("b", &["a"])];
        let outcome = build_initial_outcome("swarm_test", &tasks);
        assert!(matches!(outcome.status, SwarmStatus::Running));
        assert_eq!(outcome.counts.total, 2);
        assert_eq!(outcome.counts.pending, 2);
    }

    #[test]
    fn parse_swarm_id_supports_alias() {
        let input = json!({ "id": "swarm_1234" });
        assert_eq!(parse_swarm_id(&input).unwrap(), "swarm_1234");
    }
}
