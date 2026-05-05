//! Swarm coordination — multi-agent topologies with consensus.
//!
//! Models inspired by Ruflo's agent orchestration layer. Provides
//! topologies (hierarchical, mesh, ring, star, adaptive), consensus
//! algorithms, and swarm lifecycle management.
//!
//! # Architecture
//!
//! ```text
//! SwarmCoordinator
//!   ├── Topology (Hierarchical/Mesh/Ring/Star/Adaptive)
//!   ├── Active agents (roles)
//!   ├── Consensus (Majority/Weighted/Byzantine)
//!   └── Communication (broadcast, point-to-point)
//! ```

use std::collections::HashMap;
use std::time::Instant;
use serde::{Deserialize, Serialize};

// ── Topology ────────────────────────────────────────────────────────────────

/// Swarm topology determining agent organization and communication patterns.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SwarmTopology {
    /// Queen/coordinator leads workers. Best for anti-drift, structured tasks.
    Hierarchical,
    /// Peer-to-peer equal agents. Best for parallel independent work.
    Mesh,
    /// Sequential ring processing. Best for pipeline workflows.
    Ring,
    /// Central coordinator with star-connected workers. Best for broadcasting.
    Star,
    /// Dynamic switching based on task characteristics.
    Adaptive,
}

impl SwarmTopology {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hierarchical => "hierarchical",
            Self::Mesh => "mesh",
            Self::Ring => "ring",
            Self::Star => "star",
            Self::Adaptive => "adaptive",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "hierarchical" | "hierarchy" => Some(Self::Hierarchical),
            "mesh" | "peer-to-peer" | "p2p" => Some(Self::Mesh),
            "ring" | "pipeline" => Some(Self::Ring),
            "star" | "centralized" => Some(Self::Star),
            "adaptive" | "auto" => Some(Self::Adaptive),
            _ => None,
        }
    }
}

// ── Consensus ───────────────────────────────────────────────────────────────

/// Consensus protocol for swarm decision-making.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsensusProtocol {
    /// Simple majority vote.
    Majority,
    /// Weighted voting (coordinator has 3x weight).
    Weighted,
    /// Byzantine fault-tolerant (requires f < n/3).
    Byzantine,
    /// Gossip-style eventual consistency.
    Gossip,
}

impl ConsensusProtocol {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Majority => "majority",
            Self::Weighted => "weighted",
            Self::Byzantine => "byzantine",
            Self::Gossip => "gossip",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "majority" | "simple" => Some(Self::Majority),
            "weighted" => Some(Self::Weighted),
            "byzantine" | "bft" => Some(Self::Byzantine),
            "gossip" => Some(Self::Gossip),
            _ => None,
        }
    }

    /// Minimum agents required for this protocol.
    #[must_use]
    pub fn min_agents(self) -> usize {
        match self {
            Self::Majority => 1,
            Self::Weighted => 2,
            Self::Byzantine => 4, // 3f+1, f>=1
            Self::Gossip => 2,
        }
    }
}

// ── Agent role ──────────────────────────────────────────────────────────────

/// Pre-defined agent roles within a swarm.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SwarmAgentRole {
    Coordinator,
    Coder,
    Tester,
    Reviewer,
    Architect,
    Researcher,
    Security,
    Documenter,
    Optimizer,
    Custom(String),
}

impl SwarmAgentRole {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Coordinator => "coordinator",
            Self::Coder => "coder",
            Self::Tester => "tester",
            Self::Reviewer => "reviewer",
            Self::Architect => "architect",
            Self::Researcher => "researcher",
            Self::Security => "security",
            Self::Documenter => "documenter",
            Self::Optimizer => "optimizer",
            Self::Custom(s) => s.as_str(),
        }
    }
}

// ── Swarm configuration ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmConfig {
    pub topology: SwarmTopology,
    pub max_agents: usize,
    pub consensus: ConsensusProtocol,
    pub objective: String,
    pub strategy: String,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            topology: SwarmTopology::Hierarchical,
            max_agents: 8,
            consensus: ConsensusProtocol::Weighted,
            objective: String::new(),
            strategy: "development".to_string(),
        }
    }
}

// ── Swarm state ─────────────────────────────────────────────────────────────

/// Snapshot of a swarm's current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmSnapshot {
    pub id: String,
    pub objective: String,
    pub topology: String,
    pub max_agents: usize,
    pub active_agents: usize,
    pub consensus: String,
    pub started_at: String,
    pub status: String,
    pub agents: Vec<SwarmAgentSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmAgentSnapshot {
    pub id: String,
    pub role: String,
    pub status: String,
    pub task: Option<String>,
}

// ── Swarm coordinator ───────────────────────────────────────────────────────

/// Manages a single swarm instance — agents, topology, and lifecycle.
#[derive(Debug, Clone)]
pub struct SwarmCoordinator {
    pub config: SwarmConfig,
    id: String,
    agents: Vec<SwarmAgentHandle>,
    started_at: Instant,
    status: SwarmStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SwarmStatus {
    Initialized,
    Running,
    Paused,
    Completed,
    Failed(String),
}

impl SwarmStatus {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Initialized => "initialized",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed(_) => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SwarmAgentHandle {
    pub id: String,
    pub role: SwarmAgentRole,
    pub task: Option<String>,
    pub spawned: bool,
}

impl SwarmCoordinator {
    /// Initialize a new swarm with the given config.
    #[must_use]
    pub fn new(config: SwarmConfig) -> Self {
        let id = format!("swarm_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        Self {
            config,
            id,
            agents: Vec::new(),
            started_at: Instant::now(),
            status: SwarmStatus::Initialized,
        }
    }

    /// Add an agent to the swarm.
    pub fn add_agent(&mut self, role: SwarmAgentRole, task: Option<String>) -> String {
        let id = format!("agent_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        self.agents.push(SwarmAgentHandle {
            id: id.clone(),
            role,
            task,
            spawned: false,
        });
        id
    }

    /// Mark the swarm as running.
    pub fn start(&mut self) {
        self.status = SwarmStatus::Running;
        self.started_at = Instant::now();
    }

    /// Generate a snapshot for display.
    #[must_use]
    pub fn snapshot(&self) -> SwarmSnapshot {
        SwarmSnapshot {
            id: self.id.clone(),
            objective: self.config.objective.clone(),
            topology: self.config.topology.as_str().to_string(),
            max_agents: self.config.max_agents,
            active_agents: self.agents.len(),
            consensus: self.config.consensus.as_str().to_string(),
            started_at: format!("{:?}", self.started_at.elapsed()),
            status: self.status.as_str().to_string(),
            agents: self
                .agents
                .iter()
                .map(|a| SwarmAgentSnapshot {
                    id: a.id.clone(),
                    role: a.role.as_str().to_string(),
                    status: if a.spawned { "active" } else { "pending" }.to_string(),
                    task: a.task.clone(),
                })
                .collect(),
        }
    }
}

// ── ToolSpec ────────────────────────────────────────────────────────────────

use async_trait::async_trait;
use serde_json::json;
use crate::tools::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Tool for initializing and managing agent swarms.
pub struct SwarmInitTool {
    swarms: std::sync::Arc<tokio::sync::Mutex<HashMap<String, SwarmCoordinator>>>,
}

impl SwarmInitTool {
    pub fn new() -> Self {
        Self {
            swarms: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ToolSpec for SwarmInitTool {
    fn name(&self) -> &'static str {
        "swarm_init"
    }

    fn description(&self) -> &'static str {
        "Initialize an agent swarm with a topology, max agents, and objective. Supports hierarchical (queen-led, anti-drift), mesh (peer-to-peer), ring (pipeline), star (centralized), and adaptive topologies."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "objective": {
                    "type": "string",
                    "description": "High-level goal for the swarm"
                },
                "topology": {
                    "type": "string",
                    "enum": ["hierarchical", "mesh", "ring", "star", "adaptive"],
                    "description": "Agent organization pattern (default: hierarchical)"
                },
                "max_agents": {
                    "type": "integer",
                    "description": "Maximum number of agents in the swarm (default: 8)"
                },
                "consensus": {
                    "type": "string",
                    "enum": ["majority", "weighted", "byzantine", "gossip"],
                    "description": "Decision-making protocol (default: weighted)"
                },
                "strategy": {
                    "type": "string",
                    "enum": ["development", "research", "security", "optimization"],
                    "description": "Execution strategy (default: development)"
                },
                "agents": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": {
                                "type": "string",
                                "enum": ["coordinator", "coder", "tester", "reviewer", "architect", "researcher", "security", "documenter", "optimizer"]
                            },
                            "task": {
                                "type": "string",
                                "description": "Optional task assignment for this agent"
                            }
                        },
                        "required": ["role"]
                    },
                    "description": "Pre-defined agent roster (optional, spawn later via agent_spawn)"
                }
            },
            "required": ["objective"]
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
        let objective = input
            .get("objective")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::invalid_input("Missing 'objective'"))?;

        let topology = input
            .get("topology")
            .and_then(|v| v.as_str())
            .and_then(SwarmTopology::from_str)
            .unwrap_or(SwarmTopology::Hierarchical);

        let max_agents = input
            .get("max_agents")
            .and_then(|v| v.as_u64())
            .map(|n| n.min(20) as usize)
            .unwrap_or(8);

        let consensus = input
            .get("consensus")
            .and_then(|v| v.as_str())
            .and_then(ConsensusProtocol::from_str)
            .unwrap_or(ConsensusProtocol::Weighted);

        let strategy = input
            .get("strategy")
            .and_then(|v| v.as_str())
            .unwrap_or("development")
            .to_string();

        let config = SwarmConfig {
            topology,
            max_agents,
            consensus,
            objective: objective.to_string(),
            strategy,
        };

        let mut coordinator = SwarmCoordinator::new(config);

        // Register pre-defined agents
        if let Some(agent_list) = input.get("agents").and_then(|v| v.as_array()) {
            for entry in agent_list {
                let role_str = entry.get("role").and_then(|v| v.as_str()).unwrap_or("coder");
                let role = match role_str {
                    "coordinator" => SwarmAgentRole::Coordinator,
                    "coder" => SwarmAgentRole::Coder,
                    "tester" => SwarmAgentRole::Tester,
                    "reviewer" => SwarmAgentRole::Reviewer,
                    "architect" => SwarmAgentRole::Architect,
                    "researcher" => SwarmAgentRole::Researcher,
                    "security" => SwarmAgentRole::Security,
                    "documenter" => SwarmAgentRole::Documenter,
                    "optimizer" => SwarmAgentRole::Optimizer,
                    other => SwarmAgentRole::Custom(other.to_string()),
                };
                let task = entry.get("task").and_then(|v| v.as_str()).map(String::from);
                coordinator.add_agent(role, task);
            }
        }

        coordinator.start();
        let snapshot = coordinator.snapshot();

        let mut swarms = self.swarms.lock().await;
        swarms.insert(snapshot.id.clone(), coordinator);

        Ok(ToolResult::success(format!(
            "Swarm initialized: {} ({} topology, {} agents, {} consensus)\n{}",
            snapshot.objective,
            snapshot.topology,
            snapshot.active_agents,
            snapshot.consensus,
            serde_json::to_string_pretty(&snapshot).unwrap_or_default()
        )))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_round_trip() {
        for topo in &[
            SwarmTopology::Hierarchical,
            SwarmTopology::Mesh,
            SwarmTopology::Ring,
            SwarmTopology::Star,
            SwarmTopology::Adaptive,
        ] {
            let s = topo.as_str();
            let parsed = SwarmTopology::from_str(s).unwrap();
            assert_eq!(*topo, parsed);
        }
    }

    #[test]
    fn test_consensus_min_agents() {
        assert_eq!(ConsensusProtocol::Majority.min_agents(), 1);
        assert_eq!(ConsensusProtocol::Weighted.min_agents(), 2);
        assert_eq!(ConsensusProtocol::Byzantine.min_agents(), 4);
        assert_eq!(ConsensusProtocol::Gossip.min_agents(), 2);
    }

    #[test]
    fn test_swarm_snapshot() {
        let config = SwarmConfig {
            topology: SwarmTopology::Hierarchical,
            max_agents: 5,
            consensus: ConsensusProtocol::Weighted,
            objective: "Build auth module".to_string(),
            strategy: "development".to_string(),
        };
        let mut coord = SwarmCoordinator::new(config);
        coord.add_agent(SwarmAgentRole::Coordinator, None);
        coord.add_agent(SwarmAgentRole::Coder, Some("Implement login".to_string()));
        coord.add_agent(SwarmAgentRole::Tester, Some("Test login endpoint".to_string()));
        coord.start();

        let snap = coord.snapshot();
        assert_eq!(snap.objective, "Build auth module");
        assert_eq!(snap.topology, "hierarchical");
        assert_eq!(snap.active_agents, 3);
        assert_eq!(snap.status, "running");
        assert_eq!(snap.agents.len(), 3);
    }

    #[test]
    fn test_swarm_init_tool_input_validation() {
        // Just test that the tool can be constructed
        let _tool = SwarmInitTool::new();
    }
}
