//! Remote attach client — connects the TUI to a remote `deepseek serve --http`
//! instance, proxying turns over the wire.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::llm_client::{LlmClient, StreamEventBox};
use crate::models::{MessageRequest, MessageResponse};
use crate::logging;

/// Establish a connection to a remote runtime API server and return an
/// attach client that proxies the TUI through it.
pub async fn connect(remote_url: &str) -> Result<AttachClient> {
    let base_url = remote_url.trim_end_matches('/').to_string();

    // Validate connectivity by hitting /health
    let health_url = format!("{base_url}/health");
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .build()
        .context("failed to build HTTP client for remote attach")?;

    let resp = client
        .get(&health_url)
        .send()
        .await
        .with_context(|| format!("failed to connect to remote server at {remote_url}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        bail!("remote server at {remote_url} returned HTTP {status}");
    }

    let health_body: HealthResponse = resp
        .json()
        .await
        .context("failed to parse health response from remote server")?;

    if health_body.service != "deepseek-runtime-api" && health_body.service != "deepseek-app-server" {
        logging::warn(format!(
            "remote service '{}' at {remote_url} is not a known DeepSeek server type",
            health_body.service
        ));
    }

    logging::info(format!(
        "attached to remote {} at {remote_url} (status: {})",
        health_body.service, health_body.status
    ));

    Ok(AttachClient {
        base_url,
        http_client: client,
        thread_id: None,
    })
}

/// Client that proxies LLM calls through a remote `deepseek serve --http` server.
#[derive(Clone)]
pub struct AttachClient {
    base_url: String,
    http_client: reqwest::Client,
    #[allow(dead_code)]
    thread_id: Option<String>,
}

impl AttachClient {
    /// Create a new thread on the remote server.
    #[allow(dead_code)]
    pub async fn create_thread(&self, config: &Config) -> Result<String> {
        let url = format!("{}/v1/threads", self.base_url);
        let model = config
            .default_text_model
            .clone()
            .unwrap_or_else(|| crate::config::DEFAULT_TEXT_MODEL.to_string());

        let body = serde_json::json!({
            "model": model,
            "mode": "agent",
            "workspace": std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        });

        let resp = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("failed to create remote thread")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("remote server returned HTTP {status} creating thread: {text}");
        }

        let thread: ThreadRecord = resp
            .json()
            .await
            .context("failed to parse thread response from remote server")?;

        logging::info(format!("created remote thread {}", thread.id));
        Ok(thread.id)
    }

    /// Send a turn to the remote server and stream back SSE events.
    #[allow(dead_code)]
    pub async fn stream_turn(
        &self,
        prompt: &str,
        thread_id: &str,
    ) -> Result<mpsc::Receiver<Result<SseEvent, String>>> {
        let url = format!("{}/v1/threads/{thread_id}/turns", self.base_url);

        let body = serde_json::json!({
            "input": prompt,
        });

        let response = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("failed to send turn to remote server")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            bail!("remote server returned HTTP {status} starting turn: {text}");
        }

        // The response creates a turn on the server; now we stream events

        let events_url = format!(
            "{}/v1/threads/{}/events?since_seq=0",
            self.base_url, thread_id
        );

        let (tx, rx) = mpsc::channel(256);

        let http_client = self.http_client.clone();
        tokio::spawn(async move {
            match http_client.get(&events_url).send().await {
                Ok(resp) => {
                    let mut stream = resp.bytes_stream();
                    let mut buf = Vec::new();
                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                buf.extend_from_slice(&chunk);
                                // Process SSE lines from the buffer
                                while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                                    let event_block = buf[..pos].to_vec();
                                    buf.drain(..=pos + 1);

                                    let text = String::from_utf8_lossy(&event_block);
                                    for line in text.lines() {
                                        if let Some(data) = line.strip_prefix("data: ") {
                                            let event = SseEvent {
                                                event: "message".to_string(),
                                                data: data.to_string(),
                                            };
                                            if tx.blocking_send(Ok(event)).is_err() {
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                let _ = tx
                                    .blocking_send(Err(format!("stream error: {err}")))
                                    .is_ok();
                                return;
                            }
                        }
                    }
                }
                Err(err) => {
                    let _ = tx
                        .blocking_send(Err(format!("events request failed: {err}")))
                        .is_ok();
                }
            }
        });

        logging::info(format!("streaming events from thread {thread_id}"));

        Ok(rx)
    }

    /// Health-check and return whether the remote is reachable.
    pub async fn ping(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        match self.http_client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

impl LlmClient for AttachClient {
    fn provider_name(&self) -> &'static str {
        "remote-attach"
    }

    fn model(&self) -> &str {
        "remote"
    }

    async fn health_check(&self) -> Result<bool> {
        self.ping().await
    }

    async fn create_message(&self, _request: MessageRequest) -> Result<MessageResponse> {
        // Remote attach uses streaming; fallback to a simple proxy via
        // the non-streaming turn endpoint.
        Err(anyhow!(
            "remote attach requires streaming — use create_message_stream"
        ))
    }

    async fn create_message_stream(&self, _request: MessageRequest) -> Result<StreamEventBox> {
        Err(anyhow!(
            "remote attach requires an active thread — use the TUI attach flow"
        ))
    }
}

// === Types shared with the remote API ===

#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
    service: String,
    #[allow(dead_code)]
    mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ThreadRecord {
    id: String,
    model: String,
    mode: String,
    workspace: PathBuf,
    archived: bool,
}

/// Parsed SSE event from the remote server.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}
