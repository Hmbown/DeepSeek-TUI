//! Duo mode state machine for hegelion's autocoding (player-coach adversarial cooperation).
//!
//! Implements the g3 paper's coach-player paradigm where:
//! - Player: implements requirements (builder role)
//! - Coach: validates implementation against requirements (critic role)
//!
//! The loop continues until the coach approves or max turns are reached.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// === Phase & Status Enums ===

/// The current phase in the autocoding loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuoPhase {
    /// Session initialized, ready to start player phase
    Init,
    /// Player is implementing requirements
    Player,
    /// Coach is validating the implementation
    Coach,
    /// Coach approved the implementation
    Approved,
    /// Maximum turns reached without approval
    Timeout,
}

impl std::fmt::Display for DuoPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DuoPhase::Init => write!(f, "init"),
            DuoPhase::Player => write!(f, "player"),
            DuoPhase::Coach => write!(f, "coach"),
            DuoPhase::Approved => write!(f, "approved"),
            DuoPhase::Timeout => write!(f, "timeout"),
        }
    }
}

/// The overall status of the autocoding session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuoStatus {
    /// Session is actively running
    Active,
    /// Coach has approved the implementation
    Approved,
    /// Coach has rejected (used for explicit rejection, not just iteration)
    Rejected,
    /// Maximum turns exhausted without approval
    Timeout,
}

impl std::fmt::Display for DuoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DuoStatus::Active => write!(f, "active"),
            DuoStatus::Approved => write!(f, "approved"),
            DuoStatus::Rejected => write!(f, "rejected"),
            DuoStatus::Timeout => write!(f, "timeout"),
        }
    }
}

// === Turn History ===

/// Record of a single turn in the autocoding loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    /// Turn number (1-indexed)
    pub turn: u32,
    /// The phase this record is for
    pub phase: DuoPhase,
    /// Summary of what happened (player implementation or coach feedback)
    pub summary: String,
    /// Quality score from coach (0.0 to 1.0), if applicable
    pub quality_score: Option<f64>,
    /// Timestamp when this turn was recorded
    #[serde(default = "chrono::Utc::now")]
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl TurnRecord {
    /// Create a new turn record.
    #[must_use]
    pub fn new(turn: u32, phase: DuoPhase, summary: String, quality_score: Option<f64>) -> Self {
        Self {
            turn,
            phase,
            summary,
            quality_score,
            timestamp: chrono::Utc::now(),
        }
    }
}

// === Main State ===

/// The complete state of a Duo autocoding session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuoState {
    /// Unique session identifier
    pub session_id: String,
    /// Optional human-readable session name
    pub session_name: Option<String>,
    /// The requirements document (source of truth for validation)
    pub requirements: String,
    /// Current turn number (1-indexed)
    pub current_turn: u32,
    /// Maximum allowed turns before timeout
    pub max_turns: u32,
    /// Current phase in the autocoding loop
    pub phase: DuoPhase,
    /// Overall session status
    pub status: DuoStatus,
    /// History of all turns
    pub turn_history: Vec<TurnRecord>,
    /// Last feedback from the coach (used in next player prompt)
    pub last_coach_feedback: Option<String>,
    /// Quality scores from each coach review
    pub quality_scores: Vec<f64>,
    /// Threshold score needed for approval (0.0 to 1.0)
    pub approval_threshold: f64,
    /// Timestamp when session was created
    #[serde(default = "chrono::Utc::now")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Timestamp of last update
    #[serde(default = "chrono::Utc::now")]
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl DuoState {
    /// Create a new Duo session with the given requirements.
    ///
    /// # Arguments
    /// * `requirements` - The requirements document (source of truth)
    /// * `session_name` - Optional human-readable name
    /// * `max_turns` - Maximum turns before timeout (default: 10)
    /// * `approval_threshold` - Score needed for approval (default: 0.9)
    #[must_use]
    pub fn create(
        requirements: String,
        session_name: Option<String>,
        max_turns: Option<u32>,
        approval_threshold: Option<f64>,
    ) -> Self {
        let now = chrono::Utc::now();
        Self {
            session_id: Uuid::new_v4().to_string(),
            session_name,
            requirements,
            current_turn: 1,
            max_turns: max_turns.unwrap_or(10),
            phase: DuoPhase::Init,
            status: DuoStatus::Active,
            turn_history: Vec::new(),
            last_coach_feedback: None,
            quality_scores: Vec::new(),
            approval_threshold: approval_threshold.unwrap_or(0.9),
            created_at: now,
            updated_at: now,
        }
    }

    /// Transition from Init or Player phase to Coach phase.
    ///
    /// Records the player's implementation summary in turn history.
    ///
    /// # Arguments
    /// * `player_summary` - Summary of what the player implemented
    ///
    /// # Returns
    /// `Ok(())` on success, `Err` if not in a valid phase for this transition
    pub fn advance_to_coach(&mut self, player_summary: String) -> Result<(), DuoError> {
        match self.phase {
            DuoPhase::Init | DuoPhase::Player => {
                // Record player turn
                let record =
                    TurnRecord::new(self.current_turn, DuoPhase::Player, player_summary, None);
                self.turn_history.push(record);
                self.phase = DuoPhase::Coach;
                self.updated_at = chrono::Utc::now();
                Ok(())
            }
            _ => Err(DuoError::InvalidPhaseTransition {
                from: self.phase,
                to: DuoPhase::Coach,
            }),
        }
    }

    /// Process coach feedback and determine the next phase.
    ///
    /// # Arguments
    /// * `coach_feedback` - The coach's feedback text
    /// * `approved` - Whether the coach approved the implementation
    /// * `compliance_score` - Optional compliance score (0.0 to 1.0)
    ///
    /// # Returns
    /// `Ok(())` on success, `Err` if not in coach phase
    pub fn advance_turn(
        &mut self,
        coach_feedback: String,
        approved: bool,
        compliance_score: Option<f64>,
    ) -> Result<(), DuoError> {
        if self.phase != DuoPhase::Coach {
            return Err(DuoError::InvalidPhaseTransition {
                from: self.phase,
                to: DuoPhase::Player,
            });
        }

        // Record coach turn
        let record = TurnRecord::new(
            self.current_turn,
            DuoPhase::Coach,
            coach_feedback.clone(),
            compliance_score,
        );
        self.turn_history.push(record);

        // Track quality score if provided
        if let Some(score) = compliance_score {
            self.quality_scores.push(score);
        }

        self.last_coach_feedback = Some(coach_feedback);
        self.updated_at = chrono::Utc::now();

        if approved {
            // Coach approved - session complete
            self.phase = DuoPhase::Approved;
            self.status = DuoStatus::Approved;
        } else if self.current_turn >= self.max_turns {
            // Max turns reached - timeout
            self.phase = DuoPhase::Timeout;
            self.status = DuoStatus::Timeout;
        } else {
            // Continue to next turn
            self.current_turn += 1;
            self.phase = DuoPhase::Player;
        }

        Ok(())
    }

    /// Check if the session is complete (approved or timed out).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(
            self.status,
            DuoStatus::Approved | DuoStatus::Rejected | DuoStatus::Timeout
        )
    }

    /// Get the number of turns remaining before timeout.
    #[must_use]
    pub fn turns_remaining(&self) -> u32 {
        self.max_turns.saturating_sub(self.current_turn)
    }

    /// Get the average quality score across all coach reviews.
    #[must_use]
    pub fn average_quality_score(&self) -> Option<f64> {
        if self.quality_scores.is_empty() {
            None
        } else {
            let sum: f64 = self.quality_scores.iter().sum();
            Some(sum / self.quality_scores.len() as f64)
        }
    }

    /// Generate a human-readable summary of the session state.
    #[must_use]
    pub fn summary(&self) -> String {
        let name = self
            .session_name
            .as_deref()
            .unwrap_or(&self.session_id[..8]);

        let avg_score = self
            .average_quality_score()
            .map(|s| format!("{:.1}%", s * 100.0))
            .unwrap_or_else(|| "N/A".to_string());

        let status_icon = match self.status {
            DuoStatus::Active => "🔄",
            DuoStatus::Approved => "✅",
            DuoStatus::Rejected => "❌",
            DuoStatus::Timeout => "⏰",
        };

        format!(
            "{status_icon} Duo Session: {name}\n\
             Phase: {} | Turn: {}/{} | Status: {}\n\
             Avg Quality: {} | Threshold: {:.0}%\n\
             History: {} records",
            self.phase,
            self.current_turn,
            self.max_turns,
            self.status,
            avg_score,
            self.approval_threshold * 100.0,
            self.turn_history.len()
        )
    }
}

// === Error Types ===

/// Errors that can occur during Duo session operations.
#[derive(Debug, Clone)]
pub enum DuoError {
    /// Invalid phase transition attempted
    InvalidPhaseTransition { from: DuoPhase, to: DuoPhase },
    /// Session not found (reserved for future multi-session management)
    #[allow(dead_code)]
    SessionNotFound { session_id: String },
    /// Session already complete (reserved for future session validation)
    #[allow(dead_code)]
    SessionAlreadyComplete { session_id: String },
}

impl std::fmt::Display for DuoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DuoError::InvalidPhaseTransition { from, to } => {
                write!(f, "Invalid phase transition from {} to {}", from, to)
            }
            DuoError::SessionNotFound { session_id } => {
                write!(f, "Session not found: {}", session_id)
            }
            DuoError::SessionAlreadyComplete { session_id } => {
                write!(f, "Session already complete: {}", session_id)
            }
        }
    }
}

impl std::error::Error for DuoError {}

// === Session Container ===

/// Container for managing multiple Duo sessions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DuoSession {
    /// The currently active session state
    pub active_state: Option<DuoState>,
    /// Saved/completed session states indexed by session_id
    pub saved_states: HashMap<String, DuoState>,
}

impl DuoSession {
    /// Create a new empty session container.
    #[must_use]
    pub fn new() -> Self {
        Self {
            active_state: None,
            saved_states: HashMap::new(),
        }
    }

    /// Start a new Duo session.
    pub fn start_session(
        &mut self,
        requirements: String,
        session_name: Option<String>,
        max_turns: Option<u32>,
        approval_threshold: Option<f64>,
    ) -> &DuoState {
        // Save any existing active session
        if let Some(state) = self.active_state.take() {
            self.saved_states.insert(state.session_id.clone(), state);
        }

        // Create new session
        let state = DuoState::create(requirements, session_name, max_turns, approval_threshold);
        self.active_state = Some(state);
        self.active_state.as_ref().expect("just set active_state")
    }

    /// Get the active session state.
    #[must_use]
    pub fn get_active(&self) -> Option<&DuoState> {
        self.active_state.as_ref()
    }

    /// Get a mutable reference to the active session state.
    pub fn get_active_mut(&mut self) -> Option<&mut DuoState> {
        self.active_state.as_mut()
    }

    /// Get a saved session by ID (reserved for future multi-session management).
    #[must_use]
    #[allow(dead_code)]
    pub fn get_saved(&self, session_id: &str) -> Option<&DuoState> {
        self.saved_states.get(session_id)
    }

    /// Save the current active session and clear it (reserved for future session management).
    #[allow(dead_code)]
    pub fn save_active(&mut self) -> Option<String> {
        if let Some(state) = self.active_state.take() {
            let id = state.session_id.clone();
            self.saved_states.insert(id.clone(), state);
            Some(id)
        } else {
            None
        }
    }

    /// Restore a saved session as the active session (reserved for future session management).
    #[allow(dead_code)]
    pub fn restore_session(&mut self, session_id: &str) -> Result<(), DuoError> {
        let state =
            self.saved_states
                .remove(session_id)
                .ok_or_else(|| DuoError::SessionNotFound {
                    session_id: session_id.to_string(),
                })?;

        // Save current active if any
        if let Some(current) = self.active_state.take() {
            self.saved_states
                .insert(current.session_id.clone(), current);
        }

        self.active_state = Some(state);
        Ok(())
    }

    /// List all session IDs (active and saved, reserved for future session management).
    #[must_use]
    #[allow(dead_code)]
    pub fn list_sessions(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.saved_states.keys().map(String::as_str).collect();
        if let Some(ref active) = self.active_state {
            ids.push(&active.session_id);
        }
        ids.sort();
        ids
    }
}

/// Thread-safe shared Duo session.
pub type SharedDuoSession = Arc<Mutex<DuoSession>>;

/// Create a new shared Duo session.
#[must_use]
pub fn new_shared_duo_session() -> SharedDuoSession {
    Arc::new(Mutex::new(DuoSession::new()))
}

// === Prompt Generation ===

/// Generate the player (implementation) prompt for the current state.
///
/// The player focuses on implementing requirements and should NOT declare success.
#[must_use]
pub fn generate_player_prompt(state: &DuoState) -> String {
    let mut prompt = String::new();

    prompt.push_str("# Player Phase - Implementation\n\n");
    prompt.push_str("You are the PLAYER in an autocoding session. Your role is to IMPLEMENT the requirements.\n\n");

    prompt.push_str("## Requirements (Source of Truth)\n\n");
    prompt.push_str(&state.requirements);
    prompt.push_str("\n\n");

    prompt.push_str(&format!(
        "## Session Info\n\n\
         - Turn: {}/{}\n\
         - Approval Threshold: {:.0}%\n",
        state.current_turn,
        state.max_turns,
        state.approval_threshold * 100.0
    ));

    if let Some(ref feedback) = state.last_coach_feedback {
        prompt.push_str("\n## Previous Coach Feedback\n\n");
        prompt.push_str("Address these issues from the last review:\n\n");
        prompt.push_str(feedback);
        prompt.push('\n');
    }

    prompt.push_str("\n## Instructions\n\n");
    prompt.push_str(
        "1. Implement the requirements above using available tools\n\
         2. Focus on making incremental progress\n\
         3. DO NOT declare success or claim completion\n\
         4. DO NOT evaluate your own work\n\
         5. The Coach will verify your implementation\n\n\
         Begin implementation now.\n",
    );

    prompt
}

/// Generate the coach (validation) prompt for the current state.
///
/// The coach verifies the implementation against requirements and ignores player self-assessment.
#[must_use]
pub fn generate_coach_prompt(state: &DuoState) -> String {
    let mut prompt = String::new();

    prompt.push_str("# Coach Phase - Validation\n\n");
    prompt.push_str("You are the COACH in an autocoding session. Your role is to VERIFY the implementation.\n\n");

    prompt.push_str("## Requirements (Source of Truth)\n\n");
    prompt.push_str(&state.requirements);
    prompt.push_str("\n\n");

    prompt.push_str(&format!(
        "## Session Info\n\n\
         - Turn: {}/{}\n\
         - Approval Threshold: {:.0}%\n\
         - Turns Remaining: {}\n",
        state.current_turn,
        state.max_turns,
        state.approval_threshold * 100.0,
        state.turns_remaining()
    ));

    if !state.quality_scores.is_empty() {
        let avg = state.average_quality_score().unwrap_or(0.0);
        prompt.push_str(&format!("- Average Quality: {:.1}%\n", avg * 100.0));
    }

    prompt.push_str("\n## Instructions\n\n");
    prompt.push_str(
        "1. Review the current implementation against the requirements\n\
         2. Create a COMPLIANCE CHECKLIST:\n\
            - [ ] or [x] for each requirement item\n\
            - Note any missing or incorrect implementations\n\
         3. Calculate a COMPLIANCE SCORE (0.0 to 1.0)\n\
         4. IGNORE any player self-assessment or claims of completion\n\
         5. If score >= threshold AND all critical items pass:\n\
            - Output: COACH APPROVED\n\
         6. Otherwise, provide specific feedback:\n\
            - What is missing\n\
            - What needs to be fixed\n\
            - Actionable next steps\n\n\
         Begin validation now.\n",
    );

    prompt
}

/// Generate a summary of the session for system prompt injection.
#[must_use]
pub fn session_summary(session: &DuoSession) -> String {
    let mut lines = Vec::new();

    if let Some(ref state) = session.active_state {
        lines.push(format!("Active Duo Session: {}", state.summary()));
    } else {
        lines.push("No active Duo session.".to_string());
    }

    if !session.saved_states.is_empty() {
        lines.push(format!("Saved sessions: {}", session.saved_states.len()));
        for (id, state) in &session.saved_states {
            let name = state
                .session_name
                .as_deref()
                .unwrap_or(&id[..8.min(id.len())]);
            lines.push(format!("  - {}: {} ({})", name, state.status, state.phase));
        }
    }

    lines.join("\n")
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_requirements() -> String {
        "## Requirements\n\
         - [ ] Create a function `add(a, b)` that returns the sum\n\
         - [ ] Add unit tests for the function\n\
         - [ ] Document the function with comments"
            .to_string()
    }

    #[test]
    fn test_create_session() {
        let state = DuoState::create(
            sample_requirements(),
            Some("test-session".to_string()),
            None,
            None,
        );

        assert_eq!(state.session_name, Some("test-session".to_string()));
        assert_eq!(state.current_turn, 1);
        assert_eq!(state.max_turns, 10);
        assert_eq!(state.phase, DuoPhase::Init);
        assert_eq!(state.status, DuoStatus::Active);
        assert!(state.turn_history.is_empty());
        assert!(state.last_coach_feedback.is_none());
        assert_eq!(state.approval_threshold, 0.9);
    }

    #[test]
    fn test_advance_to_coach() {
        let mut state = DuoState::create(sample_requirements(), None, None, None);

        assert!(
            state
                .advance_to_coach("Implemented add function".to_string())
                .is_ok()
        );
        assert_eq!(state.phase, DuoPhase::Coach);
        assert_eq!(state.turn_history.len(), 1);
        assert_eq!(state.turn_history[0].phase, DuoPhase::Player);
    }

    #[test]
    fn test_advance_turn_approved() {
        let mut state = DuoState::create(sample_requirements(), None, None, None);
        state
            .advance_to_coach("Implemented everything".to_string())
            .unwrap();

        assert!(
            state
                .advance_turn(
                    "COACH APPROVED - All requirements met".to_string(),
                    true,
                    Some(0.95)
                )
                .is_ok()
        );

        assert_eq!(state.phase, DuoPhase::Approved);
        assert_eq!(state.status, DuoStatus::Approved);
        assert!(state.is_complete());
        assert_eq!(state.quality_scores, vec![0.95]);
    }

    #[test]
    fn test_advance_turn_continue() {
        let mut state = DuoState::create(sample_requirements(), None, None, None);
        state
            .advance_to_coach("Partial implementation".to_string())
            .unwrap();

        assert!(
            state
                .advance_turn("Missing tests".to_string(), false, Some(0.5))
                .is_ok()
        );

        assert_eq!(state.phase, DuoPhase::Player);
        assert_eq!(state.status, DuoStatus::Active);
        assert_eq!(state.current_turn, 2);
        assert!(!state.is_complete());
        assert_eq!(state.last_coach_feedback, Some("Missing tests".to_string()));
    }

    #[test]
    fn test_timeout() {
        let mut state = DuoState::create(sample_requirements(), None, Some(2), None);

        // Turn 1
        state.advance_to_coach("Attempt 1".to_string()).unwrap();
        state
            .advance_turn("Not good enough".to_string(), false, Some(0.3))
            .unwrap();

        // Turn 2 (max)
        state.advance_to_coach("Attempt 2".to_string()).unwrap();
        state
            .advance_turn("Still not good enough".to_string(), false, Some(0.4))
            .unwrap();

        assert_eq!(state.phase, DuoPhase::Timeout);
        assert_eq!(state.status, DuoStatus::Timeout);
        assert!(state.is_complete());
    }

    #[test]
    fn test_invalid_phase_transition() {
        let mut state = DuoState::create(sample_requirements(), None, None, None);
        state.phase = DuoPhase::Approved;

        let result = state.advance_to_coach("Should fail".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_turns_remaining() {
        let state = DuoState::create(sample_requirements(), None, Some(10), None);
        assert_eq!(state.turns_remaining(), 9);
    }

    #[test]
    fn test_average_quality_score() {
        let mut state = DuoState::create(sample_requirements(), None, None, None);
        assert!(state.average_quality_score().is_none());

        state.quality_scores = vec![0.5, 0.7, 0.9];
        let avg = state.average_quality_score().unwrap();
        assert!((avg - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_session_container() {
        let mut session = DuoSession::new();

        // Start first session
        session.start_session(
            sample_requirements(),
            Some("session-1".to_string()),
            None,
            None,
        );
        assert!(session.get_active().is_some());

        // Start second session (first gets saved)
        session.start_session(
            "Other requirements".to_string(),
            Some("session-2".to_string()),
            None,
            None,
        );
        assert_eq!(session.saved_states.len(), 1);

        // Get active
        let active = session.get_active().unwrap();
        assert_eq!(active.session_name, Some("session-2".to_string()));
    }

    #[test]
    fn test_generate_player_prompt() {
        let state = DuoState::create(sample_requirements(), None, None, None);
        let prompt = generate_player_prompt(&state);

        assert!(prompt.contains("Player Phase"));
        assert!(prompt.contains("Requirements (Source of Truth)"));
        assert!(prompt.contains("Turn: 1/10"));
        assert!(prompt.contains("DO NOT declare success"));
    }

    #[test]
    fn test_generate_coach_prompt() {
        let state = DuoState::create(sample_requirements(), None, None, None);
        let prompt = generate_coach_prompt(&state);

        assert!(prompt.contains("Coach Phase"));
        assert!(prompt.contains("COMPLIANCE CHECKLIST"));
        assert!(prompt.contains("COACH APPROVED"));
        assert!(prompt.contains("IGNORE any player self-assessment"));
    }

    #[test]
    fn test_shared_session() {
        let shared = new_shared_duo_session();

        {
            let mut session = shared.lock().unwrap();
            session.start_session(sample_requirements(), None, None, None);
        }

        {
            let session = shared.lock().unwrap();
            assert!(session.get_active().is_some());
        }
    }

    #[test]
    fn test_summary() {
        let state = DuoState::create(sample_requirements(), Some("test".to_string()), None, None);
        let summary = state.summary();

        assert!(summary.contains("Duo Session: test"));
        assert!(summary.contains("Phase: init"));
        assert!(summary.contains("Turn: 1/10"));
    }

    #[test]
    fn test_session_summary() {
        let mut session = DuoSession::new();
        session.start_session(
            sample_requirements(),
            Some("active-session".to_string()),
            None,
            None,
        );

        let summary = session_summary(&session);
        assert!(summary.contains("Active Duo Session"));
        assert!(summary.contains("active-session"));
    }
}
