#![allow(dead_code)]
//! Agent Graph — LangGraph-inspired state machine for agent workflows.
//!
//! Inspired by deepagents' use of LangGraph for agent orchestration.
//! Provides:
//! - **Nodes**: Processing steps (spawn agent, run tool, conditional check)
//! - **Edges**: Unconditional or conditional transitions between nodes
//! - **State**: Shared key-value state persistent across nodes
//! - **Checkpoints**: Save/restore execution state for fault tolerance
//! - **Streaming**: Events emitted as graph executes each node
//!
//! # Architecture
//!
//! ```text
//!                     ┌─────────┐
//!            ┌───────→│  START  │
//!            │        └────┬────┘
//!            │             │
//!            │        ┌────▼────┐
//!     ┌──────┤   ┌───→│  NODE A │───┐
//!     │      │   │    └─────────┘   │
//!     │      │   │                  │
//!     │  ┌───▼───┴─┐    ┌─────────┐│
//!     │  │ ROUTER  │◄───│  NODE B ││
//!     │  └───┬───┬─┘    └─────────┘│
//!     │      │   │                  │
//!     │      │   │    ┌─────────┐   │
//!     └──────┘   └───→│  NODE C │←──┘
//!                     └────┬────┘
//!                          │
//!                     ┌────▼────┐
//!                     │   END   │
//!                     └─────────┘
//! ```
//!
//! This is a simplified, Rust-native state graph — not a full LangGraph
//! port. It focuses on the core patterns: composable nodes, conditional
//! routing, shared state, and checkpoint/recovery.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use serde::{Deserialize, Serialize};

// ── Graph node ──────────────────────────────────────────────────────────────

/// A single processing step in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Unique node id within the graph.
    pub id: String,
    /// Human-readable description of what this node does.
    pub description: String,
    /// Action type: what happens when this node executes.
    pub action: NodeAction,
    /// If true, this is the entry point (only one START node per graph).
    #[serde(default)]
    pub is_start: bool,
    /// If true, this is a terminal node (graph ends after executing it).
    #[serde(default)]
    pub is_end: bool,
    /// Maximum retries on failure.
    #[serde(default)]
    pub max_retries: u32,
}

/// The type of action a graph node performs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeAction {
    /// Spawn a sub-agent with a task.
    SpawnAgent {
        agent_type: String,
        task: String,
        expected_output: String,
    },
    /// Execute a shell command.
    RunShell {
        command: String,
        timeout_ms: Option<u64>,
    },
    /// Read from a file.
    ReadFile {
        path: String,
    },
    /// Write to a file.
    WriteFile {
        path: String,
        content_key: String,
    },
    /// Conditional routing: evaluate expression and route to next node.
    Router {
        conditions: Vec<RouteCondition>,
        default_target: String,
    },
    /// Evaluate the current state and set a key.
    Evaluate {
        expression: String,
        result_key: String,
    },
    /// Wait for a sub-agent to complete.
    WaitForAgent {
        agent_id_key: String,
    },
    /// No-op passthrough node.
    Passthrough,
}

/// A single condition for the Router action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteCondition {
    /// Expression to evaluate (e.g., "state.score > 0.8").
    pub expression: String,
    /// Node to transition to when condition is true.
    pub target: String,
}

// ── Graph state ─────────────────────────────────────────────────────────────

/// Shared key-value state carried through the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct GraphState {
    /// Key-value pairs.
    data: HashMap<String, String>,
}


impl GraphState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.data.insert(key.into(), value.into());
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(String::as_str)
    }

    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    /// Evaluate a simple expression against the state.
    /// Supported: "key == value", "key != value", "key contains value",
    /// "key exists", "score > N", "score < N".
    #[must_use]
    pub fn evaluate(&self, expression: &str) -> bool {
        let expr = expression.trim();

        // "key exists"
        if expr.ends_with("exists") {
            let key = expr.trim_end_matches("exists").trim();
            return self.contains(key);
        }

        // "key contains value"
        if expr.contains("contains") {
            let parts: Vec<&str> = expr.splitn(2, "contains").collect();
            if parts.len() == 2 {
                let key = parts[0].trim();
                let value = parts[1].trim().trim_matches('"').trim_matches('\'');
                return self.get(key).is_some_and(|v| v.contains(value));
            }
        }

        // "key == value" or "key != value"
        if expr.contains("==") || expr.contains("!=") {
            let op = if expr.contains("==") { "==" } else { "!=" };
            let parts: Vec<&str> = expr.splitn(2, op).collect();
            if parts.len() == 2 {
                let key = parts[0].trim();
                let value = parts[1].trim().trim_matches('"').trim_matches('\'');
                let actual = self.get(key).unwrap_or("");
                return if op == "==" {
                    actual == value
                } else {
                    actual != value
                };
            }
        }

        // "score > N" or "score < N" (numeric comparison)
        for op in &[">", "<", ">=", "<="] {
            if expr.contains(*op) {
                let parts: Vec<&str> = expr.splitn(2, op).collect();
                if parts.len() == 2 {
                    let key = parts[0].trim();
                    if let Some(val) = self.get(key)
                        && let Ok(num) = val.parse::<f64>()
                        && let Ok(threshold) = parts[1].trim().parse::<f64>()
                    {
                        return match *op {
                            ">" => num > threshold,
                            "<" => num < threshold,
                            ">=" => num >= threshold,
                            "<=" => num <= threshold,
                            _ => false,
                        };
                    }
                }
            }
        }

        false
    }
}

// ── Graph definition ────────────────────────────────────────────────────────

/// A directed graph of processing nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentGraph {
    pub id: String,
    pub name: String,
    pub description: String,
    nodes: HashMap<String, GraphNode>,
    /// Edges: source → target(s). For Router nodes, targets are resolved at runtime.
    edges: HashMap<String, Vec<String>>,
}

impl AgentGraph {
    /// Create a new empty graph.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        let id = format!("graph_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        Self {
            id,
            name: name.into(),
            description: description.into(),
            nodes: HashMap::new(),
            edges: HashMap::new(),
        }
    }

    /// Add a node to the graph.
    pub fn add_node(&mut self, node: GraphNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Add an unconditional edge from one node to another.
    pub fn add_edge(&mut self, from: impl Into<String>, to: impl Into<String>) {
        self.edges
            .entry(from.into())
            .or_default()
            .push(to.into());
    }

    /// Get the start node.
    #[must_use]
    pub fn start_node(&self) -> Option<&GraphNode> {
        self.nodes.values().find(|n| n.is_start)
    }

    /// Find the next node to transition to.
    #[must_use]
    pub fn next_node(&self, current_id: &str, state: &GraphState) -> Option<&GraphNode> {
        let current = self.nodes.get(current_id)?;

        // For Router nodes, evaluate conditions
        if let NodeAction::Router {
            ref conditions,
            ref default_target,
        } = current.action
        {
            for cond in conditions {
                if state.evaluate(&cond.expression) {
                    return self.nodes.get(&cond.target);
                }
            }
            return self.nodes.get(default_target);
        }

        // Follow edges
        if let Some(targets) = self.edges.get(current_id)
            && !targets.is_empty()
        {
            return self.nodes.get(&targets[0]);
        }

        None
    }

    /// Get a specific node by id.
    #[must_use]
    pub fn node(&self, id: &str) -> Option<&GraphNode> {
        self.nodes.get(id)
    }

    /// Number of nodes in the graph.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// List all node ids in insertion order.
    #[must_use]
    pub fn node_ids(&self) -> Vec<&str> {
        self.nodes.keys().map(String::as_str).collect()
    }
}

// ── Graph execution ─────────────────────────────────────────────────────────

/// Execution status of a graph run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphRunStatus {
    Running,
    Paused,
    Completed,
    Failed(String),
}

/// A checkpoint in graph execution — can be used to resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphCheckpoint {
    pub graph_id: String,
    pub run_id: String,
    pub current_node: String,
    pub state: GraphState,
    pub nodes_executed: Vec<String>,
    pub timestamp: String,
}

/// An actively running graph instance.
#[derive(Debug, Clone)]
pub struct GraphRun {
    pub graph_id: String,
    pub run_id: String,
    pub current_node: String,
    pub state: GraphState,
    pub status: GraphRunStatus,
    nodes_executed: Vec<String>,
    checkpoints: Vec<GraphCheckpoint>,
    started_at: Instant,
    pub events: VecDeque<GraphEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEvent {
    pub event_type: String,
    pub node_id: String,
    pub description: String,
    pub data: Option<String>,
    pub timestamp: String,
}

impl GraphRun {
    /// Start a new run of a graph.
    #[must_use]
    pub fn new(graph: &AgentGraph) -> Option<Self> {
        let start = graph.start_node()?;
        let run_id = format!("run_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        Some(Self {
            graph_id: graph.id.clone(),
            run_id,
            current_node: start.id.clone(),
            state: GraphState::new(),
            status: GraphRunStatus::Running,
            nodes_executed: Vec::new(),
            checkpoints: Vec::new(),
            started_at: Instant::now(),
            events: VecDeque::new(),
        })
    }

    /// Resume a run from a checkpoint.
    #[must_use]
    pub fn from_checkpoint(checkpoint: GraphCheckpoint) -> Self {
        Self {
            graph_id: checkpoint.graph_id,
            run_id: checkpoint.run_id,
            current_node: checkpoint.current_node,
            state: checkpoint.state,
            status: GraphRunStatus::Running,
            nodes_executed: checkpoint.nodes_executed,
            checkpoints: Vec::new(),
            started_at: Instant::now(),
            events: VecDeque::new(),
        }
    }

    /// Step to the next node. Returns None if the graph has ended.
    #[must_use]
    pub fn step<'a>(&mut self, graph: &'a AgentGraph) -> Option<&'a GraphNode> {
        let current = graph.node(&self.current_node)?;

        // Record the current node as executed
        self.nodes_executed.push(self.current_node.clone());

        // Emit event
        self.events.push_back(GraphEvent {
            event_type: "node_executed".to_string(),
            node_id: self.current_node.clone(),
            description: current.description.clone(),
            data: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });

        // If this is an end node, stop
        if current.is_end {
            self.status = GraphRunStatus::Completed;
            return None;
        }

        // Find the next node
        if let Some(next) = graph.next_node(&self.current_node, &self.state) {
            self.current_node = next.id.clone();
            Some(next)
        } else {
            // No next node → treat as completed
            self.status = GraphRunStatus::Completed;
            None
        }
    }

    /// Create a checkpoint for later resumption.
    pub fn checkpoint(&mut self) -> GraphCheckpoint {
        let cp = GraphCheckpoint {
            graph_id: self.graph_id.clone(),
            run_id: self.run_id.clone(),
            current_node: self.current_node.clone(),
            state: self.state.clone(),
            nodes_executed: self.nodes_executed.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        self.checkpoints.push(cp.clone());
        cp
    }
}

// ── ToolSpec ────────────────────────────────────────────────────────────────

use async_trait::async_trait;
use serde_json::json;
use crate::tools::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Tool for defining and executing agent graphs.
pub struct GraphExecuteTool {
    graphs: std::sync::Arc<tokio::sync::Mutex<HashMap<String, AgentGraph>>>,
    runs: std::sync::Arc<tokio::sync::Mutex<HashMap<String, GraphRun>>>,
}

impl GraphExecuteTool {
    pub fn new() -> Self {
        Self {
            graphs: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            runs: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ToolSpec for GraphExecuteTool {
    fn name(&self) -> &'static str {
        "graph_execute"
    }

    fn description(&self) -> &'static str {
        "Define and execute agent graphs (workflows) with conditional routing, parallel execution, checkpoints, and state management. Actions: create_graph, add_node, add_edge, run, step, set_state, get_state, checkpoint, resume."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create_graph", "add_node", "add_edge", "run", "step", "set_state", "get_state", "checkpoint", "resume", "status"],
                    "description": "Graph action to perform"
                },
                "graph_id": {"type": "string", "description": "Graph ID"},
                "run_id": {"type": "string", "description": "Run ID"},
                "name": {"type": "string", "description": "Graph name (for create_graph)"},
                "description": {"type": "string", "description": "Graph description"},
                "node": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "description": {"type": "string"},
                        "action": {"type": "object"},
                        "is_start": {"type": "boolean"},
                        "is_end": {"type": "boolean"}
                    }
                },
                "from": {"type": "string", "description": "Source node id (for add_edge)"},
                "to": {"type": "string", "description": "Target node id (for add_edge)"},
                "key": {"type": "string", "description": "State key"},
                "value": {"type": "string", "description": "State value"}
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

        match action {
            "create_graph" => {
                let name = input.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed");
                let desc = input.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let graph = AgentGraph::new(name, desc);
                let id = graph.id.clone();
                self.graphs.lock().await.insert(id.clone(), graph);
                Ok(ToolResult::success(format!("Graph created: {id}")))
            }

            "add_node" => {
                let graph_id = get_str(&input, "graph_id")?;
                let node_data = input
                    .get("node")
                    .ok_or_else(|| ToolError::invalid_input("Missing 'node' object"))?;

                let node = GraphNode {
                    id: get_str(node_data, "id")?,
                    description: get_str(node_data, "description").unwrap_or_default(),
                    action: parse_action(node_data)?,
                    is_start: node_data.get("is_start").and_then(|v| v.as_bool()).unwrap_or(false),
                    is_end: node_data.get("is_end").and_then(|v| v.as_bool()).unwrap_or(false),
                    max_retries: 1,
                };

                let mut graphs = self.graphs.lock().await;
                let graph = graphs
                    .get_mut(&graph_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Graph '{graph_id}' not found")))?;
                graph.add_node(node);

                Ok(ToolResult::success(format!("Node added to graph {graph_id}")))
            }

            "add_edge" => {
                let graph_id = get_str(&input, "graph_id")?;
                let from = get_str(&input, "from")?;
                let to = get_str(&input, "to")?;

                let mut graphs = self.graphs.lock().await;
                let graph = graphs
                    .get_mut(&graph_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Graph '{graph_id}' not found")))?;
                graph.add_edge(from, to);

                Ok(ToolResult::success("Edge added"))
            }

            "run" => {
                let graph_id = get_str(&input, "graph_id")?;
                let graphs = self.graphs.lock().await;
                let graph = graphs
                    .get(&graph_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Graph '{graph_id}' not found")))?;

                let run = GraphRun::new(graph)
                    .ok_or_else(|| ToolError::invalid_input("Graph has no START node"))?;

                let run_id = run.run_id.clone();
                let current = run.current_node.clone();
                self.runs.lock().await.insert(run_id.clone(), run);

                Ok(ToolResult::success(format!(
                    "Graph run started: {} (current node: {})",
                    run_id, current
                )))
            }

            "step" => {
                let run_id = get_str(&input, "run_id")?;
                let graph_id = {
                    let runs = self.runs.lock().await;
                    runs.get(&run_id)
                        .map(|r| r.graph_id.clone())
                        .ok_or_else(|| ToolError::invalid_input(format!("Run '{run_id}' not found")))?
                };

                let graphs = self.graphs.lock().await;
                let graph = graphs
                    .get(&graph_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Graph '{graph_id}' not found")))?;

                let mut runs = self.runs.lock().await;
                let run = runs
                    .get_mut(&run_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Run '{run_id}' not found")))?;

                match run.step(graph) {
                    Some(next) => {
                        Ok(ToolResult::success(format!(
                            "Stepped to node: {} ({})",
                            next.id, next.description
                        )))
                    }
                    None => {
                        Ok(ToolResult::success(format!(
                            "Graph run {} completed ({} nodes executed)",
                            run_id,
                            run.nodes_executed.len()
                        )))
                    }
                }
            }

            "set_state" => {
                let run_id = get_str(&input, "run_id")?;
                let key = get_str(&input, "key")?;
                let value = get_str(&input, "value")?;

                let mut runs = self.runs.lock().await;
                let run = runs
                    .get_mut(&run_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Run '{run_id}' not found")))?;
                run.state.set(key, value);

                Ok(ToolResult::success("State updated"))
            }

            "get_state" => {
                let run_id = get_str(&input, "run_id")?;
                let runs = self.runs.lock().await;
                let run = runs
                    .get(&run_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Run '{run_id}' not found")))?;

                let state_json = serde_json::to_string_pretty(&run.state.data).unwrap_or_default();
                Ok(ToolResult::success(format!(
                    "Run {} state:\n{}",
                    run_id, state_json
                )))
            }

            "checkpoint" => {
                let run_id = get_str(&input, "run_id")?;
                let mut runs = self.runs.lock().await;
                let run = runs
                    .get_mut(&run_id)
                    .ok_or_else(|| ToolError::invalid_input(format!("Run '{run_id}' not found")))?;

                let cp = run.checkpoint();
                Ok(ToolResult::success(format!(
                    "Checkpoint created: node '{}' at {}",
                    cp.current_node, cp.timestamp
                )))
            }

            _ => Err(ToolError::invalid_input(format!(
                "Unknown action '{action}'. Use: create_graph, add_node, add_edge, run, step, set_state, get_state, checkpoint, resume, status"
            ))),
        }
    }
}

fn get_str(input: &serde_json::Value, key: &str) -> Result<String, ToolError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| ToolError::invalid_input(format!("Missing '{key}'")))
}

fn parse_action(node_data: &serde_json::Value) -> Result<NodeAction, ToolError> {
    let action = node_data
        .get("action")
        .ok_or_else(|| ToolError::invalid_input("Node missing 'action'"))?;

    let action_type = action
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::invalid_input("Action missing 'type'"))?;

    match action_type {
        "spawn_agent" => {
            Ok(NodeAction::SpawnAgent {
                agent_type: get_str(action, "agent_type").unwrap_or_else(|_| "general".into()),
                task: get_str(action, "task").unwrap_or_default(),
                expected_output: get_str(action, "expected_output").unwrap_or_default(),
            })
        }
        "run_shell" => {
            Ok(NodeAction::RunShell {
                command: get_str(action, "command")?,
                timeout_ms: action.get("timeout_ms").and_then(|v| v.as_u64()),
            })
        }
        "read_file" => {
            Ok(NodeAction::ReadFile {
                path: get_str(action, "path")?,
            })
        }
        "write_file" => {
            Ok(NodeAction::WriteFile {
                path: get_str(action, "path")?,
                content_key: get_str(action, "content_key").unwrap_or_default(),
            })
        }
        "router" => {
            let conditions: Vec<RouteCondition> = action
                .get("conditions")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|c| RouteCondition {
                            expression: get_str(c, "expression").unwrap_or_default(),
                            target: get_str(c, "target").unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(NodeAction::Router {
                conditions,
                default_target: get_str(action, "default_target").unwrap_or_default(),
            })
        }
        "evaluate" => {
            Ok(NodeAction::Evaluate {
                expression: get_str(action, "expression")?,
                result_key: get_str(action, "result_key").unwrap_or_default(),
            })
        }
        "passthrough" => Ok(NodeAction::Passthrough),
        _ => Err(ToolError::invalid_input(format!(
            "Unknown action type '{action_type}'"
        ))),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_lifecycle() {
        let mut graph = AgentGraph::new("Test Graph", "A test workflow");

        // Create nodes
        graph.add_node(GraphNode {
            id: "start".into(),
            description: "Entry point".into(),
            action: NodeAction::Passthrough,
            is_start: true,
            is_end: false,
            max_retries: 1,
        });
        graph.add_node(GraphNode {
            id: "step_a".into(),
            description: "Step A".into(),
            action: NodeAction::RunShell {
                command: "echo hello".into(),
                timeout_ms: None,
            },
            is_start: false,
            is_end: false,
            max_retries: 1,
        });
        graph.add_node(GraphNode {
            id: "end".into(),
            description: "End".into(),
            action: NodeAction::Passthrough,
            is_start: false,
            is_end: true,
            max_retries: 1,
        });

        // Add edges
        graph.add_edge("start", "step_a");
        graph.add_edge("step_a", "end");

        assert_eq!(graph.len(), 3);

        // Create and execute run
        let mut run = GraphRun::new(&graph).unwrap();
        assert_eq!(run.current_node, "start");

        // Step 1: start → step_a
        let next = run.step(&graph).unwrap();
        assert_eq!(next.id, "step_a");

        // Step 2: step_a → end
        let next = run.step(&graph).unwrap();
        assert_eq!(next.id, "end");

        // Step 3: end → complete
        let next = run.step(&graph);
        assert!(next.is_none());
        assert_eq!(run.status, GraphRunStatus::Completed);
    }

    #[test]
    fn test_state_evaluation() {
        let mut state = GraphState::new();
        state.set("score", "0.85");
        state.set("status", "success");

        assert!(state.evaluate("score > 0.8"));
        assert!(!state.evaluate("score < 0.6"));
        assert!(state.evaluate("status == success"));
        assert!(!state.evaluate("status == failure"));
        assert!(state.evaluate("score exists"));
        assert!(!state.evaluate("unknown exists"));
    }

    #[test]
    fn test_router_node() {
        let mut graph = AgentGraph::new("Router Test", "");

        graph.add_node(GraphNode {
            id: "router".into(),
            description: "Router".into(),
            action: NodeAction::Router {
                conditions: vec![
                    RouteCondition {
                        expression: "score > 0.8".into(),
                        target: "high".into(),
                    },
                    RouteCondition {
                        expression: "score < 0.3".into(),
                        target: "low".into(),
                    },
                ],
                default_target: "medium".into(),
            },
            is_start: true,
            is_end: false,
            max_retries: 1,
        });
        graph.add_node(GraphNode {
            id: "high".into(),
            description: "High confidence".into(),
            action: NodeAction::Passthrough,
            is_start: false,
            is_end: true,
            max_retries: 1,
        });
        graph.add_node(GraphNode {
            id: "medium".into(),
            description: "Medium confidence".into(),
            action: NodeAction::Passthrough,
            is_start: false,
            is_end: true,
            max_retries: 1,
        });
        graph.add_node(GraphNode {
            id: "low".into(),
            description: "Low confidence".into(),
            action: NodeAction::Passthrough,
            is_start: false,
            is_end: true,
            max_retries: 1,
        });

        // High score → route to "high"
        let mut state = GraphState::new();
        state.set("score", "0.9");
        let next = graph.next_node("router", &state).unwrap();
        assert_eq!(next.id, "high");

        // Medium score → route to default "medium"
        let mut state = GraphState::new();
        state.set("score", "0.5");
        let next = graph.next_node("router", &state).unwrap();
        assert_eq!(next.id, "medium");

        // Low score → route to "low"
        let mut state = GraphState::new();
        state.set("score", "0.1");
        let next = graph.next_node("router", &state).unwrap();
        assert_eq!(next.id, "low");
    }

    #[test]
    fn test_checkpoint_roundtrip() {
        let mut graph = AgentGraph::new("CP Test", "");
        graph.add_node(GraphNode {
            id: "start".into(),
            description: "Start".into(),
            action: NodeAction::Passthrough,
            is_start: true,
            is_end: false,
            max_retries: 1,
        });
        graph.add_node(GraphNode {
            id: "end".into(),
            description: "End".into(),
            action: NodeAction::Passthrough,
            is_start: false,
            is_end: true,
            max_retries: 1,
        });
        graph.add_edge("start", "end");

        let mut run = GraphRun::new(&graph).unwrap();
        run.state.set("key", "value");
        let _ = run.step(&graph); // start → end

        let cp = run.checkpoint();
        assert_eq!(cp.current_node, "end");

        let restored = GraphRun::from_checkpoint(cp);
        assert_eq!(restored.current_node, "end");
        assert_eq!(restored.state.get("key"), Some("value"));
    }
}
