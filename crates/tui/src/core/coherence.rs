//! Plain-language session coherence state derived from capacity events.

use serde::{Deserialize, Serialize};

use crate::core::capacity::{GuardrailAction, RiskBand};

/// User-facing coherence ladder for session health.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoherenceState {
    #[default]
    Healthy,
    GettingCrowded,
    RefreshingContext,
    VerifyingRecentWork,
    ResettingPlan,
}

impl CoherenceState {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::GettingCrowded => "getting crowded",
            Self::RefreshingContext => "refreshing context",
            Self::VerifyingRecentWork => "verifying recent work",
            Self::ResettingPlan => "resetting plan",
        }
    }

    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::Healthy => "The session is stable and focused.",
            Self::GettingCrowded => "The session is approaching context pressure.",
            Self::RefreshingContext => "The engine is refreshing context before continuing.",
            Self::VerifyingRecentWork => {
                "The engine is checking recent tool results before continuing."
            }
            Self::ResettingPlan => {
                "The engine is rebuilding from canonical context and replanning."
            }
        }
    }
}

/// Synthetic input to the coherence reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoherenceSignal {
    CapacityDecision {
        risk_band: RiskBand,
        action: GuardrailAction,
        cooldown_blocked: bool,
    },
    CapacityIntervention {
        action: GuardrailAction,
    },
    CompactionStarted,
    CompactionCompleted,
    CompactionFailed,
}

/// Pure transition function for the plain-language coherence ladder.
#[must_use]
pub fn next_coherence_state(current: CoherenceState, signal: CoherenceSignal) -> CoherenceState {
    match signal {
        CoherenceSignal::CompactionStarted => CoherenceState::RefreshingContext,
        CoherenceSignal::CompactionCompleted => CoherenceState::Healthy,
        CoherenceSignal::CompactionFailed => CoherenceState::GettingCrowded,
        CoherenceSignal::CapacityIntervention { action }
        | CoherenceSignal::CapacityDecision { action, .. } => match action {
            GuardrailAction::NoIntervention => match signal {
                CoherenceSignal::CapacityDecision {
                    risk_band,
                    cooldown_blocked,
                    ..
                } => {
                    if cooldown_blocked {
                        return current;
                    }
                    match risk_band {
                        RiskBand::Low => CoherenceState::Healthy,
                        RiskBand::Medium | RiskBand::High => CoherenceState::GettingCrowded,
                    }
                }
                _ => current,
            },
            GuardrailAction::TargetedContextRefresh => CoherenceState::RefreshingContext,
            GuardrailAction::VerifyWithToolReplay => CoherenceState::VerifyingRecentWork,
            GuardrailAction::VerifyAndReplan => CoherenceState::ResettingPlan,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_capacity_event_log_drives_plain_language_ladder() {
        let log = [
            CoherenceSignal::CapacityDecision {
                risk_band: RiskBand::Low,
                action: GuardrailAction::NoIntervention,
                cooldown_blocked: false,
            },
            CoherenceSignal::CapacityDecision {
                risk_band: RiskBand::Medium,
                action: GuardrailAction::NoIntervention,
                cooldown_blocked: false,
            },
            CoherenceSignal::CapacityDecision {
                risk_band: RiskBand::Medium,
                action: GuardrailAction::TargetedContextRefresh,
                cooldown_blocked: false,
            },
            CoherenceSignal::CompactionCompleted,
            CoherenceSignal::CapacityDecision {
                risk_band: RiskBand::High,
                action: GuardrailAction::VerifyWithToolReplay,
                cooldown_blocked: false,
            },
            CoherenceSignal::CapacityDecision {
                risk_band: RiskBand::High,
                action: GuardrailAction::VerifyAndReplan,
                cooldown_blocked: false,
            },
        ];

        let mut state = CoherenceState::Healthy;
        let mut states = Vec::new();
        for signal in log {
            state = next_coherence_state(state, signal);
            states.push(state);
        }

        assert_eq!(
            states,
            vec![
                CoherenceState::Healthy,
                CoherenceState::GettingCrowded,
                CoherenceState::RefreshingContext,
                CoherenceState::Healthy,
                CoherenceState::VerifyingRecentWork,
                CoherenceState::ResettingPlan,
            ]
        );
    }
}
