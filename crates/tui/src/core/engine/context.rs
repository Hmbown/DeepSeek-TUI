//! Context budgeting and prompt-shaping helpers for the engine.
//!
//! These functions are shared by the streaming turn loop, capacity flow, and
//! engine session maintenance code. Keeping them here prevents the top-level
//! engine module from accumulating unrelated context-policy details.

use crate::compaction::estimate_tokens;
use crate::error_taxonomy::ErrorCategory;
use crate::models::{Message, SystemBlock, SystemPrompt, context_window_for_model};
use crate::tools::spec::ToolResult;

/// Max output tokens requested for normal agent turns. Generous on purpose:
/// V4 thinking models can produce tens of thousands of reasoning tokens on
/// hard prompts before the visible reply, and DeepSeek V4 ships with a 1M
/// context window. v0.7.5 keeps this cap fixed instead of silently lowering
/// `max_tokens` near pressure; hard-cycle/preflight checks reserve this budget
/// plus safety headroom before sending the next request.
pub(super) const TURN_MAX_OUTPUT_TOKENS: u32 = 262_144;
/// Keep this many most recent messages when emergency trimming is required.
pub(super) const MIN_RECENT_MESSAGES_TO_KEEP: usize = 4;
/// Allow a few emergency recovery attempts before failing the turn.
pub(super) const MAX_CONTEXT_RECOVERY_ATTEMPTS: u8 = 2;
/// Reserve additional headroom to avoid hitting provider hard limits.
const CONTEXT_HEADROOM_TOKENS: usize = 1024;
/// Hard cap for any tool output inserted into model context.
const TOOL_RESULT_CONTEXT_HARD_LIMIT_CHARS: usize = 12_000;
/// Soft cap for known noisy tools inserted into model context.
const TOOL_RESULT_CONTEXT_SOFT_LIMIT_CHARS: usize = 2_000;
/// Snippet length kept when compacting tool output for model context.
const TOOL_RESULT_CONTEXT_SNIPPET_CHARS: usize = 900;
/// Hard cap for tool output inserted into a large-context model.
const LARGE_CONTEXT_TOOL_RESULT_HARD_LIMIT_CHARS: usize = 180_000;
/// Soft cap for known noisy tools inserted into a large-context model.
const LARGE_CONTEXT_TOOL_RESULT_SOFT_LIMIT_CHARS: usize = 60_000;
/// Snippet length kept when compacting large-context tool output.
const LARGE_CONTEXT_TOOL_RESULT_SNIPPET_CHARS: usize = 40_000;
/// Context window size at which tool output limits can be relaxed.
const LARGE_CONTEXT_WINDOW_TOKENS: u32 = 500_000;
/// Max chars to keep from metadata-provided output summaries.
const TOOL_RESULT_METADATA_SUMMARY_CHARS: usize = 320;

pub(super) const COMPACTION_SUMMARY_MARKER: &str = "Conversation Summary (Auto-Generated)";
pub(super) const WORKING_SET_SUMMARY_MARKER: &str = "## Repo Working Set";

#[derive(Debug, Clone, Copy)]
struct ToolResultContextLimits {
    hard_limit_chars: usize,
    noisy_soft_limit_chars: usize,
    snippet_chars: usize,
}

pub(super) fn summarize_text(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let take = limit.saturating_sub(3);
    let mut out: String = text.chars().take(take).collect();
    out.push_str("...");
    out
}

fn summarize_text_head_tail(text: &str, limit: usize) -> String {
    let total = text.chars().count();
    if total <= limit {
        return text.to_string();
    }
    if limit <= 20 {
        return summarize_text(text, limit);
    }

    let marker = "\n\n[... output truncated for context ...]\n\n";
    let marker_len = marker.chars().count();
    if limit <= marker_len + 20 {
        return summarize_text(text, limit);
    }

    let remaining = limit - marker_len;
    let head_len = remaining.saturating_mul(2) / 3;
    let tail_len = remaining.saturating_sub(head_len);
    let head: String = text.chars().take(head_len).collect();
    let tail_vec: Vec<char> = text.chars().rev().take(tail_len).collect();
    let tail: String = tail_vec.into_iter().rev().collect();
    format!("{head}{marker}{tail}")
}

fn tool_result_is_noisy(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "exec_shell"
            | "exec_shell_wait"
            | "exec_shell_interact"
            | "multi_tool_use.parallel"
            | "web_search"
    )
}

fn tool_result_metadata_summary(metadata: Option<&serde_json::Value>) -> Option<String> {
    let obj = metadata?.as_object()?;
    for key in ["summary", "stdout_summary", "stderr_summary", "message"] {
        if let Some(text) = obj.get(key).and_then(serde_json::Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(summarize_text(trimmed, TOOL_RESULT_METADATA_SUMMARY_CHARS));
            }
        }
    }
    None
}

fn tool_result_context_limits_for_model(model: &str) -> ToolResultContextLimits {
    let is_large_context =
        context_window_for_model(model).is_some_and(|window| window >= LARGE_CONTEXT_WINDOW_TOKENS);

    if is_large_context {
        ToolResultContextLimits {
            hard_limit_chars: LARGE_CONTEXT_TOOL_RESULT_HARD_LIMIT_CHARS,
            noisy_soft_limit_chars: LARGE_CONTEXT_TOOL_RESULT_SOFT_LIMIT_CHARS,
            snippet_chars: LARGE_CONTEXT_TOOL_RESULT_SNIPPET_CHARS,
        }
    } else {
        ToolResultContextLimits {
            hard_limit_chars: TOOL_RESULT_CONTEXT_HARD_LIMIT_CHARS,
            noisy_soft_limit_chars: TOOL_RESULT_CONTEXT_SOFT_LIMIT_CHARS,
            snippet_chars: TOOL_RESULT_CONTEXT_SNIPPET_CHARS,
        }
    }
}

pub(crate) fn compact_tool_result_for_context(
    model: &str,
    tool_name: &str,
    output: &ToolResult,
) -> String {
    let raw = output.content.trim();
    if raw.is_empty() {
        return String::new();
    }

    let limits = tool_result_context_limits_for_model(model);
    let raw_chars = raw.chars().count();
    let should_compact = raw_chars > limits.hard_limit_chars
        || (tool_result_is_noisy(tool_name) && raw_chars > limits.noisy_soft_limit_chars);
    if !should_compact {
        return raw.to_string();
    }

    let snippet = summarize_text_head_tail(raw, limits.snippet_chars);
    let omitted = raw_chars.saturating_sub(snippet.chars().count());
    let summary = tool_result_metadata_summary(output.metadata.as_ref());

    if let Some(summary) = summary {
        format!(
            "[{tool_name} output compacted to protect context]\nSummary: {summary}\nSnippet: {snippet}\n(Original: {raw_chars} chars, omitted: {omitted} chars.)"
        )
    } else {
        format!(
            "[{tool_name} output compacted to protect context]\nSnippet: {snippet}\n(Original: {raw_chars} chars, omitted: {omitted} chars.)"
        )
    }
}

pub(super) fn extract_compaction_summary_prompt(
    prompt: Option<SystemPrompt>,
) -> Option<SystemPrompt> {
    match prompt {
        Some(SystemPrompt::Blocks(blocks)) => {
            let summary_blocks: Vec<_> = blocks
                .into_iter()
                .filter(|block| block.text.contains(COMPACTION_SUMMARY_MARKER))
                .collect();
            if summary_blocks.is_empty() {
                None
            } else {
                Some(SystemPrompt::Blocks(summary_blocks))
            }
        }
        Some(SystemPrompt::Text(text)) => {
            if text.contains(COMPACTION_SUMMARY_MARKER) {
                Some(SystemPrompt::Text(text))
            } else {
                None
            }
        }
        None => None,
    }
}

pub(super) fn remove_working_set_summary(prompt: Option<&SystemPrompt>) -> Option<SystemPrompt> {
    match prompt {
        Some(SystemPrompt::Blocks(blocks)) => {
            let filtered: Vec<SystemBlock> = blocks
                .iter()
                .filter(|block| !block.text.contains(WORKING_SET_SUMMARY_MARKER))
                .cloned()
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some(SystemPrompt::Blocks(filtered))
            }
        }
        Some(SystemPrompt::Text(text)) => Some(SystemPrompt::Text(text.clone())),
        None => None,
    }
}

pub(super) fn append_working_set_summary(
    prompt: Option<SystemPrompt>,
    working_set_summary: Option<&str>,
) -> Option<SystemPrompt> {
    let Some(summary) = working_set_summary.map(str::trim).filter(|s| !s.is_empty()) else {
        return prompt;
    };
    let working_set_block = SystemBlock {
        block_type: "text".to_string(),
        text: summary.to_string(),
        cache_control: None,
    };

    match prompt {
        Some(SystemPrompt::Text(text)) => Some(SystemPrompt::Blocks(vec![
            SystemBlock {
                block_type: "text".to_string(),
                text,
                cache_control: None,
            },
            working_set_block,
        ])),
        Some(SystemPrompt::Blocks(mut blocks)) => {
            blocks.retain(|block| !block.text.contains(WORKING_SET_SUMMARY_MARKER));
            blocks.push(working_set_block);
            Some(SystemPrompt::Blocks(blocks))
        }
        None => Some(SystemPrompt::Blocks(vec![working_set_block])),
    }
}

fn estimate_text_tokens_conservative(text: &str) -> usize {
    text.chars().count().div_ceil(3)
}

fn estimate_system_tokens_conservative(system: Option<&SystemPrompt>) -> usize {
    match system {
        Some(SystemPrompt::Text(text)) => estimate_text_tokens_conservative(text),
        Some(SystemPrompt::Blocks(blocks)) => blocks
            .iter()
            .map(|block| estimate_text_tokens_conservative(&block.text))
            .sum(),
        None => 0,
    }
}

pub(super) fn estimate_input_tokens_conservative(
    messages: &[Message],
    system: Option<&SystemPrompt>,
) -> usize {
    let message_tokens = estimate_tokens(messages).saturating_mul(3).div_ceil(2);
    let system_tokens = estimate_system_tokens_conservative(system);
    let framing_overhead = messages.len().saturating_mul(12).saturating_add(48);
    message_tokens
        .saturating_add(system_tokens)
        .saturating_add(framing_overhead)
}

pub(super) fn context_input_budget(model: &str, requested_output_tokens: u32) -> Option<usize> {
    let window = usize::try_from(context_window_for_model(model)?).ok()?;
    let output = usize::try_from(requested_output_tokens).ok()?;
    window
        .checked_sub(output)
        .and_then(|v| v.checked_sub(CONTEXT_HEADROOM_TOKENS))
}

pub(super) fn turn_response_headroom_tokens() -> u64 {
    u64::from(TURN_MAX_OUTPUT_TOKENS).saturating_add(CONTEXT_HEADROOM_TOKENS as u64)
}

pub(super) fn is_context_length_error_message(message: &str) -> bool {
    crate::error_taxonomy::classify_error_message(message) == ErrorCategory::InvalidInput
}
