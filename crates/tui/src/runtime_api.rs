//! Runtime HTTP/SSE API for local DeepSeek automation.

use std::collections::HashSet;
use std::convert::Infallible;
use std::fs;
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_stream::stream;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Html;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::automation_manager::{
    AutomationManager, AutomationRecord, AutomationRunRecord, AutomationSchedulerConfig,
    CreateAutomationRequest, SharedAutomationManager, UpdateAutomationRequest, spawn_scheduler,
};
use crate::config::{Config, DEFAULT_TEXT_MODEL};
use crate::mcp::{McpConfig, McpPool};
use crate::runtime_threads::{
    CompactThreadRequest, CreateThreadRequest, RuntimeThreadManager, RuntimeThreadManagerConfig,
    SharedRuntimeThreadManager, StartTurnRequest, SteerTurnRequest, ThreadDetail, ThreadListFilter,
    ThreadRecord, TurnItemKind, TurnRecord, UpdateThreadRequest, UsageGroupBy,
};
use crate::session_manager::{SavedSession, SessionManager, SessionMetadata, default_sessions_dir};
use crate::skills::SkillRegistry;
use crate::task_manager::{
    NewTaskRequest, SharedTaskManager, TaskManager, TaskManagerConfig, TaskRecord, TaskSummary,
};

#[derive(Clone)]
pub struct RuntimeApiState {
    config: Config,
    workspace: PathBuf,
    task_manager: SharedTaskManager,
    runtime_threads: SharedRuntimeThreadManager,
    cors_origins: Vec<String>,
    sessions_dir: PathBuf,
    mcp_config_path: PathBuf,
    automations: SharedAutomationManager,
    runtime_token: Option<String>,
    mobile_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct RuntimeApiOptions {
    pub host: String,
    pub port: u16,
    pub workers: usize,
    /// Additional CORS origins to allow on top of the built-in defaults
    /// (`http://localhost:{3000,1420}`, `http://127.0.0.1:{3000,1420}`,
    /// `tauri://localhost`). Populated by `--cors-origin` (repeatable),
    /// `DEEPSEEK_CORS_ORIGINS` (comma-separated), and `[runtime_api]
    /// cors_origins` in `config.toml`. Whalescale#255 / #561.
    pub cors_origins: Vec<String>,
    /// Enables the built-in mobile control page at `/mobile`.
    pub mobile: bool,
    /// Optional bearer token required by API routes. If `mobile` is enabled and
    /// this is absent, a one-time process-local token is generated.
    pub auth_token: Option<String>,
}

impl Default for RuntimeApiOptions {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 7878,
            workers: 2,
            cors_origins: Vec::new(),
            mobile: false,
            auth_token: None,
        }
    }
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
    manual_approval: Option<bool>,
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
struct SessionDetailResponse {
    metadata: SessionMetadata,
    messages: Vec<serde_json::Value>,
    system_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResumeSessionRequest {
    model: Option<String>,
    mode: Option<String>,
}

#[derive(Debug, Serialize)]
struct ResumeSessionResponse {
    thread_id: String,
    session_id: String,
    message_count: usize,
    summary: String,
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
    /// When `true`, returns archived threads only (overrides `include_archived`).
    /// Whalescale#260 / #563.
    archived_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ThreadSummaryQuery {
    limit: Option<usize>,
    search: Option<String>,
    include_archived: Option<bool>,
    /// When `true`, returns archived threads only (overrides `include_archived`).
    /// Whalescale#260 / #563.
    archived_only: Option<bool>,
}

fn resolve_thread_filter(
    include_archived: Option<bool>,
    archived_only: Option<bool>,
) -> ThreadListFilter {
    if archived_only.unwrap_or(false) {
        ThreadListFilter::ArchivedOnly
    } else if include_archived.unwrap_or(false) {
        ThreadListFilter::IncludeArchived
    } else {
        ThreadListFilter::ActiveOnly
    }
}

#[derive(Debug, Serialize)]
struct ThreadSummary {
    id: String,
    title: String,
    preview: String,
    model: String,
    mode: String,
    archived: bool,
    updated_at: chrono::DateTime<Utc>,
    latest_turn_id: Option<String>,
    latest_turn_status: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkspaceStatusResponse {
    workspace: PathBuf,
    git_repo: bool,
    branch: Option<String>,
    staged: usize,
    unstaged: usize,
    untracked: usize,
    ahead: Option<u32>,
    behind: Option<u32>,
}

#[derive(Debug, Serialize)]
struct SkillEntry {
    name: String,
    description: String,
    path: PathBuf,
}

#[derive(Debug, Serialize)]
struct SkillsResponse {
    directory: PathBuf,
    warnings: Vec<String>,
    skills: Vec<SkillEntry>,
}

#[derive(Debug, Serialize)]
struct McpServerEntry {
    name: String,
    enabled: bool,
    required: bool,
    command: Option<String>,
    url: Option<String>,
    connected: bool,
    enabled_tools: Vec<String>,
    disabled_tools: Vec<String>,
}

#[derive(Debug, Serialize)]
struct McpServersResponse {
    servers: Vec<McpServerEntry>,
}

#[derive(Debug, Deserialize)]
struct McpToolsQuery {
    server: Option<String>,
}

#[derive(Debug, Serialize)]
struct McpToolEntry {
    server: String,
    name: String,
    prefixed_name: String,
    description: Option<String>,
    input_schema: Value,
}

#[derive(Debug, Serialize)]
struct McpToolsResponse {
    tools: Vec<McpToolEntry>,
}

#[derive(Debug, Deserialize)]
struct AutomationRunsQuery {
    limit: Option<usize>,
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
    let automations = Arc::new(Mutex::new(AutomationManager::default_location()?));
    runtime_threads.attach_automation_manager(automations.clone());
    let scheduler_cancel = CancellationToken::new();
    let scheduler_handle = spawn_scheduler(
        automations.clone(),
        task_manager.clone(),
        scheduler_cancel.clone(),
        AutomationSchedulerConfig::default(),
    );

    let sessions_dir = default_sessions_dir().unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|h| h.join(".deepseek").join("sessions"))
            .unwrap_or_else(|| PathBuf::from(".deepseek").join("sessions"))
    });

    let mut runtime_token = options
        .auth_token
        .clone()
        .or_else(|| std::env::var("DEEPSEEK_RUNTIME_TOKEN").ok())
        .filter(|token| !token.trim().is_empty());
    if options.mobile && runtime_token.is_none() {
        runtime_token = Some(format!(
            "dsrt_{}",
            Uuid::new_v4().to_string().replace('-', "")
        ));
    }

    let state = RuntimeApiState {
        config: config.clone(),
        workspace,
        task_manager,
        runtime_threads,
        cors_origins: options.cors_origins.clone(),
        sessions_dir,
        mcp_config_path: config.mcp_config_path(),
        automations,
        runtime_token,
        mobile_enabled: options.mobile,
    };
    let mobile_token_for_log = if options.mobile {
        state.runtime_token.clone()
    } else {
        None
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
    if options.auth_token.is_some() || std::env::var("DEEPSEEK_RUNTIME_TOKEN").is_ok() {
        println!("Auth: API routes require a bearer token.");
    }
    if options.mobile {
        print_mobile_urls(addr, mobile_token_for_log.as_deref());
    }
    let serve_result = axum::serve(listener, app)
        .await
        .map_err(|e| anyhow!("Runtime API server error: {e}"));
    scheduler_cancel.cancel();
    scheduler_handle.abort();
    serve_result
}

pub fn build_router(state: RuntimeApiState) -> Router {
    let api_routes = Router::new()
        .route("/v1/sessions", get(list_sessions))
        .route("/v1/sessions/{id}", get(get_session).delete(delete_session))
        .route(
            "/v1/sessions/{id}/resume-thread",
            post(resume_session_thread),
        )
        .route("/v1/workspace/status", get(workspace_status))
        .route("/v1/stream", post(stream_turn))
        .route("/v1/threads", get(list_threads).post(create_thread))
        .route("/v1/threads/summary", get(list_threads_summary))
        .route("/v1/threads/{id}", get(get_thread).patch(update_thread))
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
        .route(
            "/v1/threads/{id}/turns/{turn_id}/approvals",
            get(list_thread_approvals),
        )
        .route(
            "/v1/threads/{id}/turns/{turn_id}/approvals/{approval_id}/approve",
            post(approve_thread_approval),
        )
        .route(
            "/v1/threads/{id}/turns/{turn_id}/approvals/{approval_id}/deny",
            post(deny_thread_approval),
        )
        .route("/v1/threads/{id}/compact", post(compact_thread))
        .route("/v1/threads/{id}/events", get(stream_thread_events))
        .route("/v1/tasks", get(list_tasks).post(create_task))
        .route("/v1/tasks/{id}", get(get_task))
        .route("/v1/tasks/{id}/cancel", post(cancel_task))
        .route("/v1/skills", get(list_skills))
        .route("/v1/apps/mcp/servers", get(list_mcp_servers))
        .route("/v1/apps/mcp/tools", get(list_mcp_tools))
        .route(
            "/v1/automations",
            get(list_automations).post(create_automation),
        )
        .route(
            "/v1/automations/{id}",
            get(get_automation)
                .patch(update_automation)
                .delete(delete_automation),
        )
        .route("/v1/automations/{id}/run", post(run_automation))
        .route("/v1/automations/{id}/pause", post(pause_automation))
        .route("/v1/automations/{id}/resume", post(resume_automation))
        .route("/v1/automations/{id}/runs", get(list_automation_runs))
        .route("/v1/usage", get(get_usage))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_runtime_token,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/mobile", get(mobile_page))
        .route("/mobile/", get(mobile_page))
        .merge(api_routes)
        .layer(cors_layer(&state.cors_origins))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "deepseek-runtime-api",
        mode: "local",
    })
}

async fn mobile_page(State(state): State<RuntimeApiState>) -> Response {
    if !state.mobile_enabled {
        return (
            StatusCode::NOT_FOUND,
            "mobile control is disabled; start with `deepseek serve --mobile`",
        )
            .into_response();
    }
    Html(MOBILE_HTML).into_response()
}

async fn require_runtime_token(
    State(state): State<RuntimeApiState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected) = state.runtime_token.as_deref() else {
        return next.run(request).await;
    };

    let header_token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .map(str::trim)
        .or_else(|| {
            request
                .headers()
                .get("x-deepseek-runtime-token")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
        });
    let query_token = token_from_query(request.uri().query());

    let authorized = header_token.is_some_and(|token| token == expected)
        || query_token
            .as_deref()
            .is_some_and(|token| token == expected);

    if authorized {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": {
                    "message": "runtime API token required",
                    "status": StatusCode::UNAUTHORIZED.as_u16(),
                }
            })),
        )
            .into_response()
    }
}

fn token_from_query(query: Option<&str>) -> Option<String> {
    let query = query?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? != "token" {
            continue;
        }
        let value = parts.next().unwrap_or_default().trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn print_mobile_urls(addr: SocketAddr, token: Option<&str>) {
    let token_query = token
        .filter(|t| !t.trim().is_empty())
        .map(|t| format!("?token={t}"))
        .unwrap_or_default();
    println!("Mobile control page enabled.");
    println!("Mobile auth: bearer token required for API routes.");

    let port = addr.port();
    if addr.ip().is_unspecified() {
        println!("  Local:   http://127.0.0.1:{port}/mobile{token_query}");
        if let Some(ip) = detect_lan_ip() {
            println!("  LAN:     http://{ip}:{port}/mobile{token_query}");
        } else {
            println!(
                "  LAN:     bind is 0.0.0.0; open http://<this-machine-ip>:{port}/mobile{token_query}"
            );
        }
    } else {
        println!("  URL:     http://{addr}/mobile{token_query}");
    }
    println!("Do not expose this port directly to the public internet.");
}

fn detect_lan_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
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

async fn get_session(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetailResponse>, ApiError> {
    let manager = SessionManager::new(state.sessions_dir.clone())
        .map_err(|e| ApiError::internal(format!("Failed to open sessions dir: {e}")))?;
    let session = manager
        .load_session(&id)
        .map_err(|e| map_session_err(&id, e, "read"))?;
    Ok(Json(session_to_detail(session)))
}

async fn resume_session_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<ResumeSessionRequest>,
) -> Result<(StatusCode, Json<ResumeSessionResponse>), ApiError> {
    let manager = SessionManager::new(state.sessions_dir.clone())
        .map_err(|e| ApiError::internal(format!("Failed to open sessions dir: {e}")))?;
    let session = manager
        .load_session(&id)
        .map_err(|e| map_session_err(&id, e, "read"))?;

    let model = req.model.unwrap_or_else(|| session.metadata.model.clone());
    let mode = req.mode.unwrap_or_else(|| {
        session
            .metadata
            .mode
            .clone()
            .unwrap_or_else(|| "agent".to_string())
    });

    let thread = state
        .runtime_threads
        .create_thread(CreateThreadRequest {
            model: Some(model),
            workspace: Some(state.workspace.clone()),
            mode: Some(mode),
            allow_shell: None,
            trust_mode: None,
            auto_approve: None,
            archived: false,
            system_prompt: session.system_prompt.clone(),
            task_id: None,
        })
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create thread: {e}")))?;

    let msg_count = session.messages.len();
    state
        .runtime_threads
        .seed_thread_from_messages(&thread.id, &session.messages)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to seed thread history: {e}")))?;

    let summary = format!(
        "Resumed session '{}' ({} messages) into thread {}",
        session.metadata.title, msg_count, thread.id
    );

    Ok((
        StatusCode::CREATED,
        Json(ResumeSessionResponse {
            thread_id: thread.id,
            session_id: id,
            message_count: msg_count,
            summary,
        }),
    ))
}

async fn delete_session(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let manager = SessionManager::new(state.sessions_dir.clone())
        .map_err(|e| ApiError::internal(format!("Failed to open sessions dir: {e}")))?;
    manager
        .delete_session(&id)
        .map_err(|e| map_session_err(&id, e, "delete"))?;
    Ok(StatusCode::NO_CONTENT)
}

fn session_to_detail(session: SavedSession) -> SessionDetailResponse {
    let messages: Vec<serde_json::Value> = session
        .messages
        .iter()
        .map(|msg| {
            let content_blocks: Vec<serde_json::Value> = msg
                .content
                .iter()
                .map(|block| match block {
                    crate::models::ContentBlock::Text { text, .. } => {
                        json!({ "type": "text", "text": text })
                    }
                    crate::models::ContentBlock::Thinking { thinking, .. } => {
                        json!({ "type": "thinking", "text": thinking })
                    }
                    _ => json!({ "type": "other" }),
                })
                .collect();
            json!({
                "role": msg.role,
                "content": content_blocks,
            })
        })
        .collect();
    SessionDetailResponse {
        metadata: session.metadata,
        messages,
        system_prompt: session.system_prompt,
    }
}

fn map_session_err(id: &str, err: std::io::Error, action: &str) -> ApiError {
    match err.kind() {
        std::io::ErrorKind::NotFound => ApiError::not_found(format!("Session '{id}' not found")),
        std::io::ErrorKind::InvalidData => {
            ApiError::bad_request(format!("Failed to parse session '{id}': {err}"))
        }
        std::io::ErrorKind::InvalidInput => {
            ApiError::bad_request(format!("Invalid session id '{id}'"))
        }
        _ => ApiError::internal(format!("Failed to {action} session '{id}': {err}")),
    }
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
    let filter = resolve_thread_filter(query.include_archived, query.archived_only);
    let threads = state
        .runtime_threads
        .list_threads(filter, query.limit)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(threads))
}

async fn list_threads_summary(
    State(state): State<RuntimeApiState>,
    Query(query): Query<ThreadSummaryQuery>,
) -> Result<Json<Vec<ThreadSummary>>, ApiError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let search = query.search.as_deref().map(str::to_ascii_lowercase);
    let filter = resolve_thread_filter(query.include_archived, query.archived_only);
    let threads = state
        .runtime_threads
        .list_threads(filter, Some(limit))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut summaries = Vec::new();
    for thread in threads {
        let detail = state
            .runtime_threads
            .get_thread_detail(&thread.id)
            .await
            .map_err(map_thread_err)?;
        let latest_turn = detail.turns.last();
        let latest_status =
            latest_turn.map(|turn| format!("{:?}", turn.status).to_ascii_lowercase());

        let title = thread
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| truncate_text(t, 72))
            .unwrap_or_else(|| {
                latest_turn
                    .map(|turn| {
                        if turn.input_summary.trim().is_empty() {
                            "New Thread".to_string()
                        } else {
                            truncate_text(&turn.input_summary, 72)
                        }
                    })
                    .unwrap_or_else(|| "New Thread".to_string())
            });

        let preview = detail
            .items
            .iter()
            .rev()
            .find_map(|item| match item.kind {
                TurnItemKind::AgentMessage | TurnItemKind::UserMessage => {
                    let text = item.detail.clone().unwrap_or_else(|| item.summary.clone());
                    if text.trim().is_empty() {
                        None
                    } else {
                        Some(truncate_text(&text, 140))
                    }
                }
                _ => None,
            })
            .unwrap_or_else(|| title.clone());

        if let Some(search) = &search {
            let haystack = format!(
                "{} {} {} {}",
                thread.id.to_ascii_lowercase(),
                title.to_ascii_lowercase(),
                preview.to_ascii_lowercase(),
                thread.model.to_ascii_lowercase()
            );
            if !haystack.contains(search) {
                continue;
            }
        }

        summaries.push(ThreadSummary {
            id: thread.id,
            title,
            preview,
            model: thread.model,
            mode: thread.mode,
            archived: thread.archived,
            updated_at: thread.updated_at,
            latest_turn_id: thread.latest_turn_id,
            latest_turn_status: latest_status,
        });
    }

    if summaries.len() > limit {
        summaries.truncate(limit);
    }

    Ok(Json(summaries))
}

async fn workspace_status(
    State(state): State<RuntimeApiState>,
) -> Result<Json<WorkspaceStatusResponse>, ApiError> {
    Ok(Json(collect_workspace_status(&state.workspace)))
}

async fn list_skills(
    State(state): State<RuntimeApiState>,
) -> Result<Json<SkillsResponse>, ApiError> {
    let skills_dir = resolve_skills_dir(&state.config, &state.workspace);
    let registry = SkillRegistry::discover(&skills_dir);
    let skills = registry
        .list()
        .iter()
        .map(|skill| SkillEntry {
            name: skill.name.clone(),
            description: skill.description.clone(),
            path: skills_dir.join(&skill.name).join("SKILL.md"),
        })
        .collect();
    Ok(Json(SkillsResponse {
        directory: skills_dir,
        warnings: registry.warnings().to_vec(),
        skills,
    }))
}

async fn list_mcp_servers(
    State(state): State<RuntimeApiState>,
) -> Result<Json<McpServersResponse>, ApiError> {
    let config = load_mcp_config_or_default(&state.mcp_config_path)?;
    let mut pool = McpPool::new(config.clone());
    let _errors = pool.connect_all().await;
    let connected: HashSet<String> = pool
        .connected_servers()
        .into_iter()
        .map(str::to_string)
        .collect();

    let mut servers = Vec::new();
    for (name, server_cfg) in config.servers {
        servers.push(McpServerEntry {
            name: name.clone(),
            enabled: server_cfg.is_enabled(),
            required: server_cfg.required,
            command: server_cfg.command.clone(),
            url: server_cfg.url.clone(),
            connected: connected.contains(&name),
            enabled_tools: server_cfg.enabled_tools.clone(),
            disabled_tools: server_cfg.disabled_tools.clone(),
        });
    }
    servers.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(McpServersResponse { servers }))
}

async fn list_mcp_tools(
    State(state): State<RuntimeApiState>,
    Query(query): Query<McpToolsQuery>,
) -> Result<Json<McpToolsResponse>, ApiError> {
    let mut pool = McpPool::from_config_path(&state.mcp_config_path)
        .map_err(|e| ApiError::internal(format!("Failed to load MCP config: {e}")))?;
    let _errors = pool.connect_all().await;

    let mut tools = Vec::new();
    for (prefixed_name, tool) in pool.all_tools() {
        let Some(rest) = prefixed_name.strip_prefix("mcp_") else {
            continue;
        };
        let Some((server, name)) = rest.split_once('_') else {
            continue;
        };

        if let Some(filter) = query.server.as_deref()
            && server != filter
        {
            continue;
        }

        tools.push(McpToolEntry {
            server: server.to_string(),
            name: name.to_string(),
            prefixed_name,
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
        });
    }

    tools.sort_by(|a, b| a.server.cmp(&b.server).then_with(|| a.name.cmp(&b.name)));

    Ok(Json(McpToolsResponse { tools }))
}

async fn list_automations(
    State(state): State<RuntimeApiState>,
) -> Result<Json<Vec<AutomationRecord>>, ApiError> {
    let manager = state.automations.lock().await;
    let automations = manager
        .list_automations()
        .map_err(|e| ApiError::internal(format!("Failed to list automations: {e}")))?;
    Ok(Json(automations))
}

async fn create_automation(
    State(state): State<RuntimeApiState>,
    Json(req): Json<CreateAutomationRequest>,
) -> Result<(StatusCode, Json<AutomationRecord>), ApiError> {
    let manager = state.automations.lock().await;
    let automation = manager
        .create_automation(req)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(automation)))
}

async fn get_automation(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<AutomationRecord>, ApiError> {
    let manager = state.automations.lock().await;
    let automation = manager.get_automation(&id).map_err(map_automation_err)?;
    Ok(Json(automation))
}

async fn update_automation(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateAutomationRequest>,
) -> Result<Json<AutomationRecord>, ApiError> {
    let manager = state.automations.lock().await;
    let automation = manager
        .update_automation(&id, req)
        .map_err(map_automation_err)?;
    Ok(Json(automation))
}

async fn delete_automation(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<AutomationRecord>, ApiError> {
    let manager = state.automations.lock().await;
    let automation = manager.delete_automation(&id).map_err(map_automation_err)?;
    Ok(Json(automation))
}

async fn run_automation(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<AutomationRunRecord>, ApiError> {
    let manager = state.automations.lock().await;
    let run = manager
        .run_now(&id, &state.task_manager)
        .await
        .map_err(map_automation_err)?;
    Ok(Json(run))
}

async fn pause_automation(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<AutomationRecord>, ApiError> {
    let manager = state.automations.lock().await;
    let automation = manager.pause_automation(&id).map_err(map_automation_err)?;
    Ok(Json(automation))
}

async fn resume_automation(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
) -> Result<Json<AutomationRecord>, ApiError> {
    let manager = state.automations.lock().await;
    let automation = manager.resume_automation(&id).map_err(map_automation_err)?;
    Ok(Json(automation))
}

async fn list_automation_runs(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Query(query): Query<AutomationRunsQuery>,
) -> Result<Json<Vec<AutomationRunRecord>>, ApiError> {
    let manager = state.automations.lock().await;
    let runs = manager
        .list_runs(&id, query.limit)
        .map_err(map_automation_err)?;
    Ok(Json(runs))
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

async fn update_thread(
    State(state): State<RuntimeApiState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateThreadRequest>,
) -> Result<Json<ThreadRecord>, ApiError> {
    let thread = state
        .runtime_threads
        .update_thread(&id, req)
        .await
        .map_err(map_thread_err)?;
    Ok(Json(thread))
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

async fn list_thread_approvals(
    State(state): State<RuntimeApiState>,
    Path((id, turn_id)): Path<(String, String)>,
) -> Result<Json<Vec<crate::runtime_threads::PendingApprovalRecord>>, ApiError> {
    let approvals = state
        .runtime_threads
        .list_pending_approvals(&id, &turn_id)
        .await
        .map_err(map_thread_err)?;
    Ok(Json(approvals))
}

async fn approve_thread_approval(
    State(state): State<RuntimeApiState>,
    Path((id, turn_id, approval_id)): Path<(String, String, String)>,
) -> Result<Json<crate::runtime_threads::PendingApprovalRecord>, ApiError> {
    let approval = state
        .runtime_threads
        .approve_pending_approval(&id, &turn_id, &approval_id)
        .await
        .map_err(map_thread_err)?;
    Ok(Json(approval))
}

async fn deny_thread_approval(
    State(state): State<RuntimeApiState>,
    Path((id, turn_id, approval_id)): Path<(String, String, String)>,
) -> Result<Json<crate::runtime_threads::PendingApprovalRecord>, ApiError> {
    let approval = state
        .runtime_threads
        .deny_pending_approval(&id, &turn_id, &approval_id)
        .await
        .map_err(map_thread_err)?;
    Ok(Json(approval))
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
    let auto_approve = req.auto_approve.unwrap_or(false);
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
            system_prompt: None,
            task_id: None,
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
                manual_approval: req.manual_approval,
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
                .unwrap_or(json!(null));
            Some(sse_json("turn.completed", json!({ "usage": usage })))
        }
        _ => None,
    }
}

fn sse_json(event: &str, payload: serde_json::Value) -> SseEvent {
    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    SseEvent::default().event(event).data(data)
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{truncated}...")
}

fn collect_workspace_status(workspace: &std::path::Path) -> WorkspaceStatusResponse {
    let mut status = WorkspaceStatusResponse {
        workspace: workspace.to_path_buf(),
        git_repo: false,
        branch: None,
        staged: 0,
        unstaged: 0,
        untracked: 0,
        ahead: None,
        behind: None,
    };

    let Some(repo_check) = run_git(workspace, &["rev-parse", "--is-inside-work-tree"]) else {
        return status;
    };
    if repo_check.trim() != "true" {
        return status;
    }

    status.git_repo = true;
    status.branch = run_git(workspace, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(porcelain) = run_git(workspace, &["status", "--porcelain=v1"]) {
        for line in porcelain.lines() {
            if line.starts_with("??") {
                status.untracked += 1;
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            if chars.len() >= 2 {
                if chars[0] != ' ' {
                    status.staged += 1;
                }
                if chars[1] != ' ' {
                    status.unstaged += 1;
                }
            }
        }
    }

    if let Some(counts) = run_git(
        workspace,
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
    ) {
        let mut parts = counts.split_whitespace();
        if let (Some(behind), Some(ahead)) = (parts.next(), parts.next()) {
            status.behind = behind.parse::<u32>().ok();
            status.ahead = ahead.parse::<u32>().ok();
        }
    }

    status
}

fn run_git(workspace: &std::path::Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn resolve_skills_dir(config: &Config, workspace: &std::path::Path) -> PathBuf {
    let agents_skills = workspace.join(".agents").join("skills");
    if agents_skills.exists() {
        return agents_skills;
    }
    let local_skills = workspace.join("skills");
    if local_skills.exists() {
        return local_skills;
    }
    config.skills_dir()
}

fn load_mcp_config_or_default(path: &std::path::Path) -> Result<McpConfig, ApiError> {
    if !path.exists() {
        return Ok(McpConfig::default());
    }
    let raw = fs::read_to_string(path).map_err(|e| {
        ApiError::internal(format!("Failed to read MCP config {}: {e}", path.display()))
    })?;
    serde_json::from_str::<McpConfig>(&raw).map_err(|e| {
        ApiError::internal(format!(
            "Failed to parse MCP config {}: {e}",
            path.display()
        ))
    })
}

#[derive(Debug, Deserialize)]
struct UsageQuery {
    /// ISO-8601 lower bound (inclusive). When omitted, no lower bound.
    since: Option<String>,
    /// ISO-8601 upper bound (inclusive). When omitted, no upper bound.
    until: Option<String>,
    /// Bucket key. One of `day` (default), `model`, `provider`, `thread`.
    group_by: Option<String>,
}

fn parse_iso8601(raw: &str, field: &str) -> Result<chrono::DateTime<Utc>, ApiError> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| ApiError::bad_request(format!("Invalid {field} (expected RFC 3339): {e}")))
}

async fn get_usage(
    State(state): State<RuntimeApiState>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<Value>, ApiError> {
    let since = match query.since.as_deref() {
        Some(raw) => Some(parse_iso8601(raw, "since")?),
        None => None,
    };
    let until = match query.until.as_deref() {
        Some(raw) => Some(parse_iso8601(raw, "until")?),
        None => None,
    };
    if let (Some(s), Some(u)) = (since, until)
        && s > u
    {
        return Err(ApiError::bad_request("since must be <= until".to_string()));
    }
    let group_by = match query.group_by.as_deref().unwrap_or("day") {
        "day" => UsageGroupBy::Day,
        "model" => UsageGroupBy::Model,
        "provider" => UsageGroupBy::Provider,
        "thread" => UsageGroupBy::Thread,
        other => {
            return Err(ApiError::bad_request(format!(
                "Unsupported group_by '{other}': expected one of day, model, provider, thread"
            )));
        }
    };

    let aggregation = state
        .runtime_threads
        .aggregate_usage(since, until, group_by)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!(aggregation)))
}

const MOBILE_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover" />
  <title>DeepSeek Mobile Remote</title>
  <style>
    :root {
      color-scheme: dark;
      --bg: #0b0f14;
      --panel: #121923;
      --panel-2: #182231;
      --line: #263345;
      --text: #e6edf5;
      --muted: #8ea1b5;
      --accent: #5fb3ff;
      --danger: #ff6b6b;
      --ok: #63d471;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      font: 15px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: var(--bg);
      color: var(--text);
    }
    header {
      position: sticky;
      top: 0;
      z-index: 5;
      padding: 14px 16px 10px;
      background: rgba(11, 15, 20, 0.94);
      border-bottom: 1px solid var(--line);
      backdrop-filter: blur(10px);
    }
    h1 { margin: 0; font-size: 18px; }
    .sub { margin-top: 4px; color: var(--muted); font-size: 12px; }
    main { padding: 12px; display: grid; gap: 12px; }
    section {
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 12px;
      overflow: hidden;
    }
    .section-head {
      padding: 10px 12px;
      border-bottom: 1px solid var(--line);
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 8px;
    }
    .section-body { padding: 12px; }
    button, input, textarea {
      font: inherit;
      color: var(--text);
      background: var(--panel-2);
      border: 1px solid var(--line);
      border-radius: 10px;
    }
    button {
      padding: 9px 11px;
      min-height: 40px;
      font-weight: 650;
    }
    button.primary { background: #0f4672; border-color: #24699f; }
    button.danger { background: #4a1d24; border-color: #8b313a; }
    input, textarea { width: 100%; padding: 10px; }
    textarea { min-height: 92px; resize: vertical; }
    .row { display: flex; gap: 8px; align-items: center; }
    .row > * { flex: 1; }
    .toggles {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 8px;
      margin-top: 8px;
    }
    label.toggle {
      display: flex;
      align-items: center;
      gap: 8px;
      color: var(--muted);
      background: var(--panel-2);
      border: 1px solid var(--line);
      border-radius: 10px;
      padding: 9px;
      font-size: 13px;
    }
    label.toggle input { width: auto; }
    #threads { display: grid; gap: 8px; }
    .thread {
      padding: 10px;
      border: 1px solid var(--line);
      border-radius: 10px;
      background: var(--panel-2);
      text-align: left;
    }
    .thread.active { border-color: var(--accent); }
    .thread-title { font-weight: 700; }
    .thread-meta, .event-meta { color: var(--muted); font-size: 12px; margin-top: 3px; }
    #events {
      display: grid;
      gap: 8px;
      max-height: 52vh;
      overflow: auto;
      padding-right: 2px;
    }
    .event {
      border: 1px solid var(--line);
      border-radius: 10px;
      background: #0e141d;
      padding: 9px 10px;
      white-space: pre-wrap;
      overflow-wrap: anywhere;
    }
    .event.agent { border-color: #2b5f8f; }
    .event.tool { border-color: #5d4b1f; }
    .event.error { border-color: #8b313a; }
    .event.ok { border-color: #2f7142; }
    .approval-actions {
      display: flex;
      gap: 8px;
      margin-top: 8px;
      flex-wrap: wrap;
    }
    .approval-actions button {
      width: auto;
      min-width: 92px;
      padding: 8px 10px;
    }
    .status {
      margin-top: 8px;
      padding: 8px 10px;
      color: var(--muted);
      border-radius: 10px;
      background: #0e141d;
      overflow-wrap: anywhere;
    }
    .status.ok { color: var(--ok); }
    .status.bad { color: var(--danger); }
    .small { font-size: 12px; color: var(--muted); }
    @media (min-width: 860px) {
      main { grid-template-columns: 320px 1fr; align-items: start; }
      section.chat { grid-column: 2; grid-row: 1 / span 3; }
    }
  </style>
</head>
<body>
  <header>
    <h1>DeepSeek Mobile Remote</h1>
    <div class="sub">Local-first remote control for the runtime API</div>
  </header>
  <main>
    <section>
      <div class="section-head">
        <strong>Connection</strong>
        <button id="save-token">Save</button>
      </div>
      <div class="section-body">
        <input id="token" autocomplete="off" placeholder="Runtime token" />
        <div id="conn" class="status">Not connected</div>
      </div>
    </section>

    <section>
      <div class="section-head">
        <strong>Threads</strong>
        <button id="new-thread">New</button>
      </div>
      <div class="section-body"><div id="threads"></div></div>
    </section>

    <section class="chat">
      <div class="section-head">
        <strong id="active-title">No thread selected</strong>
        <button id="refresh">Refresh</button>
      </div>
      <div class="section-body"><div id="events"></div></div>
    </section>

    <section>
      <div class="section-head">
        <strong>Composer</strong>
        <button id="interrupt" class="danger">Interrupt</button>
      </div>
      <div class="section-body">
        <textarea id="prompt" placeholder="Tell DeepSeek what to do..."></textarea>
        <div class="toggles">
          <label class="toggle"><input id="allow-shell" type="checkbox" /> allow shell</label>
          <label class="toggle"><input id="auto-approve" type="checkbox" /> auto approve</label>
        </div>
        <div class="row" style="margin-top: 8px;">
          <button id="send" class="primary">Send</button>
          <button id="steer">Steer</button>
        </div>
        <p class="small">Keep auto approve off unless this server is on a trusted network.</p>
      </div>
    </section>
  </main>

  <script>
    const $ = (id) => document.getElementById(id);
    const state = { threadId: "", activeTurnId: "", source: null, threads: [] };

    function takeTokenFromUrl() {
      const params = new URLSearchParams(location.search);
      const token = params.get("token");
      if (token) {
        localStorage.setItem("deepseek_runtime_token", token);
        params.delete("token");
        const qs = params.toString();
        history.replaceState(null, "", location.pathname + (qs ? "?" + qs : ""));
      }
    }
    function setStatus(message, good) {
      const el = $("conn");
      el.textContent = message;
      el.className = "status " + (good ? "ok" : "bad");
    }
    function token() { return $("token").value.trim(); }
    async function api(path, options = {}) {
      const headers = Object.assign({}, options.headers || {});
      headers["Content-Type"] = "application/json";
      if (token()) headers["Authorization"] = "Bearer " + token();
      const res = await fetch(path, Object.assign({}, options, { headers }));
      if (!res.ok) {
        let detail = await res.text();
        try { detail = JSON.parse(detail).error?.message || detail; } catch (_) {}
        throw new Error(detail || ("HTTP " + res.status));
      }
      if (res.status === 204) return null;
      return await res.json();
    }
    function escapeHtml(raw) {
      return String(raw).replace(/[&<>"']/g, (c) => ({
        "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#039;"
      }[c]));
    }
    function eventText(name, data) {
      if (name === "item.delta") return data.payload?.delta || "";
      if (name === "item.started") {
        const tool = data.payload?.tool;
        if (tool) return "Tool started: " + tool.name + "\n" + JSON.stringify(tool.input || {}, null, 2);
      }
      if (name === "item.completed" || name === "item.failed") {
        const item = data.payload?.item || {};
        return (name === "item.completed" ? "Completed: " : "Failed: ") + (item.summary || item.kind || "");
      }
      if (name === "approval.required") return "Approval required: " + (data.payload?.tool_name || "") + "\n" + (data.payload?.description || "");
      if (name === "approval.resolved") return "Approval " + (data.payload?.decision || "resolved") + ": " + (data.payload?.tool_name || data.payload?.id || "");
      if (name === "sandbox.denied") return "Sandbox denied: " + JSON.stringify(data.payload || data, null, 2);
      if (name === "turn.completed") return "Turn completed";
      if (name === "turn.lifecycle") return "Turn status: " + (data.payload?.status || "");
      return JSON.stringify(data.payload || data, null, 2);
    }
    function approvalInfo(name, data) {
      const payload = data.payload || {};
      const turnId = data.turn_id || payload.turn_id || state.activeTurnId;
      if (name === "approval.required" && payload.pending && payload.id && turnId) {
        return {
          id: payload.id,
          turnId,
          approveLabel: "Allow",
          denyLabel: "Deny"
        };
      }
      if (name === "sandbox.denied" && payload.pending && payload.tool_id && turnId) {
        return {
          id: payload.tool_id,
          turnId,
          approveLabel: "Retry full access",
          denyLabel: "Deny"
        };
      }
      return null;
    }
    async function resolveApproval(turnId, approvalId, decision, container) {
      if (!state.threadId || !turnId || !approvalId) throw new Error("Missing approval context");
      for (const button of container.querySelectorAll("button")) button.disabled = true;
      const path = "/v1/threads/" + encodeURIComponent(state.threadId)
        + "/turns/" + encodeURIComponent(turnId)
        + "/approvals/" + encodeURIComponent(approvalId)
        + "/" + decision;
      const result = await api(path, { method: "POST", body: "{}" });
      container.innerHTML = "<span class='small'>Decision sent: " + escapeHtml(result.id || approvalId) + "</span>";
    }
    function appendEvent(name, data) {
      const text = eventText(name, data);
      if (!text) return;
      const item = document.createElement("div");
      item.className = "event";
      if (name === "item.delta") item.classList.add("agent");
      if (name.includes("tool") || name === "item.started") item.classList.add("tool");
      if (name.includes("failed") || name.includes("denied")) item.classList.add("error");
      if (name === "turn.completed") item.classList.add("ok");
      item.innerHTML = "<div class='event-meta'>" + name + "</div>" + escapeHtml(text);
      const approval = approvalInfo(name, data);
      if (approval) {
        const actions = document.createElement("div");
        actions.className = "approval-actions";
        const approve = document.createElement("button");
        approve.className = "primary";
        approve.textContent = approval.approveLabel;
        approve.onclick = () => resolveApproval(approval.turnId, approval.id, "approve", actions)
          .catch((err) => setStatus(err.message, false));
        const deny = document.createElement("button");
        deny.className = "danger";
        deny.textContent = approval.denyLabel;
        deny.onclick = () => resolveApproval(approval.turnId, approval.id, "deny", actions)
          .catch((err) => setStatus(err.message, false));
        actions.appendChild(approve);
        actions.appendChild(deny);
        item.appendChild(actions);
      }
      $("events").appendChild(item);
      $("events").scrollTop = $("events").scrollHeight;
    }
    async function loadThreads() {
      const threads = await api("/v1/threads/summary?limit=50&include_archived=false");
      state.threads = threads;
      const list = $("threads");
      list.innerHTML = "";
      if (!threads.length) {
        list.innerHTML = "<div class='small'>No active threads.</div>";
        return;
      }
      for (const thread of threads) {
        const el = document.createElement("button");
        el.className = "thread" + (thread.id === state.threadId ? " active" : "");
        el.innerHTML = "<div class='thread-title'>" + escapeHtml(thread.title || thread.id) + "</div><div class='thread-meta'>" + escapeHtml(thread.mode || "") + " / " + escapeHtml(thread.model || "") + "</div>";
        el.onclick = () => selectThread(thread.id, thread.title || thread.id);
        list.appendChild(el);
      }
    }
    async function newThread() {
      const thread = await api("/v1/threads", {
        method: "POST",
        body: JSON.stringify({
          mode: "agent",
          allow_shell: $("allow-shell").checked,
          trust_mode: false,
          auto_approve: $("auto-approve").checked
        })
      });
      await loadThreads();
      selectThread(thread.id, thread.title || thread.id);
    }
    async function selectThread(id, title) {
      state.threadId = id;
      state.activeTurnId = "";
      $("active-title").textContent = title || id;
      $("events").innerHTML = "";
      if (state.source) state.source.close();
      const qs = "?since_seq=0" + (token() ? "&token=" + encodeURIComponent(token()) : "");
      const source = new EventSource("/v1/threads/" + encodeURIComponent(id) + "/events" + qs);
      state.source = source;
      const names = ["thread.started", "turn.started", "turn.lifecycle", "turn.steered", "turn.interrupt_requested", "turn.completed", "item.started", "item.delta", "item.completed", "item.failed", "approval.required", "approval.resolved", "sandbox.denied", "coherence.state"];
      for (const name of names) {
        source.addEventListener(name, (ev) => {
          let data = {};
          try { data = JSON.parse(ev.data || "{}"); } catch (_) {}
          if (data.turn_id) state.activeTurnId = data.turn_id;
          if (data.payload?.turn?.id) state.activeTurnId = data.payload.turn.id;
          appendEvent(name, data);
        });
      }
      source.onerror = () => setStatus("Event stream disconnected", false);
      setStatus("Connected", true);
      await loadThreads();
    }
    async function sendPrompt() {
      if (!state.threadId) await newThread();
      const prompt = $("prompt").value.trim();
      if (!prompt) return;
      const res = await api("/v1/threads/" + encodeURIComponent(state.threadId) + "/turns", {
        method: "POST",
        body: JSON.stringify({
          prompt,
          allow_shell: $("allow-shell").checked,
          trust_mode: false,
          auto_approve: $("auto-approve").checked,
          manual_approval: !$("auto-approve").checked
        })
      });
      state.activeTurnId = res.turn?.id || state.activeTurnId;
      $("prompt").value = "";
    }
    async function steerTurn() {
      if (!state.threadId || !state.activeTurnId) throw new Error("No active turn");
      const prompt = $("prompt").value.trim();
      if (!prompt) return;
      await api("/v1/threads/" + encodeURIComponent(state.threadId) + "/turns/" + encodeURIComponent(state.activeTurnId) + "/steer", {
        method: "POST",
        body: JSON.stringify({ prompt })
      });
      $("prompt").value = "";
    }
    async function interruptTurn() {
      if (!state.threadId || !state.activeTurnId) throw new Error("No active turn");
      await api("/v1/threads/" + encodeURIComponent(state.threadId) + "/turns/" + encodeURIComponent(state.activeTurnId) + "/interrupt", { method: "POST", body: "{}" });
    }
    async function boot() {
      takeTokenFromUrl();
      $("token").value = localStorage.getItem("deepseek_runtime_token") || "";
      $("save-token").onclick = async () => {
        localStorage.setItem("deepseek_runtime_token", token());
        await loadThreads().catch((err) => setStatus(err.message, false));
      };
      $("new-thread").onclick = () => newThread().catch((err) => setStatus(err.message, false));
      $("refresh").onclick = () => loadThreads().catch((err) => setStatus(err.message, false));
      $("send").onclick = () => sendPrompt().catch((err) => setStatus(err.message, false));
      $("steer").onclick = () => steerTurn().catch((err) => setStatus(err.message, false));
      $("interrupt").onclick = () => interruptTurn().catch((err) => setStatus(err.message, false));
      await loadThreads();
      setStatus("Connected", true);
    }
    boot().catch((err) => setStatus(err.message, false));
  </script>
</body>
</html>"#;

/// Built-in dev origins always allowed by the runtime API (whalescale#255).
const DEFAULT_CORS_ORIGINS: &[&str] = &[
    "http://localhost:3000",
    "http://127.0.0.1:3000",
    "http://localhost:1420",
    "http://127.0.0.1:1420",
    "tauri://localhost",
];

fn cors_layer(extra_origins: &[String]) -> CorsLayer {
    let mut origins: Vec<HeaderValue> = DEFAULT_CORS_ORIGINS
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    for raw in extra_origins {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match HeaderValue::from_str(trimmed) {
            Ok(value) if !origins.contains(&value) => origins.push(value),
            Ok(_) => {}
            Err(err) => tracing::warn!(
                "Ignoring invalid CORS origin '{trimmed}': {err}; expected scheme://host[:port]"
            ),
        }
    }
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any)
}

fn map_task_err(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.contains("not found") {
        ApiError::not_found(message)
    } else {
        ApiError::bad_request(message)
    }
}

fn map_automation_err(err: anyhow::Error) -> ApiError {
    let message = err.to_string();
    if message.contains("Failed to read automation")
        || message.contains("No such file or directory")
    {
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
    use tokio::sync::{Mutex, mpsc};
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

    async fn spawn_test_server_with_root(
        root: PathBuf,
        sessions_dir: PathBuf,
    ) -> Result<
        Option<(
            SocketAddr,
            SharedRuntimeThreadManager,
            tokio::task::JoinHandle<()>,
        )>,
    > {
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
            deepseek_v4_pro_prior: None,
            deepseek_v4_flash_prior: None,
            fallback_default_prior: None,
        });
        let runtime_threads: SharedRuntimeThreadManager = Arc::new(RuntimeThreadManager::open(
            config,
            PathBuf::from("."),
            RuntimeThreadManagerConfig::from_task_data_dir(root.join("runtime")),
        )?);
        runtime_threads.attach_task_manager(manager.clone());
        let automations = Arc::new(Mutex::new(AutomationManager::open(
            root.join("automations"),
        )?));
        runtime_threads.attach_automation_manager(automations.clone());

        let state = RuntimeApiState {
            config: Config::default(),
            workspace: PathBuf::from("."),
            task_manager: manager,
            runtime_threads: runtime_threads.clone(),
            cors_origins: Vec::new(),
            sessions_dir,
            mcp_config_path: root.join("mcp.json"),
            automations,
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

    async fn spawn_test_server() -> Result<
        Option<(
            SocketAddr,
            SharedRuntimeThreadManager,
            tokio::task::JoinHandle<()>,
        )>,
    > {
        let root = std::env::temp_dir().join(format!("deepseek-runtime-api-{}", Uuid::new_v4()));
        let sessions_dir = root.join("sessions");
        spawn_test_server_with_root(root, sessions_dir).await
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
    async fn workspace_and_automation_endpoints_work() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let workspace: serde_json::Value = client
            .get(format!("http://{addr}/v1/workspace/status"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(workspace.get("workspace").is_some());

        let created: serde_json::Value = client
            .post(format!("http://{addr}/v1/automations"))
            .json(&json!({
                "name": "Smoke automation",
                "prompt": "automation smoke test",
                "rrule": "FREQ=HOURLY;INTERVAL=2",
                "status": "active"
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let automation_id = created["id"]
            .as_str()
            .context("missing automation id")?
            .to_string();

        let listed: serde_json::Value = client
            .get(format!("http://{addr}/v1/automations"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(
            listed
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["id"] == automation_id))
        );

        let run_now: serde_json::Value = client
            .post(format!("http://{addr}/v1/automations/{automation_id}/run"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(run_now["automation_id"], automation_id);

        let paused: serde_json::Value = client
            .post(format!(
                "http://{addr}/v1/automations/{automation_id}/pause"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(paused["status"], "paused");

        let resumed: serde_json::Value = client
            .post(format!(
                "http://{addr}/v1/automations/{automation_id}/resume"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(resumed["status"], "active");

        let updated: serde_json::Value = client
            .patch(format!("http://{addr}/v1/automations/{automation_id}"))
            .json(&json!({
                "name": "Smoke automation edited",
                "rrule": "FREQ=WEEKLY;BYDAY=MO,WE;BYHOUR=10;BYMINUTE=15"
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(updated["name"], "Smoke automation edited");

        let runs: serde_json::Value = client
            .get(format!(
                "http://{addr}/v1/automations/{automation_id}/runs?limit=5"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(
            runs.as_array().is_some_and(|items| !items.is_empty()),
            "expected at least one run entry"
        );

        let _deleted: serde_json::Value = client
            .delete(format!("http://{addr}/v1/automations/{automation_id}"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let missing_status = client
            .get(format!("http://{addr}/v1/automations/{automation_id}"))
            .send()
            .await?
            .status();
        assert_eq!(missing_status, StatusCode::NOT_FOUND);

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

        let archived: serde_json::Value = client
            .patch(format!("http://{addr}/v1/threads/{thread_id}"))
            .json(&json!({ "archived": true }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(archived["id"], thread_id);
        assert_eq!(archived["archived"], true);

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
                .is_some_and(|threads| threads.iter().all(|t| t["id"] != thread_id))
        );

        let listed_all: serde_json::Value = client
            .get(format!(
                "http://{addr}/v1/threads/summary?include_archived=true&limit=100"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(
            listed_all
                .as_array()
                .is_some_and(|threads| threads.iter().any(|t| t["id"] == thread_id))
        );

        let unarchived: serde_json::Value = client
            .patch(format!("http://{addr}/v1/threads/{thread_id}"))
            .json(&json!({ "archived": false }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(unarchived["archived"], false);

        let invalid_patch = client
            .patch(format!("http://{addr}/v1/threads/{thread_id}"))
            .json(&json!({}))
            .send()
            .await?;
        assert_eq!(invalid_patch.status(), StatusCode::BAD_REQUEST);

        let missing_patch = client
            .patch(format!("http://{addr}/v1/threads/thr_missing"))
            .json(&json!({ "archived": true }))
            .send()
            .await?;
        assert_eq!(missing_patch.status(), StatusCode::NOT_FOUND);

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

        // Install a mock engine so the turn completes without calling the real API.
        // The mock handles both SendMessage and CompactContext ops so the
        // compact endpoint tested later also works.
        let harness = crate::core::engine::mock_engine_handle();
        runtime_threads
            .install_test_engine(&thread_id, harness.handle.clone())
            .await?;
        let mut rx_op = harness.rx_op;
        let tx_event = harness.tx_event;
        tokio::spawn(async move {
            while let Some(op) = rx_op.recv().await {
                match op {
                    Op::SendMessage { .. } => {
                        let _ = tx_event
                            .send(EngineEvent::TurnStarted {
                                turn_id: "mock_lifecycle".to_string(),
                            })
                            .await;
                        let _ = tx_event
                            .send(EngineEvent::MessageStarted { index: 0 })
                            .await;
                        let _ = tx_event
                            .send(EngineEvent::MessageDelta {
                                index: 0,
                                content: "mock reply".to_string(),
                            })
                            .await;
                        let _ = tx_event
                            .send(EngineEvent::MessageComplete { index: 0 })
                            .await;
                        let _ = tx_event
                            .send(EngineEvent::TurnComplete {
                                usage: Usage {
                                    input_tokens: 10,
                                    output_tokens: 5,
                                    ..Usage::default()
                                },
                                status: TurnOutcomeStatus::Completed,
                                error: None,
                            })
                            .await;
                    }
                    Op::CompactContext => {
                        let _ = tx_event
                            .send(EngineEvent::TurnComplete {
                                usage: Usage {
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    ..Usage::default()
                                },
                                status: TurnOutcomeStatus::Completed,
                                error: None,
                            })
                            .await;
                    }
                    _ => {}
                }
            }
        });

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

        // Install a mock engine so the turn completes without calling the real API.
        let harness = crate::core::engine::mock_engine_handle();
        runtime_threads
            .install_test_engine(&thread_id, harness.handle.clone())
            .await?;
        let mut rx_op = harness.rx_op;
        let tx_event = harness.tx_event;
        tokio::spawn(async move {
            if !matches!(rx_op.recv().await, Some(Op::SendMessage { .. })) {
                return;
            }
            let _ = tx_event
                .send(EngineEvent::TurnStarted {
                    turn_id: "mock_cursor".to_string(),
                })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageStarted { index: 0 })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageComplete { index: 0 })
                .await;
            let _ = tx_event
                .send(EngineEvent::TurnComplete {
                    usage: Usage {
                        input_tokens: 5,
                        output_tokens: 3,
                        ..Usage::default()
                    },
                    status: TurnOutcomeStatus::Completed,
                    error: None,
                })
                .await;
        });

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
                        ..Usage::default()
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
        let Some((addr, runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        // Create a thread and install a mock engine so /v1/stream doesn't call the real API.
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
        let tx_event = harness.tx_event;
        tokio::spawn(async move {
            if !matches!(rx_op.recv().await, Some(Op::SendMessage { .. })) {
                return;
            }
            let _ = tx_event
                .send(EngineEvent::TurnStarted {
                    turn_id: "mock_stream".to_string(),
                })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageStarted { index: 0 })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageDelta {
                    index: 0,
                    content: "streamed".to_string(),
                })
                .await;
            let _ = tx_event
                .send(EngineEvent::MessageComplete { index: 0 })
                .await;
            let _ = tx_event
                .send(EngineEvent::TurnComplete {
                    usage: Usage {
                        input_tokens: 4,
                        output_tokens: 2,
                        ..Usage::default()
                    },
                    status: TurnOutcomeStatus::Completed,
                    error: None,
                })
                .await;
        });

        // Start the turn and consume events via the SSE endpoint.
        let turn_start: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads/{thread_id}/turns"))
            .json(&json!({ "prompt": "compatibility stream" }))
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

        // Verify that the persisted events include the expected turn lifecycle events.
        let events = runtime_threads.events_since(&thread_id, None)?;
        assert!(
            events.iter().any(|ev| ev.event == "turn.started"),
            "expected turn.started event"
        );
        assert!(
            events.iter().any(|ev| ev.event == "turn.completed"),
            "expected turn.completed event"
        );

        // Verify the SSE endpoint returns event-stream content type.
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

        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn session_get_returns_404_for_missing_id() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let resp = client
            .get(format!("http://{addr}/v1/sessions/nonexistent_id"))
            .send()
            .await?;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn session_endpoints_reject_invalid_id() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let get_resp = client
            .get(format!("http://{addr}/v1/sessions/invalid%20id"))
            .send()
            .await?;
        assert_eq!(get_resp.status(), StatusCode::BAD_REQUEST);

        let resume_resp = client
            .post(format!(
                "http://{addr}/v1/sessions/invalid%20id/resume-thread"
            ))
            .json(&json!({}))
            .send()
            .await?;
        assert_eq!(resume_resp.status(), StatusCode::BAD_REQUEST);

        let delete_resp = client
            .delete(format!("http://{addr}/v1/sessions/invalid%20id"))
            .send()
            .await?;
        assert_eq!(delete_resp.status(), StatusCode::BAD_REQUEST);

        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn session_resume_thread_returns_404_for_missing_session() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let resp = client
            .post(format!(
                "http://{addr}/v1/sessions/nonexistent_session/resume-thread"
            ))
            .json(&json!({}))
            .send()
            .await?;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn session_resume_thread_creates_thread_from_saved_session() -> Result<()> {
        let root = std::env::temp_dir().join(format!("deepseek-session-resume-{}", Uuid::new_v4()));
        let sessions_dir = root.join("sessions");
        fs::create_dir_all(&sessions_dir)?;
        let session_id = "sess_test_resume";
        let session = json!({
            "schema_version": 1,
            "metadata": {
                "id": session_id,
                "title": "Test resume session",
                "created_at": "2025-01-01T00:00:00Z",
                "updated_at": "2025-01-01T00:10:00Z",
                "message_count": 2,
                "total_tokens": 100,
                "model": "deepseek-v4-pro",
                "workspace": "/tmp/test",
                "mode": "agent"
            },
            "messages": [
                {
                    "role": "user",
                    "content": [{ "type": "text", "text": "Hello, world!" }]
                },
                {
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "Hello! How can I help you?" }]
                }
            ],
            "system_prompt": null
        });
        fs::write(
            sessions_dir.join(format!("{session_id}.json")),
            serde_json::to_string_pretty(&session)?,
        )?;

        let Some((addr, _runtime_threads, handle)) =
            spawn_test_server_with_root(root.clone(), sessions_dir.clone()).await?
        else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let resp = client
            .post(format!(
                "http://{addr}/v1/sessions/{session_id}/resume-thread"
            ))
            .json(&json!({ "model": "deepseek-v4-pro" }))
            .send()
            .await?;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resumed: serde_json::Value = resp.json().await?;
        assert_eq!(resumed["session_id"], session_id);
        assert_eq!(resumed["message_count"], 2);

        let thread_id = resumed["thread_id"]
            .as_str()
            .context("missing resumed thread id")?;
        let detail: serde_json::Value = client
            .get(format!("http://{addr}/v1/threads/{thread_id}"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(detail["thread"]["id"], thread_id);
        assert_eq!(detail["turns"].as_array().map_or(0, Vec::len), 1);
        assert_eq!(detail["items"].as_array().map_or(0, Vec::len), 2);

        handle.abort();
        Ok(())
    }

    #[tokio::test]
    async fn session_delete_returns_404_for_missing_id() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();
        let resp = client
            .delete(format!("http://{addr}/v1/sessions/nonexistent-id"))
            .send()
            .await?;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        handle.abort();
        Ok(())
    }

    /// #561 / whalescale#255 — extra CORS origins from `RuntimeApiOptions`
    /// are added on top of the built-in defaults and propagate through to the
    /// `Access-Control-Allow-Origin` response header for preflight requests.
    /// Built-in defaults must keep working unchanged.
    #[tokio::test]
    async fn cors_layer_appends_extra_origins_and_keeps_defaults() -> Result<()> {
        // The cors_layer fn is the layer factory — exercise it through a
        // Router with a single trivial route so we can issue OPTIONS preflights
        // and observe the response headers.
        let extra = vec!["http://localhost:5173".to_string()];
        let layer = cors_layer(&extra);
        let router: Router = Router::new()
            .route("/probe", get(|| async { "ok" }))
            .layer(layer);

        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        let addr = listener.local_addr()?;
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let client = reqwest::Client::new();

        // The user-supplied origin is allowed.
        let resp = client
            .request(reqwest::Method::OPTIONS, format!("http://{addr}/probe"))
            .header("Origin", "http://localhost:5173")
            .header("Access-Control-Request-Method", "GET")
            .send()
            .await?;
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("http://localhost:5173")
        );

        // A built-in default origin still works.
        let resp = client
            .request(reqwest::Method::OPTIONS, format!("http://{addr}/probe"))
            .header("Origin", "http://localhost:1420")
            .header("Access-Control-Request-Method", "GET")
            .send()
            .await?;
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("http://localhost:1420")
        );

        // An origin that's neither configured nor a default is rejected
        // (CorsLayer omits the Allow-Origin header on mismatch).
        let resp = client
            .request(reqwest::Method::OPTIONS, format!("http://{addr}/probe"))
            .header("Origin", "http://malicious.example")
            .header("Access-Control-Request-Method", "GET")
            .send()
            .await?;
        assert!(
            resp.headers().get("access-control-allow-origin").is_none(),
            "non-allowed origin must not be echoed back"
        );

        handle.abort();
        Ok(())
    }

    /// #561 — invalid origins (non-ASCII, etc.) are skipped without aborting
    /// the layer build.
    #[test]
    fn cors_layer_skips_invalid_origins() {
        let extras = vec![
            "http://valid.example".to_string(),
            // Embedded NUL char makes `HeaderValue::from_str` fail.
            "http://invalid.example\0".to_string(),
            "  ".to_string(), // whitespace-only is dropped
        ];
        // Should not panic.
        let _ = cors_layer(&extras);
    }

    /// #562 / whalescale#256 — `PATCH /v1/threads/{id}` accepts the new
    /// fields (allow_shell, trust_mode, auto_approve, model, mode, title,
    /// system_prompt). Each is independently optional; an empty string clears
    /// `title` / `system_prompt` back to None.
    #[tokio::test]
    async fn patch_thread_accepts_extended_field_set() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let created: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads"))
            .json(&json!({
                "model": "deepseek-v4-flash",
                "mode": "agent"
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let thread_id = created["id"]
            .as_str()
            .context("missing thread id")?
            .to_string();

        // Patch every new field at once.
        let patched: serde_json::Value = client
            .patch(format!("http://{addr}/v1/threads/{thread_id}"))
            .json(&json!({
                "allow_shell": true,
                "trust_mode": true,
                "auto_approve": true,
                "model": "deepseek-v4-pro",
                "mode": "yolo",
                "title": "Whalescale UI test thread",
                "system_prompt": "You are a useful assistant."
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        assert_eq!(patched["allow_shell"], true);
        assert_eq!(patched["trust_mode"], true);
        assert_eq!(patched["auto_approve"], true);
        assert_eq!(patched["model"], "deepseek-v4-pro");
        assert_eq!(patched["mode"], "yolo");
        assert_eq!(patched["title"], "Whalescale UI test thread");
        assert_eq!(patched["system_prompt"], "You are a useful assistant.");

        // Empty string clears title back to None.
        let cleared: serde_json::Value = client
            .patch(format!("http://{addr}/v1/threads/{thread_id}"))
            .json(&json!({ "title": "" }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert!(
            cleared["title"].is_null() || !cleared.as_object().unwrap().contains_key("title"),
            "empty title must serialize as None: {cleared:?}"
        );

        // Empty patch (no fields) is still rejected.
        let empty = client
            .patch(format!("http://{addr}/v1/threads/{thread_id}"))
            .json(&json!({}))
            .send()
            .await?;
        assert_eq!(empty.status(), StatusCode::BAD_REQUEST);

        // Empty model is rejected (validation).
        let bad_model = client
            .patch(format!("http://{addr}/v1/threads/{thread_id}"))
            .json(&json!({ "model": "  " }))
            .send()
            .await?;
        assert_eq!(bad_model.status(), StatusCode::BAD_REQUEST);

        handle.abort();
        Ok(())
    }

    /// #563 / whalescale#260 — `archived_only=true` returns archived-only
    /// (no active threads), distinct from `include_archived=true` which
    /// returns both.
    #[tokio::test]
    async fn list_threads_archived_only_filter_matches_only_archived() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        // Two threads — keep one active, archive the other.
        let active: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads"))
            .json(&json!({}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let active_id = active["id"].as_str().unwrap().to_string();

        let archived: serde_json::Value = client
            .post(format!("http://{addr}/v1/threads"))
            .json(&json!({}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let archived_id = archived["id"].as_str().unwrap().to_string();

        client
            .patch(format!("http://{addr}/v1/threads/{archived_id}"))
            .json(&json!({ "archived": true }))
            .send()
            .await?
            .error_for_status()?;

        // Default (active only) → only the unarchived one.
        let active_list: serde_json::Value = client
            .get(format!("http://{addr}/v1/threads"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let ids: Vec<&str> = active_list
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["id"].as_str())
            .collect();
        assert!(ids.contains(&active_id.as_str()));
        assert!(!ids.contains(&archived_id.as_str()));

        // archived_only=true → only the archived one.
        let archived_list: serde_json::Value = client
            .get(format!("http://{addr}/v1/threads?archived_only=true"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let ids: Vec<&str> = archived_list
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["id"].as_str())
            .collect();
        assert_eq!(ids, vec![archived_id.as_str()]);

        // archived_only=true takes precedence over include_archived=true.
        let archived_list: serde_json::Value = client
            .get(format!(
                "http://{addr}/v1/threads?include_archived=true&archived_only=true"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let ids: Vec<&str> = archived_list
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["id"].as_str())
            .collect();
        assert_eq!(ids, vec![archived_id.as_str()]);

        // Same filter works on the summary endpoint.
        let summary: serde_json::Value = client
            .get(format!(
                "http://{addr}/v1/threads/summary?archived_only=true&limit=10"
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let summary_ids: Vec<&str> = summary
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["id"].as_str())
            .collect();
        assert_eq!(summary_ids, vec![archived_id.as_str()]);

        handle.abort();
        Ok(())
    }

    /// #564 / whalescale#261 — `GET /v1/usage` aggregates per-turn token +
    /// cost data. With no threads the response is well-formed and totals are
    /// zero with empty buckets (never a 404).
    #[tokio::test]
    async fn usage_endpoint_returns_empty_aggregation_for_fresh_store() -> Result<()> {
        let Some((addr, _runtime_threads, handle)) = spawn_test_server().await? else {
            return Ok(());
        };
        let client = reqwest::Client::new();

        let body: serde_json::Value = client
            .get(format!("http://{addr}/v1/usage"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(body["group_by"], "day");
        assert_eq!(body["totals"]["input_tokens"], 0);
        assert_eq!(body["totals"]["output_tokens"], 0);
        assert_eq!(body["totals"]["turns"], 0);
        assert!(
            body["buckets"].as_array().unwrap().is_empty(),
            "buckets must be empty when no turns exist: {body}"
        );

        // group_by query options are validated.
        let bad_group = client
            .get(format!("http://{addr}/v1/usage?group_by=galaxy"))
            .send()
            .await?;
        assert_eq!(bad_group.status(), StatusCode::BAD_REQUEST);

        // Each accepted group_by value succeeds.
        for gb in ["day", "model", "provider", "thread"] {
            let resp = client
                .get(format!("http://{addr}/v1/usage?group_by={gb}"))
                .send()
                .await?;
            assert!(resp.status().is_success(), "group_by={gb} failed: {resp:?}");
        }

        // Bad ISO-8601 timestamp rejected.
        let bad_since = client
            .get(format!("http://{addr}/v1/usage?since=not-a-date"))
            .send()
            .await?;
        assert_eq!(bad_since.status(), StatusCode::BAD_REQUEST);

        // since > until rejected.
        let inverted = client
            .get(format!(
                "http://{addr}/v1/usage?since=2030-01-02T00:00:00Z&until=2030-01-01T00:00:00Z"
            ))
            .send()
            .await?;
        assert_eq!(inverted.status(), StatusCode::BAD_REQUEST);

        handle.abort();
        Ok(())
    }
}
