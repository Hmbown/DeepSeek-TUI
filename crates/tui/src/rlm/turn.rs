//! True RLM turn loop — Algorithm 1 from Zhang et al. (arXiv:2512.24601).
//!
//! # Algorithm
//!
//! ```text
//! state ← InitREPL(prompt=P)
//! state ← AddFunction(state, sub_RLM)
//! hist ← [Metadata(state)]
//! while True:
//!     code ← LLM(hist)
//!     (state, stdout) ← REPL(state, code)
//!     hist ← hist ∥ code ∥ Metadata(stdout)
//!     if state[Final] is set:
//!         return state[Final]
//! ```
//!
//! Key invariants:
//! 1. P is stored as `PROMPT` in the REPL — NEVER in the LLM context
//! 2. Only metadata (length, preview, variable names) goes to LLM context
//! 3. The LLM writes Python code, executed by the REPL
//! 4. The REPL provides `llm_query()` for recursive sub-calls

use std::time::{Duration, Instant};

use serde_json::json;
use tokio::sync::mpsc;

use crate::client::DeepSeekClient;
use crate::core::events::Event;
use crate::llm_client::LlmClient;
use crate::models::{ContentBlock, Message, MessageRequest, Usage};
use crate::repl::runtime::PythonRuntime;
use crate::repl::sandbox::parse_final;

use super::prompt::rlm_system_prompt;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of RLM iterations before the loop gives up.
const MAX_RLM_ITERATIONS: u32 = 25;

/// Max output tokens for the root LLM — just needs to generate code, not
/// the full answer.
const ROOT_MAX_TOKENS: u32 = 4096;

/// Max chars of stdout shown as metadata to the root LLM in next iteration.
/// Matches the paper's "only metadata about stdout" constraint.
const STDOUT_METADATA_PREVIEW_LEN: usize = 800;

/// Max chars of PROMPT shown as preview in metadata.
const PROMPT_PREVIEW_LEN: usize = 500;

/// Temperature for root LLM calls. Low to keep code generation focused.
const ROOM_TEMPERATURE: f32 = 0.3;

/// Per-iteration timeout for the entire LLM+REPL round.
const ROUND_TIMEOUT: Duration = Duration::from_secs(180);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Result of an RLM turn.
#[derive(Debug, Clone)]
pub struct RlmTurnResult {
    /// The final answer (from FINAL(), or the model's raw text if no code).
    pub answer: String,
    /// Number of iterations used.
    pub iterations: u32,
    /// Total wall-clock duration.
    pub duration: Duration,
    /// Error message if the turn failed.
    pub error: Option<String>,
    /// Usage from the root LLM calls (total across iterations).
    pub usage: Usage,
}

/// Run a full RLM turn per Algorithm 1.
///
/// The user's `prompt` is stored as `PROMPT` in the REPL and never placed
/// into the LLM's context window. The LLM receives only metadata about the
/// REPL state and generates code, which is then executed. When `FINAL()` is
/// called inside the code, the loop ends and the value is returned.
pub async fn run_rlm_turn(
    client: &DeepSeekClient,
    model: String,
    prompt: String,
    _child_model: String,
    tx_event: mpsc::Sender<Event>,
) -> RlmTurnResult {
    let start = Instant::now();
    let mut total_usage = Usage::default();

    // ------------------------------------------------------------------
    // 1. Initialise REPL with PROMPT variable
    // ------------------------------------------------------------------
    let state_dir = std::env::temp_dir().join("deepseek_rlm");
    let _ = std::fs::create_dir_all(&state_dir);
    let state_path = state_dir.join(format!("rlm_{}.json", uuid::Uuid::new_v4()));

    // Write PROMPT into the REPL state before the REPL even starts.
    let initial_vars = json!({"PROMPT": &prompt});
    if let Err(e) = std::fs::write(&state_path, serde_json::to_string(&initial_vars).unwrap()) {
        return RlmTurnResult {
            answer: String::new(),
            iterations: 0,
            duration: start.elapsed(),
            error: Some(format!("Failed to write REPL state: {e}")),
            usage: total_usage,
        };
    }

    let mut repl = PythonRuntime::with_state_path(state_path.clone());

    let _ = tx_event
        .send(Event::status(
            "RLM: REPL initialised with PROMPT variable".to_string(),
        ))
        .await;

    // ------------------------------------------------------------------
    // 2. Build metadata-only conversation history
    // ------------------------------------------------------------------
    let system = rlm_system_prompt();
    let metadata_msg = build_metadata_message(&prompt, 0, None, None);

    // The conversation history for the root LLM contains ONLY:
    //   - Metadata(state) — initial
    //   - code (assistant) + Metadata(stdout) (user) — for each iteration
    // This keeps the root LLM context constant-size regardless of PROMPT size.
    let mut messages: Vec<Message> = vec![metadata_msg];

    // ------------------------------------------------------------------
    // 3. RLM loop (Algorithm 1)
    // ------------------------------------------------------------------
    for iteration in 0..MAX_RLM_ITERATIONS {
        if start.elapsed() > ROUND_TIMEOUT {
            return RlmTurnResult {
                answer: String::new(),
                iterations: iteration,
                duration: start.elapsed(),
                error: Some(format!(
                    "RLM turn timed out after {}s",
                    ROUND_TIMEOUT.as_secs()
                )),
                usage: total_usage,
            };
        }

        let _ = tx_event
            .send(Event::status(format!(
                "RLM iteration {}/{}",
                iteration + 1,
                MAX_RLM_ITERATIONS
            )))
            .await;

        // 3a. LLM generates code from metadata-only context
        let request = MessageRequest {
            model: model.clone(),
            messages: messages.clone(),
            max_tokens: ROOT_MAX_TOKENS,
            system: Some(system.clone()),
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: Some(false),
            temperature: Some(ROOM_TEMPERATURE),
            top_p: Some(0.9_f32),
        };

        let response = match client.create_message(request).await {
            Ok(r) => r,
            Err(e) => {
                return RlmTurnResult {
                    answer: String::new(),
                    iterations: iteration + 1,
                    duration: start.elapsed(),
                    error: Some(format!("Root LLM call failed: {e}")),
                    usage: total_usage,
                };
            }
        };

        // Accumulate usage
        total_usage.input_tokens = total_usage
            .input_tokens
            .saturating_add(response.usage.input_tokens);
        total_usage.output_tokens = total_usage
            .output_tokens
            .saturating_add(response.usage.output_tokens);

        // Extract text from response
        let response_text = extract_text_blocks(&response.content);

        let _ = tx_event
            .send(Event::MessageDelta {
                index: iteration as usize,
                content: format!("\n[RLM iteration {}]\n", iteration + 1),
            })
            .await;

        // 3b. Extract Python code from the response
        let code = extract_python_code(&response_text);

        let (code_to_run, _is_direct_answer) = match code {
            Some(c) => (c, false),
            None => {
                // No code block — the model gave a direct text answer.
                // This is a valid exit: the model decided it doesn't need
                // the REPL and is returning a final answer directly.
                let _ = tx_event
                    .send(Event::MessageDelta {
                        index: iteration as usize,
                        content: response_text.clone(),
                    })
                    .await;
                return RlmTurnResult {
                    answer: response_text,
                    iterations: iteration + 1,
                    duration: start.elapsed(),
                    error: None,
                    usage: total_usage,
                };
            }
        };

        let _ = tx_event
            .send(Event::MessageDelta {
                index: iteration as usize,
                content: format!("```python\n{code_to_run}\n```\n"),
            })
            .await;

        // 3c. Execute code in REPL
        let round = match repl.execute(&code_to_run).await {
            Ok(r) => r,
            Err(e) => {
                let _ = tx_event
                    .send(Event::status(format!("RLM REPL error: {e}")))
                    .await;
                return RlmTurnResult {
                    answer: String::new(),
                    iterations: iteration + 1,
                    duration: start.elapsed(),
                    error: Some(format!("REPL execution failed: {e}")),
                    usage: total_usage,
                };
            }
        };

        // 3d. Check for FINAL
        if let Some(final_val) = &round.final_value {
            let _ = tx_event
                .send(Event::status(
                    "RLM: FINAL detected, ending loop".to_string(),
                ))
                .await;
            return RlmTurnResult {
                answer: final_val.clone(),
                iterations: iteration + 1,
                duration: start.elapsed(),
                error: None,
                usage: total_usage,
            };
        }

        // Also check raw stdout for FINAL (in case the parse missed it)
        let (_cleaned, raw_final) = parse_final(&round.full_stdout);
        if let Some(final_val) = raw_final {
            let _ = tx_event
                .send(Event::status(
                    "RLM: FINAL detected (raw parse), ending loop".to_string(),
                ))
                .await;
            return RlmTurnResult {
                answer: final_val,
                iterations: iteration + 1,
                duration: start.elapsed(),
                error: None,
                usage: total_usage,
            };
        }

        // 3e. Build metadata for next iteration and append to history
        //     hist ← hist ∥ code ∥ Metadata(stdout)
        let stdout_display = if round.stdout.is_empty() && !round.stderr.is_empty() {
            format!(
                "[stderr]\n{}",
                truncate_text(&round.stderr, STDOUT_METADATA_PREVIEW_LEN)
            )
        } else {
            truncate_text(&round.stdout, STDOUT_METADATA_PREVIEW_LEN)
        };

        // Assistant message: the code the model wrote
        messages.push(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: format!("```python\n{code_to_run}\n```"),
                cache_control: None,
            }],
        });

        // User message: metadata about stdout + current REPL state
        let next_metadata = build_metadata_message(
            &prompt,
            iteration + 1,
            Some(&code_to_run),
            Some(&stdout_display),
        );
        messages.push(next_metadata);

        // Emit stdout preview as a status update
        let _ = tx_event
            .send(Event::status(format!(
                "REPL round {}: {} bytes output{}",
                iteration + 1,
                round.full_stdout.len(),
                if round.has_error { " (error)" } else { "" },
            )))
            .await;

        // Limit the messages vector to prevent unbounded growth.
        // Keep at most 10 metadata+code pairs (the context is already small
        // since each is just metadata, but we should still bound it).
        // The paper's Algorithm 1 only trims per-iteration tokens, not
        // iterations themselves, but we add this as a practical guard.
        const MAX_HISTORY_PAIRS: usize = 20; // 10 iterations × 2 messages each
        if messages.len() > MAX_HISTORY_PAIRS {
            // Remove oldest pair but keep the first metadata message.
            let mut kept = vec![messages[0].clone()];
            kept.extend(messages.drain(messages.len() - MAX_HISTORY_PAIRS + 1..));
            messages = kept;
        }
    }

    // Loop exhausted without FINAL
    RlmTurnResult {
        answer: String::new(),
        iterations: MAX_RLM_ITERATIONS,
        duration: start.elapsed(),
        error: Some(format!(
            "RLM loop exhausted after {MAX_RLM_ITERATIONS} iterations without FINAL"
        )),
        usage: total_usage,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a metadata message describing the current REPL state.
///
/// This is what the paper calls `Metadata(state)` — it contains:
/// - Length of PROMPT (not the content itself)
/// - A short preview of PROMPT
/// - Current iteration number
/// - Previous code (if any)
/// - Previous stdout summary (if any)
fn build_metadata_message(
    prompt: &str,
    iteration: u32,
    previous_code: Option<&str>,
    previous_stdout: Option<&str>,
) -> Message {
    let prompt_len = prompt.len();
    let prompt_preview = truncate_text(prompt, PROMPT_PREVIEW_LEN);

    let mut parts = Vec::new();

    parts.push(format!("## REPL State (Round {iteration})"));
    parts.push(String::new());
    parts.push("**PROMPT** — stored as REPL variable `PROMPT`".to_string());
    parts.push(format!("- Length: {prompt_len} characters"));
    parts.push(format!("- Preview: \"{prompt_preview}\""));
    parts.push(String::new());

    if iteration > 0 {
        parts.push("**Previous Round**".to_string());
        if let Some(code) = previous_code {
            // Only show the first/last lines as metadata
            let code_lines: Vec<&str> = code.lines().collect();
            let code_summary = if code_lines.len() > 8 {
                let first_few: Vec<&str> = code_lines.iter().take(4).copied().collect();
                let last_few: Vec<&str> = code_lines.iter().rev().take(3).rev().copied().collect();
                format!(
                    "{} lines: {} ... {}",
                    code_lines.len(),
                    first_few.join("\n"),
                    last_few.join("\n")
                )
            } else {
                code.to_string()
            };
            parts.push(format!("- Code: {code_summary}"));
        }
        if let Some(stdout) = previous_stdout {
            // Only show truncated stdout
            let stdout_clean = stdout.trim();
            if !stdout_clean.is_empty() {
                parts.push(format!("- Stdout preview: \"{stdout_clean}\""));
            } else {
                parts.push("- Stdout: (empty)".to_string());
            }
        }
        parts.push(String::new());
    }

    parts.push(
        "**Available functions**: `repl_get()`, `repl_set()`, `llm_query(prompt)`".to_string(),
    );
    parts.push("**End the loop with**: `FINAL(value)`".to_string());

    let text = parts.join("\n");

    Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text,
            cache_control: None,
        }],
    }
}

/// Extract text from content blocks, joining all text blocks together.
fn extract_text_blocks(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract the first ```python code block from text.
/// Returns `None` if no python fence is found.
fn extract_python_code(text: &str) -> Option<String> {
    // Look for ```python or ```py
    let start_markers = ["```python\n", "```py\n", "```python\r\n", "```py\r\n"];
    let mut best_start: Option<(usize, &str)> = None;

    for marker in &start_markers {
        if let Some(idx) = text.find(marker) {
            let end_pos = idx + marker.len();
            match best_start {
                Some((best_idx, _)) if idx < best_idx => {
                    best_start = Some((idx, &text[end_pos..]));
                }
                None => {
                    best_start = Some((idx, &text[end_pos..]));
                }
                _ => {}
            }
        }
    }

    let after_fence = best_start.map(|(_, rest)| rest)?;

    // Find the closing ```
    let end_idx = after_fence
        .find("\n```")
        .or_else(|| after_fence.find("```"))?;

    let code = after_fence[..end_idx].trim().to_string();
    if code.is_empty() {
        return None;
    }
    Some(code)
}

/// Truncate text to `max_chars`, adding an ellipsis if truncated.
fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let take = max_chars.saturating_sub(3);
    let mut result: String = text.chars().take(take).collect();
    result.push_str("...");
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_python_code_finds_simple_block() {
        let text = "Here's some code:\n```python\nprint('hello')\n```\nEnd.";
        let code = extract_python_code(text).unwrap();
        assert_eq!(code, "print('hello')");
    }

    #[test]
    fn extract_python_code_finds_short_marker() {
        let text = "Code:\n```py\nx = 1 + 2\n```";
        let code = extract_python_code(text).unwrap();
        assert_eq!(code, "x = 1 + 2");
    }

    #[test]
    fn extract_python_code_returns_none_when_missing() {
        let text = "Just some text without code fences.";
        assert!(extract_python_code(text).is_none());
    }

    #[test]
    fn extract_python_code_returns_none_on_empty_block() {
        let text = "Code:\n```python\n\n```";
        assert!(extract_python_code(text).is_none());
    }

    #[test]
    fn extract_python_code_handles_multiple_blocks() {
        let text = "First:\n```python\na=1\n```\nSecond:\n```python\nb=2\n```";
        let code = extract_python_code(text).unwrap();
        assert_eq!(code, "a=1"); // Returns first block
    }

    #[test]
    fn extract_python_code_ignores_other_fences() {
        let text = "```\nsome text\n```\nActual:\n```python\nreal_code()\n```";
        let code = extract_python_code(text).unwrap();
        assert_eq!(code, "real_code()");
    }

    #[test]
    fn build_metadata_contains_key_information() {
        let prompt = "Hello, world!";
        let msg = build_metadata_message(prompt, 0, None, None);
        let text = extract_text_blocks(&msg.content);
        assert!(text.contains("PROMPT"));
        assert!(text.contains("Hello, world!"));
        assert!(text.contains("Round 0"));
        assert!(text.contains("llm_query"));
        assert!(text.contains("FINAL"));
    }

    #[test]
    fn build_metadata_with_iteration_shows_previous_code() {
        let prompt = "Test prompt";
        let msg = build_metadata_message(prompt, 3, Some("print('hi')"), Some("hi"));
        let text = extract_text_blocks(&msg.content);
        assert!(text.contains("Round 3"));
        assert!(text.contains("print('hi')"));
        assert!(text.contains("hi"));
    }

    #[test]
    fn truncate_text_leaves_short_text_alone() {
        assert_eq!(truncate_text("hello", 100), "hello");
    }

    #[test]
    fn truncate_text_shortens_long_text() {
        let long = "a".repeat(1000);
        let truncated = truncate_text(&long, 10);
        // 7 chars of 'a' + "..." = 10 chars/bytes total
        assert_eq!(truncated.len(), 10);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn extract_text_blocks_joins_text_blocks() {
        let blocks = vec![
            ContentBlock::Text {
                text: "first".to_string(),
                cache_control: None,
            },
            ContentBlock::Thinking {
                thinking: "skip".to_string(),
            },
            ContentBlock::Text {
                text: "second".to_string(),
                cache_control: None,
            },
        ];
        assert_eq!(extract_text_blocks(&blocks), "first\nsecond");
    }

    #[test]
    fn extract_text_blocks_returns_empty_on_no_text() {
        let blocks = vec![ContentBlock::Thinking {
            thinking: "only thinking".to_string(),
        }];
        assert_eq!(extract_text_blocks(&blocks), "");
    }

    #[test]
    fn metadata_msg_role_is_user() {
        let msg = build_metadata_message("test", 0, None, None);
        assert_eq!(msg.role, "user");
    }

    #[test]
    fn metadata_with_previous_code_shows_code_summary() {
        let msg = build_metadata_message(
            "test",
            2,
            Some("for i in range(10):\n    print(i)"),
            Some("0\n1\n2"),
        );
        let text = extract_text_blocks(&msg.content);
        assert!(text.contains("Round 2"));
        assert!(text.contains("for i"));
        assert!(text.contains("0\n1\n2"));
    }
}
