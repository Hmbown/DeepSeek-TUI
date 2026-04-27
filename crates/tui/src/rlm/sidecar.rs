//! HTTP sidecar that services `llm_query()` and `sub_rlm()` calls made
//! from inside the RLM Python REPL.
//!
//! Why HTTP? The Python REPL runs as a short-lived `python3 -c` subprocess
//! per round. We need synchronous request/response between Python (running)
//! and Rust (servicing the request) — and we need it to work for arbitrary
//! recursion depth. A localhost HTTP server with axum is the cleanest fit:
//! Python calls `urllib.request.urlopen(...)` and blocks until Rust returns
//! the LLM completion. No long-lived process, no FIFO/pipe gymnastics.
//!
//! The sidecar binds to `127.0.0.1:0` (kernel-assigned port), runs for the
//! lifetime of one root `run_rlm_turn`, and is aborted on return.
//!
//! Endpoints:
//! - `POST /llm`  — one-shot child completion via the configured `child_model`.
//! - `POST /rlm`  — full recursive RLM turn at depth-1 (paper's `sub_RLM`).
//!
//! Cumulative token usage is tracked in the shared [`SidecarCtx`] so the
//! parent turn can fold it into its own [`Usage`].

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::client::DeepSeekClient;
use crate::llm_client::LlmClient as _;
use crate::models::{ContentBlock, Message, MessageRequest, Usage};

/// Default per-child request timeout — mirrors `tools/rlm_query.rs`.
const CHILD_TIMEOUT_SECS: u64 = 120;
/// Default `max_tokens` for one-shot child completions.
const DEFAULT_CHILD_MAX_TOKENS: u32 = 4096;

/// Shared state for the sidecar — the LLM client, the child model name,
/// the recursion budget, and a usage accumulator.
pub struct SidecarCtx {
    pub client: DeepSeekClient,
    pub child_model: String,
    /// Recursion budget remaining for `/rlm` calls. `0` means "no further
    /// recursion" — `/rlm` will return an error.
    pub depth_remaining: u32,
    /// Cumulative usage across all sidecar-served calls in this turn.
    pub usage: Mutex<Usage>,
}

impl SidecarCtx {
    pub fn new(client: DeepSeekClient, child_model: String, depth_remaining: u32) -> Arc<Self> {
        Arc::new(Self {
            client,
            child_model,
            depth_remaining,
            usage: Mutex::new(Usage::default()),
        })
    }
}

#[derive(Deserialize)]
struct LlmReq {
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    system: Option<String>,
}

#[derive(Serialize)]
struct LlmResp {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn llm_handler(State(ctx): State<Arc<SidecarCtx>>, Json(req): Json<LlmReq>) -> Json<LlmResp> {
    let model = req
        .model
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| ctx.child_model.clone());
    let max_tokens = req.max_tokens.unwrap_or(DEFAULT_CHILD_MAX_TOKENS);

    let request = MessageRequest {
        model,
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: req.prompt,
                cache_control: None,
            }],
        }],
        max_tokens,
        system: req.system.map(crate::models::SystemPrompt::Text),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: None,
        stream: Some(false),
        temperature: Some(0.4_f32),
        top_p: Some(0.9_f32),
    };

    let fut = ctx.client.create_message(request);
    let response =
        match tokio::time::timeout(std::time::Duration::from_secs(CHILD_TIMEOUT_SECS), fut).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                return Json(LlmResp {
                    text: String::new(),
                    error: Some(format!("llm_query failed: {e}")),
                });
            }
            Err(_) => {
                return Json(LlmResp {
                    text: String::new(),
                    error: Some(format!("llm_query timed out after {CHILD_TIMEOUT_SECS}s")),
                });
            }
        };

    let text = response
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    {
        let mut u = ctx.usage.lock().await;
        u.input_tokens = u.input_tokens.saturating_add(response.usage.input_tokens);
        u.output_tokens = u.output_tokens.saturating_add(response.usage.output_tokens);
    }

    Json(LlmResp { text, error: None })
}

#[derive(Deserialize)]
struct SubRlmReq {
    prompt: String,
}

async fn sub_rlm_handler(
    State(ctx): State<Arc<SidecarCtx>>,
    Json(req): Json<SubRlmReq>,
) -> Json<LlmResp> {
    if ctx.depth_remaining == 0 {
        return Json(LlmResp {
            text: String::new(),
            error: Some(
                "sub_rlm: recursion depth budget exhausted (configure /rlm with deeper budget)"
                    .to_string(),
            ),
        });
    }

    // Sub-RLM uses the child_model as its own root model — paper's pattern
    // is to run sub_RLM with a smaller model, and to also use child_model
    // for any further `llm_query` calls inside the sub-turn.
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

    // The recursive future-type cycle here
    // (sub_rlm_handler → run_rlm_turn_inner → start_sidecar → sub_rlm_handler)
    // is broken by `run_rlm_turn_inner` returning a concrete
    // `Pin<Box<dyn Future + Send>>` rather than `impl Future`.
    let result = super::turn::run_rlm_turn_inner(
        &ctx.client,
        ctx.child_model.clone(),
        req.prompt,
        ctx.child_model.clone(),
        tx,
        ctx.depth_remaining.saturating_sub(1),
    )
    .await;

    drain.abort();

    {
        let mut u = ctx.usage.lock().await;
        u.input_tokens = u.input_tokens.saturating_add(result.usage.input_tokens);
        u.output_tokens = u.output_tokens.saturating_add(result.usage.output_tokens);
    }

    Json(LlmResp {
        text: result.answer,
        error: result.error,
    })
}

/// Result of starting the sidecar — the bound socket address and the task
/// handle. Drop or abort the handle to stop the server.
pub struct SidecarHandle {
    pub addr: SocketAddr,
    task: JoinHandle<()>,
}

impl SidecarHandle {
    pub fn llm_url(&self) -> String {
        format!("http://{}/llm", self.addr)
    }
    pub fn rlm_url(&self) -> String {
        format!("http://{}/rlm", self.addr)
    }
    pub fn shutdown(self) {
        self.task.abort();
    }
}

/// Bind a sidecar on `127.0.0.1` with a kernel-assigned port and start
/// serving in a background task.
pub async fn start_sidecar(ctx: Arc<SidecarCtx>) -> std::io::Result<SidecarHandle> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let app = Router::new()
        .route("/llm", post(llm_handler))
        .route("/rlm", post(sub_rlm_handler))
        .with_state(ctx);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(SidecarHandle { addr, task })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_resp_skips_none_error() {
        let r = LlmResp {
            text: "hello".to_string(),
            error: None,
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("error"));
        assert!(s.contains("hello"));
    }

    #[test]
    fn llm_resp_includes_error_when_set() {
        let r = LlmResp {
            text: String::new(),
            error: Some("boom".to_string()),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("boom"));
    }
}
