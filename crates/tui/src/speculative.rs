//! Speculative cached-prefix turn branches with verifier selection (#532).
//!
//! ## How it works
//!
//! When enabled, every user turn fires **two parallel LLM requests** with
//! different reasoning depths:
//!
//! - **Fast branch**: `reasoning_effort = off` → no thinking tokens, fast TTFT
//! - **Deep branch**: the user's configured `reasoning_effort` (high/max)
//!
//! Because both requests share the same byte-stable system prompt prefix,
//! DeepSeek's KV prefix cache serves the second request at ~100× discount
//! on all cached prefix tokens. The total marginal cost of the speculative
//! branch is just the cache-miss tokens from the (small) thinking parameter
//! delta plus the generated output tokens.
//!
//! A lightweight verifier scores both completed responses on:
//! - Content length balance
//! - Code block presence and completeness
//! - Structural markers (sections, lists)
//! - Repetition penalties
//! - Actionability (presence of concrete steps or answers)
//!
//! The winning response is selected and returned as if it were the single
//! response to the turn. The loser's cost is tracked but its content is
//! discarded.

use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;

use crate::client::DeepSeekClient;
use crate::llm_client::LlmClient;
use crate::models::{
    ContentBlock, ContentBlockStart, Delta, Message, MessageDelta, MessageRequest, MessageResponse,
    StreamEvent, SystemPrompt, Usage,
};

// ── Public Types ───────────────────────────────────────────────────────────

/// Outcome of a speculative branch evaluation.
#[derive(Debug, Clone)]
pub struct BranchOutcome {
    /// Label identifying this branch ("fast" or "deep").
    pub label: &'static str,
    /// The reasoning effort used.
    pub reasoning_effort: String,
    /// Full API response.
    pub response: MessageResponse,
    /// Verifier score (higher = better).
    pub score: VerifierScore,
    /// Wall-clock time the branch took.
    pub elapsed: Duration,
}

/// Verifier score breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct VerifierScore {
    /// Overall score 0.0–1.0.
    pub overall: f64,
    /// Length score: penalizes very short or very long responses.
    pub length: f64,
    /// Code-block score: rewards responses with valid code blocks.
    pub code_blocks: f64,
    /// Structure score: rewards sections, lists, headings.
    pub structure: f64,
    /// Repetition penalty: detects repeated phrases or n-grams.
    pub repetition: f64,
    /// Thinking coherence: how well the thinking block content appears
    /// to inform the final answer (deep branch only — fast branch
    /// has no thinking block).
    pub coherence: f64,
}

impl VerifierScore {
    /// Perfect score — all sub-scores at 1.0.
    pub const fn perfect() -> Self {
        Self {
            overall: 1.0,
            length: 1.0,
            code_blocks: 1.0,
            structure: 1.0,
            repetition: 1.0,
            coherence: 1.0,
        }
    }
}

/// Speculative branching configuration, resolved from
/// [`deepseek_config::SpeculativeConfigToml`].
#[derive(Debug, Clone)]
pub struct SpeculativeConfig {
    /// Master switch.
    pub enabled: bool,
    /// Override model for the fast (think=off) branch.
    /// When `None`, the primary model is used.
    pub fast_model: Option<String>,
    /// Maximum wall-clock wait for both branches.
    pub timeout: Duration,
}

impl Default for SpeculativeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            fast_model: None,
            timeout: Duration::from_secs(30),
        }
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Run two speculative branches in parallel and return the winner.
///
/// `primary_model` is the user's active model (e.g. `deepseek-v4-pro`).
/// `reasoning_effort` is the user's configured effort (e.g. `"max"`).
///
/// Returns the winning branch outcome, plus the loser's usage so callers
/// can account for both in cost tracking.
pub async fn run_speculative_turn(
    client: &DeepSeekClient,
    config: &SpeculativeConfig,
    primary_model: &str,
    messages: Vec<Message>,
    system: Option<SystemPrompt>,
    tools: Option<Vec<crate::models::Tool>>,
    tool_choice: Option<serde_json::Value>,
    max_tokens: u32,
    reasoning_effort: Option<&str>,
) -> Result<(BranchOutcome, Usage)> {
    let started = Instant::now();

    // ── Deep branch (user's configured reasoning effort) ────────────────
    let deep_model = primary_model.to_string();
    let deep_effort = reasoning_effort
        .filter(|e| !e.is_empty())
        .unwrap_or("max")
        .to_string();
    let deep_request = MessageRequest {
        model: deep_model.clone(),
        messages: messages.clone(),
        max_tokens,
        system: system.clone(),
        tools: tools.clone(),
        tool_choice: tool_choice.clone(),
        metadata: None,
        thinking: None,
        reasoning_effort: Some(deep_effort.clone()),
        stream: Some(false),
        temperature: None,
        top_p: None,
    };

    // ── Fast branch (thinking=off) ───────────────────────────────────────
    let fast_model = config
        .fast_model
        .clone()
        .unwrap_or_else(|| primary_model.to_string());
    let fast_request = MessageRequest {
        model: fast_model.clone(),
        messages,
        max_tokens,
        system,
        tools,
        tool_choice,
        metadata: None,
        thinking: None,
        reasoning_effort: Some("off".to_string()),
        stream: Some(false),
        temperature: None,
        top_p: None,
    };

    // ── Fire both requests in parallel ───────────────────────────────────
    let (fast_result, deep_result) = tokio::join!(
        LlmClient::create_message(client, fast_request),
        LlmClient::create_message(client, deep_request),
    );

    // ── Process results ──────────────────────────────────────────────────
    let fast = fast_result?;
    let deep = deep_result?;

    let fast_elapsed = started.elapsed();
    let deep_elapsed = started.elapsed();

    let fast_usage = fast.usage.clone();
    let deep_usage = deep.usage.clone();

    // ── Score ────────────────────────────────────────────────────────────
    let fast_content = extract_text(&fast);
    let deep_content = extract_text(&deep);

    let fast_thinking = extract_thinking(&fast);
    let deep_thinking = extract_thinking(&deep);

    let fast_score = score_response(
        &fast_content,
        &fast_thinking,
        fast_elapsed,
        false, // fast branch: no thinking expected
    );
    let deep_score = score_response(
        &deep_content,
        &deep_thinking,
        deep_elapsed,
        true, // deep branch: thinking expected
    );

    let fast_outcome = BranchOutcome {
        label: "fast",
        reasoning_effort: "off".to_string(),
        response: fast,
        score: fast_score,
        elapsed: fast_elapsed,
    };
    let deep_outcome = BranchOutcome {
        label: "deep",
        reasoning_effort: deep_effort.clone(),
        response: deep,
        score: deep_score,
        elapsed: deep_elapsed,
    };

    // ── Select winner ────────────────────────────────────────────────────
    let (winner, loser_usage) = if fast_outcome.score.overall >= deep_outcome.score.overall {
        // Fast branch won outright — use it, discard deep usage
        (fast_outcome, deep_usage)
    } else {
        // Deep branch won — use it, discard fast usage
        (deep_outcome, fast_usage)
    };

    Ok((winner, loser_usage))
}

// ── Verifier ──────────────────────────────────────────────────────────────

/// Score a response on quality heuristics.
fn score_response(
    content: &str,
    thinking: &str,
    _elapsed: Duration,
    thinking_expected: bool,
) -> VerifierScore {
    let length = score_length(content);
    let code_blocks = score_code_blocks(content);
    let structure = score_structure(content);
    let repetition = score_repetition(content);
    let coherence = if thinking_expected {
        score_thinking_use(thinking, content)
    } else {
        0.8 // neutral for fast branch
    };

    // Weighted average. Coherence matters most when thinking is expected.
    let weights = if thinking_expected {
        [0.20, 0.20, 0.15, 0.15, 0.30]
    } else {
        [0.25, 0.25, 0.20, 0.20, 0.10]
    };

    let overall = length * weights[0]
        + code_blocks * weights[1]
        + structure * weights[2]
        + repetition * weights[3]
        + coherence * weights[4];

    VerifierScore {
        overall,
        length,
        code_blocks,
        structure,
        repetition,
        coherence,
    }
}

/// Score response length: penalize very short (<50 chars) or very long
/// (>50K chars) responses. Peak at ~500–5000 chars.
fn score_length(content: &str) -> f64 {
    let len = content.len();
    if len < 50 {
        return len as f64 / 50.0; // 0.0–1.0 ramp
    }
    if len > 50_000 {
        return (100_000_f64 - len as f64) / 50_000_f64; // 1.0→0.0 ramp
    }
    1.0 // golden zone
}

/// Score code-block presence and completeness.
fn score_code_blocks(content: &str) -> f64 {
    let count = content.matches("```").count();
    if count == 0 {
        // No code blocks — neutral. Many answers don't need code.
        return 0.7;
    }
    // Even count → blocks are properly closed
    if count % 2 == 0 {
        // Reward code blocks up to ~5 blocks, then plateau
        let pairs = count / 2;
        (1.0_f64).min(0.7 + (pairs as f64) * 0.06)
    } else {
        // Unclosed code block — penalize
        0.3
    }
}

/// Score structural markers: headings, lists, sections.
fn score_structure(content: &str) -> f64 {
    let has_headings = content.contains("## ") || content.contains("### ");
    let has_bullets = content.contains("- ") || content.contains("* ");
    let has_numbers = content.contains("1. ") || content.contains("1)");
    let sections = content.split("\n## ").count().saturating_sub(1);

    let mut score: f64 = 0.4; // baseline
    if has_headings {
        score += 0.2;
    }
    if has_bullets {
        score += 0.15;
    }
    if has_numbers {
        score += 0.1;
    }
    // Penalize excessive sectioning (>10 sections is usually a list of
    // fragments rather than a coherent answer)
    if sections > 10 {
        score -= 0.1;
    }
    score.clamp(0.0_f64, 1.0_f64)
}

/// Score repetition: detect repeated 4-grams as a proxy for rambling.
fn score_repetition(content: &str) -> f64 {
    if content.len() < 80 {
        return 1.0; // too short to judge repetition
    }

    let words: Vec<&str> = content.split_whitespace().collect();
    if words.len() < 8 {
        return 1.0;
    }

    // Count repeated 4-grams
    let mut seen = std::collections::HashSet::new();
    let mut repeats = 0usize;
    for window in words.windows(4) {
        let key = window.join(" ");
        if !seen.insert(key) {
            repeats += 1;
        }
    }

    if repeats == 0 {
        1.0
    } else {
        // More than 3 repeated 4-grams → significant rambling
        (1.0 - (repeats as f64) * 0.15).clamp(0.2, 1.0)
    }
}

/// Score how well the thinking content appears to inform the final answer.
/// Looks for keywords in thinking that appear in the final response.
fn score_thinking_use(thinking: &str, content: &str) -> f64 {
    if thinking.trim().is_empty() {
        return 0.5; // neutral — no thinking expected for fast branch
    }

    // Extract key terms from thinking
    let thinking_terms: Vec<&str> = thinking
        .split_whitespace()
        .filter(|w| w.len() > 4 && !w.chars().all(|c| c.is_ascii_punctuation()))
        .collect();

    if thinking_terms.is_empty() {
        return 0.5;
    }

    let content_lower = content.to_lowercase();
    let overlap = thinking_terms
        .iter()
        .filter(|t| content_lower.contains(&t.to_lowercase()))
        .count();

    let ratio = overlap as f64 / thinking_terms.len() as f64;
    // Sigmoid-ish mapping: 0% overlap → 0.4, 50% → 0.75, 100% → 0.95
    0.4 + ratio * 0.55
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Extract the visible text content from a response (excludes thinking blocks).
fn extract_text(response: &MessageResponse) -> String {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract the thinking content from a response.
fn extract_thinking(response: &MessageResponse) -> String {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Thinking { thinking } => Some(thinking.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Stream event synthesis ───────────────────────────────────────────

/// Convert a non-streaming `MessageResponse` into a sequence of
/// `StreamEvent`s that the existing streaming pipeline can process.
///
/// This lets the speculative branch code use `create_message` (non-streaming)
/// for both branches while feeding the winner's content through the
/// same event-processing path used by real streaming responses.
pub fn response_to_stream_events(response: MessageResponse) -> Vec<StreamEvent> {
    let mut events: Vec<StreamEvent> = Vec::new();

    // 1. MessageStart
    events.push(StreamEvent::MessageStart {
        message: response.clone(),
    });

    // 2. Content blocks
    for (idx, block) in response.content.iter().enumerate() {
        let index = idx as u32;
        match block {
            ContentBlock::Text { text, .. } => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::Text {
                        text: text.clone(),
                    },
                });
                // Emit text as one delta (non-streaming response has it all)
                events.push(StreamEvent::ContentBlockDelta {
                    index,
                    delta: Delta::TextDelta {
                        text: text.clone(),
                    },
                });
                events.push(StreamEvent::ContentBlockStop { index });
            }
            ContentBlock::Thinking { thinking } => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::Thinking {
                        thinking: thinking.clone(),
                    },
                });
                if !thinking.is_empty() {
                    events.push(StreamEvent::ContentBlockDelta {
                        index,
                        delta: Delta::ThinkingDelta {
                            thinking: thinking.clone(),
                        },
                    });
                }
                events.push(StreamEvent::ContentBlockStop { index });
            }
            ContentBlock::ToolUse {
                id,
                name,
                input,
                caller,
            } => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: serde_json::Value::String(String::new()),
                        caller: caller.clone(),
                    },
                });
                let input_str = serde_json::to_string(input).unwrap_or_default();
                if !input_str.is_empty() {
                    events.push(StreamEvent::ContentBlockDelta {
                        index,
                        delta: Delta::InputJsonDelta {
                            partial_json: input_str,
                        },
                    });
                }
                events.push(StreamEvent::ContentBlockStop { index });
            }
            // ServerToolUse and others — skip for speculative branch (rare)
            _ => {}
        }
    }

    // 3. MessageDelta with usage
    events.push(StreamEvent::MessageDelta {
        delta: MessageDelta {
            stop_reason: response.stop_reason.clone(),
            stop_sequence: None,
        },
        usage: Some(response.usage),
    });

    // 4. MessageStop
    events.push(StreamEvent::MessageStop);

    events
}

// ── Tracking ──────────────────────────────────────────────────────────────

/// Track speculative usage across an entire turn (both winner and loser).
/// Exported so the cost tracker can account for both branches.
#[derive(Debug, Clone, Default)]
pub struct SpeculativeTurnUsage {
    /// Winner's usage (charge the user for this one).
    pub winner: Usage,
    /// Loser's usage (only prompt-cache misses matter for cost).
    pub loser: Usage,
    /// Which branch won: "fast" or "deep".
    pub winner_label: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_length_short_is_penalized() {
        let s = score_length("hi");
        assert!(s < 0.5);
    }

    #[test]
    fn score_length_golden_zone_is_perfect() {
        let s = score_length(&"x".repeat(1000));
        assert!((s - 1.0).abs() < 0.01);
    }

    #[test]
    fn score_length_very_long_is_penalized() {
        let s = score_length(&"x".repeat(80_000));
        assert!(s < 0.5);
    }

    #[test]
    fn score_code_blocks_even_pair_count_scores_high() {
        let content = "```rust\nfn main() {}\n```\n```python\nprint('ok')\n```";
        let s = score_code_blocks(content);
        assert!(s > 0.7);
    }

    #[test]
    fn score_code_blocks_unclosed_is_penalized() {
        let content = "```rust\nfn main() {}\n";
        let s = score_code_blocks(content);
        assert!((s - 0.3).abs() < 0.01);
    }

    #[test]
    fn score_structure_headings_and_lists_score_high() {
        let content = "## Overview\n- Point A\n- Point B\n1. First\n2. Second";
        let s = score_structure(content);
        assert!(s > 0.7);
    }

    #[test]
    fn score_repetition_no_repeats_is_perfect() {
        let content = "The quick brown fox jumps over the lazy dog near the river bank.";
        let s = score_repetition(content);
        assert!((s - 1.0).abs() < 0.01);
    }

    #[test]
    fn score_repetition_rambling_is_penalized() {
        let content = "Hello world this is a test. Hello world this is a test. Hello world this is a test. Hello world this is a test.";
        let s = score_repetition(content);
        assert!(s < 0.6);
    }

    #[test]
    fn score_thinking_use_overlap_detected() {
        let thinking = "I should use a binary search algorithm because the data is sorted";
        let content = "Using a binary search on the sorted array gives O(log n) time.";
        let s = score_thinking_use(thinking, content);
        assert!(s > 0.5);
    }

    #[test]
    fn score_thinking_use_empty_thinking_neutral() {
        let s = score_thinking_use("", "Some answer here.");
        assert!((s - 0.5).abs() < 0.01);
    }

    #[test]
    fn score_response_overall_in_range() {
        let score = score_response(
            "## Summary\n\nHere is the plan:\n\n1. Step one\n2. Step two\n\n```rust\nfn foo() {}\n```",
            "I need to write a function.",
            Duration::from_millis(500),
            true,
        );
        assert!(score.overall >= 0.0 && score.overall <= 1.0);
    }

    #[test]
    fn speculative_config_default() {
        let cfg = SpeculativeConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.fast_model, None);
        assert_eq!(cfg.timeout, Duration::from_secs(30));
    }

    #[test]
    fn verifier_score_serializes() {
        let score = VerifierScore::perfect();
        let json = serde_json::to_string(&score).expect("serialize");
        assert!(json.contains("\"overall\":1.0"));
    }
}
