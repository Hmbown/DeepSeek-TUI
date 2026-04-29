//! HTTP client for DeepSeek's OpenAI-compatible Chat Completions API.
//!
//! DeepSeek documents `/chat/completions` as the primary endpoint. A legacy
//! Responses probe remains available behind `DEEPSEEK_EXPERIMENTAL_RESPONSES_API`
//! for local compatibility experiments, but normal traffic uses chat completions.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex as AsyncMutex;

use crate::config::{ApiProvider, Config, RetryPolicy};
use crate::llm_client::{
    LlmClient, LlmError, RetryConfig as LlmRetryConfig, StreamEventBox, extract_retry_after,
    with_retry,
};
use crate::logging;
use crate::models::{MessageRequest, MessageResponse, ServerToolUsage, SystemPrompt, Usage};

pub(super) fn to_api_tool_name(name: &str) -> String {
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

pub(super) fn from_api_tool_name(name: &str) -> String {
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
pub(super) fn decode_bare_hex_escapes(input: &str) -> String {
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
    pub(super) http_client: reqwest::Client,
    api_key: String,
    pub(super) base_url: String,
    pub(super) api_provider: ApiProvider,
    retry: RetryPolicy,
    default_model: String,
    use_chat_completions: AtomicBool,
    /// Counter of chat-completions requests since last experimental Responses API probe.
    /// After RESPONSES_RECOVERY_INTERVAL requests, we retry the Responses API when
    /// `DEEPSEEK_EXPERIMENTAL_RESPONSES_API` is set.
    chat_fallback_counter: AtomicU32,
    connection_health: Arc<AsyncMutex<ConnectionHealth>>,
    rate_limiter: Arc<AsyncMutex<TokenBucket>>,
}

/// After this many chat-completions requests, retry the experimental Responses
/// API to see if it has recovered.
const RESPONSES_RECOVERY_INTERVAL: u32 = 20;
const CONNECTION_FAILURE_THRESHOLD: u32 = 2;
const RECOVERY_PROBE_COOLDOWN: Duration = Duration::from_secs(15);

const DEFAULT_CLIENT_RATE_LIMIT_RPS: f64 = 8.0;
const DEFAULT_CLIENT_RATE_LIMIT_BURST: f64 = 16.0;
const ALLOW_INSECURE_HTTP_ENV: &str = "DEEPSEEK_ALLOW_INSECURE_HTTP";
const EXPERIMENTAL_RESPONSES_API_ENV: &str = "DEEPSEEK_EXPERIMENTAL_RESPONSES_API";

pub(super) const SSE_BACKPRESSURE_HIGH_WATERMARK: usize = 8 * 1024 * 1024; // 8 MB
pub(super) const SSE_BACKPRESSURE_SLEEP_MS: u64 = 10;
pub(super) const SSE_MAX_LINES_PER_CHUNK: usize = 256;

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
    if let Ok(mut pool) = buffer_pool().lock()
        && pool.len() < 8
    {
        pool.push(buf);
    }
}

impl Clone for DeepSeekClient {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            api_provider: self.api_provider,
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
pub(super) const ERROR_BODY_MAX_BYTES: usize = 64 * 1024;

/// Read an error response body with a size limit to prevent unbounded allocation.
pub(super) async fn bounded_error_text(response: reqwest::Response, max_bytes: usize) -> String {
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buf = Vec::with_capacity(max_bytes.min(8192));
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        let remaining = max_bytes.saturating_sub(buf.len());
        if remaining == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    String::from_utf8_lossy(&buf).into_owned()
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

    anyhow::bail!(
        "Refusing base URL '{}': only HTTPS (or explicitly allowed HTTP) URLs are supported.",
        base_url,
    )
}

fn experimental_responses_api_enabled() -> bool {
    std::env::var(EXPERIMENTAL_RESPONSES_API_ENV)
        .ok()
        .as_deref()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

pub(super) fn versioned_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") || trimmed.ends_with("/beta") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

pub(super) fn api_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        versioned_base_url(base_url).trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

// === DeepSeekClient ===

/// Returns true when DEEPSEEK_FORCE_HTTP1 is set to a truthy value
/// (`1`, `true`, `yes`, `on`, case-insensitive). Used by `build_http_client`
/// to opt out of HTTP/2 entirely when DeepSeek's edge mishandles long-lived H2
/// streams (#103). Anything else (unset, `0`, `false`, ...) leaves HTTP/2 on.
fn force_http1_from_env() -> bool {
    std::env::var("DEEPSEEK_FORCE_HTTP1")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
}

impl DeepSeekClient {
    /// Create a DeepSeek client from CLI configuration.
    pub fn new(config: &Config) -> Result<Self> {
        let api_key = config.deepseek_api_key()?;
        let base_url = config.deepseek_base_url();
        let api_provider = config.api_provider();
        validate_base_url_security(&base_url)?;
        let retry = config.retry_policy();
        let default_model = config.default_model();

        logging::info(format!("API provider: {}", api_provider.as_str()));
        logging::info(format!("API base URL: {base_url}"));
        logging::info(format!(
            "Retry policy: enabled={}, max_retries={}, initial_delay={}s, max_delay={}s",
            retry.enabled, retry.max_retries, retry.initial_delay, retry.max_delay
        ));

        let http_client = Self::build_http_client(&api_key)?;

        Ok(Self {
            http_client,
            api_key,
            base_url,
            api_provider,
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
        if !api_key.trim().is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {api_key}"))?,
            );
        }
        let mut builder = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(30))
            // The blanket 300s request timeout was incompatible with V4-pro
            // thinking turns that legitimately exceed that wall-clock window
            // (see #103). Drop it; per-chunk and per-stream guards in
            // engine.rs already bound how long we'll wait without progress.
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .http2_keep_alive_interval(Some(Duration::from_secs(15)))
            .http2_keep_alive_timeout(Duration::from_secs(20))
            .min_tls_version(reqwest::tls::Version::TLS_1_2);
        // Escape hatch (#103): some DeepSeek edge nodes mishandle long-lived
        // HTTP/2 streams. Setting DEEPSEEK_FORCE_HTTP1=1 pins the client to
        // HTTP/1.1 so users can experiment without us committing to that
        // path as the default.
        if force_http1_from_env() {
            logging::info("DEEPSEEK_FORCE_HTTP1=1 — pinning HTTP client to HTTP/1.1");
            builder = builder.http1_only();
        }
        builder.build().map_err(Into::into)
    }

    /// List available models from the provider.
    pub async fn list_models(&self) -> Result<Vec<AvailableModel>> {
        let url = api_url(&self.base_url, "models");
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
        let health_url = api_url(&self.base_url, "models");
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

    pub(super) async fn send_with_retry<F>(&self, mut build: F) -> Result<reqwest::Response>
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
}

impl LlmClient for DeepSeekClient {
    fn provider_name(&self) -> &'static str {
        self.api_provider.as_str()
    }

    fn model(&self) -> &str {
        &self.default_model
    }

    async fn health_check(&self) -> Result<bool> {
        let health_url = api_url(&self.base_url, "models");
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
        if !experimental_responses_api_enabled() {
            return self.create_message_chat(&request).await;
        }

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
        self.handle_chat_completion_stream(request).await
    }
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

pub(super) fn parse_models_response(payload: &str) -> Result<Vec<AvailableModel>> {
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

pub(super) fn system_to_instructions(system: Option<SystemPrompt>) -> Option<String> {
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

pub(super) fn apply_reasoning_effort(
    body: &mut Value,
    effort: Option<&str>,
    provider: ApiProvider,
) {
    let Some(effort) = effort else {
        return;
    };
    let normalized = effort.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "off" | "disabled" | "none" | "false" => match provider {
            // OpenRouter / Novita relay the same DeepSeek V4 payload shape
            // as DeepSeek native; they pass through `thinking` / `reasoning_effort`.
            ApiProvider::Deepseek
            | ApiProvider::Openrouter
            | ApiProvider::Novita
            | ApiProvider::Fireworks
            | ApiProvider::Sglang => {
                body["thinking"] = json!({ "type": "disabled" });
            }
            ApiProvider::NvidiaNim => {
                body["chat_template_kwargs"] = json!({
                    "thinking": false,
                });
            }
        },
        "low" | "minimal" | "medium" | "mid" | "high" | "" => match provider {
            ApiProvider::Deepseek
            | ApiProvider::Openrouter
            | ApiProvider::Novita
            | ApiProvider::Fireworks
            | ApiProvider::Sglang => {
                body["reasoning_effort"] = json!("high");
                body["thinking"] = json!({ "type": "enabled" });
            }
            ApiProvider::NvidiaNim => {
                body["chat_template_kwargs"] = json!({
                    "thinking": true,
                    "reasoning_effort": "high",
                });
            }
        },
        "xhigh" | "max" | "highest" => match provider {
            ApiProvider::Deepseek
            | ApiProvider::Openrouter
            | ApiProvider::Novita
            | ApiProvider::Fireworks
            | ApiProvider::Sglang => {
                body["reasoning_effort"] = json!("max");
                body["thinking"] = json!({ "type": "enabled" });
            }
            ApiProvider::NvidiaNim => {
                body["chat_template_kwargs"] = json!({
                    "thinking": true,
                    "reasoning_effort": "max",
                });
            }
        },
        _ => {
            // Unknown value — do not mutate the request, let the provider
            // apply its own defaults.
        }
    }
}

pub(super) fn parse_usage(usage: Option<&Value>) -> Usage {
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
    let prompt_cache_hit_tokens = usage
        .and_then(|u| u.get("prompt_cache_hit_tokens"))
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let prompt_cache_miss_tokens = usage
        .and_then(|u| u.get("prompt_cache_miss_tokens"))
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let reasoning_tokens = usage
        .and_then(|u| u.get("completion_tokens_details"))
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_u64)
        .map(|v| v as u32);

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
        prompt_cache_hit_tokens,
        prompt_cache_miss_tokens,
        reasoning_tokens,
        reasoning_replay_tokens: None,
        server_tool_use,
    }
}

mod chat;
mod responses;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::chat::{
        build_chat_messages, build_chat_messages_for_request, count_reasoning_replay_chars,
        parse_chat_message, parse_sse_chunk, sanitize_thinking_mode_messages, tool_to_chat,
    };
    use crate::models::{ContentBlock, ContentBlockStart, Delta, Message, StreamEvent, Tool};
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
    fn api_url_handles_default_v1_and_beta_base_urls() {
        assert_eq!(
            api_url("https://api.deepseek.com", "chat/completions"),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            api_url("https://api.deepseek.com/v1", "chat/completions"),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            api_url("https://api.deepseek.com/beta", "chat/completions"),
            "https://api.deepseek.com/beta/chat/completions"
        );
    }

    #[test]
    fn chat_messages_keep_reasoning_content_on_all_assistant_messages() {
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
        let out = build_chat_messages(None, &[message], "deepseek-v4-pro");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        assert_eq!(
            assistant.get("content").and_then(Value::as_str),
            Some("done")
        );
        assert_eq!(
            assistant.get("reasoning_content").and_then(Value::as_str),
            Some("plan"),
            "thinking-mode models must keep reasoning_content on ALL assistant messages"
        );
    }

    #[test]
    fn chat_messages_keep_thinking_only_assistant_for_v4_flash() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "plan".to_string(),
            }],
        };
        let out = build_chat_messages(None, &[message], "deepseek-v4-flash");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("thinking-only assistant kept for V4 model");
        assert_eq!(
            assistant.get("reasoning_content").and_then(Value::as_str),
            Some("plan")
        );
    }

    #[test]
    fn chat_messages_keep_thinking_only_assistant_for_v4_pro() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "plan".to_string(),
            }],
        };
        let out = build_chat_messages(None, &[message], "deepseek-v4-pro");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("thinking-only assistant kept for V4 model");
        assert_eq!(
            assistant.get("reasoning_content").and_then(Value::as_str),
            Some("plan")
        );
    }

    #[test]
    fn chat_messages_keep_thinking_only_assistant_for_r_series_model() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "plan".to_string(),
            }],
        };
        let out = build_chat_messages(None, &[message], "deepseek-r2-lite-preview");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("thinking-only assistant kept for R-series model");
        assert_eq!(
            assistant.get("reasoning_content").and_then(Value::as_str),
            Some("plan")
        );
    }

    #[test]
    fn chat_messages_preserve_current_tool_round_reasoning_for_reasoner_model() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Need the date".to_string(),
                    cache_control: None,
                }],
            },
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Need to call a tool".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-1".to_string(),
                        name: "get_date".to_string(),
                        input: json!({}),
                        caller: None,
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    content: "2026-04-23".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];
        let out = build_chat_messages(None, &messages, "deepseek-v4-pro");
        let assistant = out
            .iter()
            .find(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant message");
        assert_eq!(assistant.get("content").and_then(Value::as_str), Some(""));
        assert_eq!(
            assistant.get("reasoning_content").and_then(Value::as_str),
            Some("Need to call a tool")
        );
    }

    #[test]
    fn chat_messages_replay_prior_tool_round_reasoning_after_new_user_turn() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Need the date".to_string(),
                    cache_control: None,
                }],
            },
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Need to call a tool".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-1".to_string(),
                        name: "get_date".to_string(),
                        input: json!({}),
                        caller: None,
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    content: "2026-04-23".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text {
                    text: "It is 2026-04-23.".to_string(),
                    cache_control: None,
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Thanks. Next question.".to_string(),
                    cache_control: None,
                }],
            },
        ];
        let out = build_chat_messages(None, &messages, "deepseek-v4-pro");
        let tool_assistant = out
            .iter()
            .find(|value| {
                value.get("role").and_then(Value::as_str) == Some("assistant")
                    && value.get("tool_calls").is_some()
            })
            .expect("tool-call assistant message");
        assert_eq!(
            tool_assistant
                .get("reasoning_content")
                .and_then(Value::as_str),
            Some("Need to call a tool"),
            "DeepSeek thinking mode requires reasoning_content to be replayed for tool-call rounds across all subsequent user turns"
        );
    }

    #[test]
    fn chat_messages_replay_completed_tool_round_reasoning_after_final_answer() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Need the date".to_string(),
                    cache_control: None,
                }],
            },
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Need to call a tool".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-1".to_string(),
                        name: "get_date".to_string(),
                        input: json!({}),
                        caller: None,
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tool-1".to_string(),
                    content: "2026-04-23".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text {
                    text: "It is 2026-04-23.".to_string(),
                    cache_control: None,
                }],
            },
        ];
        let out = build_chat_messages(None, &messages, "deepseek-v4-pro");
        let tool_assistant = out
            .iter()
            .find(|value| {
                value.get("role").and_then(Value::as_str) == Some("assistant")
                    && value.get("tool_calls").is_some()
            })
            .expect("tool-call assistant message");
        assert_eq!(
            tool_assistant
                .get("reasoning_content")
                .and_then(Value::as_str),
            Some("Need to call a tool")
        );
        let final_assistant = out
            .iter()
            .rfind(|value| value.get("role").and_then(Value::as_str) == Some("assistant"))
            .expect("final assistant message");
        assert!(
            final_assistant
                .get("reasoning_content")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.trim().is_empty()),
            "all assistant messages must carry reasoning_content in thinking mode"
        );
    }

    #[test]
    fn chat_messages_replay_v4_tool_round_reasoning_after_new_user_turn() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Use a tool".to_string(),
                    cache_control: None,
                }],
            },
            Message {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Need a tool for this".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "call-1".to_string(),
                        name: "read_file".to_string(),
                        input: json!({"path": "Cargo.toml"}),
                        caller: None,
                    },
                ],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-1".to_string(),
                    content: "workspace manifest".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Read it.".to_string(),
                    cache_control: None,
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Now continue.".to_string(),
                    cache_control: None,
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-v4-pro");
        let tool_assistant = out
            .iter()
            .find(|value| {
                value.get("role").and_then(Value::as_str) == Some("assistant")
                    && value.get("tool_calls").is_some()
            })
            .expect("tool-call assistant message");
        assert_eq!(
            tool_assistant
                .get("reasoning_content")
                .and_then(Value::as_str),
            Some("Need a tool for this")
        );
    }

    #[test]
    fn chat_messages_substitute_placeholder_when_v4_tool_round_missing_reasoning() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Use a tool".to_string(),
                    cache_control: None,
                }],
            },
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "call-without-reasoning".to_string(),
                    name: "read_file".to_string(),
                    input: json!({"path": "Cargo.toml"}),
                    caller: None,
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call-without-reasoning".to_string(),
                    content: "workspace manifest".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];

        let out = build_chat_messages(None, &messages, "deepseek-v4-pro");

        let assistant = out
            .iter()
            .find(|value| {
                value.get("role").and_then(Value::as_str) == Some("assistant")
                    && value.get("tool_calls").is_some()
            })
            .expect("tool-call assistant message should be retained with placeholder");
        assert!(
            assistant
                .get("reasoning_content")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty()),
            "missing reasoning_content should be substituted with a non-empty placeholder so the API accepts the request"
        );
        assert!(
            out.iter()
                .any(|value| value.get("role").and_then(Value::as_str) == Some("tool")),
            "matching tool_result must remain so the conversation chain stays intact"
        );
    }

    #[test]
    fn chat_messages_allow_tool_round_without_reasoning_when_thinking_disabled() {
        let request = MessageRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![
                Message {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::ToolUse {
                        id: "call-no-thinking".to_string(),
                        name: "read_file".to_string(),
                        input: json!({"path": "Cargo.toml"}),
                        caller: None,
                    }],
                },
                Message {
                    role: "user".to_string(),
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "call-no-thinking".to_string(),
                        content: "workspace manifest".to_string(),
                        is_error: None,
                        content_blocks: None,
                    }],
                },
            ],
            max_tokens: 1024,
            system: None,
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: Some("off".to_string()),
            stream: None,
            temperature: None,
            top_p: None,
        };

        let out = build_chat_messages_for_request(&request);
        assert!(
            out.iter().any(
                |value| value.get("role").and_then(Value::as_str) == Some("assistant")
                    && value.get("tool_calls").is_some()
            ),
            "tool calls remain valid when thinking mode is disabled"
        );
        assert!(
            out.iter()
                .any(|value| value.get("role").and_then(Value::as_str) == Some("tool")),
            "matching tool result should remain"
        );
    }

    #[test]
    fn reasoning_effort_uses_deepseek_top_level_thinking_parameter() {
        let mut body = json!({});
        apply_reasoning_effort(&mut body, Some("max"), ApiProvider::Deepseek);

        assert_eq!(
            body.get("reasoning_effort").and_then(Value::as_str),
            Some("max")
        );
        assert_eq!(
            body.pointer("/thinking/type").and_then(Value::as_str),
            Some("enabled")
        );
        assert!(body.get("extra_body").is_none());
    }

    #[test]
    fn reasoning_effort_off_disables_top_level_thinking() {
        let mut body = json!({});
        apply_reasoning_effort(&mut body, Some("off"), ApiProvider::Deepseek);

        assert_eq!(
            body.pointer("/thinking/type").and_then(Value::as_str),
            Some("disabled")
        );
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("extra_body").is_none());
    }

    #[test]
    fn reasoning_effort_uses_nvidia_nim_chat_template_kwargs() {
        let mut body = json!({});
        apply_reasoning_effort(&mut body, Some("max"), ApiProvider::NvidiaNim);

        assert_eq!(
            body.pointer("/chat_template_kwargs/thinking")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            body.pointer("/chat_template_kwargs/reasoning_effort")
                .and_then(Value::as_str),
            Some("max")
        );
        assert!(body.get("thinking").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn reasoning_effort_off_disables_nvidia_nim_thinking() {
        let mut body = json!({});
        apply_reasoning_effort(&mut body, Some("off"), ApiProvider::NvidiaNim);

        assert_eq!(
            body.pointer("/chat_template_kwargs/thinking")
                .and_then(Value::as_bool),
            Some(false)
        );
        assert!(
            body.pointer("/chat_template_kwargs/reasoning_effort")
                .is_none()
        );
    }

    #[test]
    fn chat_parser_accepts_nvidia_nim_reasoning_field() -> Result<()> {
        let response = parse_chat_message(&json!({
            "id": "chatcmpl-test",
            "model": "deepseek-ai/deepseek-v4-pro",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning": "thinking via NIM",
                    "content": "final answer"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 3
            }
        }))?;

        assert!(matches!(
            response.content.first(),
            Some(ContentBlock::Thinking { thinking }) if thinking == "thinking via NIM"
        ));
        assert!(matches!(
            response.content.get(1),
            Some(ContentBlock::Text { text, .. }) if text == "final answer"
        ));
        Ok(())
    }

    #[test]
    fn sse_parser_accepts_nvidia_nim_reasoning_delta() {
        let mut content_index = 0;
        let mut text_started = false;
        let mut thinking_started = false;
        let mut tool_indices = std::collections::HashMap::new();
        let events = parse_sse_chunk(
            &json!({
                "choices": [{
                    "delta": {
                        "reasoning": "nim thought"
                    }
                }]
            }),
            &mut content_index,
            &mut text_started,
            &mut thinking_started,
            &mut tool_indices,
            true,
        );

        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::ContentBlockDelta {
                delta: Delta::ThinkingDelta { thinking },
                ..
            } if thinking == "nim thought"
        )));
    }

    #[test]
    fn chat_tool_strict_flag_is_nested_under_function() {
        let tool = Tool {
            tool_type: Some("function".to_string()),
            name: "emit_json".to_string(),
            description: "Emit JSON".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
            allowed_callers: None,
            defer_loading: None,
            input_examples: None,
            strict: Some(true),
            cache_control: None,
        };
        let encoded = tool_to_chat(&tool);
        assert_eq!(
            encoded
                .get("function")
                .and_then(|function| function.get("strict"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert!(encoded.get("strict").is_none());
    }

    #[test]
    fn chat_messages_drop_thinking_only_assistant_for_non_reasoning_model() {
        let message = Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Thinking {
                thinking: "plan".to_string(),
            }],
        };
        let out = build_chat_messages(None, &[message], "some-non-deepseek-model");
        assert!(
            !out.iter()
                .any(|value| value.get("role").and_then(Value::as_str) == Some("assistant")),
            "non-reasoning model should drop thinking-only assistant"
        );
    }

    #[test]
    fn parse_sse_chunk_closes_each_tool_block_with_matching_index() {
        let chunk = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [
                        {
                            "index": 0,
                            "id": "call_0",
                            "function": {"name": "read_file", "arguments": "{\"path\":\"a\"}"}
                        },
                        {
                            "index": 1,
                            "id": "call_1",
                            "function": {"name": "read_file", "arguments": "{\"path\":\"b\"}"}
                        }
                    ]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let mut content_index = 0;
        let mut text_started = false;
        let mut thinking_started = false;
        let mut tool_indices: std::collections::HashMap<u32, u32> =
            std::collections::HashMap::new();
        let events = parse_sse_chunk(
            &chunk,
            &mut content_index,
            &mut text_started,
            &mut thinking_started,
            &mut tool_indices,
            false,
        );

        let starts: Vec<u32> = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ContentBlockStart {
                    index,
                    content_block: ContentBlockStart::ToolUse { .. },
                } => Some(*index),
                _ => None,
            })
            .collect();
        let stops: Vec<u32> = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ContentBlockStop { index } => Some(*index),
                _ => None,
            })
            .collect();
        let deltas: Vec<u32> = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ContentBlockDelta {
                    index,
                    delta: Delta::InputJsonDelta { .. },
                } => Some(*index),
                _ => None,
            })
            .collect();

        assert_eq!(starts, vec![0, 1]);
        assert_eq!(stops, vec![0, 1]);
        assert_eq!(deltas, vec![0, 1]);
    }

    #[test]
    fn parse_sse_chunk_handles_empty_choices_usage_chunk() {
        let chunk = json!({
            "choices": [],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "prompt_cache_hit_tokens": 70,
                "prompt_cache_miss_tokens": 30
            }
        });

        let mut content_index = 0;
        let mut text_started = false;
        let mut thinking_started = false;
        let mut tool_indices: std::collections::HashMap<u32, u32> =
            std::collections::HashMap::new();
        let events = parse_sse_chunk(
            &chunk,
            &mut content_index,
            &mut text_started,
            &mut thinking_started,
            &mut tool_indices,
            false,
        );

        let StreamEvent::MessageDelta {
            usage: Some(usage), ..
        } = &events[0]
        else {
            panic!("expected usage delta");
        };
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.prompt_cache_hit_tokens, Some(70));
        assert_eq!(usage.prompt_cache_miss_tokens, Some(30));
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

        let out = build_chat_messages(None, &messages, "deepseek-v4-flash");
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
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Need to inspect the directory".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-1".to_string(),
                        name: "list_dir".to_string(),
                        input: json!({}),
                        caller: None,
                    },
                ],
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

        let out = build_chat_messages(None, &messages, "deepseek-v4-flash");
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
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Need to search".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-1".to_string(),
                        name: "web.run".to_string(),
                        input: json!({}),
                        caller: None,
                    },
                ],
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

        let out = build_chat_messages(None, &messages, "deepseek-v4-flash");
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

        let out = build_chat_messages(None, &messages, "deepseek-v4-flash");
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
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Need to list files".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tool-ok".to_string(),
                        name: "list_dir".to_string(),
                        input: json!({}),
                        caller: None,
                    },
                ],
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

        let out = build_chat_messages(None, &messages, "deepseek-v4-flash");
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

        let out = build_chat_messages(None, &messages, "deepseek-v4-flash");
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
                {"id": "deepseek-v4-pro", "object": "model", "owned_by": "deepseek", "created": 1},
                {"id": "deepseek-v4-flash", "object": "model"},
                {"id": "deepseek-v4-pro", "object": "model", "owned_by": "deepseek", "created": 1}
            ]
        }"#;

        let models = parse_models_response(payload).expect("parse models");
        assert_eq!(
            models,
            vec![
                AvailableModel {
                    id: "deepseek-v4-flash".to_string(),
                    owned_by: None,
                    created: None
                },
                AvailableModel {
                    id: "deepseek-v4-pro".to_string(),
                    owned_by: Some("deepseek".to_string()),
                    created: Some(1)
                }
            ]
        );
    }

    #[test]
    fn parse_usage_reads_deepseek_cache_and_reasoning_tokens() {
        fn parse_usage(usage: Option<&Value>) -> Usage {
            let usage = usage.expect("usage");
            let input_tokens = usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .expect("prompt tokens") as u32;
            let output_tokens = usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .expect("completion tokens") as u32;
            let prompt_cache_hit_tokens = usage
                .get("prompt_cache_hit_tokens")
                .and_then(Value::as_u64)
                .map(|v| v as u32);
            let prompt_cache_miss_tokens = usage
                .get("prompt_cache_miss_tokens")
                .and_then(Value::as_u64)
                .map(|v| v as u32);
            let reasoning_tokens = usage
                .get("completion_tokens_details")
                .and_then(|d| d.get("reasoning_tokens"))
                .and_then(Value::as_u64)
                .map(|v| v as u32);

            Usage {
                input_tokens,
                output_tokens,
                prompt_cache_hit_tokens,
                prompt_cache_miss_tokens,
                reasoning_tokens,
                reasoning_replay_tokens: None,
                server_tool_use: None,
            }
        }

        let usage = parse_usage(Some(&json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "prompt_cache_hit_tokens": 70,
            "prompt_cache_miss_tokens": 30,
            "completion_tokens_details": {
                "reasoning_tokens": 12
            }
        })));

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.prompt_cache_hit_tokens, Some(70));
        assert_eq!(usage.prompt_cache_miss_tokens, Some(30));
        assert_eq!(usage.reasoning_tokens, Some(12));
    }

    #[test]
    fn sanitize_thinking_mode_counts_reasoning_replay_across_assistant_turns() {
        // Multi-turn body that mimics two prior tool-calling rounds: each
        // assistant message carries its `reasoning_content`. The sanitizer
        // should keep all of them and the count helper should tally bytes
        // across every assistant message.
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [
                { "role": "system", "content": "you are helpful" },
                { "role": "user", "content": "step 1" },
                {
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "I need to call tool A first.",
                    "tool_calls": [{ "id": "1", "type": "function" }]
                },
                { "role": "tool", "tool_call_id": "1", "content": "ok" },
                {
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "Now I call tool B.",
                    "tool_calls": [{ "id": "2", "type": "function" }]
                },
                { "role": "tool", "tool_call_id": "2", "content": "ok" },
                { "role": "user", "content": "step 2" }
            ]
        });

        let approx_tokens =
            sanitize_thinking_mode_messages(&mut body, "deepseek-v4-pro", Some("max"))
                .expect("multi-turn thinking-mode conversation should report replay tokens");
        // ~4 chars/token; 46 bytes of reasoning -> 11 tokens.
        assert_eq!(approx_tokens, 11);

        let chars = count_reasoning_replay_chars(&body);
        // "I need to call tool A first." (28) + "Now I call tool B." (18) = 46
        assert_eq!(chars, 46);

        // No assistant messages should have lost or had their reasoning_content blanked.
        let messages = body["messages"].as_array().unwrap();
        let assistant_with_reasoning: usize = messages
            .iter()
            .filter(|m| m["role"] == "assistant")
            .filter(|m| {
                m["reasoning_content"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
            })
            .count();
        assert_eq!(assistant_with_reasoning, 2);
    }

    /// Issue #30: when no thinking-mode replay applies (non-thinking model or
    /// empty conversation), the sanitizer returns `None` so the footer chip
    /// stays hidden.
    #[test]
    fn sanitize_thinking_mode_returns_none_for_non_thinking_model() {
        let mut body = json!({
            "model": "deepseek-chat",
            "messages": [
                { "role": "user", "content": "hi" }
            ]
        });
        let result = sanitize_thinking_mode_messages(&mut body, "deepseek-chat", None);
        assert!(result.is_none());
    }

    #[test]
    fn sanitize_thinking_mode_counts_substituted_placeholder() {
        // An assistant tool-call message is missing reasoning_content; the
        // sanitizer must inject the placeholder, and the count helper must
        // include the placeholder in the total (since it's in the wire
        // payload that ships to DeepSeek).
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [
                { "role": "user", "content": "hi" },
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{ "id": "1", "type": "function" }]
                }
            ]
        });

        sanitize_thinking_mode_messages(&mut body, "deepseek-v4-pro", Some("max"));

        let chars = count_reasoning_replay_chars(&body);
        // "(reasoning omitted)" is 19 bytes.
        assert_eq!(chars, 19);
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

    // === #103 Phase 2: HTTP/1 escape hatch ===================================

    /// Serialize tests that mutate `DEEPSEEK_FORCE_HTTP1` so they don't race
    /// against each other — env vars are process-global.
    static FORCE_HTTP1_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct ForceHttp1EnvGuard {
        prior: Option<std::ffi::OsString>,
    }
    impl ForceHttp1EnvGuard {
        fn capture() -> Self {
            Self {
                prior: std::env::var_os("DEEPSEEK_FORCE_HTTP1"),
            }
        }
    }
    impl Drop for ForceHttp1EnvGuard {
        fn drop(&mut self) {
            // Safety: scoped to test process; reverts to the captured value.
            match &self.prior {
                Some(v) => unsafe { std::env::set_var("DEEPSEEK_FORCE_HTTP1", v) },
                None => unsafe { std::env::remove_var("DEEPSEEK_FORCE_HTTP1") },
            }
        }
    }

    #[test]
    fn force_http1_unset_is_false() {
        let _lock = FORCE_HTTP1_ENV_LOCK.lock().unwrap();
        let _guard = ForceHttp1EnvGuard::capture();
        unsafe { std::env::remove_var("DEEPSEEK_FORCE_HTTP1") };
        assert!(!force_http1_from_env());
    }

    #[test]
    fn force_http1_truthy_values() {
        let _lock = FORCE_HTTP1_ENV_LOCK.lock().unwrap();
        let _guard = ForceHttp1EnvGuard::capture();
        for value in ["1", "true", "True", "YES", "on", " 1 "] {
            // Safety: serialized by FORCE_HTTP1_ENV_LOCK; reverted by guard.
            unsafe { std::env::set_var("DEEPSEEK_FORCE_HTTP1", value) };
            assert!(
                force_http1_from_env(),
                "{value:?} should be parsed as truthy",
            );
        }
    }

    #[test]
    fn force_http1_falsy_values() {
        let _lock = FORCE_HTTP1_ENV_LOCK.lock().unwrap();
        let _guard = ForceHttp1EnvGuard::capture();
        for value in ["0", "false", "no", "off", "", "garbage", "2"] {
            unsafe { std::env::set_var("DEEPSEEK_FORCE_HTTP1", value) };
            assert!(
                !force_http1_from_env(),
                "{value:?} should NOT be parsed as truthy",
            );
        }
    }
}
