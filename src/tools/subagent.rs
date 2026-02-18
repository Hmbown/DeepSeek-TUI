//! Sub-agent spawning system.
//!
//! Provides tools to spawn background sub-agents, query their status,
//! and retrieve results. Sub-agents run with a filtered toolset and
//! inherit the workspace configuration from the main session.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
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

// === Types ===

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
            "general" | "general-purpose" | "general_purpose" => Some(Self::General),
            "explore" | "exploration" => Some(Self::Explore),
            "plan" | "planning" => Some(Self::Plan),
            "review" | "code-review" | "code_review" => Some(Self::Review),
            "custom" => Some(Self::Custom),
            _ => None,
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
                "weather",
                "finance",
                "sports",
                "time",
                "calculator",
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
            ],
            Self::Explore => vec![
                "list_dir",
                "read_file",
                "grep_files",
                "file_search",
                "web.run",
                "web_search",
                "multi_tool_use.parallel",
                "weather",
                "finance",
                "sports",
                "time",
                "calculator",
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
                "weather",
                "finance",
                "sports",
                "time",
                "calculator",
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
    Failed(String),
    Cancelled,
}

/// Snapshot of sub-agent state for tool results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    pub agent_id: String,
    pub agent_type: SubAgentType,
    pub status: SubAgentStatus,
    pub result: Option<String>,
    pub steps_taken: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
struct SubAgentInput {
    text: String,
    interrupt: bool,
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
        allowed_tools: Vec<String>,
        input_tx: mpsc::UnboundedSender<SubAgentInput>,
    ) -> Self {
        let id = format!("agent_{}", &Uuid::new_v4().to_string()[..8]);

        Self {
            id,
            agent_type,
            prompt,
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
    workspace: PathBuf,
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
            max_steps: DEFAULT_MAX_STEPS,
            max_agents,
        }
    }

    /// Count running agents.
    pub fn running_count(&self) -> usize {
        self.agents
            .values()
            .filter(|agent| agent.status == SubAgentStatus::Running)
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
        if self.running_count() >= self.max_agents {
            return Err(anyhow!(
                "Sub-agent limit reached (max {}). Cancel or wait for an existing agent to finish.",
                self.max_agents
            ));
        }

        let tools = build_allowed_tools(&agent_type, allowed_tools, runtime.allow_shell)?;
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let mut agent = SubAgent::new(agent_type.clone(), prompt.clone(), tools.clone(), input_tx);
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
            allowed_tools: tools,
            started_at,
            max_steps,
            input_rx,
        };
        let handle = tokio::spawn(run_subagent_task(task));
        agent.task_handle = Some(handle);
        self.agents.insert(agent_id.clone(), agent);

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
        let agent = self
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;

        if agent.status == SubAgentStatus::Running {
            agent.status = SubAgentStatus::Cancelled;
            if let Some(handle) = agent.task_handle.take() {
                handle.abort();
            }
        }

        Ok(agent.snapshot())
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

    /// List all agents and their status.
    #[must_use]
    pub fn list(&self) -> Vec<SubAgentResult> {
        self.agents.values().map(SubAgent::snapshot).collect()
    }

    /// Clean up completed agents older than the given duration.
    pub fn cleanup(&mut self, max_age: Duration) {
        self.agents.retain(|_, agent| {
            if agent.status == SubAgentStatus::Running {
                true
            } else {
                agent.started_at.elapsed() < max_age
            }
        });
    }

    fn update_from_result(&mut self, agent_id: &str, result: SubAgentResult) {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.status = result.status;
            agent.result = result.result;
            agent.steps_taken = result.steps_taken;
            agent.task_handle = None;
        }
    }

    fn update_failed(&mut self, agent_id: &str, error: String) {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.status = SubAgentStatus::Failed(error);
            agent.task_handle = None;
        }
    }
}

/// Thread-safe wrapper for `SubAgentManager`.
pub type SharedSubAgentManager = Arc<Mutex<SubAgentManager>>;

/// Create a shared sub-agent manager with a configurable limit.
#[must_use]
pub fn new_shared_subagent_manager(workspace: PathBuf, max_agents: usize) -> SharedSubAgentManager {
    let max_agents = max_agents.clamp(1, MAX_SUBAGENTS);
    Arc::new(Mutex::new(SubAgentManager::new(workspace, max_agents)))
}

// === Tool Implementations ===

/// Tool to spawn a background sub-agent.
pub struct AgentSpawnTool {
    manager: SharedSubAgentManager,
    runtime: SubAgentRuntime,
}

impl AgentSpawnTool {
    /// Create a new spawn tool.
    #[must_use]
    pub fn new(manager: SharedSubAgentManager, runtime: SubAgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[async_trait]
impl ToolSpec for AgentSpawnTool {
    fn name(&self) -> &'static str {
        "agent_spawn"
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
                "type": {
                    "type": "string",
                    "description": "Sub-agent type: general, explore, plan, review, custom"
                },
                "allowed_tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Explicit tool allowlist (required for custom type)"
                }
            },
            "required": ["prompt"]
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
        let prompt = required_str(&input, "prompt")?.to_string();
        let agent_type = if let Some(kind) = input.get("type").and_then(|v| v.as_str()) {
            SubAgentType::from_str(kind).ok_or_else(|| {
                ToolError::invalid_input(format!(
                    "Invalid sub-agent type '{kind}'. Use: general, explore, plan, review, custom"
                ))
            })?
        } else {
            SubAgentType::General
        };

        let allowed_tools = input
            .get("allowed_tools")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            });

        let mut manager = self.manager.lock().await;

        let result = manager
            .spawn_background(
                Arc::clone(&self.manager),
                self.runtime.clone(),
                agent_type,
                prompt,
                allowed_tools,
            )
            .map_err(|e| ToolError::execution_failed(format!("Failed to spawn sub-agent: {e}")))?;

        let mut tool_result =
            ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        if result.status == SubAgentStatus::Running {
            tool_result.metadata = Some(json!({ "status": "Running" }));
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
                "block": {
                    "type": "boolean",
                    "description": "Wait for completion (default: false)"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Max wait time in milliseconds (default: 30000)"
                }
            },
            "required": ["agent_id"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let agent_id = required_str(&input, "agent_id")?;
        let block = optional_bool(&input, "block", false);
        let timeout_ms = optional_u64(&input, "timeout_ms", 30_000).clamp(1000, 300_000);

        let result = if block {
            wait_for_result(&self.manager, agent_id, Duration::from_millis(timeout_ms)).await?
        } else {
            let manager = self.manager.lock().await;
            manager
                .get_result(agent_id)
                .map_err(|e| ToolError::execution_failed(e.to_string()))?
        };

        let mut tool_result =
            ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        if result.status == SubAgentStatus::Running {
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
        let manager = self.manager.lock().await;
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
                "interrupt": {
                    "type": "boolean",
                    "description": "Prioritize this message over pending inputs"
                }
            },
            "required": ["message"]
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
        let message = input
            .get("message")
            .or_else(|| input.get("input"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::missing_field("message"))?;
        let interrupt = optional_bool(&input, "interrupt", false);

        let mut manager = self.manager.lock().await;
        manager
            .send_input(agent_id, message.to_string(), interrupt)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;
        let snapshot = manager
            .get_result(agent_id)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        ToolResult::json(&snapshot).map_err(|e| ToolError::execution_failed(e.to_string()))
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
                    "description": "Agent IDs to wait on"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Single agent ID"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Max wait time in milliseconds (default: 30000)"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let timeout_ms = optional_u64(&input, "timeout_ms", 30_000).clamp(1000, 300_000);
        let mut ids: Vec<String> = Vec::new();
        if let Some(list) = input.get("ids").and_then(|v| v.as_array()) {
            ids.extend(
                list.iter()
                    .filter_map(|item| item.as_str().map(str::to_string)),
            );
        }
        if ids.is_empty() {
            if let Some(id) = input.get("agent_id").and_then(|v| v.as_str()) {
                ids.push(id.to_string());
            }
        }
        if ids.is_empty() {
            return Err(ToolError::missing_field("ids"));
        }

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let snapshots = {
                let manager = self.manager.lock().await;
                ids.iter()
                    .map(|id| {
                        manager
                            .get_result(id)
                            .map_err(|e| ToolError::execution_failed(e.to_string()))
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };

            let any_done = snapshots
                .iter()
                .any(|snapshot| snapshot.status != SubAgentStatus::Running);
            if any_done || Instant::now() >= deadline {
                return ToolResult::json(&snapshots)
                    .map_err(|e| ToolError::execution_failed(e.to_string()));
            }

            tokio::time::sleep(RESULT_POLL_INTERVAL).await;
        }
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
                    "description": "Name or type of the agent: general, explore, plan, review"
                },
                "objective": {
                    "type": "string",
                    "description": "The goal or task description for the agent"
                }
            },
            "required": ["objective"]
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
        let objective = required_str(&input, "objective")?.to_string();
        let agent_type_str = input
            .get("agent_name")
            .and_then(|v| v.as_str())
            .unwrap_or("general");

        let spawn_input = json!({
            "prompt": objective,
            "type": agent_type_str
        });

        let spawn_tool = AgentSpawnTool::new(self.manager.clone(), self.runtime.clone());
        spawn_tool.execute(spawn_input, context).await
    }
}

// === Sub-agent Execution ===

struct SubAgentTask {
    manager_handle: SharedSubAgentManager,
    runtime: SubAgentRuntime,
    agent_id: String,
    agent_type: SubAgentType,
    prompt: String,
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
    let tools = tool_registry.tools_for_model();

    let mut messages = vec![Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: prompt,
            cache_control: None,
        }],
    }];

    let mut steps = 0;
    let mut final_result: Option<String> = None;
    let mut pending_inputs: VecDeque<SubAgentInput> = VecDeque::new();

    for _step in 0..max_steps {
        steps += 1;

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
                break;
            }
            continue;
        }

        let mut tool_results: Vec<ContentBlock> = Vec::new();
        for (tool_id, tool_name, tool_input) in tool_uses {
            let result = match tokio::time::timeout(TOOL_TIMEOUT, async {
                tool_registry.execute(&tool_name, tool_input).await
            })
            .await
            {
                Ok(Ok(output)) => output,
                Ok(Err(e)) => format!("Error: {e}"),
                Err(_) => format!("Error: Tool {tool_name} timed out"),
            };

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
) -> Result<SubAgentResult, ToolError> {
    let deadline = Instant::now() + timeout;

    loop {
        let snapshot = {
            let manager = manager.lock().await;
            manager
                .get_result(agent_id)
                .map_err(|e| ToolError::execution_failed(e.to_string()))?
        };

        if snapshot.status != SubAgentStatus::Running || Instant::now() >= deadline {
            return Ok(snapshot);
        }

        tokio::time::sleep(RESULT_POLL_INTERVAL).await;
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
            .with_todo_tool(todo_list)
            .with_plan_tool(plan_state);

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

    async fn execute(&self, name: &str, input: Value) -> Result<String> {
        if !self.allowed_tools.iter().any(|tool| tool == name) {
            return Err(anyhow!("Tool {name} not allowed for this sub-agent"));
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

    Ok(tools)
}

fn summarize_subagent_result(result: &SubAgentResult) -> String {
    match (&result.status, result.result.as_ref()) {
        (SubAgentStatus::Completed, Some(text)) => truncate_preview(text),
        (SubAgentStatus::Completed, None) => "Completed (no output)".to_string(),
        (SubAgentStatus::Cancelled, _) => "Cancelled".to_string(),
        (SubAgentStatus::Failed(error), _) => format!("Failed: {error}"),
        (SubAgentStatus::Running, _) => "Running".to_string(),
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

Your capabilities:
- Full file system access (read, write, edit)
- Shell command execution
- Note taking and todo management

Guidelines:
- Focus solely on the assigned task
- Be thorough but efficient
- Return a clear, concise summary of your findings/actions
- If you encounter errors, try alternative approaches
- Do not ask for user input - work autonomously

Complete the task and provide your final result.
";

const EXPLORE_AGENT_PROMPT: &str = r"You are a fast exploration sub-agent specialized for codebase search.

Your capabilities:
- Read files and directories
- Execute shell commands (grep, find, etc.)

Guidelines:
- Focus on finding relevant code quickly
- Use shell commands for efficient searching
- Read only files that seem relevant
- Summarize your findings concisely
- Return file paths and key code snippets

Complete the exploration and provide your findings.
";

const PLAN_AGENT_PROMPT: &str = r"You are a planning sub-agent specialized for architectural analysis.

Your capabilities:
- Read files and directories
- Take notes
- Update plans

Guidelines:
- Analyze the codebase structure
- Identify key components and patterns
- Consider trade-offs and alternatives
- Provide clear recommendations
- Document your analysis

Complete the analysis and provide your plan.
";

const REVIEW_AGENT_PROMPT: &str = r"You are a code review sub-agent.

Your capabilities:
- Read files and directories
- Take notes

Guidelines:
- Focus on code quality and correctness
- Check for bugs, security issues, and best practices
- Note any concerns or suggestions
- Be constructive in your feedback
- Prioritize issues by severity

Complete the review and provide your feedback.
";

const CUSTOM_AGENT_PROMPT: &str = r"You are a custom sub-agent with specific tool access.

Work autonomously to complete the assigned task using only the tools available to you.

Complete the task and provide your final result.
";

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(SubAgentType::from_str("invalid"), None);
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
    fn test_custom_agent_requires_allowed_tools() {
        let err = build_allowed_tools(&SubAgentType::Custom, None, true).unwrap_err();
        assert!(err.to_string().contains("requires"));
    }

    #[test]
    fn test_running_count_respects_limit() {
        let mut manager = SubAgentManager::new(PathBuf::from("."), 1);
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let mut agent = SubAgent::new(
            SubAgentType::Explore,
            "prompt".to_string(),
            vec!["read_file".to_string()],
            input_tx,
        );
        agent.status = SubAgentStatus::Running;
        manager.agents.insert(agent.id.clone(), agent);

        assert_eq!(manager.running_count(), 1);
    }
}
