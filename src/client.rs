//! HTTP client for the DeepSeek OpenAI-compatible APIs.
//!
//! Uses the OpenAI Responses API when available, falling back to Chat Completions
//! if the Responses endpoint is unsupported by the target base URL.

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex as AsyncMutex;

use crate::config::{Config, DEFAULT_TEXT_MODEL, RetryPolicy};
use crate::llm_client::{
    LlmClient, LlmError, RetryConfig as LlmRetryConfig, StreamEventBox, extract_retry_after,
    with_retry,
};
use crate::logging;
use crate::models::{
    ContentBlock, ContentBlockStart, Delta, Message, MessageDelta, MessageRequest, MessageResponse,
    ServerToolUsage, StreamEvent, SystemPrompt, Tool, ToolCaller, Usage,
};

fn to_api_tool_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if ch == '-' {
            out.push_str("--");
        } else {
            out.push_str("-x");
            out.push_str(&format!("{:06X}", ch as u32));
            out.push('-');
        }
    }
    out
}

fn from_api_tool_name(name: &str) -> String {
    let mut out = String::new();
    let mut iter = name.chars().peekable();
    while let Some(ch) = iter.next() {
        if ch != '-' {
            out.push(ch);
            continue;
        }
        if let Some('-') = iter.peek().copied() {
            iter.next();
            out.push('-');
            continue;
        }
        if iter.peek().copied() == Some('x') {
            iter.next();
            let mut hex = String::new();
            for _ in 0..6 {
                if let Some(h) = iter.next() {
                    hex.push(h);
                } else {
                    break;
                }
            }
            if let Ok(code) = u32::from_str_radix(&hex, 16)
                && let Some(decoded) = std::char::from_u32(code)
            {
                if let Some('-') = iter.peek().copied() {
                    iter.next();
                }
                out.push(decoded);
                continue;
            }
            out.push('-');
            out.push('x');
            out.push_str(&hex);
            continue;
        }
        out.push('-');
    }

    // Second pass: decode bare hex escapes (e.g. `x00002E`) that the model
    // may produce when it mangles the `-x00002E-` delimiter form.  Only
    // decode when the resulting character is one that `to_api_tool_name`
    // would have encoded (not alphanumeric, not `_`, not `-`).
    decode_bare_hex_escapes(&out)
}

/// Decode bare `x[0-9A-Fa-f]{6}` sequences (optionally followed by `-`)
/// that survive the standard delimiter-based pass.  This handles cases
/// where the model strips or replaces the leading `-` of `-x00002E-`.
fn decode_bare_hex_escapes(input: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;

    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"x([0-9A-Fa-f]{6})-?").unwrap());

    let result = re.replace_all(input, |caps: &regex::Captures| {
        let hex = &caps[1];
        if let Ok(code) = u32::from_str_radix(hex, 16)
            && let Some(decoded) = std::char::from_u32(code)
        {
            // Only decode characters that to_api_tool_name would have encoded
            if !decoded.is_ascii_alphanumeric() && decoded != '_' && decoded != '-' {
                return decoded.to_string();
            }
        }
        // Not a character we'd encode — leave as-is
        caps[0].to_string()
    });
    result.into_owned()
}

// === Types ===

/// Model descriptor returned by the provider's `/v1/models` endpoint.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AvailableModel {
    pub id: String,
    pub owned_by: Option<String>,
    pub created: Option<u64>,
}

/// Client for DeepSeek's OpenAI-compatible APIs.
#[must_use]
pub struct DeepSeekClient {
    http_client: reqwest::Client,
    api_key: String,
    base_url: String,
    retry: RetryPolicy,
    default_model: String,
    use_chat_completions: AtomicBool,
    /// Counter of chat-completions requests since last Responses API probe.
    /// After RESPONSES_RECOVERY_INTERVAL requests, we retry the Responses API.
    chat_fallback_counter: AtomicU32,
    connection_health: Arc<AsyncMutex<ConnectionHealth>>,
    rate_limiter: Arc<AsyncMutex<TokenBucket>>,
}

/// After this many chat-completions requests, retry the Responses API to see
/// if it has recovered.
const RESPONSES_RECOVERY_INTERVAL: u32 = 20;
const CONNECTION_FAILURE_THRESHOLD: u32 = 2;
const RECOVERY_PROBE_COOLDOWN: Duration = Duration::from_secs(15);

const DEFAULT_CLIENT_RATE_LIMIT_RPS: f64 = 8.0;
const DEFAULT_CLIENT_RATE_LIMIT_BURST: f64 = 16.0;
const ALLOW_INSECURE_HTTP_ENV: &str = "DEEPSEEK_ALLOW_INSECURE_HTTP";

const SSE_BACKPRESSURE_HIGH_WATERMARK: usize = 8 * 1024 * 1024; // 8 MB
const SSE_BACKPRESSURE_SLEEP_MS: u64 = 10;
const SSE_MAX_LINES_PER_CHUNK: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Healthy,
    Degraded,
    Recovering,
}

#[derive(Debug)]
struct ConnectionHealth {
    state: ConnectionState,
    consecutive_failures: u32,
    last_failure: Option<Instant>,
    last_success: Option<Instant>,
    last_probe: Option<Instant>,
}

impl Default for ConnectionHealth {
    fn default() -> Self {
        Self {
            state: ConnectionState::Healthy,
            consecutive_failures: 0,
            last_failure: None,
            last_success: None,
            last_probe: None,
        }
    }
}

#[derive(Debug)]
struct TokenBucket {
    enabled: bool,
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn from_env() -> Self {
        let rps = std::env::var("DEEPSEEK_RATE_LIMIT_RPS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_CLIENT_RATE_LIMIT_RPS)
            .max(0.0);
        let burst = std::env::var("DEEPSEEK_RATE_LIMIT_BURST")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_CLIENT_RATE_LIMIT_BURST)
            .max(1.0);
        let enabled = rps > 0.0;
        Self {
            enabled,
            capacity: burst,
            tokens: burst,
            refill_per_sec: rps,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self, now: Instant) {
        if !self.enabled {
            return;
        }
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
    }

    fn delay_until_available(&mut self, tokens: f64) -> Option<Duration> {
        if !self.enabled {
            return None;
        }
        let now = Instant::now();
        self.refill(now);
        if self.tokens >= tokens {
            self.tokens -= tokens;
            return None;
        }
        let needed = tokens - self.tokens;
        self.tokens = 0.0;
        if self.refill_per_sec <= 0.0 {
            return Some(Duration::from_secs(1));
        }
        Some(Duration::from_secs_f64(needed / self.refill_per_sec))
    }
}

fn apply_request_success(health: &mut ConnectionHealth, now: Instant) -> bool {
    let recovered = health.state != ConnectionState::Healthy;
    health.state = ConnectionState::Healthy;
    health.consecutive_failures = 0;
    health.last_success = Some(now);
    recovered
}

fn apply_request_failure(health: &mut ConnectionHealth, now: Instant) {
    health.consecutive_failures = health.consecutive_failures.saturating_add(1);
    health.last_failure = Some(now);
    if health.consecutive_failures >= CONNECTION_FAILURE_THRESHOLD {
        health.state = ConnectionState::Degraded;
    }
}

fn mark_recovery_probe_if_due(health: &mut ConnectionHealth, now: Instant) -> bool {
    if health.state == ConnectionState::Healthy {
        return false;
    }
    if health
        .last_probe
        .is_some_and(|last| now.duration_since(last) < RECOVERY_PROBE_COOLDOWN)
    {
        return false;
    }
    health.last_probe = Some(now);
    health.state = ConnectionState::Recovering;
    true
}

fn buffer_pool() -> &'static StdMutex<Vec<Vec<u8>>> {
    static POOL: OnceLock<StdMutex<Vec<Vec<u8>>>> = OnceLock::new();
    POOL.get_or_init(|| StdMutex::new(Vec::new()))
}

fn acquire_stream_buffer() -> Vec<u8> {
    if let Ok(mut pool) = buffer_pool().lock() {
        pool.pop().unwrap_or_else(|| Vec::with_capacity(8192))
    } else {
        Vec::with_capacity(8192)
    }
}

fn release_stream_buffer(mut buf: Vec<u8>) {
    buf.clear();
    if buf.capacity() > 256 * 1024 {
        buf.shrink_to(256 * 1024);
    }
    if let Ok(mut pool) = buffer_pool().lock() {
        if pool.len() < 8 {
            pool.push(buf);
        }
    }
}

impl Clone for DeepSeekClient {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            retry: self.retry.clone(),
            default_model: self.default_model.clone(),
            use_chat_completions: AtomicBool::new(
                self.use_chat_completions.load(Ordering::Relaxed),
            ),
            chat_fallback_counter: AtomicU32::new(
                self.chat_fallback_counter.load(Ordering::Relaxed),
            ),
            connection_health: self.connection_health.clone(),
            rate_limiter: self.rate_limiter.clone(),
        }
    }
}

// === Helpers ===

/// Maximum bytes to read from an error response body (64 KB).
const ERROR_BODY_MAX_BYTES: usize = 64 * 1024;

/// Read an error response body with a size limit to prevent unbounded allocation.
async fn bounded_error_text(response: reqwest::Response, max_bytes: usize) -> String {
    match response.bytes().await {
        Ok(bytes) => {
            let truncated = &bytes[..bytes.len().min(max_bytes)];
            String::from_utf8_lossy(truncated).into_owned()
        }
        Err(_) => String::new(),
    }
}

fn validate_base_url_security(base_url: &str) -> Result<()> {
    if base_url.starts_with("https://")
        || base_url.starts_with("http://localhost")
        || base_url.starts_with("http://127.0.0.1")
        || base_url.starts_with("http://[::1]")
    {
        return Ok(());
    }

    if base_url.starts_with("http://")
        && std::env::var(ALLOW_INSECURE_HTTP_ENV)
            .ok()
            .as_deref()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    {
        logging::warn(format!(
            "Using insecure HTTP base URL because {} is set",
            ALLOW_INSECURE_HTTP_ENV
        ));
        return Ok(());
    }

    if base_url.starts_with("http://") {
        anyhow::bail!(
            "Refusing insecure base URL '{}'. Use HTTPS or set {}=1 to override for trusted environments.",
            base_url,
            ALLOW_INSECURE_HTTP_ENV
        );
    }

    Ok(())
}

// === DeepSeekClient ===

impl DeepSeekClient {
    /// Create a DeepSeek client from CLI configuration.
    pub fn new(config: &Config) -> Result<Self> {
        let api_key = config.deepseek_api_key()?;
        let base_url = config.deepseek_base_url();
        validate_base_url_security(&base_url)?;
        let retry = config.retry_policy();
        let default_model = config
            .default_text_model
            .clone()
            .unwrap_or_else(|| DEFAULT_TEXT_MODEL.to_string());

        logging::info(format!("DeepSeek base URL: {base_url}"));
        logging::info(format!(
            "Retry policy: enabled={}, max_retries={}, initial_delay={}s, max_delay={}s",
            retry.enabled, retry.max_retries, retry.initial_delay, retry.max_delay
        ));

        let http_client = Self::build_http_client(&api_key)?;

        Ok(Self {
            http_client,
            api_key,
            base_url,
            retry,
            default_model,
            use_chat_completions: AtomicBool::new(false),
            chat_fallback_counter: AtomicU32::new(0),
            connection_health: Arc::new(AsyncMutex::new(ConnectionHealth::default())),
            rate_limiter: Arc::new(AsyncMutex::new(TokenBucket::from_env())),
        })
    }

    fn build_http_client(api_key: &str) -> Result<reqwest::Client> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {api_key}"))?,
        );
        reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(300))
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .build()
            .map_err(Into::into)
    }

    /// List available models from the provider.
    pub async fn list_models(&self) -> Result<Vec<AvailableModel>> {
        let url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));
        let response = self.send_with_retry(|| self.http_client.get(&url)).await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            anyhow::bail!("Failed to list models: HTTP {status}: {error_text}");
        }
        let response_text = response.text().await.unwrap_or_default();

        parse_models_response(&response_text)
    }

    async fn wait_for_rate_limit(&self) {
        let maybe_delay = {
            let mut limiter = self.rate_limiter.lock().await;
            limiter.delay_until_available(1.0)
        };
        if let Some(delay) = maybe_delay {
            tokio::time::sleep(delay).await;
        }
    }

    async fn mark_request_success(&self) {
        let mut health = self.connection_health.lock().await;
        if apply_request_success(&mut health, Instant::now()) {
            logging::info("Connection recovered");
        }
    }

    async fn mark_request_failure(&self, reason: &str) {
        let mut health = self.connection_health.lock().await;
        apply_request_failure(&mut health, Instant::now());
        logging::warn(format!(
            "Connection degraded (failures={}): {}",
            health.consecutive_failures, reason
        ));
    }

    async fn maybe_probe_recovery(&self) {
        let should_probe = {
            let mut health = self.connection_health.lock().await;
            mark_recovery_probe_if_due(&mut health, Instant::now())
        };
        if !should_probe {
            return;
        }
        let health_url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));
        let probe = self.http_client.get(health_url).send().await;
        match probe {
            Ok(resp) if resp.status().is_success() => {
                self.mark_request_success().await;
                logging::info("Recovery probe succeeded");
            }
            Ok(resp) => {
                self.mark_request_failure(&format!("probe status={}", resp.status()))
                    .await;
            }
            Err(err) => {
                self.mark_request_failure(&format!("probe error={err}"))
                    .await;
            }
        }
    }

    async fn send_with_retry<F>(&self, mut build: F) -> Result<reqwest::Response>
    where
        F: FnMut() -> reqwest::RequestBuilder,
    {
        let retry_cfg: LlmRetryConfig = self.retry.clone().into();
        let request_result = with_retry(
            &retry_cfg,
            || {
                let request = build();
                async move {
                    self.wait_for_rate_limit().await;
                    let response = request
                        .send()
                        .await
                        .map_err(|err| LlmError::from_reqwest(&err))?;
                    let status = response.status();
                    if status.is_success() {
                        return Ok(response);
                    }
                    let retryable = status.as_u16() == 429 || status.is_server_error();
                    if !retryable {
                        return Ok(response);
                    }
                    let retry_after = extract_retry_after(response.headers());
                    let body = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
                    Err(LlmError::from_http_response_with_retry_after(
                        status.as_u16(),
                        &body,
                        retry_after,
                    ))
                }
            },
            Some(Box::new(|err, attempt, delay| {
                logging::warn(format!(
                    "HTTP retry reason={} attempt={} delay={:.2}s",
                    match err {
                        LlmError::RateLimited { .. } => "rate_limited",
                        LlmError::ServerError { .. } => "server_error",
                        LlmError::NetworkError(_) => "network_error",
                        LlmError::Timeout(_) => "timeout",
                        _ => "other",
                    },
                    attempt + 1,
                    delay.as_secs_f64(),
                ));
            })),
        )
        .await;

        match request_result {
            Ok(response) => {
                self.mark_request_success().await;
                Ok(response)
            }
            Err(err) => {
                self.mark_request_failure(&err.to_string()).await;
                self.maybe_probe_recovery().await;
                Err(anyhow::anyhow!(err.to_string()))
            }
        }
    }

    async fn create_message_responses(
        &self,
        request: &MessageRequest,
    ) -> Result<Result<MessageResponse, ResponsesFallback>> {
        let mut body = json!({
            "model": request.model,
            "input": build_responses_input(&request.messages),
            "store": false,
            "max_output_tokens": request.max_tokens,
        });

        if let Some(instructions) = system_to_instructions(request.system.clone()) {
            body["instructions"] = json!(instructions);
        }
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(top_p) = request.top_p {
            body["top_p"] = json!(top_p);
        }
        if let Some(tools) = request.tools.as_ref() {
            body["tools"] = json!(tools.iter().map(tool_to_responses).collect::<Vec<_>>());
        }
        if let Some(choice) = request.tool_choice.as_ref() {
            body["tool_choice"] = choice.clone();
        }

        let url = format!("{}/v1/responses", self.base_url.trim_end_matches('/'));
        let response = self
            .send_with_retry(|| self.http_client.post(&url).json(&body))
            .await?;

        let status = response.status();

        if status.as_u16() == 404 || status.as_u16() == 405 {
            let body = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            return Ok(Err(ResponsesFallback {
                status: status.as_u16(),
                body,
            }));
        }

        if !status.is_success() {
            let error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            anyhow::bail!("Failed to call DeepSeek Responses API: HTTP {status}: {error_text}");
        }

        let response_text = response.text().await.unwrap_or_default();
        let value: Value =
            serde_json::from_str(&response_text).context("Failed to parse Responses API JSON")?;
        let message = parse_responses_message(&value)?;
        Ok(Ok(message))
    }

    async fn create_message_chat(&self, request: &MessageRequest) -> Result<MessageResponse> {
        let messages =
            build_chat_messages(request.system.as_ref(), &request.messages, &request.model);
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens,
        });

        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(top_p) = request.top_p {
            body["top_p"] = json!(top_p);
        }
        if let Some(tools) = request.tools.as_ref() {
            body["tools"] = json!(tools.iter().map(tool_to_chat).collect::<Vec<_>>());
        }
        if let Some(choice) = request.tool_choice.as_ref() {
            if let Some(mapped) = map_tool_choice_for_chat(choice) {
                body["tool_choice"] = mapped;
            }
        }

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let response = self
            .send_with_retry(|| self.http_client.post(&url).json(&body))
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            anyhow::bail!("Failed to call DeepSeek Chat API: HTTP {status}: {error_text}");
        }

        let response_text = response.text().await.unwrap_or_default();
        let value: Value =
            serde_json::from_str(&response_text).context("Failed to parse Chat API JSON")?;
        parse_chat_message(&value)
    }
}

// === Trait Implementations ===

impl LlmClient for DeepSeekClient {
    fn provider_name(&self) -> &'static str {
        "deepseek"
    }

    fn model(&self) -> &str {
        &self.default_model
    }

    async fn health_check(&self) -> Result<bool> {
        let health_url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));
        self.wait_for_rate_limit().await;
        let response = self.http_client.get(health_url).send().await;
        match response {
            Ok(resp) if resp.status().is_success() => {
                self.mark_request_success().await;
                Ok(true)
            }
            Ok(resp) => {
                self.mark_request_failure(&format!("health status={}", resp.status()))
                    .await;
                Ok(false)
            }
            Err(err) => {
                self.mark_request_failure(&format!("health error={err}"))
                    .await;
                Ok(false)
            }
        }
    }

    async fn create_message(&self, request: MessageRequest) -> Result<MessageResponse> {
        // Check if it's time to probe Responses API recovery
        if self.use_chat_completions.load(Ordering::Relaxed) {
            let count = self.chat_fallback_counter.fetch_add(1, Ordering::Relaxed);
            if count > 0 && count.is_multiple_of(RESPONSES_RECOVERY_INTERVAL) {
                logging::info("Probing Responses API recovery...");
                let request_clone = request.clone();
                match self.create_message_responses(&request).await? {
                    Ok(message) => {
                        logging::info("Responses API recovered! Switching back.");
                        self.use_chat_completions.store(false, Ordering::Relaxed);
                        self.chat_fallback_counter.store(0, Ordering::Relaxed);
                        return Ok(message);
                    }
                    Err(_) => {
                        logging::info("Responses API still unavailable, continuing with chat.");
                    }
                }
                return self.create_message_chat(&request_clone).await;
            }
            return self.create_message_chat(&request).await;
        }

        let request_clone = request.clone();
        match self.create_message_responses(&request).await? {
            Ok(message) => Ok(message),
            Err(fallback) => {
                logging::warn(format!(
                    "Responses API unavailable (HTTP {}). Falling back to chat completions.",
                    fallback.status
                ));
                logging::info(format!(
                    "Responses fallback body: {}",
                    crate::utils::truncate_with_ellipsis(&fallback.body, 500, "...")
                ));
                self.use_chat_completions.store(true, Ordering::Relaxed);
                self.chat_fallback_counter.store(0, Ordering::Relaxed);
                self.create_message_chat(&request_clone).await
            }
        }
    }

    async fn create_message_stream(&self, request: MessageRequest) -> Result<StreamEventBox> {
        // Try true SSE streaming via chat completions (widely supported)
        let messages =
            build_chat_messages(request.system.as_ref(), &request.messages, &request.model);
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens,
            "stream": true,
        });

        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(top_p) = request.top_p {
            body["top_p"] = json!(top_p);
        }
        if let Some(tools) = request.tools.as_ref() {
            body["tools"] = json!(tools.iter().map(tool_to_chat).collect::<Vec<_>>());
        }
        if let Some(choice) = request.tool_choice.as_ref() {
            if let Some(mapped) = map_tool_choice_for_chat(choice) {
                body["tool_choice"] = mapped;
            }
        }

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let response = self
            .send_with_retry(|| self.http_client.post(&url).json(&body))
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            anyhow::bail!("SSE stream request failed: HTTP {status}: {error_text}");
        }

        let model = request.model.clone();
        let byte_stream = response.bytes_stream();

        let stream = async_stream::stream! {
            use futures_util::StreamExt;

            // Emit a synthetic MessageStart
            yield Ok(StreamEvent::MessageStart {
                message: MessageResponse {
                    id: String::new(),
                    r#type: "message".to_string(),
                    role: "assistant".to_string(),
                    content: Vec::new(),
                    model: model.clone(),
                    stop_reason: None,
                    stop_sequence: None,
                    container: None,
                    usage: Usage {
                        input_tokens: 0,
                        output_tokens: 0,
                        server_tool_use: None,
                    },
                },
            });

            let mut line_buf = String::new();
            let mut byte_buf = acquire_stream_buffer();
            let mut content_index: u32 = 0;
            let mut text_started = false;
            let mut thinking_started = false;
            let mut tool_indices: std::collections::HashMap<u32, bool> = std::collections::HashMap::new();
            let is_reasoning_model = requires_reasoning_content(&model);

            let mut byte_stream = std::pin::pin!(byte_stream);

            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = match chunk_result {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        yield Err(anyhow::anyhow!("Stream read error: {e}"));
                        break;
                    }
                };

                byte_buf.extend_from_slice(&chunk);

                // Guard against unbounded buffer growth (e.g., malformed stream without newlines)
                const MAX_SSE_BUF: usize = 10 * 1024 * 1024; // 10 MB
                if byte_buf.len() > MAX_SSE_BUF {
                    yield Err(anyhow::anyhow!("SSE buffer exceeded {MAX_SSE_BUF} bytes — aborting stream"));
                    break;
                }

                if byte_buf.len() > SSE_BACKPRESSURE_HIGH_WATERMARK {
                    tokio::time::sleep(Duration::from_millis(SSE_BACKPRESSURE_SLEEP_MS)).await;
                }

                // Process complete SSE lines from the buffer
                let mut lines_processed = 0usize;
                loop {
                    let buf_str = String::from_utf8_lossy(&byte_buf);
                    let Some(newline_pos) = buf_str.find('\n') else { break };
                    let line: String = buf_str[..newline_pos].trim_end_matches('\r').to_string();
                    let consumed = newline_pos + 1;
                    byte_buf.drain(..consumed);

                    if line.is_empty() {
                        // Empty line = event boundary, process accumulated data
                        if !line_buf.is_empty() {
                            let data = std::mem::take(&mut line_buf);
                            if data.trim() == "[DONE]" {
                                // Stream complete
                            } else if let Ok(chunk_json) = serde_json::from_str::<Value>(&data) {
                                // Parse the SSE chunk into stream events
                                for event in parse_sse_chunk(
                                    &chunk_json,
                                    &mut content_index,
                                    &mut text_started,
                                    &mut thinking_started,
                                    &mut tool_indices,
                                    is_reasoning_model,
                                ) {
                                    yield Ok(event);
                                }
                            }
                        }
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        line_buf.push_str(data);
                    }
                    // Ignore other SSE fields (event:, id:, retry:)

                    lines_processed = lines_processed.saturating_add(1);
                    if lines_processed >= SSE_MAX_LINES_PER_CHUNK {
                        // Yield backpressure relief to avoid starving downstream consumers.
                        break;
                    }
                }
            }

            // Close any open blocks
            if thinking_started {
                yield Ok(StreamEvent::ContentBlockStop { index: content_index.saturating_sub(1) });
            }
            if text_started {
                yield Ok(StreamEvent::ContentBlockStop { index: content_index.saturating_sub(1) });
            }

            release_stream_buffer(byte_buf);
            yield Ok(StreamEvent::MessageStop);
        };

        Ok(Pin::from(Box::new(stream)
            as Box<
                dyn futures_util::Stream<Item = Result<StreamEvent>> + Send,
            >))
    }
}

// === Responses API Helpers ===

#[derive(Debug)]
struct ResponsesFallback {
    status: u16,
    body: String,
}

#[derive(Debug, Deserialize)]
struct ModelsListResponse {
    data: Vec<ModelListItem>,
}

#[derive(Debug, Deserialize)]
struct ModelListItem {
    id: String,
    #[serde(default)]
    owned_by: Option<String>,
    #[serde(default)]
    created: Option<u64>,
}

fn parse_models_response(payload: &str) -> Result<Vec<AvailableModel>> {
    let parsed: ModelsListResponse =
        serde_json::from_str(payload).context("Failed to parse model list JSON")?;

    let mut models = parsed
        .data
        .into_iter()
        .map(|item| AvailableModel {
            id: item.id,
            owned_by: item.owned_by,
            created: item.created,
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(models)
}

fn system_to_instructions(system: Option<SystemPrompt>) -> Option<String> {
    match system {
        Some(SystemPrompt::Text(text)) => Some(text),
        Some(SystemPrompt::Blocks(blocks)) => {
            let joined = blocks
                .into_iter()
                .map(|b| b.text)
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");
            if joined.trim().is_empty() {
                None
            } else {
                Some(joined)
            }
        }
        None => None,
    }
}

fn build_responses_input(messages: &[Message]) -> Vec<Value> {
    let mut items = Vec::new();

    for message in messages {
        let role = message.role.as_str();
        let text_type = if role == "user" {
            "input_text"
        } else {
            "output_text"
        };

        for block in &message.content {
            match block {
                ContentBlock::Text { text, .. } => {
                    items.push(json!({
                        "type": "message",
                        "role": role,
                        "content": [{
                            "type": text_type,
                            "text": text,
                        }]
                    }));
                }
                ContentBlock::ToolUse {
                    id,
                    name,
                    input,
                    caller,
                } => {
                    let args = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
                    let mut item = json!({
                        "type": "function_call",
                        "call_id": id,
                        "name": to_api_tool_name(name),
                        "arguments": args,
                    });
                    if let Some(caller) = caller {
                        item["caller"] = json!({
                            "type": caller.caller_type,
                            "tool_id": caller.tool_id,
                        });
                    }
                    items.push(item);
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    ..
                } => {
                    let mut item = json!({
                        "type": "function_call_output",
                        "call_id": tool_use_id,
                        "output": content,
                    });
                    if let Some(is_error) = is_error {
                        item["is_error"] = json!(is_error);
                    }
                    items.push(item);
                }
                ContentBlock::Thinking { .. } => {}
                ContentBlock::ServerToolUse { id, name, input } => {
                    items.push(json!({
                        "type": "server_tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
                ContentBlock::ToolSearchToolResult {
                    tool_use_id,
                    content,
                } => {
                    items.push(json!({
                        "type": "tool_search_tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                    }));
                }
                ContentBlock::CodeExecutionToolResult {
                    tool_use_id,
                    content,
                } => {
                    items.push(json!({
                        "type": "code_execution_tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                    }));
                }
            }
        }
    }

    items
}

fn tool_to_responses(tool: &Tool) -> Value {
    let tool_type = tool.tool_type.as_deref().unwrap_or("function");
    let mut value = if tool_type == "function" {
        json!({
            "type": "function",
            "name": to_api_tool_name(&tool.name),
            "description": tool.description,
            "parameters": tool.input_schema,
        })
    } else if tool_type == "code_execution_20250825" {
        json!({
            "type": tool_type,
            "name": to_api_tool_name(&tool.name),
        })
    } else {
        json!({
            "type": tool_type,
            "name": to_api_tool_name(&tool.name),
            "description": tool.description,
            "input_schema": tool.input_schema,
        })
    };

    if let Some(allowed_callers) = &tool.allowed_callers {
        value["allowed_callers"] = json!(allowed_callers);
    }
    if let Some(defer_loading) = tool.defer_loading {
        value["defer_loading"] = json!(defer_loading);
    }
    if let Some(input_examples) = &tool.input_examples {
        value["input_examples"] = json!(input_examples);
    }
    if let Some(strict) = tool.strict {
        value["strict"] = json!(strict);
    }
    value
}

fn parse_responses_message(payload: &Value) -> Result<MessageResponse> {
    let id = payload
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("response")
        .to_string();
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let usage = parse_usage(payload.get("usage"));
    let mut content = Vec::new();

    if let Some(output) = payload.get("output").and_then(Value::as_array) {
        for item in output {
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            match item_type {
                "message" => {
                    if let Some(role) = item.get("role").and_then(Value::as_str)
                        && role != "assistant"
                    {
                        continue;
                    }
                    if let Some(content_items) = item.get("content").and_then(Value::as_array) {
                        for content_item in content_items {
                            let content_type = content_item
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or("output_text");
                            if content_type != "output_text" && content_type != "text" {
                                continue;
                            }
                            if let Some(text) = content_item.get("text").and_then(Value::as_str) {
                                if !text.trim().is_empty() {
                                    content.push(ContentBlock::Text {
                                        text: text.to_string(),
                                        cache_control: None,
                                    });
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    let call_id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("tool_call")
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string();
                    let input = match item.get("arguments") {
                        Some(Value::String(raw)) => {
                            serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone()))
                        }
                        Some(other) => other.clone(),
                        None => Value::Null,
                    };
                    let caller = item.get("caller").and_then(|v| {
                        v.get("type")
                            .and_then(Value::as_str)
                            .map(|caller_type| ToolCaller {
                                caller_type: caller_type.to_string(),
                                tool_id: v
                                    .get("tool_id")
                                    .and_then(Value::as_str)
                                    .map(std::string::ToString::to_string),
                            })
                    });
                    content.push(ContentBlock::ToolUse {
                        id: call_id,
                        name: from_api_tool_name(&name),
                        input,
                        caller,
                    });
                }
                "function_call_output" => {
                    let tool_use_id = item
                        .get("call_id")
                        .or_else(|| item.get("tool_use_id"))
                        .and_then(Value::as_str)
                        .unwrap_or("tool_call")
                        .to_string();
                    let content_text = item
                        .get("output")
                        .or_else(|| item.get("content"))
                        .map(|v| {
                            if let Some(s) = v.as_str() {
                                s.to_string()
                            } else {
                                v.to_string()
                            }
                        })
                        .unwrap_or_default();
                    let is_error = item.get("is_error").and_then(Value::as_bool);
                    content.push(ContentBlock::ToolResult {
                        tool_use_id,
                        content: content_text,
                        is_error,
                        content_blocks: None,
                    });
                }
                "server_tool_use" => {
                    let id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("server_tool")
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("server_tool")
                        .to_string();
                    let input = item.get("input").cloned().unwrap_or(Value::Null);
                    content.push(ContentBlock::ServerToolUse { id, name, input });
                }
                "tool_search_tool_result" => {
                    let tool_use_id = item
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .unwrap_or("tool_search")
                        .to_string();
                    let content_value = item.get("content").cloned().unwrap_or(Value::Null);
                    content.push(ContentBlock::ToolSearchToolResult {
                        tool_use_id,
                        content: content_value,
                    });
                }
                "code_execution_tool_result" => {
                    let tool_use_id = item
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .unwrap_or("code_execution")
                        .to_string();
                    let content_value = item.get("content").cloned().unwrap_or(Value::Null);
                    content.push(ContentBlock::CodeExecutionToolResult {
                        tool_use_id,
                        content: content_value,
                    });
                }
                "reasoning" => {
                    if let Some(summary) = item.get("summary").and_then(Value::as_array) {
                        let summary_text = summary
                            .iter()
                            .filter_map(|s| s.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n");
                        if !summary_text.trim().is_empty() {
                            content.push(ContentBlock::Thinking {
                                thinking: summary_text,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if content.is_empty() {
        if let Some(text) = payload.get("output_text").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                content.push(ContentBlock::Text {
                    text: text.to_string(),
                    cache_control: None,
                });
            }
        }
    }

    Ok(MessageResponse {
        id,
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model,
        stop_reason: None,
        stop_sequence: None,
        container: payload
            .get("container")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok()),
        usage,
    })
}

// === Chat Completions Helpers ===

fn build_chat_messages(
    system: Option<&SystemPrompt>,
    messages: &[Message],
    model: &str,
) -> Vec<Value> {
    let mut out = Vec::new();
    let include_reasoning = requires_reasoning_content(model);
    let mut pending_tool_calls: HashSet<String> = HashSet::new();

    if let Some(instructions) = system_to_instructions(system.cloned()) {
        if !instructions.trim().is_empty() {
            out.push(json!({
                "role": "system",
                "content": instructions,
            }));
        }
    }

    for message in messages {
        let role = message.role.as_str();
        let mut text_parts = Vec::new();
        let mut thinking_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_call_ids = Vec::new();
        let mut tool_results: Vec<(String, Value)> = Vec::new();

        for block in &message.content {
            match block {
                ContentBlock::Text { text, .. } => text_parts.push(text.clone()),
                ContentBlock::Thinking { thinking } => thinking_parts.push(thinking.clone()),
                ContentBlock::ToolUse {
                    id,
                    name,
                    input,
                    caller,
                    ..
                } => {
                    let args = serde_json::to_string(input).unwrap_or_else(|_| input.to_string());
                    let mut call = json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": to_api_tool_name(name),
                            "arguments": args,
                        }
                    });
                    if let Some(caller) = caller {
                        call["caller"] = json!({
                            "type": caller.caller_type,
                            "tool_id": caller.tool_id,
                        });
                    }
                    tool_calls.push(call);
                    tool_call_ids.push(id.clone());
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } => {
                    tool_results.push((
                        tool_use_id.clone(),
                        json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content,
                        }),
                    ));
                }
                ContentBlock::ServerToolUse { .. }
                | ContentBlock::ToolSearchToolResult { .. }
                | ContentBlock::CodeExecutionToolResult { .. } => {}
            }
        }

        if role == "assistant" {
            let content = text_parts.join("\n");
            let has_text = !content.trim().is_empty();
            let has_tool_calls = !tool_calls.is_empty();

            // DeepSeek rejects assistant messages where both `content` and
            // `tool_calls` are missing/null. Skip such entries even if they
            // carry reasoning-only metadata.
            if !has_text && !has_tool_calls {
                pending_tool_calls.clear();
                continue;
            }

            let mut msg = json!({
                "role": "assistant",
                "content": if has_text { json!(content) } else { Value::Null },
            });
            if include_reasoning {
                msg["reasoning_content"] = json!(thinking_parts.join("\n"));
            }
            if has_tool_calls {
                msg["tool_calls"] = json!(tool_calls);
                pending_tool_calls = tool_call_ids.into_iter().collect();
            } else {
                pending_tool_calls.clear();
            }
            out.push(msg);
        } else if role == "user" {
            let content = text_parts.join("\n");
            if !content.trim().is_empty() {
                out.push(json!({
                    "role": "user",
                    "content": content,
                }));
            }
        }

        if !tool_results.is_empty() {
            if pending_tool_calls.is_empty() {
                logging::warn("Dropping tool results without matching tool_calls");
            } else {
                for (tool_id, tool_msg) in tool_results {
                    if pending_tool_calls.remove(&tool_id) {
                        out.push(tool_msg);
                    } else {
                        logging::warn(format!(
                            "Dropping tool result for unknown tool_call_id: {tool_id}"
                        ));
                    }
                }
            }
        } else if role != "assistant" {
            pending_tool_calls.clear();
        }
    }

    // Safety net: after compaction, an assistant message may have tool_calls
    // whose results were summarized away. The API rejects these, so strip
    // the tool_calls (downgrading to a plain assistant message) and remove
    // the now-orphaned tool result messages.
    let mut i = 0;
    while i < out.len() {
        let is_assistant_with_tools = out[i].get("role").and_then(Value::as_str)
            == Some("assistant")
            && out[i].get("tool_calls").is_some();

        if is_assistant_with_tools {
            let expected_ids: HashSet<String> = out[i]
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|c| c.get("id").and_then(Value::as_str).map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            // Collect tool result IDs immediately following this assistant message.
            let mut found_ids: HashSet<String> = HashSet::new();
            let mut tool_result_end = i + 1;
            while tool_result_end < out.len() {
                if out[tool_result_end].get("role").and_then(Value::as_str) == Some("tool") {
                    if let Some(id) = out[tool_result_end]
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                    {
                        found_ids.insert(id.to_string());
                    }
                    tool_result_end += 1;
                } else {
                    break;
                }
            }

            // Also scan non-contiguous tool results up to the next assistant message
            // in case compaction left gaps.
            let mut scan = tool_result_end;
            while scan < out.len() {
                if out[scan].get("role").and_then(Value::as_str) == Some("assistant") {
                    break;
                }
                if out[scan].get("role").and_then(Value::as_str) == Some("tool") {
                    if let Some(id) = out[scan].get("tool_call_id").and_then(Value::as_str) {
                        found_ids.insert(id.to_string());
                    }
                }
                scan += 1;
            }

            if !expected_ids.is_subset(&found_ids) {
                let missing: Vec<_> = expected_ids.difference(&found_ids).collect();
                logging::warn(format!(
                    "Stripping orphaned tool_calls from assistant message \
                     (expected {} tool results, found {}, missing: {:?})",
                    expected_ids.len(),
                    found_ids.len(),
                    missing
                ));
                if let Some(obj) = out[i].as_object_mut() {
                    obj.remove("tool_calls");
                }
                // If tool_calls were the only assistant content, remove the now-invalid
                // assistant message entirely (DeepSeek requires content or tool_calls).
                let assistant_content_empty = out[i]
                    .get("content")
                    .is_none_or(|v| v.is_null() || v.as_str().is_some_and(str::is_empty));
                if assistant_content_empty {
                    // Remove orphaned tool results tied to this stripped assistant call set.
                    let mut j = out.len();
                    while j > i + 1 {
                        j -= 1;
                        if out[j].get("role").and_then(Value::as_str) == Some("tool")
                            && let Some(id) = out[j].get("tool_call_id").and_then(Value::as_str)
                            && expected_ids.contains(id)
                        {
                            out.remove(j);
                        }
                    }
                    out.remove(i);
                    i = i.saturating_sub(1);
                    continue;
                }
                // Remove contiguous tool results first
                if tool_result_end > i + 1 {
                    out.drain((i + 1)..tool_result_end);
                }
                // Remove any remaining non-contiguous tool results referencing expected_ids
                // (scan backward to avoid index shifting issues)
                let mut j = out.len();
                while j > i + 1 {
                    j -= 1;
                    if out[j].get("role").and_then(Value::as_str) == Some("tool") {
                        if let Some(id) = out[j].get("tool_call_id").and_then(Value::as_str) {
                            if expected_ids.contains(id) {
                                out.remove(j);
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }

    out
}

fn tool_to_chat(tool: &Tool) -> Value {
    let mut value = json!({
        "type": "function",
        "function": {
            "name": to_api_tool_name(&tool.name),
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    });
    if let Some(allowed_callers) = &tool.allowed_callers {
        value["allowed_callers"] = json!(allowed_callers);
    }
    if let Some(defer_loading) = tool.defer_loading {
        value["defer_loading"] = json!(defer_loading);
    }
    if let Some(input_examples) = &tool.input_examples {
        value["input_examples"] = json!(input_examples);
    }
    if let Some(strict) = tool.strict {
        value["strict"] = json!(strict);
    }
    value
}

fn map_tool_choice_for_chat(choice: &Value) -> Option<Value> {
    if let Some(choice_str) = choice.as_str() {
        return Some(json!(choice_str));
    }
    let Some(choice_type) = choice.get("type").and_then(Value::as_str) else {
        return Some(choice.clone());
    };

    match choice_type {
        "auto" | "none" => Some(json!(choice_type)),
        "any" => Some(json!("auto")),
        "tool" => choice.get("name").and_then(Value::as_str).map(|name| {
            json!({
                "type": "function",
                "function": { "name": to_api_tool_name(name) }
            })
        }),
        _ => Some(choice.clone()),
    }
}

fn requires_reasoning_content(model: &str) -> bool {
    let lower = model.to_lowercase();
    lower.contains("deepseek-v3.2")
        || lower.contains("reasoner")
        || lower.contains("-reasoning")
        || lower.contains("-thinking")
        || has_deepseek_r_series_marker(&lower)
}

fn has_deepseek_r_series_marker(model_lower: &str) -> bool {
    const PREFIX: &str = "deepseek-r";
    model_lower.match_indices(PREFIX).any(|(idx, _)| {
        model_lower[idx + PREFIX.len()..]
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit())
    })
}

fn parse_chat_message(payload: &Value) -> Result<MessageResponse> {
    let id = payload
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("chatcmpl")
        .to_string();
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let choices = payload
        .get("choices")
        .and_then(Value::as_array)
        .context("Chat API response missing choices")?;
    let choice = choices
        .get(0)
        .context("Chat API response missing first choice")?;
    let message = choice
        .get("message")
        .context("Chat API response missing message")?;

    let mut content_blocks = Vec::new();
    if let Some(reasoning) = message.get("reasoning_content").and_then(Value::as_str) {
        if !reasoning.trim().is_empty() {
            content_blocks.push(ContentBlock::Thinking {
                thinking: reasoning.to_string(),
            });
        }
    }
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        if !text.trim().is_empty() {
            content_blocks.push(ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            });
        }
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("tool_call")
                .to_string();
            let function = call.get("function");
            let name = function
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            let arguments = function
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
                .map(|raw| serde_json::from_str(raw).unwrap_or(Value::String(raw.to_string())))
                .unwrap_or(Value::Null);
            let caller = call.get("caller").and_then(|v| {
                v.get("type")
                    .and_then(Value::as_str)
                    .map(|caller_type| ToolCaller {
                        caller_type: caller_type.to_string(),
                        tool_id: v
                            .get("tool_id")
                            .and_then(Value::as_str)
                            .map(std::string::ToString::to_string),
                    })
            });

            content_blocks.push(ContentBlock::ToolUse {
                id,
                name: from_api_tool_name(&name),
                input: arguments,
                caller,
            });
        }
    }

    let usage = parse_usage(payload.get("usage"));

    Ok(MessageResponse {
        id,
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content: content_blocks,
        model,
        stop_reason: choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        stop_sequence: None,
        container: None,
        usage,
    })
}

fn parse_usage(usage: Option<&Value>) -> Usage {
    let input_tokens = usage
        .and_then(|u| u.get("input_tokens").or_else(|| u.get("prompt_tokens")))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|u| {
            u.get("output_tokens")
                .or_else(|| u.get("completion_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let server_tool_use = usage.and_then(|u| u.get("server_tool_use")).map(|server| {
        let code_execution_requests = server
            .get("code_execution_requests")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        let tool_search_requests = server
            .get("tool_search_requests")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        ServerToolUsage {
            code_execution_requests,
            tool_search_requests,
        }
    });

    Usage {
        input_tokens: input_tokens as u32,
        output_tokens: output_tokens as u32,
        server_tool_use,
    }
}

// === Streaming Helpers ===

/// Build synthetic stream events from a non-streaming response (used as fallback).
#[allow(dead_code)]
fn build_stream_events(response: &MessageResponse) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    let mut index = 0u32;

    events.push(StreamEvent::MessageStart {
        message: response.clone(),
    });

    for block in &response.content {
        match block {
            ContentBlock::Text { text, .. } => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::Text {
                        text: String::new(),
                    },
                });
                if !text.is_empty() {
                    events.push(StreamEvent::ContentBlockDelta {
                        index,
                        delta: Delta::TextDelta { text: text.clone() },
                    });
                }
                events.push(StreamEvent::ContentBlockStop { index });
            }
            ContentBlock::Thinking { thinking } => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::Thinking {
                        thinking: String::new(),
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
                id, name, input, ..
            } => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                        caller: None,
                    },
                });
                events.push(StreamEvent::ContentBlockStop { index });
            }
            ContentBlock::ToolResult { .. } => {}
            ContentBlock::ServerToolUse { id, name, input } => {
                events.push(StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::ServerToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    },
                });
                events.push(StreamEvent::ContentBlockStop { index });
            }
            ContentBlock::ToolSearchToolResult { .. }
            | ContentBlock::CodeExecutionToolResult { .. } => {}
        }
        index = index.saturating_add(1);
    }

    events.push(StreamEvent::MessageDelta {
        delta: MessageDelta {
            stop_reason: response.stop_reason.clone(),
            stop_sequence: response.stop_sequence.clone(),
        },
        usage: Some(response.usage.clone()),
    });
    events.push(StreamEvent::MessageStop);

    events
}

// === SSE Chunk Parser ===

/// Parse a single SSE chunk from the Chat Completions streaming API into
/// our internal `StreamEvent` representation.
fn parse_sse_chunk(
    chunk: &Value,
    content_index: &mut u32,
    text_started: &mut bool,
    thinking_started: &mut bool,
    tool_indices: &mut std::collections::HashMap<u32, bool>,
    is_reasoning_model: bool,
) -> Vec<StreamEvent> {
    let mut events = Vec::new();

    let Some(choices) = chunk.get("choices").and_then(Value::as_array) else {
        // Usage-only chunk (sent at end with stream_options)
        if let Some(usage_val) = chunk.get("usage") {
            let usage = parse_usage(Some(usage_val));
            events.push(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: None,
                    stop_sequence: None,
                },
                usage: Some(usage),
            });
        }
        return events;
    };

    for choice in choices {
        let delta = choice.get("delta");
        let finish_reason = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string);

        if let Some(delta) = delta {
            // Handle reasoning_content (DeepSeek-Reasoner thinking)
            if is_reasoning_model {
                if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
                    if !reasoning.is_empty() {
                        if !*thinking_started {
                            events.push(StreamEvent::ContentBlockStart {
                                index: *content_index,
                                content_block: ContentBlockStart::Thinking {
                                    thinking: String::new(),
                                },
                            });
                            *thinking_started = true;
                        }
                        events.push(StreamEvent::ContentBlockDelta {
                            index: *content_index,
                            delta: Delta::ThinkingDelta {
                                thinking: reasoning.to_string(),
                            },
                        });
                    }
                }
            }

            // Handle regular content
            if let Some(content) = delta.get("content").and_then(Value::as_str) {
                if !content.is_empty() {
                    // Close thinking block if transitioning to text
                    if *thinking_started {
                        events.push(StreamEvent::ContentBlockStop {
                            index: *content_index,
                        });
                        *content_index += 1;
                        *thinking_started = false;
                    }
                    if !*text_started {
                        events.push(StreamEvent::ContentBlockStart {
                            index: *content_index,
                            content_block: ContentBlockStart::Text {
                                text: String::new(),
                            },
                        });
                        *text_started = true;
                    }
                    events.push(StreamEvent::ContentBlockDelta {
                        index: *content_index,
                        delta: Delta::TextDelta {
                            text: content.to_string(),
                        },
                    });
                }
            }

            // Handle tool calls
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for tc in tool_calls {
                    let tc_index = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;

                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        tool_indices.entry(tc_index)
                    {
                        // Close text block if transitioning to tool use
                        if *text_started {
                            events.push(StreamEvent::ContentBlockStop {
                                index: *content_index,
                            });
                            *content_index += 1;
                            *text_started = false;
                        }
                        if *thinking_started {
                            events.push(StreamEvent::ContentBlockStop {
                                index: *content_index,
                            });
                            *content_index += 1;
                            *thinking_started = false;
                        }

                        let id = tc
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("tool_call")
                            .to_string();
                        let name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let caller = tc.get("caller").and_then(|v| {
                            v.get("type")
                                .and_then(Value::as_str)
                                .map(|caller_type| ToolCaller {
                                    caller_type: caller_type.to_string(),
                                    tool_id: v
                                        .get("tool_id")
                                        .and_then(Value::as_str)
                                        .map(std::string::ToString::to_string),
                                })
                        });

                        entry.insert(true);
                        events.push(StreamEvent::ContentBlockStart {
                            index: *content_index,
                            content_block: ContentBlockStart::ToolUse {
                                id,
                                name: from_api_tool_name(&name),
                                input: json!({}),
                                caller,
                            },
                        });
                    }

                    // Stream tool call arguments
                    if let Some(args) = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str)
                    {
                        if !args.is_empty() {
                            events.push(StreamEvent::ContentBlockDelta {
                                index: *content_index,
                                delta: Delta::InputJsonDelta {
                                    partial_json: args.to_string(),
                                },
                            });
                        }
                    }
                }
            }
        }

        // Handle finish reason
        if let Some(reason) = finish_reason {
            // Close any open blocks
            if *text_started {
                events.push(StreamEvent::ContentBlockStop {
                    index: *content_index,
                });
                *text_started = false;
            }
            if *thinking_started {
                events.push(StreamEvent::ContentBlockStop {
                    index: *content_index,
                });
                *thinking_started = false;
            }
            // Close tool blocks
            for _ in tool_indices.drain() {
                events.push(StreamEvent::ContentBlockStop {
                    index: *content_index,
                });
            }

            // Emit usage from the chunk if available
            let chunk_usage = chunk.get("usage").map(|u| parse_usage(Some(u)));
            events.push(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: Some(reason),
                    stop_sequence: None,
                },
                usage: chunk_usage,
            });
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_name_roundtrip_dot() {
        let original = "multi_tool_use.parallel";
        let encoded = to_api_tool_name(original);
        assert_eq!(encoded, "multi_tool_use-x00002E-parallel");
        let decoded = from_api_tool_name(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn tool_name_decode_mangled_dot_prefix() {
        // Model replaces leading `-` with `.` in `-x00002E-`
        let mangled = "multi_tool_use.x00002E-parallel";
        let decoded = from_api_tool_name(mangled);
        assert_eq!(decoded, "multi_tool_use..parallel");
    }

    #[test]
    fn tool_name_decode_bare_hex_no_trailing_dash() {
        // Bare hex without trailing dash
        let mangled = "foo_x00002Ebar";
        let decoded = from_api_tool_name(mangled);
        assert_eq!(decoded, "foo_.bar");
    }

    #[test]
    fn tool_name_bare_hex_preserves_alnum() {
        // x000041 = 'A' — should NOT be decoded (alphanumeric)
        let input = "foox000041bar";
        let decoded = from_api_tool_name(input);
        assert_eq!(decoded, input);
    }

    #[test]
    fn tool_name_bare_hex_preserves_underscore() {
        // x00005F = '_' — should NOT be decoded
        let input = "foox00005Fbar";
        let decoded = from_api_tool_name(input);
        assert_eq!(decoded, input);
    }

    #[test]
    fn tool_name_roundtrip_colon() {
        let original = "mcp__server:tool_name";
        let encoded = to_api_tool_name(original);
        let decoded = from_api_tool_name(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn chat_messages_include_reasoning_content_for_reasoner() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Thinking {
                    thinking: "plan".to_string(),
                },
                ContentBlock::Text {
                    text: "done".to_string(),
                    cache_control: None,
                },
            ],
        };
        let out = build_chat_messages(None, &[message], "deepseek-reasoner");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        assert_eq!(
            assistant.get("reasoning_content").and_then(Value::as_str),
            Some("plan")
        );
    }

    #[test]
    fn chat_messages_drop_thinking_only_assistant_for_chat_model() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "plan".to_string(),
            }],
        };
        let out = build_chat_messages(None, &[message], "deepseek-chat");
        assert!(
            !out.iter()
                .any(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
        );
    }

    #[test]
    fn chat_messages_drop_thinking_only_assistant_for_r_series_model() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "plan".to_string(),
            }],
        };
        let out = build_chat_messages(None, &[message], "deepseek-r2-lite-preview");
        assert!(
            !out.iter()
                .any(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
        );
    }

    #[test]
    fn chat_messages_drop_thinking_only_assistant_for_v4_mini_model() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "plan".to_string(),
            }],
        };
        let out = build_chat_messages(None, &[message], "deepseek-v4-mini");
        assert!(
            !out.iter()
                .any(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
        );
    }

    #[test]
    fn chat_messages_drop_orphan_tool_results() {
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-1".to_string(),
                content: "ok".to_string(),
                is_error: None,
                content_blocks: None,
            }],
        }];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        assert!(
            !out.iter()
                .any(|value| { value.get("role").and_then(Value::as_str) == Some("tool") })
        );
    }

    #[test]
    fn chat_messages_include_tool_results_when_call_present() {
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "tool-1".to_string(),
                    name: "list_dir".to_string(),
                    input: json!({}),
                    caller: None,
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    content: "ok".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        assert!(
            out.iter()
                .any(|value| { value.get("role").and_then(Value::as_str) == Some("tool") })
        );
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        assert!(assistant.get("tool_calls").is_some());
    }

    #[test]
    fn chat_messages_encode_tool_call_names() {
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "tool-1".to_string(),
                    name: "web.run".to_string(),
                    input: json!({}),
                    caller: None,
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    content: "ok".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        let tool_calls = assistant
            .get("tool_calls")
            .and_then(Value::as_array)
            .expect("tool_calls array");
        let function_name = tool_calls
            .first()
            .and_then(|call| call.get("function"))
            .and_then(|func| func.get("name"))
            .and_then(Value::as_str)
            .expect("tool call function name");

        assert_eq!(function_name, to_api_tool_name("web.run"));
    }

    #[test]
    fn chat_messages_strips_orphaned_tool_calls_after_compaction() {
        // Simulates post-compaction state: assistant has tool_calls but the
        // tool result messages were summarized away.
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "tool-orphan".to_string(),
                    name: "read_file".to_string(),
                    input: json!({"path": "src/main.rs"}),
                    caller: None,
                }],
            },
            // No tool result follows — it was removed by compaction.
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "continue".to_string(),
                    cache_control: None,
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"));
        // The safety net may drop the assistant message entirely if it only
        // contained orphaned tool_calls and no text content.
        assert!(
            assistant.is_none(),
            "assistant without content/tool_calls should be removed"
        );
        assert!(
            !out.iter()
                .any(|v| v.get("role").and_then(Value::as_str) == Some("tool")),
            "orphaned tool results should also be removed"
        );
    }

    #[test]
    fn chat_messages_keeps_valid_tool_calls_intact() {
        // Complete call+result pair should NOT be stripped.
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "tool-ok".to_string(),
                    name: "list_dir".to_string(),
                    input: json!({}),
                    caller: None,
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-ok".to_string(),
                    content: "files".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        assert!(
            assistant.get("tool_calls").is_some(),
            "valid tool_calls should remain intact"
        );
        assert!(
            out.iter()
                .any(|value| value.get("role").and_then(Value::as_str) == Some("tool")),
            "tool result should remain"
        );
    }

    #[test]
    fn chat_messages_strips_partial_tool_results() {
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::ToolUse {
                        id: "t1".to_string(),
                        name: "read_file".to_string(),
                        input: json!({"path": "a.rs"}),
                        caller: None,
                    },
                    ContentBlock::ToolUse {
                        id: "t2".to_string(),
                        name: "read_file".to_string(),
                        input: json!({"path": "b.rs"}),
                        caller: None,
                    },
                    ContentBlock::ToolUse {
                        id: "t3".to_string(),
                        name: "shell".to_string(),
                        input: json!({"cmd": "ls"}),
                        caller: None,
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t1".to_string(),
                    content: "content a".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "t2".to_string(),
                    content: "content b".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
            // No result for t3
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "continue".to_string(),
                    cache_control: None,
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-chat");
        let assistant = out
            .iter()
            .find(|v| v.get("role").and_then(Value::as_str) == Some("assistant"));
        assert!(
            assistant.is_none(),
            "assistant with only partial tool_calls should be removed"
        );
        assert!(
            !out.iter()
                .any(|v| v.get("role").and_then(Value::as_str) == Some("tool")),
            "all orphaned tool results should be removed"
        );
    }

    #[test]
    fn parse_models_response_parses_and_deduplicates() {
        let payload = r#"{
            "object": "list",
            "data": [
                {"id": "deepseek-r1", "object": "model", "owned_by": "deepseek", "created": 1},
                {"id": "deepseek-chat", "object": "model"},
                {"id": "deepseek-r1", "object": "model", "owned_by": "deepseek", "created": 1}
            ]
        }"#;

        let models = parse_models_response(payload).expect("parse models");
        assert_eq!(
            models,
            vec![
                AvailableModel {
                    id: "deepseek-chat".to_string(),
                    owned_by: None,
                    created: None
                },
                AvailableModel {
                    id: "deepseek-r1".to_string(),
                    owned_by: Some("deepseek".to_string()),
                    created: Some(1)
                }
            ]
        );
    }

    #[test]
    fn token_bucket_enforces_delay_when_empty() {
        let now = Instant::now();
        let mut bucket = TokenBucket {
            enabled: true,
            capacity: 1.0,
            tokens: 1.0,
            refill_per_sec: 2.0,
            last_refill: now,
        };

        assert!(bucket.delay_until_available(1.0).is_none());
        let delay = bucket
            .delay_until_available(1.0)
            .expect("bucket should require refill delay");
        assert!(
            delay >= Duration::from_millis(400) && delay <= Duration::from_millis(600),
            "unexpected refill delay: {delay:?}"
        );
    }

    #[test]
    fn stream_buffer_pool_reuses_released_buffers() {
        let mut first = acquire_stream_buffer();
        first.extend_from_slice(b"hello");
        let released_capacity = first.capacity();
        release_stream_buffer(first);

        let second = acquire_stream_buffer();
        assert!(second.is_empty());
        assert!(
            second.capacity() >= released_capacity,
            "pooled buffer capacity should be reused"
        );
    }

    #[test]
    fn base_url_security_rejects_insecure_non_local_http() {
        let err = validate_base_url_security("http://api.deepseek.com")
            .expect_err("non-local insecure HTTP should be rejected");
        assert!(err.to_string().contains("Refusing insecure base URL"));
    }

    #[test]
    fn base_url_security_allows_localhost_http() {
        assert!(validate_base_url_security("http://localhost:8080").is_ok());
        assert!(validate_base_url_security("http://127.0.0.1:8080").is_ok());
    }

    #[test]
    fn connection_health_degrades_and_recovers() {
        let now = Instant::now();
        let mut health = ConnectionHealth::default();
        assert_eq!(health.state, ConnectionState::Healthy);

        apply_request_failure(&mut health, now);
        assert_eq!(health.state, ConnectionState::Healthy);

        apply_request_failure(&mut health, now + Duration::from_millis(1));
        assert_eq!(health.state, ConnectionState::Degraded);
        assert_eq!(health.consecutive_failures, 2);

        let recovered = apply_request_success(&mut health, now + Duration::from_secs(1));
        assert!(recovered);
        assert_eq!(health.state, ConnectionState::Healthy);
        assert_eq!(health.consecutive_failures, 0);
    }

    #[test]
    fn recovery_probe_respects_cooldown() {
        let now = Instant::now();
        let mut health = ConnectionHealth {
            state: ConnectionState::Degraded,
            ..ConnectionHealth::default()
        };

        assert!(mark_recovery_probe_if_due(&mut health, now));
        assert_eq!(health.state, ConnectionState::Recovering);
        assert!(!mark_recovery_probe_if_due(
            &mut health,
            now + Duration::from_secs(1)
        ));
        assert!(mark_recovery_probe_if_due(
            &mut health,
            now + RECOVERY_PROBE_COOLDOWN + Duration::from_millis(1)
        ));
    }
}
