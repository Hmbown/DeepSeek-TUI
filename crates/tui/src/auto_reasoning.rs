//! Adaptive reasoning-effort tier selection for `Auto` mode (#663).
//!
//! When the user sets `reasoning_effort = "auto"`, the engine calls
//! [`select`] before each turn-level request to pick the actual tier
//! based on the current message.
//!
//! The selection uses a weighted scoring system: each pattern that matches
//! contributes a score, and the final tier is chosen based on the aggregate.
//! This replaces the prior two-keyword approach with broader coverage for
//! coding tasks (architecture, security, performance, refactoring, etc.)
//! while keeping trivial queries on Low.

use crate::tui::app::ReasoningEffort;

/// Score thresholds for tier selection.
const MAX_THRESHOLD: i32 = 2;
const LOW_CEILING: i32 = -1;

/// A keyword/phrase pattern with its associated score weight.
struct Pattern {
    keywords: &'static [&'static str],
    score: i32,
}

/// Patterns that indicate complex tasks requiring deep reasoning.
const MAX_PATTERNS: &[Pattern] = &[
    // Debugging & errors
    Pattern { keywords: &["debug", "debugging", "debugger"], score: 3 },
    Pattern { keywords: &["error", "err:", "error:", "traceback", "stacktrace", "panic"], score: 2 },
    Pattern { keywords: &["crash", "crashing", "segfault", "core dump"], score: 3 },
    Pattern { keywords: &["race condition", "deadlock", "data race", "thread safety"], score: 3 },
    // Architecture & design
    Pattern { keywords: &["architect", "design pattern", "system design"], score: 2 },
    Pattern { keywords: &["refactor", "restructure", "reorganize"], score: 2 },
    Pattern { keywords: &["security", "vulnerability", "exploit", "injection", "xss", "csrf"], score: 3 },
    Pattern { keywords: &["performance", "optimize", "bottleneck", "profil", "benchmark"], score: 2 },
    // Complex coding tasks
    Pattern { keywords: &["migration", "migrate", "schema change"], score: 2 },
    Pattern { keywords: &["concurrent", "parallel", "async", "lock", "mutex", "channel"], score: 1 },
    Pattern { keywords: &["algorithm", "complexity", "big-o"], score: 2 },
];

/// Patterns that indicate simple lookup tasks suitable for lower reasoning.
const LOW_PATTERNS: &[Pattern] = &[
    Pattern { keywords: &["search", "lookup", "find", "grep", "locate"], score: -2 },
    Pattern { keywords: &["what is", "what are", "what does", "who is"], score: -2 },
    Pattern { keywords: &["show", "list", "print", "display", "ls "], score: -1 },
    Pattern { keywords: &["read", "cat ", "open file"], score: -1 },
    Pattern { keywords: &["version", "help", "how to"], score: -1 },
    Pattern { keywords: &["rename", "move file", "copy file"], score: -1 },
];

/// Choose a concrete `ReasoningEffort` tier for the next API request.
///
/// Scoring logic:
/// - Sub-agent contexts always get `Low` (they handle narrow sub-tasks).
/// - Each matching pattern adds/subtracts from a running score.
/// - Score >= `MAX_THRESHOLD` -> `Max`
/// - Score <= `LOW_CEILING` -> `Low`
/// - Otherwise -> `High`
///
/// Message length is also used as a tiebreaker: very short messages
/// (under 30 chars) with no strong signals default to `Low`.
#[must_use]
pub fn select(is_subagent: bool, last_msg: &str) -> ReasoningEffort {
    if is_subagent {
        return ReasoningEffort::Low;
    }

    let lower = last_msg.to_ascii_lowercase();

    // Quick exit for empty input.
    if lower.is_empty() {
        return ReasoningEffort::Low;
    }

    let mut score: i32 = 0;

    for pattern in MAX_PATTERNS {
        if pattern.keywords.iter().any(|kw| lower.contains(kw)) {
            score += pattern.score;
        }
    }

    for pattern in LOW_PATTERNS {
        if pattern.keywords.iter().any(|kw| lower.contains(kw)) {
            score += pattern.score;
        }
    }

    // Short messages with no strong signals are likely simple queries.
    if last_msg.len() < 30 && score == 0 {
        return ReasoningEffort::Low;
    }

    if score >= MAX_THRESHOLD {
        ReasoningEffort::Max
    } else if score <= LOW_CEILING {
        ReasoningEffort::Low
    } else {
        ReasoningEffort::High
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Sub-agent tests ===

    #[test]
    fn subagent_returns_low() {
        assert_eq!(select(true, "anything"), ReasoningEffort::Low);
        assert_eq!(select(true, "debug this"), ReasoningEffort::Low);
        assert_eq!(select(true, "search query"), ReasoningEffort::Low);
    }

    // === Max-tier tests ===

    #[test]
    fn debug_returns_max() {
        assert_eq!(select(false, "debug crash in the auth module"), ReasoningEffort::Max);
        assert_eq!(select(false, "debugging session"), ReasoningEffort::Max);
    }

    #[test]
    fn error_with_context_returns_max() {
        assert_eq!(select(false, "Error: timeout in connection pool"), ReasoningEffort::Max);
        assert_eq!(select(false, "fix this error in the parser"), ReasoningEffort::Max);
        assert_eq!(select(false, "DEBUG output shows err: segfault"), ReasoningEffort::Max);
    }

    #[test]
    fn crash_returns_max() {
        assert_eq!(select(false, "the app is crashing on startup"), ReasoningEffort::Max);
    }

    #[test]
    fn security_returns_max() {
        assert_eq!(select(false, "check for security vulnerabilities"), ReasoningEffort::Max);
        assert_eq!(select(false, "prevent xss injection in the form"), ReasoningEffort::Max);
    }

    #[test]
    fn race_condition_returns_max() {
        assert_eq!(select(false, "fix the race condition in worker threads"), ReasoningEffort::Max);
    }

    #[test]
    fn architecture_returns_max() {
        // architecture (2) + refactor (2) = 4 >= MAX_THRESHOLD
        assert_eq!(select(false, "refactor the architecture of the auth system"), ReasoningEffort::Max);
    }

    #[test]
    fn performance_debug_returns_max() {
        // performance (2) + debug (3) = 5 >= MAX_THRESHOLD
        assert_eq!(select(false, "debug the performance bottleneck"), ReasoningEffort::Max);
    }

    // === High-tier tests ===

    #[test]
    fn moderate_task_returns_high() {
        assert_eq!(select(false, "refactor this module to use async"), ReasoningEffort::High);
        assert_eq!(select(false, "optimize the database queries"), ReasoningEffort::High);
    }

    #[test]
    fn concurrent_coding_returns_high() {
        assert_eq!(select(false, "add async support to the client"), ReasoningEffort::High);
    }

    // === Low-tier tests ===

    #[test]
    fn search_returns_low() {
        assert_eq!(select(false, "search for the file"), ReasoningEffort::Low);
        assert_eq!(select(false, "lookup docs"), ReasoningEffort::Low);
        assert_eq!(select(false, "grep for TODO comments"), ReasoningEffort::Low);
    }

    #[test]
    fn simple_query_returns_low() {
        assert_eq!(select(false, "what is Rust"), ReasoningEffort::Low);
        assert_eq!(select(false, "list files"), ReasoningEffort::Low);
        assert_eq!(select(false, "show me the config"), ReasoningEffort::Low);
    }

    #[test]
    fn short_message_returns_low() {
        assert_eq!(select(false, "hello"), ReasoningEffort::Low);
        assert_eq!(select(false, "hi"), ReasoningEffort::Low);
        assert_eq!(select(false, ""), ReasoningEffort::Low);
    }

    #[test]
    fn file_ops_return_low() {
        assert_eq!(select(false, "rename main.rs to lib.rs"), ReasoningEffort::Low);
        assert_eq!(select(false, "read the config file"), ReasoningEffort::Low);
    }

    // === Edge cases ===

    #[test]
    fn mixed_signals_prefer_dominant() {
        // search (-2) + error (2) = 0 -> High
        assert_eq!(select(false, "search for the error in logs"), ReasoningEffort::High);
    }

    #[test]
    fn long_message_without_patterns_returns_high() {
        let long_msg = "please help me understand how this code works and explain the flow";
        assert_eq!(select(false, long_msg), ReasoningEffort::High);
    }
}
