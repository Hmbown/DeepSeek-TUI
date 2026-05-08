use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use deepseek_agent::ModelRegistry;
use deepseek_config::{CliRuntimeOverrides, ConfigStore};
use deepseek_core::Runtime;
use deepseek_execpolicy::ExecPolicyEngine;
use deepseek_hooks::{HookDispatcher, JsonlHookSink, StdoutHookSink};
use deepseek_mcp::McpManager;
use deepseek_protocol::{
    AppRequest, AppResponse, PromptRequest, PromptResponse, ThreadRequest, ThreadResponse,
};
use deepseek_state::StateStore;
use deepseek_tools::{ToolCall, ToolRegistry};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AppServerOptions {
    pub listen: SocketAddr,
    pub config_path: Option<PathBuf>,
}

#[derive(Clone)]
struct AppState {
    config_path: Option<PathBuf>,
    config: Arc<RwLock<deepseek_config::ConfigToml>>,
    runtime: Arc<Mutex<Runtime>>,
    registry: ModelRegistry,
    stdio_bridge: Arc<Mutex<Option<RuntimeBridge>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallRequest {
    call: ToolCall,
    #[serde(default)]
    cwd: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug)]
struct JsonRpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

#[derive(Debug)]
struct StdioDispatchResult {
    result: Value,
    should_exit: bool,
}

#[derive(Debug, Deserialize)]
struct ConfigGetParams {
    key: String,
}

#[derive(Debug, Deserialize)]
struct ConfigSetParams {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct ThreadIdParams {
    thread_id: String,
}

#[derive(Debug, Deserialize)]
struct ThreadMessageParams {
    thread_id: String,
    input: String,
}

#[derive(Debug)]
struct RuntimeBridge {
    base_url: String,
    client: reqwest::Client,
    auth_token: Option<String>,
    child: Option<Child>,
    last_seq_by_thread: HashMap<String, u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnTerminalStatus {
    Completed,
    Failed,
    Interrupted,
    Canceled,
}

pub async fn run(options: AppServerOptions) -> Result<()> {
    let state = build_state(options.config_path.clone())?;

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/thread", post(thread_handler))
        .route("/app", post(app_handler))
        .route("/prompt", post(prompt_handler))
        .route("/tool", post(tool_handler))
        .route("/jobs", get(jobs_handler))
        .route("/mcp/startup", post(mcp_startup_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(options.listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn run_stdio(config_path: Option<PathBuf>) -> Result<()> {
    let state = build_state(config_path)?;
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut writer = tokio::io::BufWriter::new(stdout);
    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(err) => {
                let response = jsonrpc_error(
                    None,
                    JsonRpcError::parse_error(format!("invalid json: {err}")),
                );
                writer.write_all(response.to_string().as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                continue;
            }
        };

        if request
            .jsonrpc
            .as_deref()
            .is_some_and(|version| version != "2.0")
        {
            let response = jsonrpc_error(
                request.id,
                JsonRpcError::invalid_request("jsonrpc version must be 2.0"),
            );
            writer.write_all(response.to_string().as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            continue;
        }

        let response = match dispatch_stdio_request(
            &state,
            &mut writer,
            &request.method,
            request.params,
        )
        .await
        {
            Ok(dispatch) => {
                let encoded = jsonrpc_result(request.id, dispatch.result);
                writer.write_all(encoded.to_string().as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                if dispatch.should_exit {
                    break;
                }
                continue;
            }
            Err(err) => jsonrpc_error(request.id, err),
        };

        writer.write_all(response.to_string().as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }

    Ok(())
}

async fn healthz() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "protocol": "v2",
        "service": "deepseek-app-server"
    }))
}

async fn thread_handler(
    State(state): State<AppState>,
    Json(req): Json<ThreadRequest>,
) -> Json<ThreadResponse> {
    match handle_thread_request(&state, req).await {
        Ok(res) => Json(res),
        Err(err) => Json(ThreadResponse {
            thread_id: "error".to_string(),
            status: format!("error:{}", err.message),
            thread: None,
            threads: Vec::new(),
            model: None,
            model_provider: None,
            cwd: None,
            approval_policy: None,
            sandbox: None,
            events: Vec::new(),
            data: json!({}),
        }),
    }
}

async fn prompt_handler(
    State(state): State<AppState>,
    Json(req): Json<PromptRequest>,
) -> Json<PromptResponse> {
    let mut runtime = state.runtime.lock().await;
    let overrides = CliRuntimeOverrides::default();
    match runtime.handle_prompt(req, &overrides).await {
        Ok(res) => Json(res),
        Err(err) => Json(PromptResponse {
            output: err.to_string(),
            model: "unknown".to_string(),
            events: Vec::new(),
        }),
    }
}

async fn tool_handler(
    State(state): State<AppState>,
    Json(req): Json<ToolCallRequest>,
) -> Json<Value> {
    let runtime = state.runtime.lock().await;
    let cwd = req
        .cwd
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    match runtime
        .invoke_tool(
            req.call,
            deepseek_execpolicy::AskForApproval::OnRequest,
            &cwd,
        )
        .await
    {
        Ok(value) => Json(value),
        Err(err) => Json(json!({ "ok": false, "error": err.to_string() })),
    }
}

async fn jobs_handler(State(state): State<AppState>) -> Json<AppResponse> {
    let runtime = state.runtime.lock().await;
    Json(runtime.app_status())
}

async fn mcp_startup_handler(State(state): State<AppState>) -> Json<Value> {
    let runtime = state.runtime.lock().await;
    let summary = runtime.mcp_startup().await;
    Json(json!({
        "ok": true,
        "summary": summary
    }))
}

async fn app_handler(
    State(state): State<AppState>,
    Json(req): Json<AppRequest>,
) -> Json<AppResponse> {
    Json(process_app_request(&state, req).await)
}

fn build_state(config_path: Option<PathBuf>) -> Result<AppState> {
    let store = ConfigStore::load(config_path.clone())?;
    let config = store.config.clone();
    let registry = ModelRegistry::default();

    let state_db_path = config_path
        .as_ref()
        .and_then(|p| p.parent().map(|parent| parent.join("state.db")));
    let state_store = StateStore::open(state_db_path)?;

    let mut hooks = HookDispatcher::default();
    hooks.add_sink(Arc::new(StdoutHookSink));
    let hook_log_path = config_path
        .as_ref()
        .and_then(|p| p.parent().map(|parent| parent.join("events.jsonl")))
        .unwrap_or_else(|| PathBuf::from(".deepseek/events.jsonl"));
    hooks.add_sink(Arc::new(JsonlHookSink::new(hook_log_path)));

    let runtime = Runtime::new(
        config.clone(),
        registry.clone(),
        state_store,
        Arc::new(ToolRegistry::default()),
        Arc::new(McpManager::default()),
        ExecPolicyEngine::new(Vec::new(), Vec::new()),
        hooks,
    );

    Ok(AppState {
        config_path,
        config: Arc::new(RwLock::new(config)),
        runtime: Arc::new(Mutex::new(runtime)),
        registry,
        stdio_bridge: Arc::new(Mutex::new(None)),
    })
}

async fn invalidate_stdio_bridge(state: &AppState) {
    let mut bridge = state.stdio_bridge.lock().await;
    *bridge = None;
}

impl RuntimeBridge {
    async fn start(config_path: Option<&Path>) -> Result<Self> {
        let port = reserve_runtime_port()?;
        let auth_token = Uuid::new_v4().to_string();
        let child = Self::runtime_command(config_path, port, &auth_token)?.spawn()?;
        let mut bridge = Self {
            base_url: format!("http://127.0.0.1:{port}"),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            auth_token: Some(auth_token),
            child: Some(child),
            last_seq_by_thread: HashMap::new(),
        };
        bridge.wait_until_ready().await?;
        Ok(bridge)
    }

    fn runtime_command(config_path: Option<&Path>, port: u16, auth_token: &str) -> Result<Command> {
        let current_exe = std::env::current_exe().ok();
        let use_current_exe = current_exe
            .as_ref()
            .and_then(|path| path.file_stem())
            .and_then(|name| name.to_str())
            .is_some_and(|name| !name.contains("app-server"));
        let mut cmd = if use_current_exe {
            Command::new(current_exe.context("failed to resolve current executable")?)
        } else {
            Command::new("deepseek")
        };
        if let Some(config) = config_path {
            cmd.arg("--config").arg(config);
        }
        cmd.arg("serve")
            .arg("--http")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--auth-token")
            .arg(auth_token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Ok(cmd)
    }

    async fn wait_until_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(child) = self.child.as_mut()
                && let Some(status) = child.try_wait()?
            {
                return Err(anyhow!(
                    "runtime API exited before becoming ready (status {status})"
                ));
            }

            match self
                .client
                .get(format!("{}/health", self.base_url))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => return Ok(()),
                _ if Instant::now() >= deadline => {
                    return Err(anyhow!(
                        "timed out waiting for runtime API at {}/health",
                        self.base_url
                    ));
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    fn authed(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.auth_token.as_deref() {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }

    async fn request_json(&self, builder: reqwest::RequestBuilder) -> Result<Value> {
        let response = builder.send().await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            let detail = body.trim();
            if detail.is_empty() {
                anyhow::bail!("runtime API returned {status}");
            }
            anyhow::bail!("runtime API returned {status}: {detail}");
        }
        serde_json::from_str(&body).with_context(|| format!("invalid runtime API json: {body}"))
    }

    async fn create_thread(
        &mut self,
        model: Option<String>,
        workspace: Option<PathBuf>,
    ) -> Result<Value> {
        let record = self
            .request_json(
                self.authed(self.client.post(format!("{}/v1/threads", self.base_url)))
                    .json(&json!({
                        "model": model,
                        "workspace": workspace,
                        "mode": "agent",
                        "archived": false,
                    })),
            )
            .await?;
        let thread_id = extract_runtime_thread_id(&record)?;
        self.last_seq_by_thread
            .entry(thread_id.to_string())
            .or_insert(0);
        Ok(thread_record_result("started", &record))
    }

    async fn resume_thread(&mut self, thread_id: &str) -> Result<Value> {
        let record = self
            .request_json(
                self.authed(
                    self.client
                        .post(format!("{}/v1/threads/{thread_id}/resume", self.base_url)),
                ),
            )
            .await?;
        self.last_seq_by_thread
            .entry(thread_id.to_string())
            .or_insert(0);
        Ok(thread_record_result("resumed", &record))
    }

    async fn fork_thread(&mut self, thread_id: &str) -> Result<Value> {
        let record = self
            .request_json(
                self.authed(
                    self.client
                        .post(format!("{}/v1/threads/{thread_id}/fork", self.base_url)),
                ),
            )
            .await?;
        let forked_id = extract_runtime_thread_id(&record)?;
        self.last_seq_by_thread
            .entry(forked_id.to_string())
            .or_insert(0);
        Ok(thread_record_result("forked", &record))
    }

    async fn read_thread(&self, thread_id: &str) -> Result<Value> {
        let detail = self
            .request_json(
                self.authed(
                    self.client
                        .get(format!("{}/v1/threads/{thread_id}", self.base_url)),
                ),
            )
            .await?;
        let thread = detail.get("thread").cloned().unwrap_or(Value::Null);
        Ok(json!({
            "thread_id": thread_id,
            "status": "ok",
            "thread": thread,
            "threads": [],
            "model": detail.pointer("/thread/model").cloned().unwrap_or(Value::Null),
            "model_provider": "deepseek",
            "cwd": detail.pointer("/thread/workspace").cloned().unwrap_or(Value::Null),
            "approval_policy": Value::Null,
            "sandbox": Value::Null,
            "events": [],
            "data": detail,
        }))
    }

    async fn list_threads(&self, include_archived: bool, limit: Option<usize>) -> Result<Value> {
        let mut url = format!(
            "{}/v1/threads?include_archived={}",
            self.base_url, include_archived
        );
        if let Some(limit) = limit {
            url.push_str(&format!("&limit={limit}"));
        }
        let threads = self.request_json(self.authed(self.client.get(url))).await?;
        Ok(json!({
            "thread_id": "list",
            "status": "ok",
            "thread": Value::Null,
            "threads": threads,
            "model": Value::Null,
            "model_provider": Value::Null,
            "cwd": Value::Null,
            "approval_policy": Value::Null,
            "sandbox": Value::Null,
            "events": [],
            "data": {},
        }))
    }

    async fn update_thread(
        &self,
        thread_id: &str,
        body: Value,
        status: &'static str,
    ) -> Result<Value> {
        let record = self
            .request_json(
                self.authed(
                    self.client
                        .patch(format!("{}/v1/threads/{thread_id}", self.base_url)),
                )
                .json(&body),
            )
            .await?;
        Ok(thread_record_result(status, &record))
    }

    async fn message_thread<W: AsyncWrite + Unpin>(
        &mut self,
        thread_id: &str,
        input: &str,
        writer: &mut W,
    ) -> Result<Value> {
        let turn = self
            .request_json(
                self.authed(
                    self.client
                        .post(format!("{}/v1/threads/{thread_id}/turns", self.base_url)),
                )
                .json(&json!({ "prompt": input })),
            )
            .await?;
        let turn_id = turn
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("runtime API turn response missing turn.id"))?;
        let response_id = format!("{thread_id}:{turn_id}");
        emit_stdio_event(
            writer,
            json!({
                "type": "response_start",
                "response_id": response_id,
            }),
        )
        .await?;

        let since_seq = self.last_seq_by_thread.get(thread_id).copied().unwrap_or(0);
        let stream_result = self
            .stream_turn_events(thread_id, turn_id, &response_id, writer, since_seq)
            .await;

        let _ = emit_stdio_event(
            writer,
            json!({
                "type": "response_end",
                "response_id": response_id,
            }),
        )
        .await;

        let (last_seq, status, error) = stream_result?;
        self.last_seq_by_thread
            .insert(thread_id.to_string(), last_seq);

        match status {
            TurnTerminalStatus::Completed => Ok(json!({
                "thread_id": thread_id,
                "status": "accepted",
                "thread": Value::Null,
                "threads": [],
                "model": Value::Null,
                "model_provider": Value::Null,
                "cwd": Value::Null,
                "approval_policy": Value::Null,
                "sandbox": Value::Null,
                "events": [],
                "data": { "turn_id": turn_id },
            })),
            TurnTerminalStatus::Interrupted => Err(anyhow!(
                "{}",
                error.unwrap_or_else(|| "turn interrupted".to_string())
            )),
            TurnTerminalStatus::Canceled => Err(anyhow!(
                "{}",
                error.unwrap_or_else(|| "turn canceled".to_string())
            )),
            TurnTerminalStatus::Failed => Err(anyhow!(
                "{}",
                error.unwrap_or_else(|| "turn failed".to_string())
            )),
        }
    }

    async fn stream_turn_events<W: AsyncWrite + Unpin>(
        &self,
        thread_id: &str,
        turn_id: &str,
        response_id: &str,
        writer: &mut W,
        since_seq: u64,
    ) -> Result<(u64, TurnTerminalStatus, Option<String>)> {
        let mut response = self
            .authed(self.client.get(format!(
                "{}/v1/threads/{thread_id}/events?since_seq={since_seq}",
                self.base_url
            )))
            .send()
            .await?
            .error_for_status()?;

        let mut buffer = Vec::new();
        let mut last_seq = since_seq;
        let mut tool_names_by_item: HashMap<String, String> = HashMap::new();

        while let Some(chunk) = response.chunk().await? {
            buffer.extend_from_slice(&chunk);
            while let Some(frame_bytes) = take_sse_frame(&mut buffer) {
                let Some((event_name, frame_data)) = parse_sse_frame(&frame_bytes) else {
                    continue;
                };
                let envelope: Value = serde_json::from_str(&frame_data)
                    .with_context(|| format!("invalid SSE json for {event_name}: {frame_data}"))?;
                if let Some(seq) = envelope.get("seq").and_then(Value::as_u64) {
                    last_seq = last_seq.max(seq);
                }
                if envelope.get("turn_id").and_then(Value::as_str) != Some(turn_id) {
                    continue;
                }
                let payload = envelope.get("payload").cloned().unwrap_or(Value::Null);
                match event_name.as_str() {
                    "item.delta" => {
                        let kind = payload
                            .get("kind")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if kind == "agent_message"
                            && let Some(delta) = payload.get("delta").and_then(Value::as_str)
                            && !delta.is_empty()
                        {
                            emit_stdio_event(
                                writer,
                                json!({
                                    "type": "response_delta",
                                    "response_id": response_id,
                                    "delta": delta,
                                }),
                            )
                            .await?;
                        }
                    }
                    "item.started" => {
                        let Some(tool) = payload.get("tool") else {
                            continue;
                        };
                        let tool_name = tool
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool")
                            .to_string();
                        if let Some(item_id) = envelope.get("item_id").and_then(Value::as_str) {
                            tool_names_by_item.insert(item_id.to_string(), tool_name.clone());
                        }
                        let arguments = tool.get("input").cloned().unwrap_or_else(|| json!({}));
                        emit_stdio_event(
                            writer,
                            json!({
                                "type": "tool_call_start",
                                "response_id": response_id,
                                "tool_name": tool_name,
                                "arguments": arguments.clone(),
                            }),
                        )
                        .await?;
                        emit_stdio_event(
                            writer,
                            json!({
                                "type": "tool_lifecycle",
                                "response_id": response_id,
                                "tool_name": tool.get("name").and_then(Value::as_str).unwrap_or("tool"),
                                "phase": "start",
                                "payload": arguments,
                            }),
                        )
                        .await?;
                    }
                    "item.completed" | "item.failed" => {
                        let Some(item) = payload.get("item") else {
                            continue;
                        };
                        let kind = item.get("kind").and_then(Value::as_str).unwrap_or_default();
                        if kind != "tool_call"
                            && kind != "file_change"
                            && kind != "command_execution"
                        {
                            continue;
                        }
                        let item_id = envelope
                            .get("item_id")
                            .and_then(Value::as_str)
                            .or_else(|| item.get("id").and_then(Value::as_str))
                            .unwrap_or_default();
                        let tool_name = tool_names_by_item
                            .remove(item_id)
                            .unwrap_or_else(|| infer_tool_name(item));
                        let output = item
                            .get("detail")
                            .and_then(Value::as_str)
                            .or_else(|| item.get("summary").and_then(Value::as_str))
                            .unwrap_or_default()
                            .to_string();
                        let success = event_name == "item.completed";
                        emit_stdio_event(
                            writer,
                            json!({
                                "type": "tool_call_result",
                                "response_id": response_id,
                                "tool_name": tool_name.clone(),
                                "success": success,
                                "output": output,
                            }),
                        )
                        .await?;
                        emit_stdio_event(
                            writer,
                            json!({
                                "type": "tool_lifecycle",
                                "response_id": response_id,
                                "tool_name": tool_name,
                                "phase": if success { "complete" } else { "error" },
                                "payload": if success {
                                    json!({ "output": output })
                                } else {
                                    json!({ "error": output })
                                },
                            }),
                        )
                        .await?;
                    }
                    "turn.completed" => {
                        let turn = payload.get("turn").cloned().unwrap_or(Value::Null);
                        let status = match turn.get("status").and_then(Value::as_str) {
                            Some("completed") => TurnTerminalStatus::Completed,
                            Some("interrupted") => TurnTerminalStatus::Interrupted,
                            Some("canceled") => TurnTerminalStatus::Canceled,
                            _ => TurnTerminalStatus::Failed,
                        };
                        let error = turn
                            .get("error")
                            .and_then(Value::as_str)
                            .map(ToString::to_string);
                        return Ok((last_seq, status, error));
                    }
                    _ => {}
                }
            }
        }

        Err(anyhow!(
            "runtime event stream ended before turn.completed for {turn_id}"
        ))
    }
}

impl Drop for RuntimeBridge {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
impl RuntimeBridge {
    fn from_base_url_for_test(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("build reqwest test client"),
            auth_token: None,
            child: None,
            last_seq_by_thread: HashMap::new(),
        }
    }
}

fn reserve_runtime_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn extract_runtime_thread_id(record: &Value) -> Result<&str> {
    record
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("runtime API thread response missing id"))
}

fn thread_record_result(status: &str, record: &Value) -> Value {
    let thread_id = record.get("id").and_then(Value::as_str).unwrap_or_default();
    json!({
        "thread_id": thread_id,
        "status": status,
        "thread": record,
        "threads": [],
        "model": record.get("model").cloned().unwrap_or(Value::Null),
        "model_provider": "deepseek",
        "cwd": record.get("workspace").cloned().unwrap_or(Value::Null),
        "approval_policy": Value::Null,
        "sandbox": Value::Null,
        "events": [],
        "data": record,
    })
}

fn infer_tool_name(item: &Value) -> String {
    item.get("summary")
        .and_then(Value::as_str)
        .and_then(|summary| summary.split(':').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("tool")
        .to_string()
}

async fn emit_stdio_event<W: AsyncWrite + Unpin>(writer: &mut W, event: Value) -> Result<()> {
    writer.write_all(event.to_string().as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

fn take_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    if let Some(pos) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
        return Some(buffer.drain(..pos + 4).collect());
    }
    buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|pos| buffer.drain(..pos + 2).collect())
}

fn parse_sse_frame(frame_bytes: &[u8]) -> Option<(String, String)> {
    let text = String::from_utf8(frame_bytes.to_vec()).ok()?;
    let mut event_name = None;
    let mut data_lines = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("event:") {
            event_name = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }
    match (event_name, data_lines.is_empty()) {
        (Some(event), false) => Some((event, data_lines.join("\n"))),
        _ => None,
    }
}

fn params_or_object(params: Value) -> Value {
    if params.is_null() { json!({}) } else { params }
}

fn parse_params<T: DeserializeOwned>(params: Value) -> std::result::Result<T, JsonRpcError> {
    serde_json::from_value(params).map_err(|err| JsonRpcError::invalid_params(err.to_string()))
}

fn jsonrpc_result(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "result": result
    })
}

fn jsonrpc_error(id: Option<Value>, err: JsonRpcError) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {
            "code": err.code,
            "message": err.message,
            "data": err.data
        }
    })
}

impl JsonRpcError {
    fn parse_error(message: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: message.into(),
            data: None,
        }
    }

    fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: message.into(),
            data: None,
        }
    }

    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("unsupported method: {method}"),
            data: None,
        }
    }

    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
            data: None,
        }
    }
}

async fn handle_thread_request(
    state: &AppState,
    req: ThreadRequest,
) -> std::result::Result<ThreadResponse, JsonRpcError> {
    let mut runtime = state.runtime.lock().await;
    runtime
        .handle_thread(req)
        .await
        .map_err(|err| JsonRpcError::internal(err.to_string()))
}

async fn handle_prompt_request(
    state: &AppState,
    req: PromptRequest,
) -> std::result::Result<PromptResponse, JsonRpcError> {
    let mut runtime = state.runtime.lock().await;
    runtime
        .handle_prompt(req, &CliRuntimeOverrides::default())
        .await
        .map_err(|err| JsonRpcError::internal(err.to_string()))
}

async fn dispatch_stdio_thread_request<W: AsyncWrite + Unpin>(
    state: &AppState,
    writer: &mut W,
    req: ThreadRequest,
) -> std::result::Result<Value, JsonRpcError> {
    let mut bridge_slot = state.stdio_bridge.lock().await;
    if bridge_slot.is_none() {
        let bridge = RuntimeBridge::start(state.config_path.as_deref())
            .await
            .map_err(|err| JsonRpcError::internal(err.to_string()))?;
        *bridge_slot = Some(bridge);
    }
    let bridge = bridge_slot
        .as_mut()
        .ok_or_else(|| JsonRpcError::internal("failed to initialize runtime bridge"))?;
    let result = match req {
        ThreadRequest::Create { .. } => bridge.create_thread(None, None).await,
        ThreadRequest::Start(params) => bridge.create_thread(params.model, params.cwd).await,
        ThreadRequest::Resume(params) => bridge.resume_thread(&params.thread_id).await,
        ThreadRequest::Fork(params) => bridge.fork_thread(&params.thread_id).await,
        ThreadRequest::List(params) => {
            bridge
                .list_threads(params.include_archived, params.limit)
                .await
        }
        ThreadRequest::Read(params) => bridge.read_thread(&params.thread_id).await,
        ThreadRequest::SetName(params) => {
            bridge
                .update_thread(&params.thread_id, json!({ "title": params.name }), "ok")
                .await
        }
        ThreadRequest::Archive { thread_id } => {
            bridge
                .update_thread(&thread_id, json!({ "archived": true }), "archived")
                .await
        }
        ThreadRequest::Unarchive { thread_id } => {
            bridge
                .update_thread(&thread_id, json!({ "archived": false }), "unarchived")
                .await
        }
        ThreadRequest::Message { thread_id, input } => {
            bridge.message_thread(&thread_id, &input, writer).await
        }
    };
    result.map_err(|err| JsonRpcError::internal(err.to_string()))
}

async fn dispatch_stdio_request(
    state: &AppState,
    writer: &mut (impl AsyncWrite + Unpin),
    method: &str,
    params: Value,
) -> std::result::Result<StdioDispatchResult, JsonRpcError> {
    let outcome = match method {
        "healthz" | "app/healthz" => StdioDispatchResult {
            result: json!({
                "status": "ok",
                "service": "deepseek-app-server",
                "transport": "stdio"
            }),
            should_exit: false,
        },
        "capabilities" => StdioDispatchResult {
            result: json!({
                "transport": "stdio",
                "families": ["thread/*", "app/*", "prompt/*"],
                "methods": [
                    "healthz",
                    "thread/capabilities",
                    "thread/request",
                    "thread/create",
                    "thread/start",
                    "thread/resume",
                    "thread/fork",
                    "thread/list",
                    "thread/read",
                    "thread/set_name",
                    "thread/archive",
                    "thread/unarchive",
                    "thread/message",
                    "app/capabilities",
                    "app/request",
                    "app/config/get",
                    "app/config/set",
                    "app/config/unset",
                    "app/config/list",
                    "app/models",
                    "app/thread_loaded_list",
                    "prompt/capabilities",
                    "prompt/request",
                    "prompt/run",
                    "shutdown"
                ]
            }),
            should_exit: false,
        },
        "thread/capabilities" => StdioDispatchResult {
            result: json!({
                "methods": [
                    "thread/request",
                    "thread/create",
                    "thread/start",
                    "thread/resume",
                    "thread/fork",
                    "thread/list",
                    "thread/read",
                    "thread/set_name",
                    "thread/archive",
                    "thread/unarchive",
                    "thread/message"
                ]
            }),
            should_exit: false,
        },
        "thread/request" => {
            let request: ThreadRequest = parse_params(params)?;
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(state, writer, request).await?,
                should_exit: false,
            }
        }
        "thread/create" => {
            #[derive(Debug, Deserialize)]
            struct CreateParams {
                #[serde(default)]
                metadata: Value,
            }
            let parsed: CreateParams = parse_params(params_or_object(params))?;
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(
                    state,
                    writer,
                    ThreadRequest::Create {
                        metadata: parsed.metadata,
                    },
                )
                .await?,
                should_exit: false,
            }
        }
        "thread/start" => {
            let request = ThreadRequest::Start(parse_params(params_or_object(params))?);
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(state, writer, request).await?,
                should_exit: false,
            }
        }
        "thread/resume" => {
            let request = ThreadRequest::Resume(parse_params(params_or_object(params))?);
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(state, writer, request).await?,
                should_exit: false,
            }
        }
        "thread/fork" => {
            let request = ThreadRequest::Fork(parse_params(params_or_object(params))?);
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(state, writer, request).await?,
                should_exit: false,
            }
        }
        "thread/list" => {
            let request = ThreadRequest::List(parse_params(params_or_object(params))?);
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(state, writer, request).await?,
                should_exit: false,
            }
        }
        "thread/read" => {
            let request = ThreadRequest::Read(parse_params(params_or_object(params))?);
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(state, writer, request).await?,
                should_exit: false,
            }
        }
        "thread/set_name" | "thread/set-name" => {
            let request = ThreadRequest::SetName(parse_params(params_or_object(params))?);
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(state, writer, request).await?,
                should_exit: false,
            }
        }
        "thread/archive" => {
            let parsed: ThreadIdParams = parse_params(params_or_object(params))?;
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(
                    state,
                    writer,
                    ThreadRequest::Archive {
                        thread_id: parsed.thread_id,
                    },
                )
                .await?,
                should_exit: false,
            }
        }
        "thread/unarchive" => {
            let parsed: ThreadIdParams = parse_params(params_or_object(params))?;
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(
                    state,
                    writer,
                    ThreadRequest::Unarchive {
                        thread_id: parsed.thread_id,
                    },
                )
                .await?,
                should_exit: false,
            }
        }
        "thread/message" => {
            let parsed: ThreadMessageParams = parse_params(params_or_object(params))?;
            StdioDispatchResult {
                result: dispatch_stdio_thread_request(
                    state,
                    writer,
                    ThreadRequest::Message {
                        thread_id: parsed.thread_id,
                        input: parsed.input,
                    },
                )
                .await?,
                should_exit: false,
            }
        }
        "app/capabilities" => {
            let response = process_app_request(state, AppRequest::Capabilities).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/request" => {
            let request: AppRequest = parse_params(params)?;
            let response = process_app_request(state, request).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/config/get" => {
            let parsed: ConfigGetParams = parse_params(params_or_object(params))?;
            let response =
                process_app_request(state, AppRequest::ConfigGet { key: parsed.key }).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/config/set" => {
            let parsed: ConfigSetParams = parse_params(params_or_object(params))?;
            let response = process_app_request(
                state,
                AppRequest::ConfigSet {
                    key: parsed.key,
                    value: parsed.value,
                },
            )
            .await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/config/unset" => {
            let parsed: ConfigGetParams = parse_params(params_or_object(params))?;
            let response =
                process_app_request(state, AppRequest::ConfigUnset { key: parsed.key }).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/config/list" => {
            let response = process_app_request(state, AppRequest::ConfigList).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/models" => {
            let response = process_app_request(state, AppRequest::Models).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "app/thread_loaded_list" | "app/thread-loaded-list" => {
            let response = process_app_request(state, AppRequest::ThreadLoadedList).await;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "prompt/capabilities" => StdioDispatchResult {
            result: json!({
                "methods": ["prompt/request", "prompt/run"]
            }),
            should_exit: false,
        },
        "prompt/request" | "prompt/run" => {
            let request: PromptRequest = parse_params(params)?;
            let response = handle_prompt_request(state, request).await?;
            StdioDispatchResult {
                result: serde_json::to_value(response)
                    .map_err(|err| JsonRpcError::internal(err.to_string()))?,
                should_exit: false,
            }
        }
        "shutdown" => StdioDispatchResult {
            result: json!({"ok": true, "status": "stopped"}),
            should_exit: true,
        },
        _ => return Err(JsonRpcError::method_not_found(method)),
    };
    Ok(outcome)
}

async fn process_app_request(state: &AppState, req: AppRequest) -> AppResponse {
    match req {
        AppRequest::Capabilities => AppResponse {
            ok: true,
            data: json!({
                "routes": ["/thread", "/app", "/prompt", "/tool", "/jobs", "/mcp/startup"],
                "config": ["get", "set", "unset", "list"],
                "events": ["response_start", "response_delta", "response_end", "tool_call_start", "tool_call_result", "tool_lifecycle", "mcp_startup_update", "mcp_startup_complete"],
                "transport": "stdio+http",
                "config_path": state.config_path.as_ref().map(|p| p.display().to_string()),
            }),
            events: Vec::new(),
        },
        AppRequest::ConfigGet { key } => {
            let cfg = state.config.read().await;
            AppResponse {
                ok: true,
                data: json!({ "key": key, "value": cfg.get_value(&key) }),
                events: Vec::new(),
            }
        }
        AppRequest::ConfigSet { key, value } => {
            let mut cfg = state.config.write().await;
            let result = cfg.set_value(&key, &value);
            let ok = result.is_ok();
            let message = result.err().map(|e| e.to_string());
            let snapshot = cfg.clone();
            drop(cfg);
            let _ = persist_config(state, snapshot).await;
            invalidate_stdio_bridge(state).await;
            AppResponse {
                ok,
                data: json!({ "key": key, "value": value, "error": message }),
                events: Vec::new(),
            }
        }
        AppRequest::ConfigUnset { key } => {
            let mut cfg = state.config.write().await;
            let result = cfg.unset_value(&key);
            let ok = result.is_ok();
            let message = result.err().map(|e| e.to_string());
            let snapshot = cfg.clone();
            drop(cfg);
            let _ = persist_config(state, snapshot).await;
            invalidate_stdio_bridge(state).await;
            AppResponse {
                ok,
                data: json!({ "key": key, "error": message }),
                events: Vec::new(),
            }
        }
        AppRequest::ConfigList => {
            let cfg = state.config.read().await;
            AppResponse {
                ok: true,
                data: json!({ "values": cfg.list_values() }),
                events: Vec::new(),
            }
        }
        AppRequest::Models => AppResponse {
            ok: true,
            data: json!({ "models": state.registry.list() }),
            events: Vec::new(),
        },
        AppRequest::ThreadLoadedList => {
            let mut runtime = state.runtime.lock().await;
            let response = runtime
                .handle_thread(deepseek_protocol::ThreadRequest::List(
                    deepseek_protocol::ThreadListParams {
                        include_archived: false,
                        limit: Some(50),
                    },
                ))
                .await;
            match response {
                Ok(thread_resp) => AppResponse {
                    ok: true,
                    data: json!({ "threads": thread_resp.threads }),
                    events: thread_resp.events,
                },
                Err(err) => AppResponse {
                    ok: false,
                    data: json!({ "error": err.to_string() }),
                    events: Vec::new(),
                },
            }
        }
    }
}

async fn persist_config(state: &AppState, config: deepseek_config::ConfigToml) -> Result<()> {
    if state.config_path.is_none() {
        return Ok(());
    }
    let mut store = ConfigStore::load(state.config_path.clone())?;
    store.config = config;
    store.save()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path as AxumPath, Query};
    use axum::http::header;
    use std::collections::HashMap;
    use tokio::io::AsyncReadExt;

    fn sse_frame(event: &str, payload: Value) -> String {
        format!("event: {event}\ndata: {payload}\n\n")
    }

    #[tokio::test]
    async fn message_thread_streams_stdio_events_before_returning() {
        async fn create_turn(AxumPath(thread_id): AxumPath<String>) -> Json<Value> {
            Json(json!({
                "thread": { "id": thread_id },
                "turn": { "id": "turn_test" },
            }))
        }

        async fn thread_events(
            AxumPath(thread_id): AxumPath<String>,
            Query(query): Query<HashMap<String, String>>,
        ) -> ([(header::HeaderName, &'static str); 1], String) {
            assert_eq!(thread_id, "thr_test");
            assert_eq!(query.get("since_seq").map(String::as_str), Some("0"));

            let body = [
                sse_frame(
                    "item.delta",
                    json!({
                        "seq": 1,
                        "turn_id": "turn_test",
                        "payload": {
                            "kind": "agent_message",
                            "delta": "hello"
                        }
                    }),
                ),
                sse_frame(
                    "item.started",
                    json!({
                        "seq": 2,
                        "turn_id": "turn_test",
                        "item_id": "tool_1",
                        "payload": {
                            "tool": {
                                "name": "shell",
                                "input": { "command": "pwd" }
                            }
                        }
                    }),
                ),
                sse_frame(
                    "item.completed",
                    json!({
                        "seq": 3,
                        "turn_id": "turn_test",
                        "item_id": "tool_1",
                        "payload": {
                            "item": {
                                "kind": "tool_call",
                                "summary": "shell: pwd",
                                "detail": "/tmp"
                            }
                        }
                    }),
                ),
                sse_frame(
                    "turn.completed",
                    json!({
                        "seq": 4,
                        "turn_id": "turn_test",
                        "payload": {
                            "turn": {
                                "status": "completed"
                            }
                        }
                    }),
                ),
            ]
            .concat();

            ([(header::CONTENT_TYPE, "text/event-stream")], body)
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");
        let app = Router::new()
            .route("/v1/threads/{thread_id}/turns", post(create_turn))
            .route("/v1/threads/{thread_id}/events", get(thread_events));

        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve test runtime");
        });

        let mut bridge = RuntimeBridge::from_base_url_for_test(format!("http://{addr}"));
        let (mut reader, mut writer) = tokio::io::duplex(4096);

        let result = bridge
            .message_thread("thr_test", "hello", &mut writer)
            .await
            .expect("message_thread should succeed");
        drop(writer);

        let mut stdout = Vec::new();
        reader
            .read_to_end(&mut stdout)
            .await
            .expect("read stdio output");
        server.abort();
        let _ = server.await;

        let lines: Vec<Value> = String::from_utf8(stdout)
            .expect("utf8 output")
            .lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect();

        assert_eq!(
            result.get("status").and_then(Value::as_str),
            Some("accepted")
        );
        assert_eq!(
            result.pointer("/data/turn_id").and_then(Value::as_str),
            Some("turn_test")
        );
        assert_eq!(bridge.last_seq_by_thread.get("thr_test"), Some(&4));

        let event_types: Vec<&str> = lines
            .iter()
            .map(|line| {
                line.get("type")
                    .and_then(Value::as_str)
                    .expect("event type")
            })
            .collect();
        assert_eq!(
            event_types,
            vec![
                "response_start",
                "response_delta",
                "tool_call_start",
                "tool_lifecycle",
                "tool_call_result",
                "tool_lifecycle",
                "response_end",
            ]
        );

        assert_eq!(lines[1]["delta"], "hello");
        assert_eq!(lines[2]["tool_name"], "shell");
        assert_eq!(lines[3]["tool_name"], "shell");
        assert_eq!(lines[3]["phase"], "start");
        assert_eq!(lines[4]["tool_name"], "shell");
        assert_eq!(lines[4]["success"], true);
        assert_eq!(lines[5]["tool_name"], "shell");
        assert_eq!(lines[5]["phase"], "complete");
    }
}
