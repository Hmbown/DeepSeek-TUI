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
//! 1. P is stored as `PROMPT` in the REPL — NEVER in the LLM context.
//! 2. Only metadata (length, preview, variable names) goes to LLM context.
//! 3. The LLM writes Python code, executed by the REPL.
//! 4. The REPL exposes `llm_query()` (one-shot child) and `sub_rlm()`
//!    (recursive RLM call), both serviced by an in-process HTTP sidecar.

use std::sync::Arc;
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
use super::sidecar::{SidecarCtx, start_sidecar};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of RLM iterations before the loop gives up.
const MAX_RLM_ITERATIONS: u32 = 25;

/// Max consecutive rounds where the model returns no `python` fence before
/// we give up. The paper requires `code → REPL → Final`; a chatty round is
/// tolerated once but not indefinitely.
const MAX_CONSECUTIVE_NO_CODE: u32 = 2;

/// Max output tokens for the root LLM — just needs to generate code, not
/// the full answer.
const ROOT_MAX_TOKENS: u32 = 4096;

/// Max chars of stdout shown as metadata to the root LLM in next iteration.
/// Matches the paper's "only metadata about stdout" constraint.
const STDOUT_METADATA_PREVIEW_LEN: usize = 800;

/// Max chars of PROMPT shown as preview in metadata.
const PROMPT_PREVIEW_LEN: usize = 500;

/// Temperature for root LLM calls. Low to keep code generation focused.
const ROOT_TEMPERATURE: f32 = 0.3;

/// Per-iteration timeout for the entire LLM+REPL round (whole-turn cap).
const ROUND_TIMEOUT: Duration = Duration::from_secs(180);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// How an RLM turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RlmTermination {
    /// `FINAL(value)` was called inside the REPL.
    Final,
    /// The model emitted a non-code answer at the top of the loop. Only
    /// possible when strict mode is disabled (currently always strict).
    DirectAnswer,
    /// Iteration cap reached without `FINAL`.
    Exhausted,
    /// Hard error (LLM call failed, REPL crashed, timeout, …).
    Error,
}

/// Result of an RLM turn.
#[derive(Debug, Clone)]
pub struct RlmTurnResult {
    /// The final answer (from FINAL(), or empty on error/exhaustion).
    pub answer: String,
    /// Number of iterations used.
    pub iterations: u32,
    /// Total wall-clock duration.
    pub duration: Duration,
    /// Error message if the turn failed.
    pub error: Option<String>,
    /// Usage from the root LLM calls + sidecar-served sub-LLM calls.
    pub usage: Usage,
    /// How the loop ended.
    pub termination: RlmTermination,
}

/// Run a full RLM turn (paper Algorithm 1).
///
/// `prompt` is loaded into the REPL as the `context` variable and never
/// enters the root LLM's window. `root_prompt` (optional) is a small
/// instruction shown to the root LLM each iteration — typically the
/// model's task ("summarize the security model"). `max_depth` controls
/// how many levels of `rlm_query()` recursion the sub-agent may use.
pub async fn run_rlm_turn(
    client: &DeepSeekClient,
    model: String,
    prompt: String,
    child_model: String,
    tx_event: mpsc::Sender<Event>,
    max_depth: u32,
) -> RlmTurnResult {
    run_rlm_turn_inner(
        client,
        model,
        prompt,
        None,
        child_model,
        tx_event,
        max_depth,
    )
    .await
}

/// Variant that lets the caller pass a small `root_prompt` shown to the
/// root LLM each iteration. The big `prompt` still lives only in the REPL.
pub async fn run_rlm_turn_with_root(
    client: &DeepSeekClient,
    model: String,
    prompt: String,
    root_prompt: Option<String>,
    child_model: String,
    tx_event: mpsc::Sender<Event>,
    max_depth: u32,
) -> RlmTurnResult {
    run_rlm_turn_inner(
        client,
        model,
        prompt,
        root_prompt,
        child_model,
        tx_event,
        max_depth,
    )
    .await
}

/// Inner entry point — also used by the sidecar's `/rlm` handler when it
/// recurses. Decrements `max_depth` for nested calls.
///
/// Returns an explicit boxed-trait-object future to break the recursive
/// opaque-type cycle:
/// `run_rlm_turn_inner` → `start_sidecar` → `sub_rlm_handler` → `run_rlm_turn_inner`.
pub(crate) fn run_rlm_turn_inner<'a>(
    client: &'a DeepSeekClient,
    model: String,
    prompt: String,
    root_prompt: Option<String>,
    child_model: String,
    tx_event: mpsc::Sender<Event>,
    max_depth: u32,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = RlmTurnResult> + Send + 'a>> {
    Box::pin(run_rlm_turn_inner_impl(
        client,
        model,
        prompt,
        root_prompt,
        child_model,
        tx_event,
        max_depth,
    ))
}

async fn run_rlm_turn_inner_impl(
    client: &DeepSeekClient,
    model: String,
    prompt: String,
    root_prompt: Option<String>,
    child_model: String,
    tx_event: mpsc::Sender<Event>,
    max_depth: u32,
) -> RlmTurnResult {
    let start = Instant::now();
    let mut total_usage = Usage::default();

    // ------------------------------------------------------------------
    // 0. Start the HTTP sidecar that services llm_query() / sub_rlm()
    //    from inside the Python REPL. Lives for the duration of this turn.
    // ------------------------------------------------------------------
    let sidecar_ctx = SidecarCtx::new(client.clone(), child_model.clone(), max_depth);
    let sidecar = match start_sidecar(Arc::clone(&sidecar_ctx)).await {
        Ok(h) => h,
        Err(e) => {
            return RlmTurnResult {
                answer: String::new(),
                iterations: 0,
                duration: start.elapsed(),
                error: Some(format!("Failed to start RLM sidecar: {e}")),
                usage: total_usage,
                termination: RlmTermination::Error,
            };
        }
    };
    let llm_url = sidecar.llm_url();
    let llm_batch_url = sidecar.llm_batch_url();
    let rlm_url = sidecar.rlm_url();
    let rlm_batch_url = sidecar.rlm_batch_url();

    // ------------------------------------------------------------------
    // 1. Initialise REPL with `context` variable
    // ------------------------------------------------------------------
    let state_dir = std::env::temp_dir().join("deepseek_rlm");
    let _ = std::fs::create_dir_all(&state_dir);
    let state_path = state_dir.join(format!("rlm_{}.json", uuid::Uuid::new_v4()));

    // Write `context` into the REPL state before the REPL even starts.
    // Matches the reference (alexzhang13/rlm) variable name.
    let initial_vars = json!({"context": &prompt});
    if let Err(e) = std::fs::write(&state_path, serde_json::to_string(&initial_vars).unwrap()) {
        sidecar.shutdown();
        return RlmTurnResult {
            answer: String::new(),
            iterations: 0,
            duration: start.elapsed(),
            error: Some(format!("Failed to write REPL state: {e}")),
            usage: total_usage,
            termination: RlmTermination::Error,
        };
    }

    let mut repl = PythonRuntime::with_state_path(state_path.clone());
    repl.set_env("REPL_LLM_URL", &llm_url);
    repl.set_env("REPL_LLM_BATCH_URL", &llm_batch_url);
    repl.set_env("REPL_RLM_URL", &rlm_url);
    repl.set_env("REPL_RLM_BATCH_URL", &rlm_batch_url);

    let _ = tx_event
        .send(Event::status(format!(
            "RLM turn started — root={model}, child={child_model}, max_depth={max_depth}"
        )))
        .await;

    // ------------------------------------------------------------------
    // 2. Build metadata-only conversation history
    // ------------------------------------------------------------------
    let system = rlm_system_prompt();
    let metadata_msg =
        build_metadata_message(&prompt, root_prompt.as_deref(), 0, None, None, &state_path);

    let mut messages: Vec<Message> = vec![metadata_msg];

    // Track consecutive no-code rounds for strict-mode termination.
    let mut consecutive_no_code: u32 = 0;

    // ------------------------------------------------------------------
    // 3. RLM loop (Algorithm 1)
    // ------------------------------------------------------------------
    let result = 'turn: {
        for iteration in 0..MAX_RLM_ITERATIONS {
            if start.elapsed() > ROUND_TIMEOUT {
                break 'turn RlmTurnResult {
                    answer: String::new(),
                    iterations: iteration,
                    duration: start.elapsed(),
                    error: Some(format!(
                        "RLM turn timed out after {}s",
                        ROUND_TIMEOUT.as_secs()
                    )),
                    usage: total_usage,
                    termination: RlmTermination::Error,
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
                temperature: Some(ROOT_TEMPERATURE),
                top_p: Some(0.9_f32),
            };

            let response = match client.create_message(request).await {
                Ok(r) => r,
                Err(e) => {
                    break 'turn RlmTurnResult {
                        answer: String::new(),
                        iterations: iteration + 1,
                        duration: start.elapsed(),
                        error: Some(format!("Root LLM call failed: {e}")),
                        usage: total_usage,
                        termination: RlmTermination::Error,
                    };
                }
            };

            // Accumulate root usage
            total_usage.input_tokens = total_usage
                .input_tokens
                .saturating_add(response.usage.input_tokens);
            total_usage.output_tokens = total_usage
                .output_tokens
                .saturating_add(response.usage.output_tokens);

            let response_text = extract_text_blocks(&response.content);

            let _ = tx_event
                .send(Event::MessageDelta {
                    index: iteration as usize,
                    content: format!("\n[RLM iteration {}]\n", iteration + 1),
                })
                .await;

            // 3b. Check for a text-level FINAL(...) / FINAL_VAR(...) in the
            // model's raw response. The reference RLM allows the model to
            // close out by writing `FINAL(my answer)` directly in its
            // message, without going through a ```repl block.
            if let Some(final_val) = parse_text_final(&response_text) {
                let _ = tx_event
                    .send(Event::status(
                        "RLM: FINAL detected in response text".to_string(),
                    ))
                    .await;
                break 'turn RlmTurnResult {
                    answer: final_val,
                    iterations: iteration + 1,
                    duration: start.elapsed(),
                    error: None,
                    usage: total_usage,
                    termination: RlmTermination::Final,
                };
            }

            // 3c. Extract a ```repl block. Match the reference RLM's
            // language identifier so the same prompts/examples work here.
            let code = extract_repl_code(&response_text);

            let code_to_run = match code {
                Some(c) => {
                    consecutive_no_code = 0;
                    c
                }
                None => {
                    consecutive_no_code = consecutive_no_code.saturating_add(1);
                    if consecutive_no_code >= MAX_CONSECUTIVE_NO_CODE {
                        // Give up — surface the model's text as a degraded
                        // direct answer rather than throwing the turn away.
                        let _ = tx_event
                            .send(Event::MessageDelta {
                                index: iteration as usize,
                                content: response_text.clone(),
                            })
                            .await;
                        break 'turn RlmTurnResult {
                            answer: response_text,
                            iterations: iteration + 1,
                            duration: start.elapsed(),
                            error: None,
                            usage: total_usage,
                            termination: RlmTermination::DirectAnswer,
                        };
                    }
                    // Append a reminder and retry.
                    messages.push(Message {
                        role: "assistant".to_string(),
                        content: vec![ContentBlock::Text {
                            text: response_text.clone(),
                            cache_control: None,
                        }],
                    });
                    messages.push(Message {
                        role: "user".to_string(),
                        content: vec![ContentBlock::Text {
                            text: "Reminder: emit Python inside a ```repl … ``` fence (or write FINAL(value) on its own line) when you have the answer. Reply with a ```repl block now.".to_string(),
                            cache_control: None,
                        }],
                    });
                    continue;
                }
            };

            let _ = tx_event
                .send(Event::MessageDelta {
                    index: iteration as usize,
                    content: format!("```repl\n{code_to_run}\n```\n"),
                })
                .await;

            // 3c. Execute code in REPL
            let round = match repl.execute(&code_to_run).await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx_event
                        .send(Event::status(format!("RLM REPL error: {e}")))
                        .await;
                    break 'turn RlmTurnResult {
                        answer: String::new(),
                        iterations: iteration + 1,
                        duration: start.elapsed(),
                        error: Some(format!("REPL execution failed: {e}")),
                        usage: total_usage,
                        termination: RlmTermination::Error,
                    };
                }
            };

            // 3d. Check for FINAL (parsed by the runtime, or in raw stdout
            //     as a belt-and-braces check).
            if let Some(final_val) = round
                .final_value
                .clone()
                .or_else(|| parse_final(&round.full_stdout).1)
            {
                let _ = tx_event
                    .send(Event::status(
                        "RLM: FINAL detected, ending loop".to_string(),
                    ))
                    .await;
                break 'turn RlmTurnResult {
                    answer: final_val,
                    iterations: iteration + 1,
                    duration: start.elapsed(),
                    error: None,
                    usage: total_usage,
                    termination: RlmTermination::Final,
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
                    text: format!("```repl\n{code_to_run}\n```"),
                    cache_control: None,
                }],
            });

            // User message: metadata about stdout + current REPL state
            let next_metadata = build_metadata_message(
                &prompt,
                root_prompt.as_deref(),
                iteration + 1,
                Some(&code_to_run),
                Some(&stdout_display),
                &state_path,
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

            // Bound history growth. Keep the original metadata + the most
            // recent N pairs so the model still sees the running thread.
            const MAX_HISTORY_MESSAGES: usize = 20;
            if messages.len() > MAX_HISTORY_MESSAGES {
                let drop_from = messages.len() - MAX_HISTORY_MESSAGES + 1;
                let mut kept = vec![messages[0].clone()];
                kept.extend(messages.drain(drop_from..));
                messages = kept;
            }
        }

        RlmTurnResult {
            answer: String::new(),
            iterations: MAX_RLM_ITERATIONS,
            duration: start.elapsed(),
            error: Some(format!(
                "RLM loop exhausted after {MAX_RLM_ITERATIONS} iterations without FINAL"
            )),
            usage: total_usage,
            termination: RlmTermination::Exhausted,
        }
    };

    // Fold sidecar usage (children + nested sub_rlm) into the totals.
    let sidecar_usage = sidecar_ctx.usage.lock().await;
    let mut final_usage = result.usage.clone();
    final_usage.input_tokens = final_usage
        .input_tokens
        .saturating_add(sidecar_usage.input_tokens);
    final_usage.output_tokens = final_usage
        .output_tokens
        .saturating_add(sidecar_usage.output_tokens);
    drop(sidecar_usage);
    // Best-effort cleanup of the per-turn state file. Non-fatal.
    let _ = std::fs::remove_file(&state_path);
    sidecar.shutdown();
    RlmTurnResult {
        usage: final_usage,
        ..result
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a metadata message describing the current REPL state.
///
/// This is `Metadata(state)` from the paper. We surface:
/// - `context` length (chars) and a short preview
/// - the small `root_prompt` (if any) — repeated each iteration
/// - REPL helpers the sub-agent can call
/// - keys currently present in the REPL variable store
/// - the previous round's code summary and stdout preview
fn build_metadata_message(
    prompt: &str,
    root_prompt: Option<&str>,
    iteration: u32,
    previous_code: Option<&str>,
    previous_stdout: Option<&str>,
    state_path: &std::path::Path,
) -> Message {
    let prompt_len = prompt.chars().count();
    let prompt_preview = truncate_text(prompt, PROMPT_PREVIEW_LEN);

    let mut parts = Vec::new();

    parts.push(format!("## REPL State (Round {iteration})"));
    parts.push(String::new());
    if let Some(rp) = root_prompt
        && !rp.trim().is_empty()
    {
        parts.push("**Original task** (re-shown every round)".to_string());
        parts.push(format!("> {}", truncate_text(rp.trim(), 600)));
        parts.push(String::new());
    }
    parts.push("**`context`** — REPL variable holding the full input".to_string());
    parts.push(format!("- Length: {prompt_len} chars"));
    parts.push(format!("- Preview: \"{prompt_preview}\""));
    parts.push(String::new());

    parts.push("**REPL helpers** (use inside ```repl blocks)".to_string());
    parts.push("- `context`                              — the full input string".to_string());
    parts.push("- `len(context)` / `context[a:b]` / `context.splitlines()` — slice it".to_string());
    parts.push("- `llm_query(prompt, model=None)`        — one-shot child LLM".to_string());
    parts.push("- `llm_query_batched([p1, p2, ...])`     — concurrent fanout".to_string());
    parts.push("- `rlm_query(prompt, model=None)`        — recursive sub-RLM".to_string());
    parts.push("- `rlm_query_batched([p1, p2, ...])`     — concurrent recursive RLMs".to_string());
    parts.push("- `SHOW_VARS()`                          — list user variables".to_string());
    parts.push("- `repl_set(name, value)` / `repl_get(name)` — explicit store".to_string());
    parts.push(
        "- `FINAL(value)`                         — end the loop with this answer".to_string(),
    );
    parts.push(
        "- `FINAL_VAR(name)`                      — end the loop with a variable's value"
            .to_string(),
    );
    parts.push(String::new());

    // Variables currently in the persistent store.
    if let Ok(text) = std::fs::read_to_string(state_path)
        && let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text)
    {
        let mut keys: Vec<String> = map.keys().cloned().collect();
        keys.sort();
        if !keys.is_empty() {
            let listed = keys
                .iter()
                .map(|k| format!("\"{k}\""))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("**Variables in REPL state**: [{listed}]"));
            parts.push(String::new());
        }
    }

    if iteration > 0 {
        parts.push("**Previous round**".to_string());
        if let Some(code) = previous_code {
            let code_summary = summarize_code(code);
            parts.push(format!("- Code: {code_summary}"));
        }
        if let Some(stdout) = previous_stdout {
            let stdout_clean = stdout.trim();
            if !stdout_clean.is_empty() {
                parts.push(format!("- Stdout preview: \"{stdout_clean}\""));
            } else {
                parts.push("- Stdout: (empty)".to_string());
            }
        }
    }

    let text = parts.join("\n");

    Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text,
            cache_control: None,
        }],
    }
}

/// Compress a code block to a short summary — first 4 + last 4 lines.
fn summarize_code(code: &str) -> String {
    let lines: Vec<&str> = code.lines().collect();
    if lines.len() <= 8 {
        return code.to_string();
    }
    let head = lines[..4].join("\n");
    let tail = lines[lines.len() - 4..].join("\n");
    format!("{} lines:\n{head}\n…\n{tail}", lines.len())
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

/// Extract the first ```repl code block from the model's response.
///
/// Matches the reference RLM (alexzhang13/rlm) language identifier so the
/// same prompts and examples work here. Falls back to ```python /
/// ```py for backward compatibility with text the model may have learned
/// from earlier prompt versions.
fn extract_repl_code(text: &str) -> Option<String> {
    let start_markers = [
        "```repl\n",
        "```repl\r\n",
        "```python\n",
        "```py\n",
        "```python\r\n",
        "```py\r\n",
    ];
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

    let end_idx = after_fence
        .find("\n```")
        .or_else(|| after_fence.find("```"))?;

    let code = after_fence[..end_idx].trim().to_string();
    if code.is_empty() {
        return None;
    }
    Some(code)
}

/// Parse a `FINAL(...)` or `FINAL_VAR(...)` directive from the model's raw
/// response text. Mirrors `find_final_answer` from the reference RLM:
/// the directive must appear at the start of a line. For `FINAL_VAR`,
/// returns `None` (we can't resolve the variable from text alone — the
/// REPL-level `FINAL_VAR()` Python helper handles that path).
fn parse_text_final(text: &str) -> Option<String> {
    // Skip if the FINAL appears inside a code fence — we already handle
    // those via the REPL's `__REPL_FINAL__::` sentinel.
    let outside_fence = strip_code_fences(text);

    for line in outside_fence.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("FINAL_VAR(") {
            // Heuristic: if a single quoted/bareword ident, defer to the
            // REPL path. Otherwise treat the inner literal as the answer.
            let inner = rest.trim_end_matches(')').trim();
            // Treat as "use REPL FINAL_VAR" — return None and let the next
            // round's REPL evaluation resolve it. Currently we just skip;
            // the model can use FINAL(value) for direct text-level exit.
            let _ = inner;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("FINAL(") {
            let inner = rest.trim_end();
            // Take everything up to the LAST closing paren on this line.
            if let Some(end) = inner.rfind(')') {
                let value = inner[..end].trim();
                if !value.is_empty() {
                    return Some(strip_quotes(value));
                }
            }
        }
    }
    None
}

/// Drop content inside ``` … ``` fenced blocks so we don't read FINAL()
/// out of code the model wrote (that path is handled by the REPL sentinel).
fn strip_code_fences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Strip one layer of matching surrounding quotes (`"…"` or `'…'`).
fn strip_quotes(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

/// Truncate text to `max_chars` (counted by Unicode chars), adding an
/// ellipsis if truncated. Char-safe: never splits a multi-byte codepoint.
fn truncate_text(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
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

    fn tmp_state_path(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("deepseek_rlm_test");
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!(
            "test_{}_{}_{}.json",
            label,
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn extract_repl_code_finds_simple_block() {
        let text = "Here's some code:\n```repl\nprint('hello')\n```\nEnd.";
        let code = extract_repl_code(text).unwrap();
        assert_eq!(code, "print('hello')");
    }

    #[test]
    fn extract_repl_code_falls_back_to_python_marker() {
        // Backward compat: if the model still emits ```python we accept it.
        let text = "Code:\n```python\nx = 1 + 2\n```";
        let code = extract_repl_code(text).unwrap();
        assert_eq!(code, "x = 1 + 2");
    }

    #[test]
    fn extract_repl_code_returns_none_when_missing() {
        let text = "Just some text without code fences.";
        assert!(extract_repl_code(text).is_none());
    }

    #[test]
    fn extract_repl_code_returns_none_on_empty_block() {
        let text = "Code:\n```repl\n\n```";
        assert!(extract_repl_code(text).is_none());
    }

    #[test]
    fn extract_repl_code_handles_multiple_blocks() {
        let text = "First:\n```repl\na=1\n```\nSecond:\n```repl\nb=2\n```";
        let code = extract_repl_code(text).unwrap();
        assert_eq!(code, "a=1");
    }

    #[test]
    fn extract_repl_code_ignores_other_fences() {
        let text = "```\nsome text\n```\nActual:\n```repl\nreal_code()\n```";
        let code = extract_repl_code(text).unwrap();
        assert_eq!(code, "real_code()");
    }

    #[test]
    fn build_metadata_contains_key_information() {
        let path = tmp_state_path("meta_basic");
        std::fs::write(&path, "{\"context\":\"Hello, world!\"}").unwrap();
        let prompt = "Hello, world!";
        let msg = build_metadata_message(prompt, None, 0, None, None, &path);
        let text = extract_text_blocks(&msg.content);
        assert!(text.contains("context"));
        assert!(text.contains("Hello, world!"));
        assert!(text.contains("Round 0"));
        assert!(text.contains("llm_query"));
        assert!(text.contains("rlm_query"));
        assert!(text.contains("FINAL"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn build_metadata_lists_state_variables() {
        let path = tmp_state_path("meta_vars");
        std::fs::write(
            &path,
            "{\"context\":\"x\",\"chunk_summaries\":[\"a\"],\"counter\":1}",
        )
        .unwrap();
        let msg = build_metadata_message("x", None, 1, Some("noop"), Some("ok"), &path);
        let text = extract_text_blocks(&msg.content);
        assert!(text.contains("Variables in REPL state"));
        assert!(text.contains("\"context\""));
        assert!(text.contains("\"chunk_summaries\""));
        assert!(text.contains("\"counter\""));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn build_metadata_with_iteration_shows_previous_code() {
        let path = tmp_state_path("meta_prev");
        std::fs::write(&path, "{}").unwrap();
        let msg = build_metadata_message(
            "Test prompt",
            None,
            3,
            Some("print('hi')"),
            Some("hi"),
            &path,
        );
        let text = extract_text_blocks(&msg.content);
        assert!(text.contains("Round 3"));
        assert!(text.contains("print('hi')"));
        assert!(text.contains("hi"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn build_metadata_includes_root_prompt_when_provided() {
        let path = tmp_state_path("meta_root");
        std::fs::write(&path, "{}").unwrap();
        let msg = build_metadata_message(
            "long context text",
            Some("Summarize the security model"),
            1,
            Some("# noop"),
            Some("ok"),
            &path,
        );
        let text = extract_text_blocks(&msg.content);
        assert!(text.contains("Original task"));
        assert!(text.contains("Summarize the security model"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn parse_text_final_extracts_simple_value() {
        let text = "OK here's the answer.\nFINAL(42)\nThanks.";
        assert_eq!(parse_text_final(text).as_deref(), Some("42"));
    }

    #[test]
    fn parse_text_final_strips_quotes() {
        let text = "FINAL(\"the answer is yes\")";
        assert_eq!(parse_text_final(text).as_deref(), Some("the answer is yes"));
    }

    #[test]
    fn parse_text_final_ignores_inside_code_fence() {
        let text =
            "Some prose.\n```repl\n# Note: when ready, call FINAL(value)\nx = 1\n```\nMore prose.";
        assert!(parse_text_final(text).is_none());
    }

    #[test]
    fn parse_text_final_returns_none_when_absent() {
        assert!(parse_text_final("just talking, no final.").is_none());
    }

    #[test]
    fn truncate_text_leaves_short_text_alone() {
        assert_eq!(truncate_text("hello", 100), "hello");
    }

    #[test]
    fn truncate_text_shortens_long_text() {
        let long = "a".repeat(1000);
        let truncated = truncate_text(&long, 10);
        assert_eq!(truncated.chars().count(), 10);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn truncate_text_is_unicode_safe() {
        // 4 multi-byte codepoints, each occupying 3 bytes.
        let s = "日本語テスト"; // 6 chars
        let out = truncate_text(s, 4);
        // Should keep 1 char + "..." == 4 chars total.
        assert_eq!(out.chars().count(), 4);
        assert!(out.ends_with("..."));
        // Must NOT split a codepoint — string is valid utf-8 by construction.
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
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
        let path = tmp_state_path("meta_role");
        std::fs::write(&path, "{}").unwrap();
        let msg = build_metadata_message("test", None, 0, None, None, &path);
        assert_eq!(msg.role, "user");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn summarize_code_keeps_short_unchanged() {
        let s = "a\nb\nc";
        assert_eq!(summarize_code(s), s);
    }

    #[test]
    fn summarize_code_compresses_long() {
        let lines: Vec<String> = (0..20).map(|i| format!("line{i}")).collect();
        let code = lines.join("\n");
        let s = summarize_code(&code);
        assert!(s.starts_with("20 lines:"));
        assert!(s.contains("line0"));
        assert!(s.contains("line3"));
        assert!(s.contains("line19"));
        assert!(s.contains("…"));
    }

    /// End-to-end test: spin up the sidecar with a real httpbin-like loopback,
    /// then drive a python3 process that calls llm_query() and confirm the
    /// HTTP path is wired correctly. We don't talk to a real LLM here — we
    /// stand up a stand-in HTTP server using the same axum stack and just
    /// verify the sidecar URL is reachable from python3.
    ///
    /// This guards against a regression where the sidecar URL doesn't get
    /// exported into the python child's environment.
    #[tokio::test]
    async fn sidecar_url_is_exported_to_python_env() {
        // Stand up a tiny axum server that always replies {"text":"pong"}.
        use axum::{Json, Router, routing::post};
        let app = Router::new().route(
            "/llm",
            post(|| async { Json(serde_json::json!({"text": "pong-from-sidecar"})) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let mut rt = PythonRuntime::with_state_path(tmp_state_path("sidecar_smoke"));
        rt.set_env("REPL_LLM_URL", format!("http://{addr}/llm"));
        let round = rt
            .execute("print(llm_query('hello'))")
            .await
            .expect("execute");
        assert!(
            round.stdout.contains("pong-from-sidecar"),
            "stdout did not contain sidecar reply: {:?}",
            round.stdout
        );
        server.abort();
    }
}
