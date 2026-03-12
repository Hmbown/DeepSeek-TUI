//! Sub-agent spawning system.
//!
//! Provides tools to spawn background sub-agents, query their status,
//! and retrieve results. Sub-agents run with a filtered toolset and
//! inherit the workspace configuration from the main session.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, Semaphore};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{sync::mpsc, task::JoinHandle};
use uuid::Uuid;

use crate::client::DeepSeekClient;
use crate::config::MAX_SUBAGENTS;
use crate::core::events::Event;
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt, Tool};
use crate::tools::plan::{PlanState, SharedPlanState};
use crate::tools::registry::{ToolRegistry, ToolRegistryBuilder};
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_bool, optional_u64, required_str,
};
use crate::tools::todo::{SharedTodoList, TodoList};

// === Constants ===

const DEFAULT_MAX_STEPS: u32 = 20;
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);
const RESULT_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_RESULT_TIMEOUT_MS: u64 = 30_000;
const MIN_WAIT_TIMEOUT_MS: u64 = 10_000;
const MAX_RESULT_TIMEOUT_MS: u64 = 3_600_000;
const COMPLETED_AGENT_RETENTION: Duration = Duration::from_secs(60 * 60);
const SUBAGENT_STATE_SCHEMA_VERSION: u32 = 1;
const SUBAGENT_STATE_FILE: &str = "subagents.v1.json";
const SUBAGENT_RESTART_REASON: &str = "Interrupted by process restart";
const DEFAULT_CSV_MAX_CONCURRENCY: u64 = 16;
const DEFAULT_CSV_MAX_RUNTIME_SECONDS: u64 = 1800;
const MAX_CSV_MAX_RUNTIME_SECONDS: u64 = 86_400;

const VALID_SUBAGENT_TYPES: &str =
    "general, explore, plan, review, custom, worker, explorer, awaiter, default";

static AGENT_JOB_REPORTS: OnceLock<StdMutex<HashMap<String, HashMap<String, AgentJobReport>>>> =
    OnceLock::new();
static AGENT_JOB_ASSIGNMENTS: OnceLock<StdMutex<HashMap<String, HashMap<String, String>>>> =
    OnceLock::new();

// === Types ===

/// Assignment metadata for sub-agent orchestration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubAgentAssignment {
    pub objective: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl SubAgentAssignment {
    fn new(objective: String, role: Option<String>) -> Self {
        Self { objective, role }
    }
}

/// Sub-agent execution types with specialized behavior and tool access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentType {
    /// General purpose - full tool access for multi-step tasks.
    #[default]
    General,
    /// Fast exploration - read-only tools for codebase search.
    Explore,
    /// Planning - analysis tools only for architectural planning.
    Plan,
    /// Code review - read + analysis tools.
    Review,
    /// Custom tool access defined at spawn time.
    Custom,
}

impl SubAgentType {
    /// Parse a sub-agent type from user input.
    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "general" | "general-purpose" | "general_purpose" | "worker" | "default" => {
                Some(Self::General)
            }
            "explore" | "exploration" | "explorer" => Some(Self::Explore),
            "plan" | "planning" | "awaiter" => Some(Self::Plan),
            "review" | "code-review" | "code_review" | "reviewer" => Some(Self::Review),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Explore => "explore",
            Self::Plan => "plan",
            Self::Review => "review",
            Self::Custom => "custom",
        }
    }

    /// Get the system prompt for this agent type.
    #[must_use]
    pub fn system_prompt(&self) -> String {
        match self {
            Self::General => GENERAL_AGENT_PROMPT.to_string(),
            Self::Explore => EXPLORE_AGENT_PROMPT.to_string(),
            Self::Plan => PLAN_AGENT_PROMPT.to_string(),
            Self::Review => REVIEW_AGENT_PROMPT.to_string(),
            Self::Custom => CUSTOM_AGENT_PROMPT.to_string(),
        }
    }

    /// Get the default allowed tools for this agent type.
    #[must_use]
    pub fn allowed_tools(&self) -> Vec<&'static str> {
        match self {
            Self::General => vec![
                "list_dir",
                "read_file",
                "write_file",
                "edit_file",
                "apply_patch",
                "grep_files",
                "file_search",
                "web.run",
                "web_search",
                "multi_tool_use.parallel",
                "exec_shell",
                "exec_shell_wait",
                "exec_shell_interact",
                "exec_wait",
                "exec_interact",
                "note",
                "todo_write",
                "todo_add",
                "todo_update",
                "todo_list",
                "update_plan",
                "report_agent_job_result",
            ],
            Self::Explore => vec![
                "list_dir",
                "read_file",
                "grep_files",
                "file_search",
                "web.run",
                "web_search",
                "multi_tool_use.parallel",
                "exec_shell",
                "exec_shell_wait",
                "exec_shell_interact",
                "exec_wait",
                "exec_interact",
            ],
            Self::Plan => vec![
                "list_dir",
                "read_file",
                "grep_files",
                "file_search",
                "web.run",
                "note",
                "update_plan",
                "todo_write",
                "todo_add",
                "todo_update",
                "todo_list",
            ],
            Self::Review => vec!["list_dir", "read_file", "grep_files", "file_search", "note"],
            Self::Custom => vec![], // Must be provided by caller.
        }
    }
}

/// Status of a sub-agent execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubAgentStatus {
    Running,
    Completed,
    Interrupted(String),
    Failed(String),
    Cancelled,
}

/// Snapshot of sub-agent state for tool results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    pub agent_id: String,
    pub agent_type: SubAgentType,
    pub assignment: SubAgentAssignment,
    pub status: SubAgentStatus,
    pub result: Option<String>,
    pub steps_taken: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitMode {
    Any,
    All,
}

impl WaitMode {
    fn from_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "any" | "first" => Some(Self::Any),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::All => "all",
        }
    }

    fn condition_met(self, snapshots: &[SubAgentResult]) -> bool {
        match self {
            Self::Any => snapshots
                .iter()
                .any(|snapshot| snapshot.status != SubAgentStatus::Running),
            Self::All => snapshots
                .iter()
                .all(|snapshot| snapshot.status != SubAgentStatus::Running),
        }
    }
}

#[derive(Debug, Clone)]
struct SubAgentInput {
    text: String,
    interrupt: bool,
}

#[derive(Debug, Clone)]
struct SpawnRequest {
    prompt: String,
    agent_type: SubAgentType,
    assignment: SubAgentAssignment,
    allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct AssignRequest {
    agent_id: String,
    objective: Option<String>,
    role: Option<String>,
    message: Option<String>,
    interrupt: bool,
}

#[derive(Debug, Clone)]
struct CsvRowTask {
    row_index: usize,
    item_id: String,
    values: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
struct CsvWorkerOutcome {
    #[serde(skip_serializing)]
    row_index: usize,
    item_id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_id: Option<String>,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_json: Option<Value>,
}

#[derive(Debug, Clone)]
struct AgentJobReport {
    result: Value,
    stop: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SpawnAgentsOnCsvSummary {
    job_id: String,
    total: usize,
    completed: usize,
    failed: usize,
    timed_out: usize,
    skipped: usize,
    output_csv_path: String,
    results: Vec<CsvWorkerOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSubAgent {
    id: String,
    agent_type: SubAgentType,
    prompt: String,
    assignment: SubAgentAssignment,
    status: SubAgentStatus,
    result: Option<String>,
    steps_taken: u32,
    duration_ms: u64,
    allowed_tools: Vec<String>,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSubAgentState {
    schema_version: u32,
    agents: Vec<PersistedSubAgent>,
}

impl Default for PersistedSubAgentState {
    fn default() -> Self {
        Self {
            schema_version: SUBAGENT_STATE_SCHEMA_VERSION,
            agents: Vec::new(),
        }
    }
}

/// Runtime configuration for spawning sub-agents.
#[derive(Clone)]
pub struct SubAgentRuntime {
    pub client: DeepSeekClient,
    pub model: String,
    pub context: ToolContext,
    pub allow_shell: bool,
    pub event_tx: Option<mpsc::Sender<Event>>,
}

impl SubAgentRuntime {
    /// Create a runtime configuration for sub-agent execution.
    #[must_use]
    pub fn new(
        client: DeepSeekClient,
        model: String,
        context: ToolContext,
        allow_shell: bool,
        event_tx: Option<mpsc::Sender<Event>>,
    ) -> Self {
        Self {
            client,
            model,
            context,
            allow_shell,
            event_tx,
        }
    }
}

/// A running sub-agent instance.
pub struct SubAgent {
    pub id: String,
    pub agent_type: SubAgentType,
    pub prompt: String,
    pub assignment: SubAgentAssignment,
    pub status: SubAgentStatus,
    pub result: Option<String>,
    pub steps_taken: u32,
    pub started_at: Instant,
    pub allowed_tools: Vec<String>,
    input_tx: Option<mpsc::UnboundedSender<SubAgentInput>>,
    task_handle: Option<JoinHandle<()>>,
}

impl SubAgent {
    /// Create a new sub-agent.
    fn new(
        agent_type: SubAgentType,
        prompt: String,
        assignment: SubAgentAssignment,
        allowed_tools: Vec<String>,
        input_tx: mpsc::UnboundedSender<SubAgentInput>,
    ) -> Self {
        let id = format!("agent_{}", &Uuid::new_v4().to_string()[..8]);

        Self {
            id,
            agent_type,
            prompt,
            assignment,
            status: SubAgentStatus::Running,
            result: None,
            steps_taken: 0,
            started_at: Instant::now(),
            allowed_tools,
            input_tx: Some(input_tx),
            task_handle: None,
        }
    }

    /// Get a snapshot of the current state.
    #[must_use]
    pub fn snapshot(&self) -> SubAgentResult {
        SubAgentResult {
            agent_id: self.id.clone(),
            agent_type: self.agent_type.clone(),
            assignment: self.assignment.clone(),
            status: self.status.clone(),
            result: self.result.clone(),
            steps_taken: self.steps_taken,
            duration_ms: u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
        }
    }
}

/// Manager for active sub-agents.
pub struct SubAgentManager {
    agents: HashMap<String, SubAgent>,
    #[allow(dead_code)] // Stored for future workspace-scoped operations
    workspace: PathBuf,
    state_path: Option<PathBuf>,
    max_steps: u32,
    max_agents: usize,
}

impl SubAgentManager {
    /// Create a new manager for sub-agents.
    #[must_use]
    pub fn new(workspace: PathBuf, max_agents: usize) -> Self {
        Self {
            agents: HashMap::new(),
            workspace,
            state_path: None,
            max_steps: DEFAULT_MAX_STEPS,
            max_agents,
        }
    }

    #[must_use]
    fn with_state_path(mut self, path: PathBuf) -> Self {
        self.state_path = Some(path);
        self
    }

    fn persist_state(&self) -> Result<()> {
        let Some(path) = self.state_path.as_ref() else {
            return Ok(());
        };
        let now_ms = epoch_millis_now();
        let mut agents = Vec::with_capacity(self.agents.len());
        for agent in self.agents.values() {
            agents.push(PersistedSubAgent {
                id: agent.id.clone(),
                agent_type: agent.agent_type.clone(),
                prompt: agent.prompt.clone(),
                assignment: agent.assignment.clone(),
                status: agent.status.clone(),
                result: agent.result.clone(),
                steps_taken: agent.steps_taken,
                duration_ms: u64::try_from(agent.started_at.elapsed().as_millis())
                    .unwrap_or(u64::MAX),
                allowed_tools: agent.allowed_tools.clone(),
                updated_at_ms: now_ms,
            });
        }
        agents.sort_by(|a, b| a.id.cmp(&b.id));

        let payload = PersistedSubAgentState {
            schema_version: SUBAGENT_STATE_SCHEMA_VERSION,
            agents,
        };
        write_json_atomic(path, &payload)
    }

    fn persist_state_best_effort(&self) {
        if let Err(err) = self.persist_state() {
            eprintln!("Failed to persist sub-agent state: {err}");
        }
    }

    fn load_state(&mut self) -> Result<()> {
        let Some(path) = self.state_path.as_ref() else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }

        let raw = fs::read_to_string(path)?;
        let state = serde_json::from_str::<PersistedSubAgentState>(&raw)?;
        if state.schema_version != SUBAGENT_STATE_SCHEMA_VERSION {
            return Err(anyhow!(
                "Unsupported sub-agent state schema {}",
                state.schema_version
            ));
        }

        self.agents.clear();
        for persisted in state.agents {
            let mut status = persisted.status;
            if matches!(status, SubAgentStatus::Running) {
                status = SubAgentStatus::Interrupted(SUBAGENT_RESTART_REASON.to_string());
            }

            let started_at = instant_from_duration(Duration::from_millis(persisted.duration_ms));
            let agent = SubAgent {
                id: persisted.id.clone(),
                agent_type: persisted.agent_type,
                prompt: persisted.prompt,
                assignment: persisted.assignment,
                status,
                result: persisted.result,
                steps_taken: persisted.steps_taken,
                started_at,
                allowed_tools: persisted.allowed_tools,
                input_tx: None,
                task_handle: None,
            };
            self.agents.insert(persisted.id, agent);
        }

        Ok(())
    }

    /// Count running agents.
    pub fn running_count(&self) -> usize {
        self.agents
            .values()
            .filter(|agent| {
                if agent.status != SubAgentStatus::Running {
                    return false;
                }
                !agent
                    .task_handle
                    .as_ref()
                    .is_some_and(tokio::task::JoinHandle::is_finished)
            })
            .count()
    }

    /// Return the maximum number of allowed agents.
    #[must_use]
    pub fn max_agents(&self) -> usize {
        self.max_agents
    }

    /// Return remaining capacity for new agents.
    #[must_use]
    pub fn available_slots(&self) -> usize {
        self.max_agents.saturating_sub(self.running_count())
    }

    /// Spawn a new background sub-agent.
    pub fn spawn_background(
        &mut self,
        manager_handle: SharedSubAgentManager,
        runtime: SubAgentRuntime,
        agent_type: SubAgentType,
        prompt: String,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<SubAgentResult> {
        self.spawn_background_with_assignment(
            manager_handle,
            runtime,
            agent_type,
            prompt.clone(),
            SubAgentAssignment::new(prompt, None),
            allowed_tools,
        )
    }

    /// Spawn a new background sub-agent with explicit assignment metadata.
    pub fn spawn_background_with_assignment(
        &mut self,
        manager_handle: SharedSubAgentManager,
        runtime: SubAgentRuntime,
        agent_type: SubAgentType,
        prompt: String,
        assignment: SubAgentAssignment,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<SubAgentResult> {
        self.cleanup(COMPLETED_AGENT_RETENTION);

        if self.running_count() >= self.max_agents {
            return Err(anyhow!(
                "Sub-agent limit reached (max {}, running {}). Cancel, close, or wait for an existing agent to finish.",
                self.max_agents,
                self.running_count()
            ));
        }

        let tools = build_allowed_tools(&agent_type, allowed_tools, runtime.allow_shell)?;
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let mut agent = SubAgent::new(
            agent_type.clone(),
            prompt.clone(),
            assignment.clone(),
            tools.clone(),
            input_tx,
        );
        let agent_id = agent.id.clone();
        let started_at = agent.started_at;
        let max_steps = self.max_steps;

        if let Some(event_tx) = runtime.event_tx.clone() {
            let _ = event_tx.try_send(Event::AgentSpawned {
                id: agent_id.clone(),
                prompt: prompt.clone(),
            });
        }

        let task = SubAgentTask {
            manager_handle,
            runtime,
            agent_id: agent_id.clone(),
            agent_type,
            prompt,
            assignment,
            allowed_tools: tools,
            started_at,
            max_steps,
            input_rx,
        };
        let handle = tokio::spawn(run_subagent_task(task));
        agent.task_handle = Some(handle);
        self.agents.insert(agent_id.clone(), agent);
        self.persist_state_best_effort();

        Ok(self
            .agents
            .get(&agent_id)
            .expect("agent should exist after spawn")
            .snapshot())
    }

    /// Get the current snapshot for an agent.
    pub fn get_result(&self, agent_id: &str) -> Result<SubAgentResult> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;
        Ok(agent.snapshot())
    }

    /// Cancel a running sub-agent.
    pub fn cancel(&mut self, agent_id: &str) -> Result<SubAgentResult> {
        let (snapshot, changed) = {
            let agent = self
                .agents
                .get_mut(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;

            let mut changed = false;
            if agent.status == SubAgentStatus::Running {
                agent.status = SubAgentStatus::Cancelled;
                if let Some(handle) = agent.task_handle.take() {
                    handle.abort();
                }
                changed = true;
            }
            (agent.snapshot(), changed)
        };

        if changed {
            self.persist_state_best_effort();
        }
        Ok(snapshot)
    }

    /// Resume a non-running sub-agent by restarting it with the original assignment.
    pub fn resume(
        &mut self,
        manager_handle: SharedSubAgentManager,
        runtime: SubAgentRuntime,
        agent_id: &str,
    ) -> Result<SubAgentResult> {
        let status = self
            .agents
            .get(agent_id)
            .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?
            .status
            .clone();

        if status == SubAgentStatus::Running {
            let agent = self
                .agents
                .get(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;
            return Ok(agent.snapshot());
        }

        if self.running_count() >= self.max_agents {
            return Err(anyhow!(
                "Sub-agent limit reached (max {}, running {}). Close or wait for an existing agent before resuming.",
                self.max_agents,
                self.running_count()
            ));
        }

        let snapshot = {
            let agent = self
                .agents
                .get_mut(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;

            let (input_tx, input_rx) = mpsc::unbounded_channel();
            let restarted_at = Instant::now();
            let task = SubAgentTask {
                manager_handle,
                runtime: runtime.clone(),
                agent_id: agent.id.clone(),
                agent_type: agent.agent_type.clone(),
                prompt: agent.prompt.clone(),
                assignment: agent.assignment.clone(),
                allowed_tools: agent.allowed_tools.clone(),
                started_at: restarted_at,
                max_steps: self.max_steps,
                input_rx,
            };
            let handle = tokio::spawn(run_subagent_task(task));

            agent.status = SubAgentStatus::Running;
            agent.result = None;
            agent.steps_taken = 0;
            agent.started_at = restarted_at;
            agent.input_tx = Some(input_tx);
            agent.task_handle = Some(handle);

            if let Some(event_tx) = runtime.event_tx {
                let _ = event_tx.try_send(Event::AgentSpawned {
                    id: agent.id.clone(),
                    prompt: format!("(resumed) {}", agent.prompt),
                });
            }

            agent.snapshot()
        };
        self.persist_state_best_effort();

        Ok(snapshot)
    }

    /// Send input to a running sub-agent.
    pub fn send_input(&mut self, agent_id: &str, text: String, interrupt: bool) -> Result<()> {
        let agent = self
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;

        if agent.status != SubAgentStatus::Running {
            return Err(anyhow!("Agent {agent_id} is not running"));
        }

        let tx = agent
            .input_tx
            .as_ref()
            .ok_or_else(|| anyhow!("Agent {agent_id} cannot accept input"))?;

        tx.send(SubAgentInput { text, interrupt })
            .map_err(|_| anyhow!("Failed to send input to agent {agent_id}"))?;

        Ok(())
    }

    /// Update assignment metadata and optionally send immediate guidance.
    pub fn assign(
        &mut self,
        agent_id: &str,
        objective: Option<String>,
        role: Option<String>,
        message: Option<String>,
        interrupt: bool,
    ) -> Result<SubAgentResult> {
        if objective.is_none() && role.is_none() && message.is_none() {
            return Err(anyhow!(
                "Provide at least one of objective, role, or message"
            ));
        }

        if message.is_some() {
            let status = self
                .agents
                .get(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?
                .status
                .clone();
            if status != SubAgentStatus::Running {
                return Err(anyhow!(
                    "Agent {agent_id} is not running; cannot deliver assignment message"
                ));
            }
        }

        let mut changed = false;
        let (input_tx, payload) = {
            let agent = self
                .agents
                .get_mut(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;

            let mut assignment_lines = Vec::new();
            if let Some(objective) = objective {
                let objective = objective.trim();
                if objective.is_empty() {
                    return Err(anyhow!("objective cannot be empty"));
                }
                if agent.assignment.objective != objective {
                    agent.assignment.objective = objective.to_string();
                    changed = true;
                }
                assignment_lines.push(format!("- objective: {}", agent.assignment.objective));
            }

            if let Some(role) = role {
                let normalized = normalize_role_alias(&role)
                    .ok_or_else(|| {
                        anyhow!(
                            "Invalid role alias '{role}'. Use: worker, explorer, awaiter, default"
                        )
                    })?
                    .to_string();
                if agent.assignment.role.as_deref() != Some(normalized.as_str()) {
                    agent.assignment.role = Some(normalized.clone());
                    changed = true;
                }
                assignment_lines.push(format!("- role: {normalized}"));
            }

            let mut payload_parts = Vec::new();
            if !assignment_lines.is_empty() && agent.status == SubAgentStatus::Running {
                payload_parts.push(format!(
                    "Assignment updated:\n{}",
                    assignment_lines.join("\n")
                ));
            }
            if let Some(message) = message {
                let message = message.trim();
                if message.is_empty() {
                    return Err(anyhow!("message cannot be empty"));
                }
                payload_parts.push(format!("Coordinator note:\n{message}"));
            }

            let payload = if payload_parts.is_empty() {
                None
            } else {
                Some(payload_parts.join("\n\n"))
            };

            (agent.input_tx.clone(), payload)
        };

        if let Some(payload) = payload {
            let tx = input_tx
                .ok_or_else(|| anyhow!("Agent {agent_id} cannot accept assignment input"))?;
            tx.send(SubAgentInput {
                text: payload,
                interrupt,
            })
            .map_err(|_| anyhow!("Failed to send assignment to agent {agent_id}"))?;
        }

        if changed {
            self.persist_state_best_effort();
        }

        self.get_result(agent_id)
    }

    /// List all agents and their status.
    #[must_use]
    pub fn list(&self) -> Vec<SubAgentResult> {
        self.agents.values().map(SubAgent::snapshot).collect()
    }

    /// Clean up completed agents older than the given duration.
    pub fn cleanup(&mut self, max_age: Duration) {
        let before = self.agents.len();
        self.agents.retain(|_, agent| {
            if agent.status == SubAgentStatus::Running {
                true
            } else {
                agent.started_at.elapsed() < max_age
            }
        });
        if self.agents.len() != before {
            self.persist_state_best_effort();
        }
    }

    fn update_from_result(&mut self, agent_id: &str, result: SubAgentResult) {
        let mut changed = false;
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.status = result.status;
            agent.assignment = result.assignment;
            agent.result = result.result;
            agent.steps_taken = result.steps_taken;
            agent.task_handle = None;
            changed = true;
        }
        if changed {
            self.persist_state_best_effort();
        }
    }

    fn update_failed(&mut self, agent_id: &str, error: String) {
        let mut changed = false;
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.status = SubAgentStatus::Failed(error);
            agent.task_handle = None;
            changed = true;
        }
        if changed {
            self.persist_state_best_effort();
        }
    }
}

/// Thread-safe wrapper for `SubAgentManager`.
pub type SharedSubAgentManager = Arc<Mutex<SubAgentManager>>;

fn default_state_path(workspace: &Path) -> PathBuf {
    workspace
        .join(".deepseek")
        .join("state")
        .join(SUBAGENT_STATE_FILE)
}

fn epoch_millis_now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        Err(_) => 0,
    }
}

fn instant_from_duration(duration: Duration) -> Instant {
    Instant::now()
        .checked_sub(duration)
        .unwrap_or_else(Instant::now)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = serde_json::to_string_pretty(value)?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, payload)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

/// Create a shared sub-agent manager with a configurable limit.
#[must_use]
pub fn new_shared_subagent_manager(workspace: PathBuf, max_agents: usize) -> SharedSubAgentManager {
    let max_agents = max_agents.clamp(1, MAX_SUBAGENTS);
    let state_path = default_state_path(&workspace);
    let mut manager = SubAgentManager::new(workspace, max_agents).with_state_path(state_path);
    if let Err(err) = manager.load_state() {
        eprintln!("Failed to load sub-agent state: {err}");
    }
    Arc::new(Mutex::new(manager))
}

// === Tool Implementations ===

/// Tool to spawn a background sub-agent.
pub struct AgentSpawnTool {
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
    name: &'static str,
}

impl AgentSpawnTool {
    /// Create a new spawn tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: SubAgentRuntime) -> Self {
        Self::with_name(manager, runtime, "agent_spawn")
    }

    /// Create a new spawn tool with a custom tool name alias.
    #[must_use]
    pub fn with_name(
        manager: SharedSubAgentManager,
        runtime: SubAgentRuntime,
        name: &'static str,
    ) -> Self {
        Self {
            manager,
            runtime,
            name,
        }
    }
}

#[async_trait]
impl ToolSpec for AgentSpawnTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        "Spawn a background sub-agent to handle a focused task. Returns an agent_id immediately; follow with agent_result to retrieve the result."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Task description for the sub-agent"
                },
                "message": {
                    "type": "string",
                    "description": "Alias for prompt"
                },
                "objective": {
                    "type": "string",
                    "description": "Alias for prompt"
                },
                "items": {
                    "type": "array",
                    "description": "Structured input items (text, mention, skill, local_image, image)",
                    "items": {
                        "type": "object"
                    }
                },
                "type": {
                    "type": "string",
                    "description": "Sub-agent type: general, explore, plan, review, custom"
                },
                "agent_type": {
                    "type": "string",
                    "description": "Alias for type"
                },
                "agent_name": {
                    "type": "string",
                    "description": "Alias for type"
                },
                "role": {
                    "type": "string",
                    "description": "Role alias: worker, explorer, awaiter, default"
                },
                "agent_role": {
                    "type": "string",
                    "description": "Alias for role"
                },
                "allowed_tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Explicit tool allowlist (required for custom type)"
                }
            }
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
        let spawn_request = parse_spawn_request(&input)?;

        let mut manager = self.manager.lock().await;

        let result = manager
            .spawn_background_with_assignment(
                Arc::clone(&self.manager),
                self.runtime.clone(),
                spawn_request.agent_type,
                spawn_request.prompt,
                spawn_request.assignment,
                spawn_request.allowed_tools,
            )
            .map_err(|e| ToolError::execution_failed(format!("Failed to spawn sub-agent: {e}")))?;

        let mut tool_result = if self.name == "spawn_agent" {
            let payload = json!({
                "agent_id": result.agent_id.clone(),
                "nickname": Value::Null
            });
            ToolResult::json(&payload).map_err(|e| ToolError::execution_failed(e.to_string()))?
        } else {
            ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))?
        };
        if result.status == SubAgentStatus::Running {
            if self.name == "spawn_agent" {
                tool_result.metadata = Some(json!({
                    "status": "Running",
                    "snapshot": result
                }));
            } else {
                tool_result.metadata = Some(json!({ "status": "Running" }));
            }
        }
        Ok(tool_result)
    }
}

/// Tool to fetch a sub-agent's result.
pub struct AgentResultTool {
    manager: SharedSubAgentManager,
}

impl AgentResultTool {
    /// Create a new result tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolSpec for AgentResultTool {
    fn name(&self) -> &'static str {
        "agent_result"
    }

    fn description(&self) -> &'static str {
        "Get the latest status or final result for a sub-agent."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "ID returned by agent_spawn"
                },
                "id": {
                    "type": "string",
                    "description": "Alias for agent_id"
                },
                "block": {
                    "type": "boolean",
                    "description": "Wait for completion (default: false)"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Max wait time in milliseconds (default: 30000, clamped to 1000-3600000)"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let agent_id = input
            .get("agent_id")
            .or_else(|| input.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::missing_field("agent_id"))?;
        let block = optional_bool(&input, "block", false);
        let timeout_ms = optional_u64(&input, "timeout_ms", DEFAULT_RESULT_TIMEOUT_MS)
            .clamp(1000, MAX_RESULT_TIMEOUT_MS);

        let (result, timed_out) = if block {
            wait_for_result(&self.manager, agent_id, Duration::from_millis(timeout_ms)).await?
        } else {
            let manager = self.manager.lock().await;
            (
                manager
                    .get_result(agent_id)
                    .map_err(|e| ToolError::execution_failed(e.to_string()))?,
                false,
            )
        };

        let mut tool_result =
            ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        if timed_out {
            tool_result.metadata = Some(json!({
                "status": "TimedOut",
                "timed_out": true,
                "timeout_ms": timeout_ms
            }));
        } else if result.status == SubAgentStatus::Running {
            tool_result.metadata = Some(json!({ "status": "Running" }));
        }
        Ok(tool_result)
    }
}

/// Tool to cancel a sub-agent.
pub struct AgentCancelTool {
    manager: SharedSubAgentManager,
}

impl AgentCancelTool {
    /// Create a new cancel tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolSpec for AgentCancelTool {
    fn name(&self) -> &'static str {
        "agent_cancel"
    }

    fn description(&self) -> &'static str {
        "Cancel a running sub-agent."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "ID returned by agent_spawn"
                }
            },
            "required": ["agent_id"]
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
        let agent_id = required_str(&input, "agent_id")?;
        let mut manager = self.manager.lock().await;
        let result = manager
            .cancel(agent_id)
            .map_err(|e| ToolError::execution_failed(format!("Failed to cancel sub-agent: {e}")))?;

        ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Tool to list all sub-agents.
pub struct AgentListTool {
    manager: SharedSubAgentManager,
}

/// Tool to close a running sub-agent (alias for cancel).
pub struct AgentCloseTool {
    manager: SharedSubAgentManager,
}

impl AgentCloseTool {
    /// Create a new close tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolSpec for AgentCloseTool {
    fn name(&self) -> &'static str {
        "close_agent"
    }

    fn description(&self) -> &'static str {
        "Close a running sub-agent. Alias for agent_cancel."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Agent id returned by agent_spawn"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Alias for id"
                }
            }
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
        let agent_id = input
            .get("id")
            .or_else(|| input.get("agent_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::missing_field("id"))?;
        let mut manager = self.manager.lock().await;
        let result = manager
            .cancel(agent_id)
            .map_err(|e| ToolError::execution_failed(format!("Failed to close sub-agent: {e}")))?;
        ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Tool to resume an existing sub-agent.
pub struct AgentResumeTool {
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
}

impl AgentResumeTool {
    /// Create a new resume tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: SubAgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[async_trait]
impl ToolSpec for AgentResumeTool {
    fn name(&self) -> &'static str {
        "resume_agent"
    }

    fn description(&self) -> &'static str {
        "Resume a previously closed or completed sub-agent by restarting its assignment."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Agent id to resume"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Alias for id"
                }
            }
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
        let agent_id = input
            .get("id")
            .or_else(|| input.get("agent_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::missing_field("id"))?;
        let mut manager = self.manager.lock().await;
        let result = manager
            .resume(Arc::clone(&self.manager), self.runtime.clone(), agent_id)
            .map_err(|e| ToolError::execution_failed(format!("Failed to resume sub-agent: {e}")))?;
        ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

impl AgentListTool {
    /// Create a new list tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolSpec for AgentListTool {
    fn name(&self) -> &'static str {
        "agent_list"
    }

    fn description(&self) -> &'static str {
        "List all active and completed sub-agents with their status."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(
        &self,
        _input: Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let mut manager = self.manager.lock().await;
        manager.cleanup(COMPLETED_AGENT_RETENTION);
        let results = manager.list();
        ToolResult::json(&results).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Tool to send input to a running sub-agent.
pub struct AgentSendInputTool {
    manager: SharedSubAgentManager,
    name: &'static str,
}

impl AgentSendInputTool {
    /// Create a new send-input tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, name: &'static str) -> Self {
        Self { manager, name }
    }
}

#[async_trait]
impl ToolSpec for AgentSendInputTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        "Send input to a running sub-agent."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "ID returned by agent_spawn"
                },
                "id": {
                    "type": "string",
                    "description": "Alias for agent_id"
                },
                "message": {
                    "type": "string",
                    "description": "Message to deliver to the agent"
                },
                "input": {
                    "type": "string",
                    "description": "Alias for message"
                },
                "items": {
                    "type": "array",
                    "description": "Structured input items (text, mention, skill, local_image, image)",
                    "items": {
                        "type": "object"
                    }
                },
                "interrupt": {
                    "type": "boolean",
                    "description": "Prioritize this message over pending inputs"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let agent_id = input
            .get("agent_id")
            .or_else(|| input.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::missing_field("agent_id"))?;
        let message = parse_text_or_items(&input, &["message", "input"], "items", "message")?;
        let interrupt = optional_bool(&input, "interrupt", false);

        let mut manager = self.manager.lock().await;
        manager
            .send_input(agent_id, message, interrupt)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;
        let snapshot = manager
            .get_result(agent_id)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        ToolResult::json(&snapshot).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Tool to update assignment metadata for a sub-agent.
pub struct AgentAssignTool {
    manager: SharedSubAgentManager,
    name: &'static str,
}

impl AgentAssignTool {
    /// Create a new assignment tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, name: &'static str) -> Self {
        Self { manager, name }
    }
}

#[async_trait]
impl ToolSpec for AgentAssignTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        "Update a sub-agent assignment and optionally send an immediate instruction."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "Agent id returned by agent_spawn"
                },
                "id": {
                    "type": "string",
                    "description": "Alias for agent_id"
                },
                "objective": {
                    "type": "string",
                    "description": "Updated assignment objective"
                },
                "role": {
                    "type": "string",
                    "description": "Updated role alias: worker, explorer, awaiter, default"
                },
                "agent_role": {
                    "type": "string",
                    "description": "Alias for role"
                },
                "message": {
                    "type": "string",
                    "description": "Optional coordinator note to send to the agent"
                },
                "input": {
                    "type": "string",
                    "description": "Alias for message"
                },
                "items": {
                    "type": "array",
                    "description": "Structured input items (text, mention, skill, local_image, image)",
                    "items": {
                        "type": "object"
                    }
                },
                "interrupt": {
                    "type": "boolean",
                    "description": "Prioritize this assignment update in the agent inbox (default: true)"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let request = parse_assign_request(&input)?;
        let mut manager = self.manager.lock().await;
        let result = manager
            .assign(
                &request.agent_id,
                request.objective,
                request.role,
                request.message,
                request.interrupt,
            )
            .map_err(|e| ToolError::execution_failed(format!("Failed to assign sub-agent: {e}")))?;

        ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Tool to wait for sub-agents to complete.
pub struct AgentWaitTool {
    manager: SharedSubAgentManager,
    name: &'static str,
}

impl AgentWaitTool {
    /// Create a new wait tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, name: &'static str) -> Self {
        Self { manager, name }
    }
}

#[async_trait]
impl ToolSpec for AgentWaitTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        "Wait for one or more sub-agents to reach a terminal status."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Agent IDs to wait on. When omitted, waits on all currently running sub-agents."
                },
                "agent_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Alias for ids"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Single agent ID"
                },
                "id": {
                    "type": "string",
                    "description": "Alias for agent_id"
                },
                "wait_mode": {
                    "type": "string",
                    "description": "Wait behavior: any (default) or all"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Max wait time in milliseconds (default: 30000, clamped to 10000-3600000)"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let timeout_ms = optional_u64(&input, "timeout_ms", DEFAULT_RESULT_TIMEOUT_MS)
            .clamp(MIN_WAIT_TIMEOUT_MS, MAX_RESULT_TIMEOUT_MS);
        let mut ids = parse_wait_ids(&input);
        if ids.is_empty() {
            let manager = self.manager.lock().await;
            ids = manager
                .list()
                .into_iter()
                .filter(|snapshot| snapshot.status == SubAgentStatus::Running)
                .map(|snapshot| snapshot.agent_id)
                .collect();
        }
        let wait_mode = parse_wait_mode(&input)?;

        if ids.is_empty() {
            let empty: Vec<SubAgentResult> = Vec::new();
            let mut result =
                ToolResult::json(&empty).map_err(|e| ToolError::execution_failed(e.to_string()))?;
            result.metadata = Some(json!({
                "wait_mode": wait_mode.as_str(),
                "timed_out": false,
                "status": "Completed",
                "timeout_ms": timeout_ms,
                "waited_ids": [],
                "completed_ids": [],
                "running_ids": [],
                "status_by_id": {}
            }));
            return Ok(result);
        }

        let waited_ids = ids.clone();

        let (snapshots, timed_out) = wait_for_agents(
            &self.manager,
            &ids,
            wait_mode,
            Duration::from_millis(timeout_ms),
        )
        .await?;

        let all_done = snapshots
            .iter()
            .all(|snapshot| snapshot.status != SubAgentStatus::Running);
        let completed_ids = snapshots
            .iter()
            .filter(|snapshot| snapshot.status != SubAgentStatus::Running)
            .map(|snapshot| snapshot.agent_id.clone())
            .collect::<Vec<_>>();
        let running_ids = snapshots
            .iter()
            .filter(|snapshot| snapshot.status == SubAgentStatus::Running)
            .map(|snapshot| snapshot.agent_id.clone())
            .collect::<Vec<_>>();
        let status_by_id = snapshots
            .iter()
            .map(|snapshot| {
                (
                    snapshot.agent_id.clone(),
                    subagent_status_name(&snapshot.status).to_string(),
                )
            })
            .collect::<HashMap<_, _>>();

        let mut result =
            ToolResult::json(&snapshots).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        result.metadata = Some(json!({
            "wait_mode": wait_mode.as_str(),
            "timed_out": timed_out,
            "status": if timed_out { "TimedOut" } else if all_done { "Completed" } else { "Partial" },
            "timeout_ms": timeout_ms,
            "waited_ids": waited_ids,
            "completed_ids": completed_ids,
            "running_ids": running_ids,
            "status_by_id": status_by_id
        }));
        Ok(result)
    }
}

/// Tool to delegate a task to a specialized agent (alias for agent_spawn).
pub struct DelegateToAgentTool {
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
}

impl DelegateToAgentTool {
    /// Create a new delegation tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: SubAgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[async_trait]
impl ToolSpec for DelegateToAgentTool {
    fn name(&self) -> &'static str {
        "delegate_to_agent"
    }

    fn description(&self) -> &'static str {
        "Delegate a task to a specialized sub-agent. This is an alias for agent_spawn."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_name": {
                    "type": "string",
                    "description": "Name/type alias for the agent (general, explore, plan, review, worker, explorer, awaiter)"
                },
                "type": {
                    "type": "string",
                    "description": "Alias for agent_name"
                },
                "agent_type": {
                    "type": "string",
                    "description": "Alias for agent_name"
                },
                "role": {
                    "type": "string",
                    "description": "Role alias: worker, explorer, awaiter, default"
                },
                "agent_role": {
                    "type": "string",
                    "description": "Alias for role"
                },
                "objective": {
                    "type": "string",
                    "description": "The goal or task description for the agent"
                },
                "prompt": {
                    "type": "string",
                    "description": "Alias for objective"
                },
                "message": {
                    "type": "string",
                    "description": "Alias for objective"
                },
                "items": {
                    "type": "array",
                    "description": "Structured input items (text, mention, skill, local_image, image)",
                    "items": {
                        "type": "object"
                    }
                },
                "allowed_tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Explicit tool allowlist (required for custom type)"
                }
            }
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

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let spawn_tool = AgentSpawnTool::new(self.manager.clone(), self.runtime.clone());
        spawn_tool.execute(input, context).await
    }
}

/// Tool to process CSV rows by spawning one worker sub-agent per row.
pub struct SpawnAgentsOnCsvTool {
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
}

struct AgentJobReportCleanup {
    job_id: String,
}

impl AgentJobReportCleanup {
    fn new(job_id: String) -> Self {
        clear_agent_job_results(&job_id);
        Self { job_id }
    }
}

impl Drop for AgentJobReportCleanup {
    fn drop(&mut self) {
        clear_agent_job_results(&self.job_id);
    }
}

impl SpawnAgentsOnCsvTool {
    /// Create a new CSV batch orchestration tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: SubAgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[async_trait]
impl ToolSpec for SpawnAgentsOnCsvTool {
    fn name(&self) -> &'static str {
        "spawn_agents_on_csv"
    }

    fn description(&self) -> &'static str {
        "Process a CSV by spawning one worker sub-agent per row. The instruction string is a template where `{column}` placeholders are replaced with row values. Each worker must call `report_agent_job_result` with a JSON object (matching `output_schema` when provided); missing reports are treated as failures. This call blocks until all rows finish and automatically exports results to `output_csv_path` (or a default path)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "csv_path": {
                    "type": "string",
                    "description": "Path to the input CSV file"
                },
                "instruction": {
                    "type": "string",
                    "description": "Instruction template. Use {column_name} placeholders for row values."
                },
                "id_column": {
                    "type": "string",
                    "description": "Optional CSV column name used as stable item id"
                },
                "max_concurrency": {
                    "type": "integer",
                    "description": "Maximum concurrent workers (default: 16)"
                },
                "max_workers": {
                    "type": "integer",
                    "description": "Alias for max_concurrency"
                },
                "max_runtime_seconds": {
                    "type": "integer",
                    "description": "Per-worker timeout in seconds (default: 1800)"
                },
                "output_csv_path": {
                    "type": "string",
                    "description": "Optional output CSV path for worker results"
                },
                "output_schema": {
                    "type": "object",
                    "description": "Optional JSON schema-like object used to validate worker JSON output"
                }
            },
            "required": ["csv_path", "instruction"]
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

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let csv_path_raw = required_str(&input, "csv_path")?;
        let csv_path = context.resolve_path(csv_path_raw)?;
        let instruction_template = required_str(&input, "instruction")?;
        if instruction_template.trim().is_empty() {
            return Err(ToolError::invalid_input(
                "instruction cannot be empty".to_string(),
            ));
        }

        let id_column = optional_input_str(&input, &["id_column"]).map(str::to_string);
        let rows = load_csv_rows(&csv_path, id_column.as_deref())?;
        if rows.is_empty() {
            return Err(ToolError::invalid_input(format!(
                "CSV '{}' has no data rows",
                csv_path.display()
            )));
        }

        let output_schema = input.get("output_schema").cloned();
        let output_csv_path = resolve_results_csv_path(context, &input, &csv_path)?;
        let max_runtime_seconds = optional_u64(
            &input,
            "max_runtime_seconds",
            DEFAULT_CSV_MAX_RUNTIME_SECONDS,
        )
        .clamp(1, MAX_CSV_MAX_RUNTIME_SECONDS);
        let requested_concurrency = parse_csv_concurrency(&input);

        let max_agents = {
            let manager = self.manager.lock().await;
            manager.max_agents().max(1)
        };
        let max_concurrency = requested_concurrency.clamp(1, max_agents as u64) as usize;

        let semaphore = Arc::new(Semaphore::new(max_concurrency));
        let timeout = Duration::from_secs(max_runtime_seconds);
        let job_id = format!("job_{}", &Uuid::new_v4().to_string()[..8]);
        let _cleanup = AgentJobReportCleanup::new(job_id.clone());
        let stop_requested = Arc::new(AtomicBool::new(false));
        let mut workers = FuturesUnordered::new();

        for row in rows {
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| ToolError::execution_failed("Worker semaphore closed"))?;
            let manager = self.manager.clone();
            let runtime = self.runtime.clone();
            let template = instruction_template.to_string();
            let schema = output_schema.clone();
            let job_id = job_id.clone();
            let stop_requested = stop_requested.clone();

            workers.push(tokio::spawn(async move {
                let _permit = permit;
                run_csv_row_agent(
                    manager,
                    runtime,
                    &job_id,
                    row,
                    &template,
                    timeout,
                    schema,
                    stop_requested,
                )
                .await
            }));
        }

        let mut outcomes = Vec::new();
        while let Some(joined) = workers.next().await {
            match joined {
                Ok(outcome) => outcomes.push(outcome),
                Err(err) => outcomes.push(CsvWorkerOutcome {
                    row_index: usize::MAX,
                    item_id: "worker_join".to_string(),
                    status: "failed".to_string(),
                    agent_id: None,
                    duration_ms: 0,
                    error: Some(format!("Worker task failed to join: {err}")),
                    result: None,
                    result_json: None,
                }),
            }
        }

        outcomes.sort_by_key(|outcome| outcome.row_index);

        write_csv_worker_outcomes(&output_csv_path, &outcomes).map_err(|err| {
            ToolError::execution_failed(format!("Failed to write output CSV: {err}"))
        })?;

        let completed = outcomes
            .iter()
            .filter(|outcome| outcome.status == "completed")
            .count();
        let skipped = outcomes
            .iter()
            .filter(|outcome| outcome.status == "skipped")
            .count();
        let timed_out = outcomes
            .iter()
            .filter(|outcome| outcome.status == "timed_out")
            .count();
        let failed = outcomes
            .iter()
            .filter(|outcome| outcome.status == "failed")
            .count()
            + timed_out;

        let summary = SpawnAgentsOnCsvSummary {
            job_id,
            total: outcomes.len(),
            completed,
            failed,
            timed_out,
            skipped,
            output_csv_path: output_csv_path.display().to_string(),
            results: outcomes,
        };
        let status = if summary.failed > 0 {
            if summary.completed == 0 && summary.skipped == 0 {
                "Failed"
            } else {
                "Partial"
            }
        } else if stop_requested.load(Ordering::Relaxed) || summary.skipped > 0 {
            "Cancelled"
        } else {
            "Completed"
        };
        let mut result =
            ToolResult::json(&summary).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        result.metadata = Some(json!({
            "status": status,
            "job_id": summary.job_id,
            "completed": summary.completed,
            "failed": summary.failed,
            "timed_out": summary.timed_out,
            "skipped": summary.skipped,
            "stop_requested": stop_requested.load(Ordering::Relaxed),
            "output_csv_path": summary.output_csv_path,
        }));
        Ok(result)
    }
}

/// Worker-oriented tool to report structured row outcomes for CSV agent jobs.
pub struct ReportAgentJobResultTool;

#[async_trait]
impl ToolSpec for ReportAgentJobResultTool {
    fn name(&self) -> &'static str {
        "report_agent_job_result"
    }

    fn description(&self) -> &'static str {
        "Worker-only tool to report a structured result for a spawn_agents_on_csv row."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "job_id": {
                    "type": "string",
                    "description": "Identifier of the CSV job"
                },
                "item_id": {
                    "type": "string",
                    "description": "Identifier of the CSV row item"
                },
                "result": {
                    "type": "object",
                    "description": "Structured JSON result to record for the row"
                },
                "stop": {
                    "type": "boolean",
                    "description": "Optional. When true, cancels remaining unstarted CSV rows for this job."
                }
            },
            "required": ["job_id", "item_id", "result"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let job_id = required_str(&input, "job_id")?.trim();
        let item_id = required_str(&input, "item_id")?.trim();
        if job_id.is_empty() {
            return Err(ToolError::invalid_input("job_id cannot be empty"));
        }
        if item_id.is_empty() {
            return Err(ToolError::invalid_input("item_id cannot be empty"));
        }
        let result = input
            .get("result")
            .cloned()
            .ok_or_else(|| ToolError::missing_field("result"))?;
        if !result.is_object() {
            return Err(ToolError::invalid_input("result must be a JSON object"));
        }
        let reporting_agent_id = input
            .get("__reporting_agent_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let stop = optional_bool(&input, "stop", false);
        let accepted =
            record_agent_job_result(job_id, item_id, result.clone(), stop, reporting_agent_id);

        let payload = json!({
            "job_id": job_id,
            "item_id": item_id,
            "accepted": accepted,
            "stop": stop,
            "result": result
        });
        ToolResult::json(&payload).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

// === Sub-agent Execution ===

struct SubAgentTask {
    manager_handle: SharedSubAgentManager,
    runtime: SubAgentRuntime,
    agent_id: String,
    agent_type: SubAgentType,
    prompt: String,
    assignment: SubAgentAssignment,
    allowed_tools: Vec<String>,
    started_at: Instant,
    max_steps: u32,
    input_rx: mpsc::UnboundedReceiver<SubAgentInput>,
}

#[allow(clippy::too_many_lines)]
async fn run_subagent_task(task: SubAgentTask) {
    let result = run_subagent(
        &task.runtime,
        task.agent_id.clone(),
        task.agent_type,
        task.prompt,
        task.assignment,
        task.allowed_tools,
        task.started_at,
        task.max_steps,
        task.input_rx,
    )
    .await;

    let mut manager = task.manager_handle.lock().await;
    match &result {
        Ok(res) => manager.update_from_result(&task.agent_id, res.clone()),
        Err(err) => manager.update_failed(&task.agent_id, err.to_string()),
    }

    if let Some(event_tx) = task.runtime.event_tx {
        let status = match &result {
            Ok(res) => summarize_subagent_result(res),
            Err(err) => format!("Failed: {err}"),
        };
        let _ = event_tx.try_send(Event::AgentComplete {
            id: task.agent_id,
            result: status,
        });
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn run_subagent(
    runtime: &SubAgentRuntime,
    agent_id: String,
    agent_type: SubAgentType,
    prompt: String,
    assignment: SubAgentAssignment,
    allowed_tools: Vec<String>,
    started_at: Instant,
    max_steps: u32,
    mut input_rx: mpsc::UnboundedReceiver<SubAgentInput>,
) -> Result<SubAgentResult> {
    let system_prompt = agent_type.system_prompt();
    let tool_registry = SubAgentToolRegistry::new(
        runtime.context.clone(),
        allowed_tools.clone(),
        runtime.allow_shell,
        Arc::new(Mutex::new(TodoList::new())),
        Arc::new(Mutex::new(PlanState::default())),
    );
    let unavailable_tools = tool_registry.unavailable_allowed_tools();
    if !unavailable_tools.is_empty() {
        return Err(anyhow!(
            "Sub-agent requested unavailable tools: {}",
            unavailable_tools.join(", ")
        ));
    }
    let tools = tool_registry.tools_for_model();
    emit_agent_progress(
        runtime.event_tx.as_ref(),
        &agent_id,
        format!("started ({})", agent_type.as_str()),
    );

    let mut messages = vec![Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: build_assignment_prompt(&prompt, &assignment, &agent_type),
            cache_control: None,
        }],
    }];

    let mut steps = 0;
    let mut final_result: Option<String> = None;
    let mut pending_inputs: VecDeque<SubAgentInput> = VecDeque::new();

    for _step in 0..max_steps {
        steps += 1;
        emit_agent_progress(
            runtime.event_tx.as_ref(),
            &agent_id,
            format!("step {steps}/{max_steps}: requesting model response"),
        );

        while let Ok(input) = input_rx.try_recv() {
            if input.interrupt {
                pending_inputs.clear();
            }
            pending_inputs.push_back(input);
        }

        while let Some(input) = pending_inputs.pop_front() {
            if !input.text.trim().is_empty() {
                messages.push(Message {
                    role: "user".to_string(),
                    content: vec![ContentBlock::Text {
                        text: input.text,
                        cache_control: None,
                    }],
                });
            }
        }

        let request = MessageRequest {
            model: runtime.model.clone(),
            messages: messages.clone(),
            max_tokens: 4096,
            system: Some(SystemPrompt::Text(system_prompt.clone())),
            tools: Some(tools.clone()),
            tool_choice: Some(json!({ "type": "auto" })),
            metadata: None,
            thinking: None,
            stream: Some(false),
            temperature: None,
            top_p: None,
        };

        let response = runtime.client.create_message(request).await?;

        let mut tool_uses = Vec::new();
        for block in &response.content {
            match block {
                ContentBlock::Text { text, .. } => {
                    if !text.trim().is_empty() {
                        final_result = Some(text.clone());
                    }
                }
                ContentBlock::ToolUse {
                    id, name, input, ..
                } => {
                    tool_uses.push((id.clone(), name.clone(), input.clone()));
                }
                _ => {}
            }
        }

        messages.push(Message {
            role: "assistant".to_string(),
            content: response.content.clone(),
        });

        if tool_uses.is_empty() {
            while let Ok(input) = input_rx.try_recv() {
                if input.interrupt {
                    pending_inputs.clear();
                }
                pending_inputs.push_back(input);
            }
            if pending_inputs.is_empty() {
                emit_agent_progress(
                    runtime.event_tx.as_ref(),
                    &agent_id,
                    format!("step {steps}/{max_steps}: complete"),
                );
                break;
            }
            continue;
        }

        emit_agent_progress(
            runtime.event_tx.as_ref(),
            &agent_id,
            format!(
                "step {steps}/{max_steps}: executing {} tool call(s)",
                tool_uses.len()
            ),
        );
        let mut tool_results: Vec<ContentBlock> = Vec::new();
        for (tool_id, tool_name, tool_input) in tool_uses {
            emit_agent_progress(
                runtime.event_tx.as_ref(),
                &agent_id,
                format!("step {steps}/{max_steps}: running tool '{tool_name}'"),
            );
            let result = match tokio::time::timeout(TOOL_TIMEOUT, async {
                tool_registry
                    .execute(&agent_id, &tool_name, tool_input)
                    .await
            })
            .await
            {
                Ok(Ok(output)) => output,
                Ok(Err(e)) => format!("Error: {e}"),
                Err(_) => format!("Error: Tool {tool_name} timed out"),
            };
            emit_agent_progress(
                runtime.event_tx.as_ref(),
                &agent_id,
                format!("step {steps}/{max_steps}: finished tool '{tool_name}'"),
            );

            tool_results.push(ContentBlock::ToolResult {
                tool_use_id: tool_id,
                content: result,
                is_error: None,
                content_blocks: None,
            });
        }

        if !tool_results.is_empty() {
            messages.push(Message {
                role: "user".to_string(),
                content: tool_results,
            });
        }
    }

    Ok(SubAgentResult {
        agent_id,
        agent_type,
        assignment,
        status: SubAgentStatus::Completed,
        result: final_result,
        steps_taken: steps,
        duration_ms: u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

async fn wait_for_result(
    manager: &SharedSubAgentManager,
    agent_id: &str,
    timeout: Duration,
) -> Result<(SubAgentResult, bool), ToolError> {
    let deadline = Instant::now() + timeout;

    loop {
        let snapshot = {
            let manager = manager.lock().await;
            manager
                .get_result(agent_id)
                .map_err(|e| ToolError::execution_failed(e.to_string()))?
        };

        if snapshot.status != SubAgentStatus::Running {
            return Ok((snapshot, false));
        }
        if Instant::now() >= deadline {
            return Ok((snapshot, true));
        }

        tokio::time::sleep(RESULT_POLL_INTERVAL).await;
    }
}

async fn wait_for_agents(
    manager: &SharedSubAgentManager,
    ids: &[String],
    wait_mode: WaitMode,
    timeout: Duration,
) -> Result<(Vec<SubAgentResult>, bool), ToolError> {
    let deadline = Instant::now() + timeout;

    loop {
        let snapshots = {
            let manager = manager.lock().await;
            ids.iter()
                .map(|id| {
                    manager
                        .get_result(id)
                        .map_err(|e| ToolError::execution_failed(e.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?
        };

        if wait_mode.condition_met(&snapshots) {
            return Ok((snapshots, false));
        }
        if Instant::now() >= deadline {
            return Ok((snapshots, true));
        }

        tokio::time::sleep(RESULT_POLL_INTERVAL).await;
    }
}

fn parse_wait_mode(input: &Value) -> Result<WaitMode, ToolError> {
    let raw_mode = input
        .get("wait_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("any");
    WaitMode::from_str(raw_mode).ok_or_else(|| {
        ToolError::invalid_input(format!("Invalid wait_mode '{raw_mode}'. Use: any or all"))
    })
}

fn parse_wait_ids(input: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    for key in ["ids", "agent_ids"] {
        if let Some(list) = input.get(key).and_then(|v| v.as_array()) {
            for value in list {
                if let Some(id) = value.as_str() {
                    let id = id.trim();
                    if !id.is_empty() && !ids.iter().any(|existing| existing == id) {
                        ids.push(id.to_string());
                    }
                }
            }
        }
    }

    for key in ["agent_id", "id"] {
        if let Some(id) = input.get(key).and_then(|v| v.as_str()) {
            let id = id.trim();
            if !id.is_empty() && !ids.iter().any(|existing| existing == id) {
                ids.push(id.to_string());
            }
        }
    }

    ids
}

fn optional_input_str<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| input.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn parse_text_or_items(
    input: &Value,
    text_keys: &[&str],
    items_key: &str,
    required_field: &str,
) -> Result<String, ToolError> {
    let text = optional_input_str(input, text_keys).map(str::to_string);
    let items = parse_items_text(input, items_key)?;
    match (text, items) {
        (Some(_), Some(_)) => Err(ToolError::invalid_input(format!(
            "Provide either {required_field} text or {items_key}, but not both"
        ))),
        (Some(text), None) => Ok(text),
        (None, Some(items)) => Ok(items),
        (None, None) => Err(ToolError::missing_field(required_field)),
    }
}

fn parse_optional_text_or_items(
    input: &Value,
    text_keys: &[&str],
    items_key: &str,
) -> Result<Option<String>, ToolError> {
    let text = optional_input_str(input, text_keys).map(str::to_string);
    let items = parse_items_text(input, items_key)?;
    match (text, items) {
        (Some(_), Some(_)) => Err(ToolError::invalid_input(format!(
            "Provide either {} text or {}, but not both",
            text_keys[0], items_key
        ))),
        (Some(text), None) => Ok(Some(text)),
        (None, Some(items)) => Ok(Some(items)),
        (None, None) => Ok(None),
    }
}

fn parse_items_text(input: &Value, key: &str) -> Result<Option<String>, ToolError> {
    let Some(items) = input.get(key) else {
        return Ok(None);
    };
    let array = items
        .as_array()
        .ok_or_else(|| ToolError::invalid_input(format!("'{key}' must be an array")))?;
    if array.is_empty() {
        return Err(ToolError::invalid_input(format!("'{key}' cannot be empty")));
    }

    let mut lines = Vec::new();
    for item in array {
        let object = item
            .as_object()
            .ok_or_else(|| ToolError::invalid_input("each item must be an object"))?;
        let item_type = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("text")
            .trim();
        let rendered = match item_type {
            "text" => object
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .ok_or_else(|| ToolError::invalid_input("text item requires non-empty text"))?,
            "mention" => {
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("mention item requires name"))?;
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("mention item requires path"))?;
                format!("[mention:${name}]({path})")
            }
            "skill" => {
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("skill item requires name"))?;
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("skill item requires path"))?;
                format!("[skill:${name}]({path})")
            }
            "local_image" => {
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("local_image item requires path"))?;
                format!("[local_image:{path}]")
            }
            "image" => {
                let url = object
                    .get("image_url")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("image item requires image_url"))?;
                format!("[image:{url}]")
            }
            _ => object
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "[input]".to_string()),
        };
        lines.push(rendered);
    }

    Ok(Some(lines.join("\n")))
}

fn parse_spawn_request(input: &Value) -> Result<SpawnRequest, ToolError> {
    let prompt = parse_text_or_items(
        input,
        &["prompt", "message", "objective"],
        "items",
        "prompt",
    )?;

    let type_input = optional_input_str(input, &["type", "agent_type", "agent_name"]);
    let role_input = optional_input_str(input, &["role", "agent_role"]);

    let parsed_type = type_input
        .map(|kind| {
            SubAgentType::from_str(kind).ok_or_else(|| {
                ToolError::invalid_input(format!(
                    "Invalid sub-agent type '{kind}'. Use: {VALID_SUBAGENT_TYPES}"
                ))
            })
        })
        .transpose()?;

    let parsed_role_type = role_input
        .map(|role| {
            SubAgentType::from_str(role).ok_or_else(|| {
                ToolError::invalid_input(format!(
                    "Invalid role alias '{role}'. Use: worker, explorer, awaiter, default"
                ))
            })
        })
        .transpose()?;

    if let (Some(type_kind), Some(role_kind)) = (&parsed_type, &parsed_role_type)
        && type_kind != role_kind
    {
        return Err(ToolError::invalid_input(
            "Conflicting type/agent_type and role/agent_role values".to_string(),
        ));
    }

    let agent_type = parsed_type
        .or(parsed_role_type)
        .unwrap_or(SubAgentType::General);

    if let Some(role) = role_input
        && normalize_role_alias(role).is_none()
    {
        return Err(ToolError::invalid_input(format!(
            "Invalid role alias '{role}'. Use: worker, explorer, awaiter, default"
        )));
    }

    let role = role_input
        .and_then(normalize_role_alias)
        .or_else(|| type_input.and_then(normalize_role_alias))
        .map(str::to_string);

    let allowed_tools = input
        .get("allowed_tools")
        .and_then(|v| v.as_array())
        .map(|items| {
            let mut tools = Vec::new();
            for item in items {
                if let Some(tool) = item.as_str() {
                    let trimmed = tool.trim();
                    if !trimmed.is_empty() && !tools.iter().any(|existing| existing == trimmed) {
                        tools.push(trimmed.to_string());
                    }
                }
            }
            tools
        });

    Ok(SpawnRequest {
        prompt: prompt.clone(),
        agent_type,
        assignment: SubAgentAssignment::new(prompt, role),
        allowed_tools,
    })
}

fn parse_assign_request(input: &Value) -> Result<AssignRequest, ToolError> {
    let agent_id = input
        .get("agent_id")
        .or_else(|| input.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ToolError::missing_field("agent_id"))?
        .to_string();
    let objective = optional_input_str(input, &["objective"]).map(str::to_string);
    let role = optional_input_str(input, &["role", "agent_role"])
        .map(|role| {
            normalize_role_alias(role).ok_or_else(|| {
                ToolError::invalid_input(format!(
                    "Invalid role alias '{role}'. Use: worker, explorer, awaiter, default"
                ))
            })
        })
        .transpose()?
        .map(str::to_string);
    let message = parse_optional_text_or_items(input, &["message", "input"], "items")?;
    let interrupt = optional_bool(input, "interrupt", true);

    if objective.is_none() && role.is_none() && message.is_none() {
        return Err(ToolError::invalid_input(
            "Provide at least one of objective, role/agent_role, message/input, or items"
                .to_string(),
        ));
    }

    Ok(AssignRequest {
        agent_id,
        objective,
        role,
        message,
        interrupt,
    })
}

fn parse_csv_concurrency(input: &Value) -> u64 {
    if input.get("max_concurrency").is_some() {
        return optional_u64(input, "max_concurrency", DEFAULT_CSV_MAX_CONCURRENCY).max(1);
    }
    if input.get("max_workers").is_some() {
        return optional_u64(input, "max_workers", DEFAULT_CSV_MAX_CONCURRENCY).max(1);
    }
    DEFAULT_CSV_MAX_CONCURRENCY
}

fn agent_job_reports_store() -> &'static StdMutex<HashMap<String, HashMap<String, AgentJobReport>>>
{
    AGENT_JOB_REPORTS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn agent_job_assignments_store() -> &'static StdMutex<HashMap<String, HashMap<String, String>>> {
    AGENT_JOB_ASSIGNMENTS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn record_agent_job_assignment(job_id: &str, item_id: &str, agent_id: &str) {
    let mut store = agent_job_assignments_store()
        .lock()
        .expect("agent job assignments lock poisoned");
    let job = store.entry(job_id.to_string()).or_default();
    job.insert(item_id.to_string(), agent_id.to_string());
}

fn remove_agent_job_assignment(job_id: &str, item_id: &str) {
    let mut store = agent_job_assignments_store()
        .lock()
        .expect("agent job assignments lock poisoned");
    if let Some(job) = store.get_mut(job_id) {
        job.remove(item_id);
        if job.is_empty() {
            store.remove(job_id);
        }
    }
}

fn clear_agent_job_assignments(job_id: &str) {
    let mut store = agent_job_assignments_store()
        .lock()
        .expect("agent job assignments lock poisoned");
    store.remove(job_id);
}

fn report_matches_assignment(
    job_id: &str,
    item_id: &str,
    reporting_agent_id: Option<&str>,
) -> bool {
    let Some(reporting_agent_id) = reporting_agent_id else {
        return false;
    };
    let store = agent_job_assignments_store()
        .lock()
        .expect("agent job assignments lock poisoned");
    store
        .get(job_id)
        .and_then(|job| job.get(item_id))
        .is_some_and(|expected| expected == reporting_agent_id)
}

fn record_agent_job_result(
    job_id: &str,
    item_id: &str,
    result: Value,
    stop: bool,
    reporting_agent_id: Option<&str>,
) -> bool {
    if !report_matches_assignment(job_id, item_id, reporting_agent_id) {
        return false;
    }
    let mut store = agent_job_reports_store()
        .lock()
        .expect("agent job reports lock poisoned");
    let job = store.entry(job_id.to_string()).or_default();
    if job.contains_key(item_id) {
        return false;
    }
    job.insert(item_id.to_string(), AgentJobReport { result, stop });
    true
}

fn take_agent_job_result(job_id: &str, item_id: &str) -> Option<AgentJobReport> {
    let mut store = agent_job_reports_store()
        .lock()
        .expect("agent job reports lock poisoned");
    let result = store.get_mut(job_id).and_then(|job| job.remove(item_id));
    if store
        .get(job_id)
        .is_some_and(|job_results| job_results.is_empty())
    {
        store.remove(job_id);
    }
    remove_agent_job_assignment(job_id, item_id);
    result
}

fn clear_agent_job_results(job_id: &str) {
    let mut store = agent_job_reports_store()
        .lock()
        .expect("agent job reports lock poisoned");
    store.remove(job_id);
    clear_agent_job_assignments(job_id);
}

fn resolve_results_csv_path(
    context: &ToolContext,
    input: &Value,
    csv_path: &Path,
) -> Result<PathBuf, ToolError> {
    if let Some(path) = optional_input_str(input, &["output_csv_path"]) {
        context.resolve_path(path)
    } else {
        Ok(default_results_csv_path(csv_path))
    }
}

fn default_results_csv_path(csv_path: &Path) -> PathBuf {
    let stem = csv_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("results");
    csv_path.with_file_name(format!("{stem}.results.csv"))
}

fn load_csv_rows(csv_path: &Path, id_column: Option<&str>) -> Result<Vec<CsvRowTask>, ToolError> {
    let mut reader = csv::ReaderBuilder::new()
        .from_path(csv_path)
        .map_err(|err| {
            ToolError::execution_failed(format!(
                "Failed to read CSV '{}': {err}",
                csv_path.display()
            ))
        })?;

    let headers = reader
        .headers()
        .map_err(|err| {
            ToolError::execution_failed(format!(
                "Failed to read CSV headers '{}': {err}",
                csv_path.display()
            ))
        })?
        .clone();
    if headers.is_empty() {
        return Err(ToolError::invalid_input(format!(
            "CSV '{}' has no headers",
            csv_path.display()
        )));
    }
    let mut seen_headers = HashSet::new();
    for header in &headers {
        if !seen_headers.insert(header.to_string()) {
            return Err(ToolError::invalid_input(format!(
                "CSV '{}' has duplicate header '{}'",
                csv_path.display(),
                header
            )));
        }
    }

    let id_index = if let Some(column_name) = id_column {
        let trimmed = column_name.trim();
        if trimmed.is_empty() {
            None
        } else {
            let index = headers
                .iter()
                .position(|header| header == trimmed)
                .ok_or_else(|| {
                    ToolError::invalid_input(format!(
                        "CSV '{}' is missing id_column '{trimmed}'",
                        csv_path.display()
                    ))
                })?;
            Some(index)
        }
    } else {
        None
    };

    let mut rows = Vec::new();
    let mut seen_item_ids = HashSet::new();
    for (row_index, row) in reader.records().enumerate() {
        let record = row.map_err(|err| {
            ToolError::execution_failed(format!(
                "Failed to parse CSV row {} in '{}': {err}",
                row_index + 1,
                csv_path.display()
            ))
        })?;
        let mut values = HashMap::new();
        for (idx, header) in headers.iter().enumerate() {
            values.insert(
                header.to_string(),
                record.get(idx).unwrap_or_default().to_string(),
            );
        }
        let base_item_id = id_index
            .and_then(|idx| record.get(idx))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("row-{}", row_index + 1));
        let mut item_id = base_item_id.clone();
        let mut suffix = 2usize;
        while !seen_item_ids.insert(item_id.clone()) {
            item_id = format!("{base_item_id}-{suffix}");
            suffix = suffix.saturating_add(1);
        }

        rows.push(CsvRowTask {
            row_index,
            item_id,
            values,
        });
    }

    Ok(rows)
}

fn render_instruction_template(template: &str, values: &HashMap<String, String>) -> String {
    const OPEN_BRACE_SENTINEL: &str = "__DEEPSEEK_OPEN_BRACE__";
    const CLOSE_BRACE_SENTINEL: &str = "__DEEPSEEK_CLOSE_BRACE__";

    let mut rendered = template
        .replace("{{", OPEN_BRACE_SENTINEL)
        .replace("}}", CLOSE_BRACE_SENTINEL);
    for (key, value) in values {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    rendered
        .replace(OPEN_BRACE_SENTINEL, "{")
        .replace(CLOSE_BRACE_SENTINEL, "}")
}

fn validate_output_schema(schema: &Value, payload: &Value) -> Result<(), String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "Expected JSON object output".to_string())?;
    if let Some(expected_type) = schema.get("type").and_then(Value::as_str)
        && expected_type != "object"
    {
        return Err("output_schema.type must be 'object' when provided".to_string());
    }
    if let Some(required_fields) = schema.get("required").and_then(Value::as_array) {
        for field in required_fields {
            let Some(field_name) = field.as_str() else {
                continue;
            };
            if !object.contains_key(field_name) {
                return Err(format!(
                    "Worker output missing required field '{field_name}'"
                ));
            }
        }
    }
    Ok(())
}

fn write_csv_worker_outcomes(csv_path: &Path, outcomes: &[CsvWorkerOutcome]) -> Result<()> {
    if let Some(parent) = csv_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = csv::WriterBuilder::new().from_path(csv_path)?;
    writer.write_record([
        "item_id",
        "status",
        "agent_id",
        "duration_ms",
        "error",
        "result",
        "result_json",
    ])?;
    for outcome in outcomes {
        let result_json = outcome
            .result_json
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?
            .unwrap_or_default();
        writer.write_record([
            outcome.item_id.clone(),
            outcome.status.clone(),
            outcome.agent_id.clone().unwrap_or_default(),
            outcome.duration_ms.to_string(),
            outcome.error.clone().unwrap_or_default(),
            outcome.result.clone().unwrap_or_default(),
            result_json,
        ])?;
    }
    writer.flush()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_csv_row_agent(
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
    job_id: &str,
    row: CsvRowTask,
    instruction_template: &str,
    timeout: Duration,
    output_schema: Option<Value>,
    stop_requested: Arc<AtomicBool>,
) -> CsvWorkerOutcome {
    let CsvRowTask {
        row_index,
        item_id,
        values,
    } = row;

    if stop_requested.load(Ordering::Relaxed) {
        return CsvWorkerOutcome {
            row_index,
            item_id,
            status: "skipped".to_string(),
            agent_id: None,
            duration_ms: 0,
            error: Some("Skipped because stop=true was reported by another worker".to_string()),
            result: None,
            result_json: None,
        };
    }

    let schema_text = output_schema
        .as_ref()
        .map(serde_json::to_string_pretty)
        .transpose()
        .unwrap_or(None)
        .unwrap_or_else(|| "{}".to_string());
    let rendered_instruction = render_instruction_template(instruction_template, &values);
    let row_json = serde_json::to_string_pretty(&values).unwrap_or_else(|_| "{}".to_string());
    let prompt = format!(
        "You are processing one item for a spawn_agents_on_csv job.\n\
Job ID: {job_id}\n\
Item ID: {item_id}\n\n\
Task instruction:\n\
{rendered_instruction}\n\n\
Input row (JSON):\n\
{row_json}\n\n\
Expected result schema (JSON Schema or {{}}):\n\
{schema_text}\n\n\
You MUST call the `report_agent_job_result` tool exactly once with:\n\
1. `job_id` = \"{job_id}\"\n\
2. `item_id` = \"{item_id}\"\n\
3. `result` = a JSON object for this row.\n\n\
If you need to stop the job early, include `stop` = true in the same tool call.\n\n\
After the tool call succeeds, stop.",
        item_id = item_id.as_str()
    );

    let assignment = SubAgentAssignment::new(
        format!("Process CSV item '{item_id}' for job '{job_id}'"),
        Some("worker".to_string()),
    );
    let spawn_deadline = Instant::now() + timeout.min(Duration::from_secs(60));
    let spawned = loop {
        if stop_requested.load(Ordering::Relaxed) {
            return CsvWorkerOutcome {
                row_index,
                item_id,
                status: "skipped".to_string(),
                agent_id: None,
                duration_ms: 0,
                error: Some("Skipped because stop=true was reported by another worker".to_string()),
                result: None,
                result_json: None,
            };
        }
        let attempt = {
            let mut manager_guard = manager.lock().await;
            manager_guard.spawn_background_with_assignment(
                manager.clone(),
                runtime.clone(),
                SubAgentType::General,
                prompt.clone(),
                assignment.clone(),
                None,
            )
        };

        match attempt {
            Ok(snapshot) => break Ok(snapshot),
            Err(err) => {
                let message = err.to_string();
                if message.contains("Sub-agent limit reached") && Instant::now() < spawn_deadline {
                    tokio::time::sleep(RESULT_POLL_INTERVAL).await;
                    continue;
                }
                break Err(message);
            }
        }
    };

    let spawn_snapshot = match spawned {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return CsvWorkerOutcome {
                row_index,
                item_id,
                status: "failed".to_string(),
                agent_id: None,
                duration_ms: 0,
                error: Some(error),
                result: None,
                result_json: None,
            };
        }
    };

    let agent_id = spawn_snapshot.agent_id.clone();
    record_agent_job_assignment(job_id, item_id.as_str(), &agent_id);
    let deadline = Instant::now() + timeout;
    let final_snapshot = loop {
        let snapshot = {
            let manager = manager.lock().await;
            manager.get_result(&agent_id)
        };
        match snapshot {
            Ok(snapshot) if snapshot.status != SubAgentStatus::Running => break Ok(snapshot),
            Ok(snapshot) => {
                if Instant::now() >= deadline {
                    let cancelled = {
                        let mut manager = manager.lock().await;
                        manager.cancel(&agent_id)
                    };
                    let mut outcome = CsvWorkerOutcome {
                        row_index,
                        item_id,
                        status: "timed_out".to_string(),
                        agent_id: Some(agent_id.clone()),
                        duration_ms: snapshot.duration_ms,
                        error: Some("Worker timed out and was cancelled".to_string()),
                        result: snapshot.result,
                        result_json: None,
                    };
                    if let Ok(cancelled_snapshot) = cancelled {
                        outcome.duration_ms = cancelled_snapshot.duration_ms;
                    }
                    return outcome;
                }
                tokio::time::sleep(RESULT_POLL_INTERVAL).await;
            }
            Err(err) => break Err(err.to_string()),
        }
    };

    let snapshot = match final_snapshot {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return CsvWorkerOutcome {
                row_index,
                item_id,
                status: "failed".to_string(),
                agent_id: Some(agent_id),
                duration_ms: 0,
                error: Some(error),
                result: None,
                result_json: None,
            };
        }
    };

    match snapshot.status {
        SubAgentStatus::Completed => {
            let Some(report) = take_agent_job_result(job_id, item_id.as_str()) else {
                return CsvWorkerOutcome {
                    row_index,
                    item_id,
                    status: "failed".to_string(),
                    agent_id: Some(snapshot.agent_id),
                    duration_ms: snapshot.duration_ms,
                    error: Some(
                        "Worker finished without calling report_agent_job_result".to_string(),
                    ),
                    result: snapshot.result,
                    result_json: None,
                };
            };

            if let Some(schema) = output_schema.as_ref()
                && let Err(error) = validate_output_schema(schema, &report.result)
            {
                return CsvWorkerOutcome {
                    row_index,
                    item_id,
                    status: "failed".to_string(),
                    agent_id: Some(snapshot.agent_id),
                    duration_ms: snapshot.duration_ms,
                    error: Some(error),
                    result: snapshot.result,
                    result_json: Some(report.result),
                };
            }

            if report.stop {
                stop_requested.store(true, Ordering::Relaxed);
            }

            CsvWorkerOutcome {
                row_index,
                item_id,
                status: "completed".to_string(),
                agent_id: Some(snapshot.agent_id),
                duration_ms: snapshot.duration_ms,
                error: None,
                result: snapshot.result,
                result_json: Some(report.result),
            }
        }
        SubAgentStatus::Interrupted(error) => CsvWorkerOutcome {
            row_index,
            item_id,
            status: "interrupted".to_string(),
            agent_id: Some(snapshot.agent_id),
            duration_ms: snapshot.duration_ms,
            error: Some(error),
            result: snapshot.result,
            result_json: None,
        },
        SubAgentStatus::Failed(error) => CsvWorkerOutcome {
            row_index,
            item_id,
            status: "failed".to_string(),
            agent_id: Some(snapshot.agent_id),
            duration_ms: snapshot.duration_ms,
            error: Some(error),
            result: snapshot.result,
            result_json: None,
        },
        SubAgentStatus::Cancelled => CsvWorkerOutcome {
            row_index,
            item_id,
            status: "failed".to_string(),
            agent_id: Some(snapshot.agent_id),
            duration_ms: snapshot.duration_ms,
            error: Some("Worker cancelled".to_string()),
            result: snapshot.result,
            result_json: None,
        },
        SubAgentStatus::Running => CsvWorkerOutcome {
            row_index,
            item_id,
            status: "failed".to_string(),
            agent_id: Some(snapshot.agent_id),
            duration_ms: snapshot.duration_ms,
            error: Some("Worker did not reach terminal status".to_string()),
            result: snapshot.result,
            result_json: None,
        },
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

fn build_assignment_prompt(
    prompt: &str,
    assignment: &SubAgentAssignment,
    agent_type: &SubAgentType,
) -> String {
    let role = assignment.role.as_deref().unwrap_or("default");
    format!(
        "Assignment metadata:\n- objective: {}\n- role: {}\n- resolved_type: {}\n\nTask:\n{}",
        assignment.objective,
        role,
        agent_type.as_str(),
        prompt
    )
}

fn emit_agent_progress(event_tx: Option<&mpsc::Sender<Event>>, agent_id: &str, status: String) {
    if let Some(event_tx) = event_tx {
        let _ = event_tx.try_send(Event::AgentProgress {
            id: agent_id.to_string(),
            status,
        });
    }
}

// === Tool Registry Helpers ===

struct SubAgentToolRegistry {
    allowed_tools: Vec<String>,
    registry: ToolRegistry,
}

impl SubAgentToolRegistry {
    fn new(
        context: ToolContext,
        allowed_tools: Vec<String>,
        allow_shell: bool,
        todo_list: SharedTodoList,
        plan_state: SharedPlanState,
    ) -> Self {
        let mut builder = ToolRegistryBuilder::new()
            .with_file_tools()
            .with_search_tools()
            .with_note_tool()
            .with_patch_tools()
            .with_web_tools()
            .with_parallel_tool()
            .with_todo_tool(todo_list)
            .with_plan_tool(plan_state)
            .with_tool(Arc::new(ReportAgentJobResultTool));

        if allow_shell {
            builder = builder.with_shell_tools();
        }

        let registry = builder.build(context);

        Self {
            allowed_tools,
            registry,
        }
    }

    fn tools_for_model(&self) -> Vec<Tool> {
        self.registry
            .to_api_tools()
            .into_iter()
            .filter(|tool| self.allowed_tools.contains(&tool.name))
            .collect()
    }

    fn unavailable_allowed_tools(&self) -> Vec<String> {
        self.allowed_tools
            .iter()
            .filter(|name| !self.registry.contains(name))
            .cloned()
            .collect()
    }

    async fn execute(&self, agent_id: &str, name: &str, mut input: Value) -> Result<String> {
        if !self.allowed_tools.iter().any(|tool| tool == name) {
            return Err(anyhow!("Tool {name} not allowed for this sub-agent"));
        }
        if name == "report_agent_job_result"
            && let Some(object) = input.as_object_mut()
        {
            object.insert(
                "__reporting_agent_id".to_string(),
                Value::String(agent_id.to_string()),
            );
        }

        self.registry
            .execute(name, input)
            .await
            .map_err(|e| anyhow!(e))
    }
}

fn build_allowed_tools(
    agent_type: &SubAgentType,
    explicit_tools: Option<Vec<String>>,
    allow_shell: bool,
) -> Result<Vec<String>> {
    let mut tools = explicit_tools.unwrap_or_else(|| {
        agent_type
            .allowed_tools()
            .iter()
            .map(|tool| (*tool).to_string())
            .collect()
    });

    if matches!(agent_type, SubAgentType::Custom) && tools.is_empty() {
        return Err(anyhow!(
            "Custom sub-agent requires a non-empty allowed_tools list"
        ));
    }

    if !allow_shell {
        tools.retain(|tool| {
            !matches!(
                tool.as_str(),
                "exec_shell"
                    | "exec_shell_wait"
                    | "exec_shell_interact"
                    | "exec_wait"
                    | "exec_interact"
            )
        });
    }

    let mut deduped = Vec::new();
    for tool in tools {
        let name = tool.trim();
        if !name.is_empty() && !deduped.iter().any(|existing: &String| existing == name) {
            deduped.push(name.to_string());
        }
    }

    Ok(deduped)
}

fn summarize_subagent_result(result: &SubAgentResult) -> String {
    match (&result.status, result.result.as_ref()) {
        (SubAgentStatus::Completed, Some(text)) => truncate_preview(text),
        (SubAgentStatus::Completed, None) => "Completed (no output)".to_string(),
        (SubAgentStatus::Interrupted(error), _) => format!("Interrupted: {error}"),
        (SubAgentStatus::Cancelled, _) => "Cancelled".to_string(),
        (SubAgentStatus::Failed(error), _) => format!("Failed: {error}"),
        (SubAgentStatus::Running, _) => "Running".to_string(),
    }
}

fn subagent_status_name(status: &SubAgentStatus) -> &'static str {
    match status {
        SubAgentStatus::Running => "running",
        SubAgentStatus::Completed => "completed",
        SubAgentStatus::Interrupted(_) => "interrupted",
        SubAgentStatus::Failed(_) => "failed",
        SubAgentStatus::Cancelled => "cancelled",
    }
}

fn truncate_preview(text: &str) -> String {
    const MAX_LEN: usize = 240;
    if text.len() <= MAX_LEN {
        text.to_string()
    } else {
        format!("{}...", text.chars().take(MAX_LEN).collect::<String>())
    }
}

// === System prompts ===

const GENERAL_AGENT_PROMPT: &str = r"You are a sub-agent spawned to handle a specific task autonomously.

Execution contract:
- Use only the tools provided at runtime.
- Do not claim actions you did not execute.
- Keep work scoped to the assigned objective.

Guidelines:
- Work autonomously and avoid asking for user input.
- Be thorough but efficient.
- If blocked, return a clear BLOCKED reason and 1-2 alternatives.
- For successful completion, return concise sections:
  SUMMARY
  EVIDENCE
  CHANGES
  RISKS

Complete the task and provide your final result.
";

const EXPLORE_AGENT_PROMPT: &str = r"You are a fast exploration sub-agent specialized for codebase search.

Execution contract:
- Use only the tools provided at runtime.
- Do not claim actions you did not execute.

Guidelines:
- Focus on finding relevant code quickly
- Use shell commands for efficient searching
- Read only files that seem relevant
- Summarize your findings concisely
- Return file paths and key snippets as evidence

Complete the exploration and provide your findings.
";

const PLAN_AGENT_PROMPT: &str = r"You are a planning sub-agent specialized for architectural analysis.

Execution contract:
- Use only the tools provided at runtime.
- Do not claim actions you did not execute.

Guidelines:
- Analyze the codebase structure
- Identify key components and patterns
- Consider trade-offs and alternatives
- Provide clear recommendations
- Document your analysis

Complete the analysis and provide your plan.
";

const REVIEW_AGENT_PROMPT: &str = r"You are a code review sub-agent.

Execution contract:
- Use only the tools provided at runtime.
- Do not claim actions you did not execute.

Guidelines:
- Focus on code quality and correctness
- Check for bugs, security issues, and best practices
- Note any concerns or suggestions
- Be constructive in your feedback
- Prioritize issues by severity

Complete the review and provide your feedback.
";

const CUSTOM_AGENT_PROMPT: &str = r"You are a custom sub-agent with specific tool access.

Use only the tools provided at runtime. Do not claim actions not executed.
If blocked, return BLOCKED with cause and alternatives.
Otherwise return concise sections: SUMMARY, EVIDENCE, CHANGES, RISKS.

Complete the task and provide your final result.
";

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_assignment() -> SubAgentAssignment {
        SubAgentAssignment::new("prompt".to_string(), Some("worker".to_string()))
    }

    fn make_snapshot(status: SubAgentStatus) -> SubAgentResult {
        SubAgentResult {
            agent_id: "agent_test".to_string(),
            agent_type: SubAgentType::General,
            assignment: make_assignment(),
            status,
            result: None,
            steps_taken: 0,
            duration_ms: 0,
        }
    }

    #[test]
    fn test_agent_type_from_str() {
        assert_eq!(
            SubAgentType::from_str("general"),
            Some(SubAgentType::General)
        );
        assert_eq!(
            SubAgentType::from_str("explore"),
            Some(SubAgentType::Explore)
        );
        assert_eq!(SubAgentType::from_str("PLAN"), Some(SubAgentType::Plan));
        assert_eq!(
            SubAgentType::from_str("code-review"),
            Some(SubAgentType::Review)
        );
        assert_eq!(
            SubAgentType::from_str("worker"),
            Some(SubAgentType::General)
        );
        assert_eq!(
            SubAgentType::from_str("default"),
            Some(SubAgentType::General)
        );
        assert_eq!(
            SubAgentType::from_str("explorer"),
            Some(SubAgentType::Explore)
        );
        assert_eq!(SubAgentType::from_str("awaiter"), Some(SubAgentType::Plan));
        assert_eq!(SubAgentType::from_str("invalid"), None);
    }

    #[test]
    fn test_parse_spawn_request_accepts_message_and_agent_type_aliases() {
        let input = json!({
            "message": "Find references to Foo",
            "agent_type": "explorer"
        });
        let parsed = parse_spawn_request(&input).expect("spawn request should parse");
        assert_eq!(parsed.prompt, "Find references to Foo");
        assert_eq!(parsed.agent_type, SubAgentType::Explore);
        assert_eq!(parsed.assignment.role.as_deref(), Some("explorer"));
    }

    #[test]
    fn test_parse_spawn_request_accepts_objective_and_role_alias() {
        let input = json!({
            "objective": "Coordinate and wait",
            "role": "awaiter"
        });
        let parsed = parse_spawn_request(&input).expect("spawn request should parse");
        assert_eq!(parsed.prompt, "Coordinate and wait");
        assert_eq!(parsed.agent_type, SubAgentType::Plan);
        assert_eq!(parsed.assignment.role.as_deref(), Some("awaiter"));
    }

    #[test]
    fn test_parse_spawn_request_accepts_items_payload() {
        let input = json!({
            "items": [
                {"type": "text", "text": "Analyze module"},
                {"type": "mention", "name": "drive", "path": "app://drive"}
            ],
            "agent_name": "explorer"
        });
        let parsed = parse_spawn_request(&input).expect("spawn request should parse");
        assert!(parsed.prompt.contains("Analyze module"));
        assert!(parsed.prompt.contains("[mention:$drive](app://drive)"));
        assert_eq!(parsed.agent_type, SubAgentType::Explore);
    }

    #[test]
    fn test_parse_spawn_request_rejects_text_and_items_together() {
        let input = json!({
            "prompt": "Analyze module",
            "items": [{"type": "text", "text": "dup"}]
        });
        let err = parse_spawn_request(&input).expect_err("text+items should fail");
        assert!(err.to_string().contains("either prompt text or items"));
    }

    #[test]
    fn test_parse_spawn_request_rejects_invalid_role() {
        let input = json!({
            "prompt": "do work",
            "role": "unknown_role"
        });
        let err = parse_spawn_request(&input).expect_err("invalid role should fail");
        assert!(err.to_string().contains("Invalid role alias"));
    }

    #[test]
    fn test_parse_spawn_request_rejects_conflicting_type_and_role() {
        let input = json!({
            "prompt": "inspect internals",
            "type": "explore",
            "role": "worker"
        });
        let err = parse_spawn_request(&input).expect_err("conflicting type+role should fail");
        assert!(
            err.to_string()
                .contains("Conflicting type/agent_type and role/agent_role")
        );
    }

    #[test]
    fn test_parse_assign_request_accepts_aliases() {
        let input = json!({
            "id": "agent_1234",
            "objective": "re-check failing tests",
            "agent_role": "explorer",
            "input": "focus on tests only",
            "interrupt": false
        });
        let request = parse_assign_request(&input).expect("assign request should parse");
        assert_eq!(request.agent_id, "agent_1234");
        assert_eq!(request.objective.as_deref(), Some("re-check failing tests"));
        assert_eq!(request.role.as_deref(), Some("explorer"));
        assert_eq!(request.message.as_deref(), Some("focus on tests only"));
        assert!(!request.interrupt);
    }

    #[test]
    fn test_parse_assign_request_rejects_invalid_role() {
        let input = json!({
            "agent_id": "agent_1234",
            "role": "unknown"
        });
        let err = parse_assign_request(&input).expect_err("invalid role should fail");
        assert!(err.to_string().contains("Invalid role alias"));
    }

    #[test]
    fn test_parse_assign_request_requires_update_fields() {
        let input = json!({
            "agent_id": "agent_1234"
        });
        let err = parse_assign_request(&input).expect_err("missing update fields should fail");
        assert!(err.to_string().contains(
            "Provide at least one of objective, role/agent_role, message/input, or items"
        ));
    }

    #[test]
    fn test_render_instruction_template_replaces_columns() {
        let mut values = HashMap::new();
        values.insert("name".to_string(), "alpha".to_string());
        values.insert("owner".to_string(), "hunter".to_string());

        let rendered = render_instruction_template("Inspect {name} for {owner}", &values);
        assert_eq!(rendered, "Inspect alpha for hunter");
    }

    #[test]
    fn test_render_instruction_template_preserves_escaped_braces() {
        let mut values = HashMap::new();
        values.insert("name".to_string(), "alpha".to_string());

        let rendered = render_instruction_template("literal {{x}} and {name}", &values);
        assert_eq!(rendered, "literal {x} and alpha");
    }

    #[test]
    fn test_record_agent_job_result_accepts_first_report_only() {
        let job_id = "job_test_reports";
        clear_agent_job_results(job_id);
        record_agent_job_assignment(job_id, "item-1", "agent_1");

        assert!(record_agent_job_result(
            job_id,
            "item-1",
            json!({"status":"ok"}),
            false,
            Some("agent_1")
        ));
        assert!(!record_agent_job_result(
            job_id,
            "item-1",
            json!({"status":"duplicate"}),
            true,
            Some("agent_1")
        ));

        let report = take_agent_job_result(job_id, "item-1").expect("report should exist");
        assert_eq!(report.result["status"], "ok");
        assert!(!report.stop);
        assert!(take_agent_job_result(job_id, "item-1").is_none());
        clear_agent_job_results(job_id);
    }

    #[test]
    fn test_record_agent_job_result_rejects_wrong_agent_assignment() {
        let job_id = "job_test_reports_wrong_agent";
        clear_agent_job_results(job_id);
        record_agent_job_assignment(job_id, "item-1", "agent_good");

        assert!(!record_agent_job_result(
            job_id,
            "item-1",
            json!({"status":"bad"}),
            false,
            Some("agent_bad")
        ));
        assert!(take_agent_job_result(job_id, "item-1").is_none());
        clear_agent_job_results(job_id);
    }

    #[test]
    fn test_record_agent_job_result_rejects_missing_agent_assignment_context() {
        let job_id = "job_test_reports_missing_agent_context";
        clear_agent_job_results(job_id);
        record_agent_job_assignment(job_id, "item-1", "agent_good");

        assert!(!record_agent_job_result(
            job_id,
            "item-1",
            json!({"status":"bad"}),
            false,
            None
        ));
        assert!(take_agent_job_result(job_id, "item-1").is_none());
        clear_agent_job_results(job_id);
    }

    #[test]
    fn test_validate_output_schema_enforces_required_fields() {
        let schema = json!({
            "type": "object",
            "required": ["status", "score"]
        });
        let ok_payload = json!({"status":"ok","score":1});
        assert!(validate_output_schema(&schema, &ok_payload).is_ok());

        let missing = json!({"status":"ok"});
        let err = validate_output_schema(&schema, &missing).expect_err("missing required field");
        assert!(err.contains("missing required field 'score'"));
    }

    #[test]
    fn test_default_results_csv_path_uses_input_stem() {
        let path = PathBuf::from("/tmp/inventory.csv");
        let output = default_results_csv_path(&path);
        assert_eq!(output, PathBuf::from("/tmp/inventory.results.csv"));
    }

    #[test]
    fn test_parse_csv_concurrency_prefers_max_concurrency() {
        let input = json!({
            "max_workers": 3,
            "max_concurrency": 9
        });
        assert_eq!(parse_csv_concurrency(&input), 9);
    }

    #[test]
    fn test_load_csv_rows_uses_id_column_and_row_fallback() {
        let tmp = tempdir().expect("tempdir");
        let csv_path = tmp.path().join("rows.csv");
        std::fs::write(&csv_path, "id,name\nalpha,First\n,Second\n").expect("write csv");

        let rows = load_csv_rows(&csv_path, Some("id")).expect("load rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].item_id, "alpha");
        assert_eq!(rows[1].item_id, "row-2");
        assert_eq!(
            rows[1].values.get("name").map(String::as_str),
            Some("Second")
        );
    }

    #[test]
    fn test_load_csv_rows_dedupes_item_ids() {
        let tmp = tempdir().expect("tempdir");
        let csv_path = tmp.path().join("rows.csv");
        std::fs::write(&csv_path, "id,name\nfoo,First\nfoo,Second\n").expect("write csv");

        let rows = load_csv_rows(&csv_path, Some("id")).expect("load rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].item_id, "foo");
        assert_eq!(rows[1].item_id, "foo-2");
    }

    #[test]
    fn test_load_csv_rows_rejects_duplicate_headers() {
        let tmp = tempdir().expect("tempdir");
        let csv_path = tmp.path().join("rows.csv");
        std::fs::write(&csv_path, "id,id\nfoo,bar\n").expect("write csv");

        let err = load_csv_rows(&csv_path, Some("id")).expect_err("duplicate headers should fail");
        assert!(err.to_string().contains("duplicate header"));
    }

    #[test]
    fn test_send_input_schema_does_not_require_message_field() {
        let manager = Arc::new(Mutex::new(SubAgentManager::new(PathBuf::from("."), 1)));
        let schema = AgentSendInputTool::new(manager, "send_input").input_schema();
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(
            !required
                .iter()
                .any(|entry| entry.as_str().is_some_and(|name| name == "message")),
            "send_input schema should allow items-only payloads"
        );
    }

    #[test]
    fn test_allowed_tools_shell_filter() {
        let tools = build_allowed_tools(&SubAgentType::General, None, false).unwrap();
        assert!(!tools.contains(&"exec_shell".to_string()));
        assert!(!tools.contains(&"exec_shell_wait".to_string()));
        assert!(!tools.contains(&"exec_shell_interact".to_string()));
        assert!(!tools.contains(&"exec_wait".to_string()));
        assert!(!tools.contains(&"exec_interact".to_string()));
    }

    #[test]
    fn test_allowed_tools_are_deduplicated() {
        let tools = build_allowed_tools(
            &SubAgentType::Custom,
            Some(vec![
                "read_file".to_string(),
                "read_file".to_string(),
                "  ".to_string(),
                "grep_files".to_string(),
            ]),
            true,
        )
        .unwrap();
        assert_eq!(
            tools,
            vec!["read_file".to_string(), "grep_files".to_string()]
        );
    }

    #[test]
    fn test_custom_agent_requires_allowed_tools() {
        let err = build_allowed_tools(&SubAgentType::Custom, None, true).unwrap_err();
        assert!(err.to_string().contains("requires"));
    }

    #[test]
    fn test_wait_mode_condition_any_and_all() {
        let one_done = vec![
            make_snapshot(SubAgentStatus::Running),
            make_snapshot(SubAgentStatus::Completed),
        ];
        let all_done = vec![
            make_snapshot(SubAgentStatus::Completed),
            make_snapshot(SubAgentStatus::Cancelled),
        ];

        assert!(WaitMode::Any.condition_met(&one_done));
        assert!(!WaitMode::All.condition_met(&one_done));
        assert!(WaitMode::All.condition_met(&all_done));
    }

    #[test]
    fn test_parse_wait_mode() {
        assert_eq!(parse_wait_mode(&json!({})).unwrap(), WaitMode::Any);
        assert_eq!(
            parse_wait_mode(&json!({"wait_mode": "all"})).unwrap(),
            WaitMode::All
        );
        assert_eq!(
            parse_wait_mode(&json!({"wait_mode": "first"})).unwrap(),
            WaitMode::Any
        );
        assert!(parse_wait_mode(&json!({"wait_mode": "invalid"})).is_err());
    }

    #[test]
    fn test_parse_wait_ids_accepts_aliases() {
        let ids = parse_wait_ids(&json!({
            "ids": ["agent_a", "agent_b"],
            "agent_id": "agent_c",
            "id": "agent_a"
        }));

        assert_eq!(ids, vec!["agent_a", "agent_b", "agent_c"]);
    }

    #[test]
    fn test_parse_wait_ids_empty_when_omitted() {
        let ids = parse_wait_ids(&json!({}));
        assert!(ids.is_empty());
    }

    #[test]
    fn test_build_assignment_prompt_includes_metadata() {
        let assignment = SubAgentAssignment::new(
            "Inspect parser behavior".to_string(),
            Some("explorer".to_string()),
        );
        let prompt = build_assignment_prompt(
            "Inspect parser behavior",
            &assignment,
            &SubAgentType::Explore,
        );
        assert!(prompt.contains("Assignment metadata"));
        assert!(prompt.contains("resolved_type: explore"));
        assert!(prompt.contains("role: explorer"));
    }

    #[test]
    fn test_subagent_tool_registry_reports_unavailable_tools() {
        let tmp = tempdir().expect("tempdir");
        let context = ToolContext::new(tmp.path().to_path_buf());
        let registry = SubAgentToolRegistry::new(
            context,
            vec!["read_file".to_string(), "missing_tool".to_string()],
            false,
            Arc::new(Mutex::new(TodoList::new())),
            Arc::new(Mutex::new(PlanState::default())),
        );
        assert_eq!(
            registry.unavailable_allowed_tools(),
            vec!["missing_tool".to_string()]
        );
    }

    #[tokio::test]
    async fn test_wait_for_result_reports_timeout_when_still_running() {
        let manager = Arc::new(Mutex::new(SubAgentManager::new(PathBuf::from("."), 2)));
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let agent = SubAgent::new(
            SubAgentType::Explore,
            "prompt".to_string(),
            make_assignment(),
            vec!["read_file".to_string()],
            input_tx,
        );
        let agent_id = agent.id.clone();
        {
            let mut guard = manager.lock().await;
            guard.agents.insert(agent_id.clone(), agent);
        }

        let (snapshot, timed_out) = wait_for_result(&manager, &agent_id, Duration::from_millis(10))
            .await
            .expect("wait_for_result should succeed");
        assert!(timed_out);
        assert_eq!(snapshot.status, SubAgentStatus::Running);
    }

    #[test]
    fn test_running_count_respects_limit() {
        let mut manager = SubAgentManager::new(PathBuf::from("."), 1);
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let mut agent = SubAgent::new(
            SubAgentType::Explore,
            "prompt".to_string(),
            make_assignment(),
            vec!["read_file".to_string()],
            input_tx,
        );
        agent.status = SubAgentStatus::Running;
        manager.agents.insert(agent.id.clone(), agent);

        assert_eq!(manager.running_count(), 1);
    }

    #[tokio::test]
    async fn test_running_count_ignores_finished_task_handles() {
        let mut manager = SubAgentManager::new(PathBuf::from("."), 1);
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let mut agent = SubAgent::new(
            SubAgentType::Explore,
            "prompt".to_string(),
            make_assignment(),
            vec!["read_file".to_string()],
            input_tx,
        );
        agent.status = SubAgentStatus::Running;
        let handle = tokio::spawn(async {});
        handle.await.expect("dummy task should finish immediately");
        agent.task_handle = Some(tokio::spawn(async {}));
        if let Some(handle) = agent.task_handle.as_ref() {
            while !handle.is_finished() {
                tokio::task::yield_now().await;
            }
        }
        manager.agents.insert(agent.id.clone(), agent);

        assert_eq!(manager.running_count(), 0);
    }

    #[test]
    fn test_assign_updates_running_agent_and_sends_message() {
        let mut manager = SubAgentManager::new(PathBuf::from("."), 2);
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let agent = SubAgent::new(
            SubAgentType::General,
            "work".to_string(),
            make_assignment(),
            vec!["read_file".to_string()],
            input_tx,
        );
        let agent_id = agent.id.clone();
        manager.agents.insert(agent_id.clone(), agent);

        let snapshot = manager
            .assign(
                &agent_id,
                Some("Re-check module boundaries".to_string()),
                Some("explorer".to_string()),
                None,
                true,
            )
            .expect("assignment should succeed");
        assert_eq!(snapshot.assignment.objective, "Re-check module boundaries");
        assert_eq!(snapshot.assignment.role.as_deref(), Some("explorer"));

        let dispatched = input_rx
            .try_recv()
            .expect("running agent should receive assignment update");
        assert!(dispatched.interrupt);
        assert!(dispatched.text.contains("Assignment updated"));
        assert!(dispatched.text.contains("objective"));
    }

    #[test]
    fn test_assign_rejects_message_for_non_running_agent() {
        let mut manager = SubAgentManager::new(PathBuf::from("."), 1);
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let mut agent = SubAgent::new(
            SubAgentType::Explore,
            "prompt".to_string(),
            make_assignment(),
            vec!["read_file".to_string()],
            input_tx,
        );
        agent.status = SubAgentStatus::Completed;
        let agent_id = agent.id.clone();
        manager.agents.insert(agent_id.clone(), agent);

        let err = manager
            .assign(&agent_id, None, None, Some("keep going".to_string()), true)
            .expect_err("non-running agent cannot receive assignment message");
        assert!(err.to_string().contains("is not running"));
    }

    #[test]
    fn test_assign_updates_non_running_metadata_without_message() {
        let mut manager = SubAgentManager::new(PathBuf::from("."), 1);
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let mut agent = SubAgent::new(
            SubAgentType::Plan,
            "prompt".to_string(),
            make_assignment(),
            vec!["read_file".to_string()],
            input_tx,
        );
        agent.status = SubAgentStatus::Completed;
        let agent_id = agent.id.clone();
        manager.agents.insert(agent_id.clone(), agent);

        let snapshot = manager
            .assign(
                &agent_id,
                Some("Draft retry plan".to_string()),
                Some("awaiter".to_string()),
                None,
                true,
            )
            .expect("metadata update should succeed");
        assert_eq!(snapshot.assignment.objective, "Draft retry plan");
        assert_eq!(snapshot.assignment.role.as_deref(), Some("awaiter"));
    }

    #[test]
    fn test_persist_and_reload_marks_running_agent_as_interrupted() {
        let tmp = tempdir().expect("tempdir");
        let workspace = tmp.path().to_path_buf();
        let state_path = default_state_path(tmp.path());

        let mut manager = SubAgentManager::new(workspace.clone(), 2).with_state_path(state_path);
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let running = SubAgent::new(
            SubAgentType::General,
            "work".to_string(),
            make_assignment(),
            vec!["read_file".to_string()],
            input_tx,
        );
        let running_id = running.id.clone();
        manager.agents.insert(running_id.clone(), running);
        manager.persist_state().expect("persist state");

        let mut reloaded =
            SubAgentManager::new(workspace, 2).with_state_path(default_state_path(tmp.path()));
        reloaded.load_state().expect("load state");
        let snapshot = reloaded
            .get_result(&running_id)
            .expect("reloaded agent should exist");
        assert!(matches!(
            snapshot.status,
            SubAgentStatus::Interrupted(ref message)
                if message.contains(SUBAGENT_RESTART_REASON)
        ));
    }

    #[test]
    fn test_interrupted_status_name_and_summary() {
        let snapshot = make_snapshot(SubAgentStatus::Interrupted(
            SUBAGENT_RESTART_REASON.to_string(),
        ));
        assert_eq!(subagent_status_name(&snapshot.status), "interrupted");
        assert!(summarize_subagent_result(&snapshot).contains(SUBAGENT_RESTART_REASON));
    }
}
