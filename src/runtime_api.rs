//! Runtime HTTP/SSE API for local DeepSeek automation.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_stream::stream;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use crate::config::{Config, DEFAULT_TEXT_MODEL};
use crate::runtime_threads::{
    CompactThreadRequest, CreateThreadRequest, RuntimeThreadManager, RuntimeThreadManagerConfig,
    SharedRuntimeThreadManager, StartTurnRequest, SteerTurnRequest, ThreadDetail, ThreadRecord,
    TurnRecord,
};
use crate::session_manager::{SessionManager, SessionMetadata, default_sessions_dir};
use crate::task_manager::{
    NewTaskRequest, SharedTaskManager, TaskManager, TaskManagerConfig, TaskRecord, TaskSummary,
};

#[derive(Clone)]
pub struct RuntimeApiState {
    config: Config,
    workspace: PathBuf,
    task_manager: SharedTaskManager,
    runtime_threads: SharedRuntimeThreadManager,
    sessions_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RuntimeApiOptions {
    pub host: String,
    pub port: u16,
    pub workers: usize,
}

#[derive(Debug, Deserialize)]
struct StreamTurnRequest {
    prompt: String,
    model: Option<String>,
    mode: Option<String>,
    workspace: Option<PathBuf>,
    allow_shell: Option<bool>,
    trust_mode: Option<bool>,
    auto_approve: Option<bool>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    mode: &'static str,
}

#[derive(Debug, Serialize)]
struct SessionsResponse {
    sessions: Vec<SessionMetadata>,
}

#[derive(Debug, Serialize)]
struct TasksResponse {
    tasks: Vec<TaskSummary>,
    counts: crate::task_manager::TaskCounts,
}

#[derive(Debug, Deserialize)]
struct SessionsQuery {
    limit: Option<usize>,
    search: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TasksQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ThreadsQuery {
    limit: Option<usize>,
    include_archived: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ThreadEventsQuery {
    since_seq: Option<u64>,
}

#[derive(Debug, Serialize)]
struct StartTurnResponse {
    thread: ThreadRecord,
    turn: TurnRecord,
}

/// Start the runtime API server.
pub async fn run_http_server(
    config: Config,
    workspace: PathBuf,
    options: RuntimeApiOptions,
) -> Result<()> {
    if options.port == 0 {
        bail!("Port must be > 0");
    }

    let task_cfg = TaskManagerConfig::from_runtime(
        &config,
        workspace.clone(),
        config.default_text_model.clone(),
        Some(options.workers),
    );
    let runtime_threads = Arc::new(RuntimeThreadManager::open(
        config.clone(),
        workspace.clone(),
        RuntimeThreadManagerConfig::from_task_data_dir(task_cfg.data_dir.clone()),
    )?);
    let task_manager =
        TaskManager::start_with_runtime_manager(task_cfg, config.clone(), runtime_threads.clone())
            .await?;

    let sessions_dir = default_sessions_dir().unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|h| h.join(".deepseek").join("sessions"))
            .unwrap_or_else(|| PathBuf::from(".deepseek").join("sessions"))
    });
    let state = RuntimeApiState {
        config: config.clone(),
        workspace,
        task_manager,
        runtime_threads,
        sessions_dir,
    };
    let app = build_router(state);

    let addr: SocketAddr = format!("{}:{}", options.host, options.port)
        .parse()
        .with_context(|| format!("Invalid bind address '{}:{}'", options.host, options.port))?;
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind {addr}"))?;

    println!("Runtime API listening on http://{addr}");
    println!("Security: this server is local-first. Do not expose it to untrusted networks.");
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow!("Runtime API server error: {e}"))
}

pub fn build_router(state: RuntimeApiState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/sessions", get(list_sessions))
        .route("/v1/stream", post(stream_turn))
        .route("/v1/threads", get(list_threads).post(create_thread))
        .route("/v1/threads/{id}", get(get_thread))
        .route("/v1/threads/{id}/resume", post(resume_thread))
        .route("/v1/threads/{id}/fork", post(fork_thread))
        .route("/v1/threads/{id}/turns", post(start_thread_turn))
        .route(
            "/v1/threads/{id}/turns/{turn_id}/steer",
            post(steer_thread_turn),
        )
        .route(
            "/v1/threads/{id}/turns/{turn_id}/interrupt",
            post(interrupt_thread_turn),
        )
        .route("/v1/threads/{id}/compact", post(compact_thread))
        .route("/v1/threads/{id}/events", get(stream_thread_events))
        .route("/v1/tasks", get(list_tasks).post(create_task))
        .route("/v1/tasks/{id}", get(get_task))
        .route("/v1/tasks/{id}/cancel", post(cancel_task))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "deepseek-runtime-api",
        mode: "local",
    })
}

async fn list_sessions(
    State(state): State<RuntimeApiState>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<SessionsResponse>, ApiError> {
    let manager = SessionManager::new(state.sessions_dir.clone())
        .map_err(|e| ApiError::internal(format!("Failed to open sessions dir: {e}")))?;
    let mut sessions = if let Some(search) = query.search {
        manager
            .search_sessions(&search)
            .map_err(|e| ApiError::internal(format!("Failed to search sessions: {e}")))?
    } else {
        manager
            .list_sessions()
            .map_err(|e| ApiError::internal(format!("Failed to list sessions: {e}")))?
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    sessions.truncate(limit);
    Ok(Json(SessionsResponse { sessions }))
}

async fn create_task(
    State(state): State<RuntimeApiState>,
    Json(mut req): Json<NewTaskRequest>,
) -> Result<(StatusCode, Json<TaskRecord>), ApiError> {
    if req.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("prompt is required"));
    }
    if req.workspace.is_none() {
        req.workspace = Some(state.workspace.clone());
    }
    if req.model.is_none() {
        req.model = Some(
            state
                .config
                .default_text_model
                .clone()
                .unwrap_or_else(|| DEFAULT_TEXT_MODEL.to_string()),
        );
    }
    let task = state
        .task_manager
        .add_task(req)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(task)))
}

async fn create_thread(
    State(state): State<RuntimeApiState>,
    Json(mut req): Json<CreateThreadRequest>,
) -> Result<(StatusCode, Json<ThreadRecord>), ApiError> {
    if req.model.as_ref().is_none_or(|m| m.trim().is_empty()) {
        req.model = Some(
            state
                .config
                .default_text_model
                .clone()
                .unwrap_or_else(|| DEFAULT_TEXT_MODEL.to_string()),
        );
    }
    if req.workspace.is_none() {
        req.workspace = Some(state.workspace.clone());
    }
    if req.mode.as_ref().is_none_or(|m| m.trim().is_empty()) {
        req.mode = Some("agent".to_string());
    }

    let thread = state
        .runtime_threads
        .create_thread(req)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(thread)))
}

async fn list_threads(
    State(state): State<RuntimeApiState>,
    Query(query): Query<ThreadsQuery>,
) -> Result<Json<Vec<ThreadRecord>>, ApiError> {
    let threads = state
        .runtime_threads
        .list_threads(query.include_archived.unwrap_or(false), query.limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(threads))
}

async fn get_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<ThreadDetail>, ApiError> {
    let detail = state
        .runtime_threads
        .get_thread_detail(&id)
        .await
        .map_err(map_thread_err)?;
    Ok(Json(detail))
}

async fn resume_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<ThreadRecord>, ApiError> {
    let thread = state
        .runtime_threads
        .resume_thread(&id)
        .await
        .map_err(map_thread_err)?;
    Ok(Json(thread))
}

async fn fork_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<ThreadRecord>), ApiError> {
    let thread = state
        .runtime_threads
        .fork_thread(&id)
        .await
        .map_err(map_thread_err)?;
    Ok((StatusCode::CREATED, Json(thread)))
}

async fn start_thread_turn(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<StartTurnRequest>,
) -> Result<(StatusCode, Json<StartTurnResponse>), ApiError> {
    let turn = state
        .runtime_threads
        .start_turn(&id, req)
        .await
        .map_err(map_thread_err)?;
    let thread = state
        .runtime_threads
        .get_thread(&id)
        .await
        .map_err(map_thread_err)?;
    Ok((
        StatusCode::CREATED,
        Json(StartTurnResponse { thread, turn }),
    ))
}

async fn steer_thread_turn(
    State(state): State<RuntimeApiState>,
    Path((id, turn_id)): Path<(String, String)>,
    Json(req): Json<SteerTurnRequest>,
) -> Result<Json<TurnRecord>, ApiError> {
    let turn = state
        .runtime_threads
        .steer_turn(&id, &turn_id, req)
        .await
        .map_err(map_thread_err)?;
    Ok(Json(turn))
}

async fn interrupt_thread_turn(
    State(state): State<RuntimeApiState>,
    Path((id, turn_id)): Path<(String, String)>,
) -> Result<Json<TurnRecord>, ApiError> {
    let turn = state
        .runtime_threads
        .interrupt_turn(&id, &turn_id)
        .await
        .map_err(map_thread_err)?;
    Ok(Json(turn))
}

async fn compact_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<CompactThreadRequest>,
) -> Result<(StatusCode, Json<StartTurnResponse>), ApiError> {
    let turn = state
        .runtime_threads
        .compact_thread(&id, req)
        .await
        .map_err(map_thread_err)?;
    let thread = state
        .runtime_threads
        .get_thread(&id)
        .await
        .map_err(map_thread_err)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(StartTurnResponse { thread, turn }),
    ))
}

async fn list_tasks(
    State(state): State<RuntimeApiState>,
    Query(query): Query<TasksQuery>,
) -> Result<Json<TasksResponse>, ApiError> {
    let tasks = state.task_manager.list_tasks(query.limit).await;
    let counts = state.task_manager.counts().await;
    Ok(Json(TasksResponse { tasks, counts }))
}

async fn get_task(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<TaskRecord>, ApiError> {
    let task = state
        .task_manager
        .get_task(&id)
        .await
        .map_err(map_task_err)?;
    Ok(Json(task))
}

async fn cancel_task(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<TaskRecord>, ApiError> {
    let task = state
        .task_manager
        .cancel_task(&id)
        .await
        .map_err(map_task_err)?;
    Ok(Json(task))
}

async fn stream_thread_events(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Query(query): Query<ThreadEventsQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    let _ = state
        .runtime_threads
        .get_thread(&id)
        .await
        .map_err(map_thread_err)?;

    let backlog = state
        .runtime_threads
        .events_since(&id, query.since_seq)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut last_seq = query.since_seq.unwrap_or(0);
    if let Some(last) = backlog.last() {
        last_seq = last.seq;
    }

    let mut live = state.runtime_threads.subscribe_events();
    let thread_id = id.clone();
    let stream = stream! {
        for event in backlog {
            let event_name = event.event.clone();
            yield Ok(sse_json(&event_name, runtime_event_payload(event)));
        }
        loop {
            let incoming = live.recv().await;
            let Ok(event) = incoming else {
                break;
            };
            if event.thread_id != thread_id {
                continue;
            }
            if event.seq <= last_seq {
                continue;
            }
            last_seq = event.seq;
            let event_name = event.event.clone();
            yield Ok(sse_json(&event_name, runtime_event_payload(event)));
        }
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}

async fn stream_turn(
    State(state): State<RuntimeApiState>,
    Json(req): Json<StreamTurnRequest>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    if req.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("prompt is required"));
    }

    let model = req.model.clone().unwrap_or_else(|| {
        state
            .config
            .default_text_model
            .clone()
            .unwrap_or_else(|| DEFAULT_TEXT_MODEL.to_string())
    });
    let workspace = req
        .workspace
        .clone()
        .unwrap_or_else(|| state.workspace.clone());
    let mode = req.mode.clone().unwrap_or_else(|| "agent".to_string());
    let allow_shell = req.allow_shell.unwrap_or(state.config.allow_shell());
    let trust_mode = req.trust_mode.unwrap_or(false);
    let auto_approve = req.auto_approve.unwrap_or(true);
    let prompt = req.prompt;

    let thread = state
        .runtime_threads
        .create_thread(CreateThreadRequest {
            model: Some(model.clone()),
            workspace: Some(workspace.clone()),
            mode: Some(mode.clone()),
            allow_shell: Some(allow_shell),
            trust_mode: Some(trust_mode),
            auto_approve: Some(auto_approve),
            archived: true,
        })
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create stream thread: {e}")))?;

    let turn = state
        .runtime_threads
        .start_turn(
            &thread.id,
            StartTurnRequest {
                prompt,
                input_summary: None,
                model: Some(model.clone()),
                mode: Some(mode.clone()),
                allow_shell: Some(allow_shell),
                trust_mode: Some(trust_mode),
                auto_approve: Some(auto_approve),
            },
        )
        .await
        .map_err(|e| ApiError::internal(format!("Failed to start stream turn: {e}")))?;

    let backlog = state
        .runtime_threads
        .events_since(&thread.id, None)
        .map_err(|e| ApiError::internal(format!("Failed to load stream backlog: {e}")))?;
    let mut live = state.runtime_threads.subscribe_events();
    let thread_id = thread.id.clone();
    let turn_id = turn.id.clone();

    let stream = stream! {
        yield Ok(sse_json("turn.started", json!({
            "thread_id": thread.id,
            "turn_id": turn.id,
            "model": model,
            "mode": mode,
            "workspace": workspace,
        })));

        for event in backlog {
            if event.thread_id != thread_id || event.turn_id.as_deref() != Some(&turn_id) {
                continue;
            }
            if let Some(mapped) = map_compat_stream_event(&event) {
                yield Ok(mapped);
            }
            if event.event == "turn.completed" {
                yield Ok(sse_json("done", json!({})));
                return;
            }
        }

        loop {
            let incoming = live.recv().await;
            let Ok(event) = incoming else {
                yield Ok(sse_json("error", json!({ "message": "event channel closed" })));
                break;
            };
            if event.thread_id != thread_id || event.turn_id.as_deref() != Some(&turn_id) {
                continue;
            }
            if let Some(mapped) = map_compat_stream_event(&event) {
                yield Ok(mapped);
            }
            if event.event == "turn.completed" {
                break;
            }
        }

        yield Ok(sse_json("done", json!({})));
    };

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    ))
}

fn runtime_event_payload(event: crate::runtime_threads::RuntimeEventRecord) -> serde_json::Value {
    json!({
        "seq": event.seq,
        "timestamp": event.timestamp,
        "thread_id": event.thread_id,
        "turn_id": event.turn_id,
        "item_id": event.item_id,
        "event": event.event,
        "payload": event.payload,
    })
}

fn map_compat_stream_event(event: &crate::runtime_threads::RuntimeEventRecord) -> Option<SseEvent> {
    let payload = &event.payload;
    match event.event.as_str() {
        "item.delta" => {
            let kind = payload
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if kind == "agent_message" {
                let content = payload
                    .get("delta")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                Some(sse_json("message.delta", json!({ "content": content })))
            } else if kind == "tool_call" {
                let output = payload
                    .get("delta")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                Some(sse_json("tool.progress", json!({ "output": output })))
            } else {
                None
            }
        }
        "item.started" => {
            let tool = payload.get("tool")?;
            let id = tool.get("id").cloned().unwrap_or(Value::Null);
            let name = tool.get("name").cloned().unwrap_or(Value::Null);
            let input = tool.get("input").cloned().unwrap_or(Value::Null);
            Some(sse_json(
                "tool.started",
                json!({
                    "id": id,
                    "name": name,
                    "input": input,
                }),
            ))
        }
        "item.completed" | "item.failed" => {
            let item = payload.get("item")?;
            let kind = item
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if kind == "tool_call" || kind == "file_change" || kind == "command_execution" {
                let id = item.get("id").cloned().unwrap_or(Value::Null);
                let success = event.event == "item.completed";
                let output = item.get("detail").cloned().unwrap_or_else(|| {
                    Value::String(
                        item.get("summary")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    )
                });
                Some(sse_json(
                    "tool.completed",
                    json!({
                        "id": id,
                        "success": success,
                        "output": output,
                    }),
                ))
            } else if kind == "status" {
                let message = item
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("summary").and_then(|v| v.as_str()))
                    .unwrap_or_default();
                Some(sse_json("status", json!({ "message": message })))
            } else if kind == "error" {
                let message = item
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .or_else(|| item.get("summary").and_then(|v| v.as_str()))
                    .unwrap_or_default();
                Some(sse_json("error", json!({ "message": message })))
            } else {
                None
            }
        }
        "approval.required" => Some(sse_json("approval.required", payload.clone())),
        "sandbox.denied" => Some(sse_json("sandbox.denied", payload.clone())),
        "turn.completed" => {
            let usage = payload
                .get("turn")
                .and_then(|turn| turn.get("usage"))
                .cloned()
                .unwrap_or_else(|| json!(null));
            Some(sse_json("turn.completed", json!({ "usage": usage })))
        }
        _ => None,
    }
}

fn sse_json(event: &str, payload: serde_json::Value) -> SseEvent {
    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    SseEvent::default().event(event).data(data)
}

fn map_task_err(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.contains("not found") {
        ApiError::not_found(message)
    } else {
        ApiError::bad_request(message)
    }
}

fn map_thread_err(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.contains("not found") {
        ApiError::not_found(message)
    } else if message.contains("already has an active turn")
        || message.contains("No active turn")
        || message.contains("is not active")
    {
        ApiError {
            status: StatusCode::CONFLICT,
            message,
        }
    } else {
        ApiError::bad_request(message)
    }
}

#[derive(Debug, Clone)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "message": self.message,
                    "status": self.status.as_u16(),
                }
            })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::events::{Event as EngineEvent, TurnOutcomeStatus};
    use crate::core::ops::Op;
    use crate::models::Usage;
    use crate::runtime_threads::RuntimeEventRecord;
    use anyhow::{Context, bail};
    use futures_util::StreamExt;
    use std::fs;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio::time::sleep;
    use uuid::Uuid;

    struct MockExecutor;

    #[async_trait::async_trait]
    impl crate::task_manager::TaskExecutor for MockExecutor {
        async fn execute(
            &self,
            _task: crate::task_manager::ExecutionTask,
            events: mpsc::UnboundedSender<crate::task_manager::TaskExecutionEvent>,
            cancel: tokio_util::sync::CancellationToken,
        ) -> crate::task_manager::TaskExecutionResult {
            let _ = events.send(crate::task_manager::TaskExecutionEvent::Status {
                message: "started".to_string(),
            });
            sleep(Duration::from_millis(100)).await;
            if cancel.is_cancelled() {
                return crate::task_manager::TaskExecutionResult {
                    status: crate::task_manager::TaskStatus::Canceled,
                    result_text: None,
                    error: None,
                };
            }
            crate::task_manager::TaskExecutionResult {
                status: crate::task_manager::TaskStatus::Completed,
                result_text: Some("ok".to_string()),
                error: None,
            }
        }
    }

    async fn spawn_test_server() -> Result<
        Option<(
            SocketAddr,
            SharedRuntimeThreadManager,
            tokio::task::JoinHandle<()>,
        )>,
    > {
        let root = std::env::temp_dir().join(format!("deepseek-runtime-api-{}", Uuid::new_v4()));
        let sessions_dir = root.join("sessions");
        fs::create_dir_all(&sessions_dir)?;
        let manager = TaskManager::start_with_executor(
            TaskManagerConfig {
                data_dir: root.join("tasks"),
                worker_count: 1,
                default_workspace: PathBuf::from("."),
                default_model: DEFAULT_TEXT_MODEL.to_string(),
                default_mode: "agent".to_string(),
                allow_shell: false,
                trust_mode: false,
                max_subagents: 2,
            },
            Arc::new(MockExecutor),
        )
        .await?;
        let mut config = Config::default();
        config.capacity = Some(crate::config::CapacityConfig {
            enabled: Some(false),
            low_risk_max: None,
            medium_risk_max: None,
            severe_min_slack: None,
            severe_violation_ratio: None,
            refresh_cooldown_turns: None,
            replan_cooldown_turns: None,
            max_replay_per_turn: None,
            min_turns_before_guardrail: None,
            profile_window: None,
            deepseek_v3_2_chat_prior: None,
            deepseek_v3_2_reasoner_prior: None,
            fallback_default_prior: None,
        });
        let runtime_threads: SharedRuntimeThreadManager = Arc::new(RuntimeThreadManager::open(
            config,
            PathBuf::from("."),
            RuntimeThreadManagerConfig::from_task_data_dir(root.join("runtime")),
        )?);

        let state = RuntimeApiState {
            config: Config::default(),
            workspace: PathBuf::from("."),
            task_manager: manager,
            runtime_threads: runtime_threads.clone(),
            sessions_dir,
        };
        let app = build_router(state);
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let addr = listener.local_addr()?;
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Some((addr, runtime_threads, handle)))
    }

    async fn read_first_sse_frame(resp: reqwest::Response) -> Result<String> {
        let mut stream = resp.bytes_stream();
        let mut buf = Vec::new();
        loop {
            let next = tokio::time::timeout(Duration::from_secs(2), stream.next())
                .await
                .context("timed out waiting for SSE frame")?
                .context("SSE stream ended unexpectedly")??;
            buf.extend_from_slice(&next);

            let text = String::from_utf8_lossy(&buf);
            if let Some(idx) = text.find("\n\n").or_else(|| text.find("\r\n\r\n")) {
                return Ok(text[..idx].to_string());
            }

            if buf.len() > 64 * 1024 {
                bail!("SSE frame exceeded 64KB without delimiter");
            }
        }
    }

    fn parse_sse_frame(frame: &str) -> Result<(String, serde_json::Value)> {
        let mut event_name: Option<String> = None;
        let mut data_lines = Vec::new();
        for line in frame.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                event_name = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.trim_start().to_string());
            }
        }
        let event_name = event_name.context("missing SSE event field")?;
        let payload = if data_lines.is_empty() {
            json!({})
        } else {
            serde_json::from_str(&data_lines.join("\n"))
                .with_context(|| format!("invalid SSE data payload: {}", data_lines.join("\n")))?
        };
        Ok((event_name, payload))
    }

    async fn wait_for_terminal_turn_status(
        client: &reqwest::Client,
        addr: SocketAddr,
        thread_id: &str,
        turn_id: &str,
        timeout: Duration,
    ) -> Result<String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let detail: serde_json::Value = client
                .get(format!("http://{addr}/v1/threads/{thread_id}"))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let status = detail["turns"]
                .as_array()
                .and_then(|turns| turns.iter().find(|turn| turn["id"] == turn_id))
                .and_then(|turn| turn.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if matches!(
                status.as_str(),
                "completed" | "failed" | "interrupted" | "canceled"
            ) {
                return Ok(status);
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("timed out waiting for terminal turn status for {turn_id}");
            }
            sleep(Duration::from_millis(25)).await;
        }
    }

    #[tokio::test]
    async fn health_and_tasks_endpoints_work() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let health: serde_json::Value = client
            .get(format!("http://{addr}/health"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(health["status"], "ok");

        let created: serde_json::Value = client
            .post(format!("http://{addr}/v1/tasks"))
            .json(&json!({ "prompt": "hello task" }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let id = created["id"].as_str().expect("task id").to_string();

        let listed: serde_json::Value = client
            .get(format!("http://{addr}/v1/tasks"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(
            listed["tasks"]
                .as_array()
                .is_some_and(|tasks| !tasks.is_empty())
        );

        let detail: serde_json::Value = client
            .get(format!("http://{addr}/v1/tasks/{id}"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(detail["id"], id);

        let _cancelled: serde_json::Value = client
            .post(format!("http://{addr}/v1/tasks/{id}/cancel"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn stream_requires_prompt() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("http://{addr}/v1/stream"))
            .json(&json!({ "prompt": "" }))
            .send()
            .await?;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn thread_endpoints_expose_lifecycle_contract() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let created: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads"))
            .json(&json!({}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let thread_id = created["id"]
            .as_str()
            .context("missing thread id")?
            .to_string();

        let listed: serde_json::Value = client
            .get(format!("http://{addr}/v1/threads"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(
            listed
                .as_array()
                .is_some_and(|threads| threads.iter().any(|t| t["id"] == thread_id))
        );

        let detail: serde_json::Value = client
            .get(format!("http://{addr}/v1/threads/{thread_id}"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(detail["thread"]["id"], thread_id);

        let resumed: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads/{thread_id}/resume"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(resumed["id"], thread_id);

        let forked: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads/{thread_id}/fork"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let forked_id = forked["id"].as_str().context("missing forked id")?;
        assert_ne!(forked_id, thread_id);

        let turn_start: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads/{thread_id}/turns"))
            .json(&json!({ "prompt": "thread endpoint test" }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let turn_id = turn_start["turn"]["id"]
            .as_str()
            .context("missing turn id")?
            .to_string();

        let _ = wait_for_terminal_turn_status(
            &client,
            addr,
            &thread_id,
            &turn_id,
            Duration::from_secs(2),
        )
        .await?;

        let steer_resp = client
            .post(format!(
                "http://{addr}/v1/threads/{thread_id}/turns/{turn_id}/steer"
            ))
            .json(&json!({ "prompt": "late steer" }))
            .send()
            .await?;
        assert_eq!(steer_resp.status(), StatusCode::CONFLICT);

        let interrupt_resp = client
            .post(format!(
                "http://{addr}/v1/threads/{thread_id}/turns/{turn_id}/interrupt"
            ))
            .send()
            .await?;
        assert_eq!(interrupt_resp.status(), StatusCode::CONFLICT);

        let compact_start: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads/{thread_id}/compact"))
            .json(&json!({ "reason": "test manual compact" }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(compact_start["thread"]["id"], thread_id);

        let events_resp = client
            .get(format!(
                "http://{addr}/v1/threads/{thread_id}/events?since_seq=0"
            ))
            .send()
            .await?
            .error_for_status()?;
        let content_type = events_resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(content_type.starts_with("text/event-stream"));
        let chunk_text = read_first_sse_frame(events_resp).await?;
        assert!(
            chunk_text.contains("event:"),
            "expected SSE event chunk, got: {chunk_text}"
        );

        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn events_endpoint_respects_since_seq_cursor() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let created: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads"))
            .json(&json!({}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let thread_id = created["id"]
            .as_str()
            .context("missing thread id")?
            .to_string();

        let started: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads/{thread_id}/turns"))
            .json(&json!({ "prompt": "cursor replay test" }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let turn_id = started["turn"]["id"]
            .as_str()
            .context("missing turn id")?
            .to_string();

        let _ = wait_for_terminal_turn_status(
            &client,
            addr,
            &thread_id,
            &turn_id,
            Duration::from_secs(2),
        )
        .await?;

        let resp_a = client
            .get(format!(
                "http://{addr}/v1/threads/{thread_id}/events?since_seq=0"
            ))
            .send()
            .await?
            .error_for_status()?;
        let frame_a = read_first_sse_frame(resp_a).await?;
        let (_event_a, payload_a) = parse_sse_frame(&frame_a)?;
        let seq_a = payload_a
            .get("seq")
            .and_then(Value::as_u64)
            .context("missing seq in first replay frame")?;

        let resp_b = client
            .get(format!(
                "http://{addr}/v1/threads/{thread_id}/events?since_seq={seq_a}"
            ))
            .send()
            .await?
            .error_for_status()?;
        let frame_b = read_first_sse_frame(resp_b).await?;
        let (_event_b, payload_b) = parse_sse_frame(&frame_b)?;
        let seq_b = payload_b
            .get("seq")
            .and_then(Value::as_u64)
            .context("missing seq in second replay frame")?;
        assert!(
            seq_b > seq_a,
            "expected seq after cursor: {seq_b} <= {seq_a}"
        );
        assert_eq!(payload_b["thread_id"], thread_id);

        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn steer_and_interrupt_endpoints_work_on_active_turn() -> Result<()> {
        let Some((addr, runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let created: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads"))
            .json(&json!({}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let thread_id = created["id"]
            .as_str()
            .context("missing thread id")?
            .to_string();

        let harness = crate::core::engine::mock_engine_handle();
        runtime_threads
            .install_test_engine(&thread_id, harness.handle.clone())
            .await?;
        let mut rx_op = harness.rx_op;
        let mut rx_steer = harness.rx_steer;
        let tx_event = harness.tx_event;
        let cancel_token = harness.cancel_token;
        tokio::spawn(async move {
            if !matches!(rx_op.recv().await, Some(Op::SendMessage { .. })) {
                return;
            }
            let _ = tx_event
                .send(EngineEvent::TurnStarted {
                    turn_id: "engine_turn_api".to_string(),
                })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageStarted { index: 0 })
                .await;
            if let Some(steer_text) = rx_steer.recv().await {
                let _ = tx_event
                    .send(EngineEvent::MessageDelta {
                        index: 0,
                        content: format!("steer:{steer_text}"),
                    })
                    .await;
            }
            cancel_token.cancelled().await;
            sleep(Duration::from_millis(60)).await;
            let _ = tx_event
                .send(EngineEvent::TurnComplete {
                    usage: Usage {
                        input_tokens: 2,
                        output_tokens: 1,
                    },
                    status: TurnOutcomeStatus::Completed,
                    error: None,
                })
                .await;
        });

        let turn_start: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads/{thread_id}/turns"))
            .json(&json!({ "prompt": "active controls" }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let turn_id = turn_start["turn"]["id"]
            .as_str()
            .context("missing turn id")?
            .to_string();

        let steer_resp: serde_json::Value = client
            .post(format!(
                "http://{addr}/v1/threads/{thread_id}/turns/{turn_id}/steer"
            ))
            .json(&json!({ "prompt": "please steer" }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(steer_resp["id"], turn_id);
        assert_eq!(steer_resp["steer_count"], 1);

        let interrupt_resp: serde_json::Value = client
            .post(format!(
                "http://{addr}/v1/threads/{thread_id}/turns/{turn_id}/interrupt"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(interrupt_resp["id"], turn_id);

        let terminal = wait_for_terminal_turn_status(
            &client,
            addr,
            &thread_id,
            &turn_id,
            Duration::from_secs(3),
        )
        .await?;
        assert_eq!(terminal, "interrupted");

        let events = runtime_threads.events_since(&thread_id, None)?;
        assert!(events.iter().any(|ev| ev.event == "turn.steered"));
        assert!(
            events
                .iter()
                .any(|ev| ev.event == "turn.interrupt_requested")
        );
        assert!(events.iter().any(|ev| {
            ev.event == "turn.completed"
                && ev
                    .payload
                    .get("turn")
                    .and_then(|turn| turn.get("status"))
                    .and_then(Value::as_str)
                    == Some("interrupted")
        }));

        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn stream_compat_mapping_handles_expected_runtime_events() -> Result<()> {
        let agent_delta = RuntimeEventRecord {
            schema_version: 1,
            seq: 1,
            timestamp: chrono::Utc::now(),
            thread_id: "thr_test".to_string(),
            turn_id: Some("turn_test".to_string()),
            item_id: Some("item_test".to_string()),
            event: "item.delta".to_string(),
            payload: json!({
                "kind": "agent_message",
                "delta": "hello",
            }),
        };
        let mapped = map_compat_stream_event(&agent_delta).context("missing mapped SSE event")?;
        let stream = async_stream::stream! {
            yield Ok::<_, Infallible>(mapped);
        };
        let body =
            axum::body::to_bytes(Sse::new(stream).into_response().into_body(), usize::MAX).await?;
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("event: message.delta"));
        assert!(text.contains("\"content\":\"hello\""));

        let tool_start = RuntimeEventRecord {
            schema_version: 1,
            seq: 2,
            timestamp: chrono::Utc::now(),
            thread_id: "thr_test".to_string(),
            turn_id: Some("turn_test".to_string()),
            item_id: Some("item_tool".to_string()),
            event: "item.started".to_string(),
            payload: json!({
                "tool": { "id": "tool_1", "name": "exec_shell", "input": { "cmd": "pwd" } }
            }),
        };
        let mapped = map_compat_stream_event(&tool_start).context("missing tool.started event")?;
        let stream = async_stream::stream! {
            yield Ok::<_, Infallible>(mapped);
        };
        let body =
            axum::body::to_bytes(Sse::new(stream).into_response().into_body(), usize::MAX).await?;
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("event: tool.started"));

        let tool_done = RuntimeEventRecord {
            schema_version: 1,
            seq: 3,
            timestamp: chrono::Utc::now(),
            thread_id: "thr_test".to_string(),
            turn_id: Some("turn_test".to_string()),
            item_id: Some("item_tool".to_string()),
            event: "item.completed".to_string(),
            payload: json!({
                "item": {
                    "id": "item_tool",
                    "kind": "tool_call",
                    "summary": "ok",
                    "detail": "done"
                }
            }),
        };
        let mapped = map_compat_stream_event(&tool_done).context("missing tool.completed event")?;
        let stream = async_stream::stream! {
            yield Ok::<_, Infallible>(mapped);
        };
        let body =
            axum::body::to_bytes(Sse::new(stream).into_response().into_body(), usize::MAX).await?;
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("event: tool.completed"));
        assert!(text.contains("\"success\":true"));

        let unknown = RuntimeEventRecord {
            schema_version: 1,
            seq: 4,
            timestamp: chrono::Utc::now(),
            thread_id: "thr_test".to_string(),
            turn_id: Some("turn_test".to_string()),
            item_id: None,
            event: "item.delta".to_string(),
            payload: json!({
                "kind": "context_compaction",
                "delta": "ignored",
            }),
        };
        assert!(map_compat_stream_event(&unknown).is_none());
        Ok(())
    }

    #[tokio::test]
    async fn stream_endpoint_remains_backward_compatible() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("http://{addr}/v1/stream"))
            .json(&json!({
                "prompt": "compatibility stream",
                "mode": "agent"
            }))
            .send()
            .await?
            .error_for_status()?;
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(content_type.starts_with("text/event-stream"));

        let body = tokio::time::timeout(Duration::from_secs(3), resp.text())
            .await
            .context("timed out reading /v1/stream response body")??;
        assert!(body.contains("event: turn.started"));
        assert!(body.contains("event: turn.completed"));
        assert!(body.contains("event: done"));

        handle.abort();
        Ok(())
    }
}
