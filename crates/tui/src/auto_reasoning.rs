//! Adaptive reasoning-effort tier selection for `Auto` mode (#663).
//!
//! When the user sets `reasoning_effort = "auto"`, the engine calls
//! [`select`] before each turn-level request to pick the actual tier
//! based on the current message.

use crate::tui::app::ReasoningEffort;

/// Choose a concrete `ReasoningEffort` tier for the next API request.
///
/// Rules:
/// - Sub-agent contexts (`is_subagent == true`) → `Low`
/// - Last user message contains `"debug"` or `"error"` → `Max`
/// - Last user message contains `"search"` or `"lookup"` → `Low`
/// - Everything else → `High`
#[must_use]
pub fn select(is_subagent: bool, last_msg: &str) -> ReasoningEffort {
    if is_subagent {
        return ReasoningEffort::Low;
    }

    let lower = last_msg.to_ascii_lowercase();

    if lower.contains("debug") || lower.contains("error") {
        return ReasoningEffort::Max;
    }

    if lower.contains("search") || lower.contains("lookup") {
        return ReasoningEffort::Low;
    }

    ReasoningEffort::High
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_returns_low() {
        assert_eq!(select(true, "anything"), ReasoningEffort::Low);
        assert_eq!(select(true, "debug this"), ReasoningEffort::Low);
        assert_eq!(select(true, "search query"), ReasoningEffort::Low);
    }

    #[test]
    fn debug_or_error_returns_max() {
        assert_eq!(select(false, "find a bug"), ReasoningEffort::High);
        assert_eq!(select(false, "debug crash"), ReasoningEffort::Max);
        assert_eq!(select(false, "Error: timeout"), ReasoningEffort::Max);
        assert_eq!(select(false, "fix this error"), ReasoningEffort::Max);
        assert_eq!(select(false, "DEBUG output"), ReasoningEffort::Max);
    }

    #[test]
    fn search_or_lookup_returns_low() {
        assert_eq!(select(false, "search for the file"), ReasoningEffort::Low);
        assert_eq!(select(false, "lookup docs"), ReasoningEffort::Low);
        assert_eq!(select(false, "SearchQuery"), ReasoningEffort::Low);
        assert_eq!(select(false, "lookup_user"), ReasoningEffort::Low);
    }

    #[test]
    fn default_returns_high() {
        assert_eq!(select(false, "hello"), ReasoningEffort::High);
        assert_eq!(select(false, "write a test"), ReasoningEffort::High);
        assert_eq!(select(false, "refactor this module"), ReasoningEffort::High);
        assert_eq!(select(false, ""), ReasoningEffort::High);
    }
}
