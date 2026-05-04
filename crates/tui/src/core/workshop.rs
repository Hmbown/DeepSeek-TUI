//! # Workshop Agent — persistent tool-output summarizer (#547)
//!
//! The workshop agent is a lightweight persistent component that lives
//! across turns in the engine. When a tool produces output larger than
//! 4 KB, it receives the raw output, uses the LLM to produce a concise
//! summary, and returns only the summary to the parent context — protecting
//! the 1 M-token context window from filling with large raw results.
//!
//! ## Design
//!
//! - **Not a sub-agent.** The workshop makes a direct non-streaming LLM
//!   chat-completion call with a narrow summarization prompt. This avoids
//!   sub-agent lifecycle overhead and keeps the summarization fast.
//! - **Persistent across turns.** The workshop lives as a field on `Engine`.
//!   Its running summary log accumulates across steps and turns so repeated
//!   tool outputs (same file, same shell) produce recognisable summaries.
//! - **Graceful fallback.** If the LLM call fails (API error, timeout), the
//!   workshop returns `None` and the caller falls through to the existing
//!   head/tail truncation path.
//! - **Silent.** The workshop does not emit UI events, tool calls visible to
//!   the parent model, or sub-agent cards. It is an invisible context filter.

use std::collections::VecDeque;

use crate::client::DeepSeekClient;
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt};

// ── Constants ────────────────────────────────────────────────────────────────

/// Threshold in characters above which workshop LLM summarization kicks in.
/// 4 KB ≈ 4096 chars of ASCII text.
const WORKSHOP_CHAR_THRESHOLD: usize = 4096;

/// Max entries in the rolling summary log before oldest are trimmed.
const MAX_SUMMARY_LOG: usize = 32;

/// Max chars for a single summary produced by the workshop LLM.
const MAX_SUMMARY_CHARS: usize = 800;

/// Model used for workshop summarization calls. Flash is cheap and fast
/// enough for summarization; no need to use the main thinking model.
const WORKSHOP_MODEL: &str = "deepseek-chat";

/// Token budget for the workshop summary response.
const WORKSHOP_MAX_TOKENS: u32 = 512;

// ── Types ────────────────────────────────────────────────────────────────────

/// A single entry in the workshop's running summary log.
#[derive(Debug, Clone)]
pub struct WorkshopEntry {
    /// The tool that produced the output.
    pub tool_name: String,
    /// The LLM-generated summary.
    pub summary: String,
}

/// Persistent workshop agent.
///
/// Constructed once in `Engine::new` and lives for the session lifetime.
/// When the engine is shut down, the workshop is dropped — its summary
/// log is transient and does not survive restarts.
#[derive(Debug)]
pub struct WorkshopAgent {
    /// Rolling log of summaries produced so far.
    summary_log: VecDeque<WorkshopEntry>,
}

impl WorkshopAgent {
    /// Create a new empty workshop agent.
    #[must_use]
    pub fn new() -> Self {
        Self {
            summary_log: VecDeque::with_capacity(MAX_SUMMARY_LOG.saturating_add(1)),
        }
    }

    /// The character threshold above which workshop summarization triggers.
    #[must_use]
    pub fn threshold(&self) -> usize {
        WORKSHOP_CHAR_THRESHOLD
    }

    /// Whether the given raw output exceeds the workshop threshold.
    #[must_use]
    pub fn should_summarize(&self, raw: &str) -> bool {
        raw.chars().count() >= WORKSHOP_CHAR_THRESHOLD
    }

    /// Summarise a large tool output using the DeepSeek chat API.
    ///
    /// Returns `Some(summary)` on success. On failure (API error, empty
    /// response) returns `None` — caller should fall back to standard
    /// head/tail truncation.
    ///
    /// The summary is recorded in the workshop's rolling log so repeated
    /// outputs from the same tool are noted.
    pub async fn summarize(
        &mut self,
        client: &DeepSeekClient,
        tool_name: &str,
        raw_content: &str,
    ) -> Option<String> {
        if !self.should_summarize(raw_content) {
            return None;
        }

        let summary = self.call_summarize_llm(client, tool_name, raw_content).await?;
        self.record_entry(tool_name, &summary);
        Some(summary)
    }

    /// Internal LLM call for summarization.
    async fn call_summarize_llm(
        &self,
        client: &DeepSeekClient,
        tool_name: &str,
        raw_content: &str,
    ) -> Option<String> {
        // Compute input statistics for the prompt.
        let raw_chars = raw_content.chars().count();
        let raw_lines = raw_content.lines().count();
        let preview: String = raw_content.chars().take(240).collect();
        let preview_suffix = if raw_chars > 240 { "…" } else { "" };

        // Build a tight summarization prompt.
        let system_prompt = SystemPrompt::Text(
            "You are a precise tool-output summarizer. Your summaries are \
             concise, factual, and lossy only in detail level — never \
             distort the meaning. Use at most 800 characters."
                .to_string(),
        );

        let user_prompt = format!(
            "Summarise this tool output from `{tool_name}` \
             ({raw_chars} chars, {raw_lines} lines). \
             Start with the key result or error; include counts, \
             paths, or numbers where they matter.\n\
             \n\
             First 240 chars of output:\n\
             {preview}{preview_suffix}\n\
             \n\
             --- Full output follows ---\n\
             {raw_content}\n\
             --- End of output ---"
        );

        let request = MessageRequest {
            model: WORKSHOP_MODEL.to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: vec![ContentBlock::Text {
                        text: match &system_prompt {
                            SystemPrompt::Text(t) => t.clone(),
                            SystemPrompt::Blocks(_) => String::new(),
                        },
                        cache_control: None,
                    }],
                },
                Message {
                    role: "user".to_string(),
                    content: vec![ContentBlock::Text {
                        text: user_prompt,
                        cache_control: None,
                    }],
                },
            ],
            max_tokens: WORKSHOP_MAX_TOKENS,
            system: Some(system_prompt),
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: Some(false),
            temperature: Some(0.3),
            top_p: None,
        };

        match client.create_message(request).await {
            Ok(response) => {
                // Extract the assistant's text reply.
                let text: String = response
                    .content
                    .into_iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text, .. } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();

                if text.is_empty() {
                    return None;
                }

                // Enforce max summary length.
                let mut summary: String = text.chars().take(MAX_SUMMARY_CHARS).collect();
                if text.chars().count() > MAX_SUMMARY_CHARS {
                    summary.push_str("…");
                }
                Some(summary)
            }
            Err(err) => {
                crate::logging::warn(format!(
                    "Workshop summarization failed for {tool_name}: {err}"
                ));
                None
            }
        }
    }

    /// Record a summary in the rolling log, trimming oldest entries.
    fn record_entry(&mut self, tool_name: &str, summary: &str) {
        if self.summary_log.len() >= MAX_SUMMARY_LOG {
            self.summary_log.pop_front();
        }
        self.summary_log.push_back(WorkshopEntry {
            tool_name: tool_name.to_string(),
            summary: summary.to_string(),
        });
    }

    /// Read-only access to the summary log.
    #[must_use]
    pub fn summary_log(&self) -> &VecDeque<WorkshopEntry> {
        &self.summary_log
    }

    /// Number of summaries logged.
    #[must_use]
    pub fn summary_count(&self) -> usize {
        self.summary_log.len()
    }

    /// Format the summary log as a compact prose block for inclusion in
    /// system prompts or context summaries.
    #[must_use]
    pub fn format_summary_log(&self) -> String {
        if self.summary_log.is_empty() {
            return String::new();
        }
        let mut out = String::from("[workshop: tool-output summaries across this session]\n");
        for entry in &self.summary_log {
            // Truncate each log entry to a single line for compactness.
            let one_line: String = entry
                .summary
                .lines()
                .next()
                .unwrap_or(&entry.summary)
                .chars()
                .take(200)
                .collect();
            out.push_str(&format!("- {}: {}\n", entry.tool_name, one_line));
        }
        out
    }
}

impl Default for WorkshopAgent {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_workshop_is_empty() {
        let w = WorkshopAgent::new();
        assert_eq!(w.summary_count(), 0);
        assert!(w.format_summary_log().is_empty());
        assert_eq!(w.threshold(), 4096);
    }

    #[test]
    fn should_summarize_under_threshold() {
        let w = WorkshopAgent::new();
        assert!(!w.should_summarize("small output"));
        assert!(!w.should_summarize(&"x".repeat(4095)));
    }

    #[test]
    fn should_summarize_at_threshold() {
        let w = WorkshopAgent::new();
        assert!(w.should_summarize(&"x".repeat(4096)));
        assert!(w.should_summarize(&"x".repeat(8192)));
    }

    #[test]
    fn record_entry_roundtrip() {
        let mut w = WorkshopAgent::new();
        w.record_entry("read_file", "saw 42 lines of cargo.toml");
        w.record_entry("exec_shell", "cargo check passed with 3 warnings");
        assert_eq!(w.summary_count(), 2);
        let formatted = w.format_summary_log();
        assert!(formatted.contains("read_file"));
        assert!(formatted.contains("exec_shell"));
        assert!(formatted.contains("cargo.toml"));
    }

    #[test]
    fn summary_log_trims_oldest() {
        let mut w = WorkshopAgent::new();
        // We set MAX_SUMMARY_LOG=32. Fill slightly past it.
        for i in 0..34 {
            w.record_entry("tool", &format!("entry {i}"));
        }
        assert_eq!(w.summary_count(), 32);
        // The oldest entries (0 and 1) are gone.
        let formatted = w.format_summary_log();
        assert!(!formatted.contains("entry 0"), "oldest should be trimmed");
        assert!(formatted.contains("entry 33"), "newest should remain");
    }

    #[test]
    fn record_entry_readable_format() {
        let mut w = WorkshopAgent::new();
        w.record_entry("read_file", "src/main.rs (285 lines)");
        let formatted = w.format_summary_log();
        assert!(formatted.starts_with("[workshop:"));
        assert!(formatted.contains("read_file"));
    }
}
