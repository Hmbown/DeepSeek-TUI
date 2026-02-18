//! Runtime HTTP/SSE API for local DeepSeek automation.

use std::collections::HashSet;
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_stream::stream;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, Method, StatusCode};
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

use crate::automation_manager::{
    AutomationManager, AutomationRecord, AutomationRunRecord, AutomationSchedulerConfig,
    CreateAutomationRequest, SharedAutomationManager, UpdateAutomationRequest, spawn_scheduler,
};
use crate::config::{Config, DEFAULT_TEXT_MODEL};
use crate::mcp::{McpConfig, McpPool};
use crate::runtime_threads::{
    CompactThreadRequest, CreateThreadRequest, RuntimeThreadManager, RuntimeThreadManagerConfig,
    SharedRuntimeThreadManager, StartTurnRequest, SteerTurnRequest, ThreadDetail, ThreadRecord,
    TurnItemKind, TurnRecord, UpdateThreadRequest,
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
    sessions_dir: PathBuf,
    mcp_config_path: PathBuf,
    automations: SharedAutomationManager,
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
}

#[derive(Debug, Deserialize)]
struct ThreadSummaryQuery {
    limit: Option<usize>,
    search: Option<String>,
    include_archived: Option<bool>,
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
    let state = RuntimeApiState {
        config: config.clone(),
        workspace,
        task_manager,
        runtime_threads,
        sessions_dir,
        mcp_config_path: config.mcp_config_path(),
        automations,
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
    let serve_result = axum::serve(listener, app)
        .await
        .map_err(|e| anyhow!("Runtime API server error: {e}"));
    scheduler_cancel.cancel();
    scheduler_handle.abort();
    serve_result
}

pub fn build_router(state: RuntimeApiState) -> Router {
    Router::new()
        .route("/health", get(health))
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
        .layer(cors_layer())
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
    let threads = state
        .runtime_threads
        .list_threads(query.include_archived.unwrap_or(false), query.limit)
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
    let threads = state
        .runtime_threads
        .list_threads(query.include_archived.unwrap_or(false), Some(limit))
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

        let title = latest_turn
            .map(|turn| {
                if turn.input_summary.trim().is_empty() {
                    "New Thread".to_string()
                } else {
                    truncate_text(&turn.input_summary, 72)
                }
            })
            .unwrap_or_else(|| "New Thread".to_string());

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
            system_prompt: None,
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

fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin([
            HeaderValue::from_static("http://localhost:3000"),
            HeaderValue::from_static("http://127.0.0.1:3000"),
            HeaderValue::from_static("http://localhost:1420"),
            HeaderValue::from_static("http://127.0.0.1:1420"),
            HeaderValue::from_static("tauri://localhost"),
        ])
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
            mcp_config_path: root.join("mcp.json"),
            automations: Arc::new(Mutex::new(AutomationManager::open(
                root.join("automations"),
            )?)),
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
                                    server_tool_use: None,
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
                                    server_tool_use: None,
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
                        server_tool_use: None,
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
                        server_tool_use: None,
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
                        server_tool_use: None,
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
                "model": "deepseek-chat",
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
            .json(&json!({ "model": "deepseek-chat" }))
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
}
