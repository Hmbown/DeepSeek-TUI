//! RPC bridge that services `llm_query` / `rlm_query` calls coming back
//! from the long-lived Python REPL during an RLM turn.
//!
//! This is the spiritual successor to the HTTP sidecar from earlier
//! versions — except instead of binding a localhost port and routing
//! through `urllib`, requests come in through stdin/stdout and we just
//! call the LLM client directly here in Rust.
//!
//! The bridge tracks cumulative token usage and the recursion budget. For
//! `Rlm` / `RlmBatch` requests it recursively calls `run_rlm_turn_inner`
//! at depth-1; the future-type cycle (bridge → run_rlm_turn_inner →
//! bridge) is broken by `run_rlm_turn_inner` returning a boxed dyn future.

use std::sync::Arc;
use std::time::Duration;

use futures_util::future::join_all;
use tokio::sync::Mutex;

use crate::client::DeepSeekClient;
use crate::llm_client::LlmClient as _;
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt, Usage};
use crate::repl::runtime::{BatchResp, RpcDispatcher, RpcRequest, RpcResponse, SingleResp};

/// Per-child completion timeout — same as the previous sidecar default.
const CHILD_TIMEOUT_SECS: u64 = 120;
/// Default `max_tokens` for one-shot child completions.
const DEFAULT_CHILD_MAX_TOKENS: u32 = 4096;
/// Hard cap on prompts per batch RPC.
pub const MAX_BATCH: usize = 16;

/// State shared with the bridge across all RPC calls in one turn.
pub struct RlmBridge {
    pub client: DeepSeekClient,
    pub child_model: String,
    /// Recursion budget remaining for `Rlm` / `RlmBatch` requests. When
    /// zero, those requests fall back to plain `Llm` completions.
    pub depth_remaining: u32,
    pub usage: Arc<Mutex<Usage>>,
}

impl RlmBridge {
    pub fn new(client: DeepSeekClient, child_model: String, depth_remaining: u32) -> Self {
        Self {
            client,
            child_model,
            depth_remaining,
            usage: Arc::new(Mutex::new(Usage::default())),
        }
    }

    pub fn usage_handle(&self) -> Arc<Mutex<Usage>> {
        Arc::clone(&self.usage)
    }

    async fn dispatch_llm(
        &self,
        prompt: String,
        model: Option<String>,
        max_tokens: Option<u32>,
        system: Option<String>,
    ) -> SingleResp {
        let request = MessageRequest {
            model: model
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| self.child_model.clone()),
            messages: vec![Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: prompt,
                    cache_control: None,
                }],
            }],
            max_tokens: max_tokens.unwrap_or(DEFAULT_CHILD_MAX_TOKENS),
            system: system.map(SystemPrompt::Text),
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: Some(false),
            temperature: Some(0.4_f32),
            top_p: Some(0.9_f32),
        };

        let fut = self.client.create_message(request);
        let response =
            match tokio::time::timeout(Duration::from_secs(CHILD_TIMEOUT_SECS), fut).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    return SingleResp {
                        text: String::new(),
                        error: Some(format!("llm_query failed: {e}")),
                    };
                }
                Err(_) => {
                    return SingleResp {
                        text: String::new(),
                        error: Some(format!("llm_query timed out after {CHILD_TIMEOUT_SECS}s")),
                    };
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
            let mut u = self.usage.lock().await;
            u.input_tokens = u.input_tokens.saturating_add(response.usage.input_tokens);
            u.output_tokens = u.output_tokens.saturating_add(response.usage.output_tokens);
        }

        SingleResp { text, error: None }
    }

    async fn dispatch_llm_batch(&self, prompts: Vec<String>, model: Option<String>) -> BatchResp {
        if prompts.is_empty() {
            return BatchResp { results: vec![] };
        }
        if prompts.len() > MAX_BATCH {
            return BatchResp {
                results: prompts
                    .iter()
                    .map(|_| SingleResp {
                        text: String::new(),
                        error: Some(format!("batch too large: {} > {MAX_BATCH}", prompts.len())),
                    })
                    .collect(),
            };
        }

        let model = Arc::new(
            model
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| self.child_model.clone()),
        );

        let futures = prompts.into_iter().map(|prompt| {
            let model = Arc::clone(&model);
            async move {
                self.dispatch_llm((*prompt).to_string(), Some((*model).clone()), None, None)
                    .await
            }
        });

        BatchResp {
            results: join_all(futures).await,
        }
    }

    async fn dispatch_rlm(&self, prompt: String, model: Option<String>) -> SingleResp {
        if self.depth_remaining == 0 {
            // Budget exhausted — fall back to a one-shot child completion
            // rather than returning an error. Matches the paper's behaviour
            // ("sub_RLM gracefully degrades to llm_query at depth=0").
            return self.dispatch_llm(prompt, model, None, None).await;
        }

        // Build a drain channel to absorb status events from the nested
        // turn (we don't surface them; this dispatch is invisible to the
        // outer agent stream).
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        let child_model = model
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| self.child_model.clone());

        // Recursive call. The dyn-erasure on `run_rlm_turn_inner` breaks
        // the `bridge → turn → bridge` opaque-future cycle.
        let result = super::turn::run_rlm_turn_inner(
            &self.client,
            child_model.clone(),
            prompt,
            None,
            child_model,
            tx,
            self.depth_remaining.saturating_sub(1),
        )
        .await;

        drain.abort();

        {
            let mut u = self.usage.lock().await;
            u.input_tokens = u.input_tokens.saturating_add(result.usage.input_tokens);
            u.output_tokens = u.output_tokens.saturating_add(result.usage.output_tokens);
        }

        SingleResp {
            text: result.answer,
            error: result.error,
        }
    }

    async fn dispatch_rlm_batch(&self, prompts: Vec<String>, model: Option<String>) -> BatchResp {
        if prompts.is_empty() {
            return BatchResp { results: vec![] };
        }
        if prompts.len() > MAX_BATCH {
            return BatchResp {
                results: prompts
                    .iter()
                    .map(|_| SingleResp {
                        text: String::new(),
                        error: Some(format!("batch too large: {} > {MAX_BATCH}", prompts.len())),
                    })
                    .collect(),
            };
        }

        let model = Arc::new(model);
        let futures = prompts.into_iter().map(|p| {
            let model = Arc::clone(&model);
            async move { self.dispatch_rlm(p, (*model).clone()).await }
        });
        BatchResp {
            results: join_all(futures).await,
        }
    }
}

impl RpcDispatcher for RlmBridge {
    fn dispatch<'a>(
        &'a self,
        req: RpcRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = RpcResponse> + Send + 'a>> {
        Box::pin(async move {
            match req {
                RpcRequest::Llm {
                    prompt,
                    model,
                    max_tokens,
                    system,
                } => {
                    RpcResponse::Single(self.dispatch_llm(prompt, model, max_tokens, system).await)
                }
                RpcRequest::LlmBatch { prompts, model } => {
                    RpcResponse::Batch(self.dispatch_llm_batch(prompts, model).await)
                }
                RpcRequest::Rlm { prompt, model } => {
                    RpcResponse::Single(self.dispatch_rlm(prompt, model).await)
                }
                RpcRequest::RlmBatch { prompts, model } => {
                    RpcResponse::Batch(self.dispatch_rlm_batch(prompts, model).await)
                }
            }
        })
    }
}
