//! Agent swarm primitives, consensus protocols, and debate modes.
//!
//! ## Swarm topologies
//!
//! - **Queen**: single coordinator dispatches to workers; workers report back.
//! - **Mesh**: peer-to-peer — every agent can talk to every other agent.
//! - **Adaptive**: the swarm self-organizes; topology evolves with task.
//!
//! ## Consensus protocols
//!
//! - **Raft**: leader election + log replication for consistent state.
//! - **Byzantine**: fault-tolerant consensus tolerating up to f malicious nodes.
//! - **Gossip**: eventual consistency through randomized peer exchanges.
//!
//! ## Debate mode
//!
//! Two-agent adversarial reasoning where a **Debater** argues a position
//! and a **Supervisor** judges the exchange, synthesizing the best outcome.

use serde::{Deserialize, Serialize};

// ── Swarm topology ───────────────────────────────────────────────────────────

/// Swarm organization topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SwarmTopology {
    /// Single coordinator dispatches to N workers. Best for well-scoped,
    /// decomposable tasks with clear subtask boundaries.
    #[default]
    Queen,
    /// Full peer-to-peer mesh. Every agent can communicate with every
    /// other agent. Best for collaborative reasoning tasks.
    Mesh,
    /// Self-organizing topology. The swarm dynamically reconfigures
    /// based on task requirements and agent capabilities.
    Adaptive,
}

impl SwarmTopology {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queen => "queen",
            Self::Mesh => "mesh",
            Self::Adaptive => "adaptive",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "queen" | "coordinator" => Some(Self::Queen),
            "mesh" | "peer-to-peer" | "p2p" => Some(Self::Mesh),
            "adaptive" | "auto" => Some(Self::Adaptive),
            _ => None,
        }
    }
}

// ── Consensus protocol ──────────────────────────────────────────────────────

/// Consensus protocol for multi-agent agreement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsensusProtocol {
    /// Raft — leader election + log replication. Strong consistency,
    /// tolerates crash faults of minority nodes.
    Raft,
    /// Byzantine Fault Tolerant — tolerates up to f malicious nodes
    /// with 3f+1 total. Used when agents may produce conflicting or
    /// deliberately wrong outputs.
    Byzantine,
    /// Gossip — randomized peer exchange for eventual consistency.
    /// Lightweight, no leader, best for large swarms where strong
    /// consistency isn't required.
    Gossip,
}

impl ConsensusProtocol {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raft => "raft",
            Self::Byzantine => "byzantine",
            Self::Gossip => "gossip",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "raft" => Some(Self::Raft),
            "byzantine" | "bft" | "pbft" => Some(Self::Byzantine),
            "gossip" | "epidemic" => Some(Self::Gossip),
            _ => None,
        }
    }

    /// Minimum number of agents required for this protocol to be safe.
    #[must_use]
    pub fn min_agents(self) -> usize {
        match self {
            Self::Raft => 3,      // 1 leader + 2 followers
            Self::Byzantine => 4, // 3f+1 with f=1
            Self::Gossip => 2,    // any two can gossip
        }
    }
}

// ── Swarm configuration ─────────────────────────────────────────────────────

/// Configuration for launching an agent swarm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmConfig {
    /// Number of agents in the swarm.
    pub agent_count: usize,
    /// How agents are organized.
    pub topology: SwarmTopology,
    /// How agents agree on results.
    pub consensus: ConsensusProtocol,
    /// Maximum rounds of consensus before accepting a result.
    pub max_rounds: u32,
    /// Timeout per consensus round in milliseconds.
    pub round_timeout_ms: u64,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            agent_count: 3,
            topology: SwarmTopology::Queen,
            consensus: ConsensusProtocol::Raft,
            max_rounds: 5,
            round_timeout_ms: 30_000,
        }
    }
}

impl SwarmConfig {
    #[must_use]
    pub fn new(agent_count: usize, topology: SwarmTopology, consensus: ConsensusProtocol) -> Self {
        Self {
            agent_count,
            topology,
            consensus,
            ..Default::default()
        }
    }
}

// ── Debate mode ──────────────────────────────────────────────────────────────

/// Role in a two-agent debate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebateRole {
    /// Argues a position.
    Debater,
    /// Judges and synthesizes.
    Supervisor,
}

impl DebateRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debater => "debater",
            Self::Supervisor => "supervisor",
        }
    }
}

/// Configuration for DEBATE_MODE — two-agent adversarial reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateConfig {
    /// The proposition or question being debated.
    pub proposition: String,
    /// Maximum number of debate rounds (exchange cycles).
    pub max_rounds: u32,
    /// Model for the debater agent.
    pub debater_model: String,
    /// Model for the supervisor agent.
    pub supervisor_model: String,
}

impl Default for DebateConfig {
    fn default() -> Self {
        Self {
            proposition: String::new(),
            max_rounds: 3,
            debater_model: "deepseek-v4-pro".to_string(),
            supervisor_model: "deepseek-v4-pro".to_string(),
        }
    }
}

impl DebateConfig {
    #[must_use]
    pub fn new(proposition: impl Into<String>) -> Self {
        Self {
            proposition: proposition.into(),
            ..Default::default()
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_round_trips() {
        for t in [
            SwarmTopology::Queen,
            SwarmTopology::Mesh,
            SwarmTopology::Adaptive,
        ] {
            assert_eq!(SwarmTopology::from_str(t.as_str()), Some(t));
        }
    }

    #[test]
    fn test_consensus_round_trips() {
        for c in [
            ConsensusProtocol::Raft,
            ConsensusProtocol::Byzantine,
            ConsensusProtocol::Gossip,
        ] {
            assert_eq!(ConsensusProtocol::from_str(c.as_str()), Some(c));
        }
    }

    #[test]
    fn test_consensus_min_agents() {
        assert_eq!(ConsensusProtocol::Raft.min_agents(), 3);
        assert_eq!(ConsensusProtocol::Byzantine.min_agents(), 4);
        assert_eq!(ConsensusProtocol::Gossip.min_agents(), 2);
    }

    #[test]
    fn test_swarm_config_defaults() {
        let cfg = SwarmConfig::default();
        assert_eq!(cfg.agent_count, 3);
        assert_eq!(cfg.topology, SwarmTopology::Queen);
        assert_eq!(cfg.consensus, ConsensusProtocol::Raft);
    }

    #[test]
    fn test_debate_config_new() {
        let cfg = DebateConfig::new("Is type-safety always worth the cost?");
        assert_eq!(cfg.max_rounds, 3);
        assert_eq!(cfg.debater_model, "deepseek-v4-pro");
    }
}
