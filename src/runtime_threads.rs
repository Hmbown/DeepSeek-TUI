//! Durable thread/turn/item runtime for the HTTP API and background tasks.
//!
//! This module keeps DeepSeek-only execution while exposing Codex-like lifecycle
//! semantics (threads, turns, items, interrupt/steer, and replayable events).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, broadcast};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::compaction::CompactionConfig;
use crate::config::{Config, DEFAULT_TEXT_MODEL, MAX_SUBAGENTS};
use crate::core::engine::{EngineConfig, EngineHandle, spawn_engine};
use crate::core::events::{Event as EngineEvent, TurnOutcomeStatus};
use crate::core::ops::Op;
use crate::models::{
    ContentBlock, Message, SystemPrompt, Usage, compaction_message_threshold_for_model,
    compaction_threshold_for_model,
};
use crate::tools::plan::new_shared_plan_state;
use crate::tools::todo::new_shared_todo_list;
use crate::tui::app::AppMode;

const EVENT_CHANNEL_CAPACITY: usize = 1024;
const MAX_ACTIVE_THREADS_DEFAULT: usize = 8;
const SUMMARY_LIMIT: usize = 280;
const CURRENT_RUNTIME_SCHEMA_VERSION: u32 = 1;

const fn default_runtime_schema_version() -> u32 {
    CURRENT_RUNTIME_SCHEMA_VERSION
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTurnStatus {
    Queued,
    InProgress,
    Completed,
    Failed,
    Interrupted,
    Canceled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnItemKind {
    UserMessage,
    AgentMessage,
    ToolCall,
    FileChange,
    CommandExecution,
    ContextCompaction,
    Status,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnItemLifecycleStatus {
    Queued,
    InProgress,
    Completed,
    Failed,
    Interrupted,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadRecord {
    #[serde(default = "default_runtime_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub model: String,
    pub workspace: PathBuf,
    pub mode: String,
    pub allow_shell: bool,
    pub trust_mode: bool,
    pub auto_approve: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_response_bookmark: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    #[serde(default = "default_runtime_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub thread_id: String,
    pub status: RuntimeTurnStatus,
    pub input_summary: String,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub item_ids: Vec<String>,
    #[serde(default)]
    pub steer_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnItemRecord {
    #[serde(default = "default_runtime_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub turn_id: String,
    pub kind: TurnItemKind,
    pub status: TurnItemLifecycleStatus,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEventRecord {
    #[serde(default = "default_runtime_schema_version")]
    pub schema_version: u32,
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    pub thread_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    pub event: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStoreState {
    #[serde(default = "default_runtime_schema_version")]
    schema_version: u32,
    next_seq: u64,
}

impl Default for RuntimeStoreState {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
            next_seq: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeThreadStore {
    threads_dir: PathBuf,
    turns_dir: PathBuf,
    items_dir: PathBuf,
    events_dir: PathBuf,
    state_path: PathBuf,
    state: Arc<Mutex<RuntimeStoreState>>,
}

impl RuntimeThreadStore {
    pub fn open(root: PathBuf) -> Result<Self> {
        let threads_dir = root.join("threads");
        let turns_dir = root.join("turns");
        let items_dir = root.join("items");
        let events_dir = root.join("events");
        fs::create_dir_all(&threads_dir)
            .with_context(|| format!("Failed to create {}", threads_dir.display()))?;
        fs::create_dir_all(&turns_dir)
            .with_context(|| format!("Failed to create {}", turns_dir.display()))?;
        fs::create_dir_all(&items_dir)
            .with_context(|| format!("Failed to create {}", items_dir.display()))?;
        fs::create_dir_all(&events_dir)
            .with_context(|| format!("Failed to create {}", events_dir.display()))?;

        let state_path = root.join("state.json");
        let state = if state_path.exists() {
            let raw = fs::read_to_string(&state_path)
                .with_context(|| format!("Failed to read {}", state_path.display()))?;
            serde_json::from_str::<RuntimeStoreState>(&raw)
                .with_context(|| format!("Failed to parse {}", state_path.display()))?
        } else {
            let default = RuntimeStoreState::default();
            write_json_atomic(&state_path, &default)?;
            default
        };

        Ok(Self {
            threads_dir,
            turns_dir,
            items_dir,
            events_dir,
            state_path,
            state: Arc::new(Mutex::new(state)),
        })
    }

    fn thread_path(&self, thread_id: &str) -> PathBuf {
        self.threads_dir.join(format!("{thread_id}.json"))
    }

    fn turn_path(&self, turn_id: &str) -> PathBuf {
        self.turns_dir.join(format!("{turn_id}.json"))
    }

    fn item_path(&self, item_id: &str) -> PathBuf {
        self.items_dir.join(format!("{item_id}.json"))
    }

    fn events_path(&self, thread_id: &str) -> PathBuf {
        self.events_dir.join(format!("{thread_id}.jsonl"))
    }

    pub fn save_thread(&self, thread: &ThreadRecord) -> Result<()> {
        write_json_atomic(&self.thread_path(&thread.id), thread)
    }

    pub fn save_turn(&self, turn: &TurnRecord) -> Result<()> {
        write_json_atomic(&self.turn_path(&turn.id), turn)
    }

    pub fn save_item(&self, item: &TurnItemRecord) -> Result<()> {
        write_json_atomic(&self.item_path(&item.id), item)
    }

    pub fn load_thread(&self, thread_id: &str) -> Result<ThreadRecord> {
        let path = self.thread_path(thread_id);
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read thread {}", path.display()))?;
        let record: ThreadRecord = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse thread {}", path.display()))?;
        if record.schema_version > CURRENT_RUNTIME_SCHEMA_VERSION {
            bail!(
                "Thread schema v{} is newer than supported v{}",
                record.schema_version,
                CURRENT_RUNTIME_SCHEMA_VERSION
            );
        }
        Ok(record)
    }

    pub fn load_turn(&self, turn_id: &str) -> Result<TurnRecord> {
        let path = self.turn_path(turn_id);
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read turn {}", path.display()))?;
        let record: TurnRecord = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse turn {}", path.display()))?;
        if record.schema_version > CURRENT_RUNTIME_SCHEMA_VERSION {
            bail!(
                "Turn schema v{} is newer than supported v{}",
                record.schema_version,
                CURRENT_RUNTIME_SCHEMA_VERSION
            );
        }
        Ok(record)
    }

    pub fn load_item(&self, item_id: &str) -> Result<TurnItemRecord> {
        let path = self.item_path(item_id);
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read item {}", path.display()))?;
        let record: TurnItemRecord = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse item {}", path.display()))?;
        if record.schema_version > CURRENT_RUNTIME_SCHEMA_VERSION {
            bail!(
                "Item schema v{} is newer than supported v{}",
                record.schema_version,
                CURRENT_RUNTIME_SCHEMA_VERSION
            );
        }
        Ok(record)
    }

    pub fn list_threads(&self) -> Result<Vec<ThreadRecord>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.threads_dir)
            .with_context(|| format!("Failed to read {}", self.threads_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let thread: ThreadRecord = serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
            if thread.schema_version > CURRENT_RUNTIME_SCHEMA_VERSION {
                bail!(
                    "Thread schema v{} is newer than supported v{}",
                    thread.schema_version,
                    CURRENT_RUNTIME_SCHEMA_VERSION
                );
            }
            out.push(thread);
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    pub fn list_turns_for_thread(&self, thread_id: &str) -> Result<Vec<TurnRecord>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.turns_dir)
            .with_context(|| format!("Failed to read {}", self.turns_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let turn: TurnRecord = serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
            if turn.schema_version > CURRENT_RUNTIME_SCHEMA_VERSION {
                bail!(
                    "Turn schema v{} is newer than supported v{}",
                    turn.schema_version,
                    CURRENT_RUNTIME_SCHEMA_VERSION
                );
            }
            if turn.thread_id == thread_id {
                out.push(turn);
            }
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(out)
    }

    pub fn list_items_for_turn(&self, turn_id: &str) -> Result<Vec<TurnItemRecord>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.items_dir)
            .with_context(|| format!("Failed to read {}", self.items_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let item: TurnItemRecord = serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
            if item.schema_version > CURRENT_RUNTIME_SCHEMA_VERSION {
                bail!(
                    "Item schema v{} is newer than supported v{}",
                    item.schema_version,
                    CURRENT_RUNTIME_SCHEMA_VERSION
                );
            }
            if item.turn_id == turn_id {
                out.push(item);
            }
        }
        out.sort_by(|a, b| {
            let left = a.started_at.unwrap_or_else(Utc::now);
            let right = b.started_at.unwrap_or_else(Utc::now);
            left.cmp(&right)
        });
        Ok(out)
    }

    pub async fn append_event(
        &self,
        thread_id: &str,
        turn_id: Option<&str>,
        item_id: Option<&str>,
        event: impl Into<String>,
        payload: Value,
    ) -> Result<RuntimeEventRecord> {
        let mut state = self.state.lock().await;
        let seq = state.next_seq;
        state.next_seq = state.next_seq.saturating_add(1);
        write_json_atomic(&self.state_path, &*state)?;
        drop(state);

        let record = RuntimeEventRecord {
            schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
            seq,
            timestamp: Utc::now(),
            thread_id: thread_id.to_string(),
            turn_id: turn_id.map(ToString::to_string),
            item_id: item_id.map(ToString::to_string),
            event: event.into(),
            payload,
        };

        let path = self.events_path(thread_id);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open {}", path.display()))?;
        let line = serde_json::to_string(&record)?;
        writeln!(file, "{line}").with_context(|| format!("Failed to append {}", path.display()))?;
        file.flush()
            .with_context(|| format!("Failed to flush {}", path.display()))?;
        Ok(record)
    }

    pub fn events_since(
        &self,
        thread_id: &str,
        since_seq: Option<u64>,
    ) -> Result<Vec<RuntimeEventRecord>> {
        let path = self.events_path(thread_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file =
            File::open(&path).with_context(|| format!("Failed to open {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: RuntimeEventRecord = serde_json::from_str(&line)
                .with_context(|| format!("Failed to parse event line in {}", path.display()))?;
            if let Some(since) = since_seq {
                if event.seq <= since {
                    continue;
                }
            }
            out.push(event);
        }
        Ok(out)
    }

    pub async fn current_seq(&self) -> u64 {
        let state = self.state.lock().await;
        state.next_seq.saturating_sub(1)
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeThreadManagerConfig {
    pub data_dir: PathBuf,
    pub max_active_threads: usize,
}

impl RuntimeThreadManagerConfig {
    #[must_use]
    pub fn from_task_data_dir(task_data_dir: PathBuf) -> Self {
        let data_dir = if let Ok(override_dir) = std::env::var("DEEPSEEK_RUNTIME_DIR") {
            if override_dir.trim().is_empty() {
                task_data_dir.join("runtime")
            } else {
                PathBuf::from(override_dir)
            }
        } else {
            task_data_dir.join("runtime")
        };
        Self {
            data_dir,
            max_active_threads: MAX_ACTIVE_THREADS_DEFAULT,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateThreadRequest {
    pub model: Option<String>,
    pub workspace: Option<PathBuf>,
    pub mode: Option<String>,
    pub allow_shell: Option<bool>,
    pub trust_mode: Option<bool>,
    pub auto_approve: Option<bool>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateThreadRequest {
    pub archived: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartTurnRequest {
    pub prompt: String,
    #[serde(default)]
    pub input_summary: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub allow_shell: Option<bool>,
    pub trust_mode: Option<bool>,
    pub auto_approve: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteerTurnRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompactThreadRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadDetail {
    pub thread: ThreadRecord,
    pub turns: Vec<TurnRecord>,
    pub items: Vec<TurnItemRecord>,
    pub latest_seq: u64,
}

#[derive(Debug, Clone)]
struct ActiveTurnState {
    turn_id: String,
    interrupt_requested: bool,
    auto_approve: bool,
    trust_mode: bool,
}

#[derive(Clone)]
struct ActiveThreadState {
    engine: EngineHandle,
    active_turn: Option<ActiveTurnState>,
}

#[derive(Default)]
struct ActiveThreads {
    engines: HashMap<String, ActiveThreadState>,
    lru: VecDeque<String>,
}

pub type SharedRuntimeThreadManager = Arc<RuntimeThreadManager>;

/// Manages active engine threads, lifecycle, and event persistence.
///
/// # Lock ordering invariant
///
/// Two `Mutex`es exist across this module:
/// - `RuntimeThreadStore::state` — protects the monotonic event sequence counter.
/// - `RuntimeThreadManager::active` — protects the set of loaded engine handles.
///
/// **No code path holds both locks simultaneously.** The `state` lock is only
/// acquired inside `RuntimeThreadStore::append_event` (where it is explicitly
/// dropped before any I/O) and `current_seq`. All `emit_event` calls (which
/// call `append_event`) happen *after* `active` has been released. If you add
/// new code that touches both, always acquire `state` before `active` to
/// preserve a consistent ordering.
#[derive(Clone)]
pub struct RuntimeThreadManager {
    config: Config,
    workspace: PathBuf,
    store: RuntimeThreadStore,
    active: Arc<Mutex<ActiveThreads>>,
    event_tx: broadcast::Sender<RuntimeEventRecord>,
    manager_cfg: RuntimeThreadManagerConfig,
    cancel_token: CancellationToken,
}

impl RuntimeThreadManager {
    pub fn open(
        config: Config,
        workspace: PathBuf,
        manager_cfg: RuntimeThreadManagerConfig,
    ) -> Result<Self> {
        let store = RuntimeThreadStore::open(manager_cfg.data_dir.clone())?;
        let (event_tx, _event_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Ok(Self {
            config,
            workspace,
            store,
            active: Arc::new(Mutex::new(ActiveThreads::default())),
            event_tx,
            manager_cfg,
            cancel_token: CancellationToken::new(),
        })
    }

    #[allow(dead_code)] // Public API for external callers (runtime API, task manager)
    pub fn shutdown(&self) {
        self.cancel_token.cancel();
    }

    #[allow(dead_code)] // Public API for external callers
    pub fn is_shutdown(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    #[must_use]
    pub fn subscribe_events(&self) -> broadcast::Receiver<RuntimeEventRecord> {
        self.event_tx.subscribe()
    }

    async fn emit_event(
        &self,
        thread_id: &str,
        turn_id: Option<&str>,
        item_id: Option<&str>,
        event: impl Into<String>,
        payload: Value,
    ) -> Result<RuntimeEventRecord> {
        let record = self
            .store
            .append_event(thread_id, turn_id, item_id, event, payload)
            .await?;
        if let Err(e) = self.event_tx.send(record.clone()) {
            tracing::debug!(
                "Runtime event broadcast failed (no receivers or channel full): {}",
                e
            );
        }
        Ok(record)
    }

    pub async fn create_thread(&self, req: CreateThreadRequest) -> Result<ThreadRecord> {
        let now = Utc::now();
        let model = req
            .model
            .filter(|m| !m.trim().is_empty())
            .or_else(|| self.config.default_text_model.clone())
            .unwrap_or_else(|| DEFAULT_TEXT_MODEL.to_string());
        let workspace = req.workspace.unwrap_or_else(|| self.workspace.clone());
        let mode = req
            .mode
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| "agent".to_string());
        let allow_shell = req.allow_shell.unwrap_or_else(|| self.config.allow_shell());
        let trust_mode = req.trust_mode.unwrap_or(false);
        let auto_approve = req.auto_approve.unwrap_or(true);

        let thread = ThreadRecord {
            schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
            id: format!("thr_{}", &Uuid::new_v4().to_string()[..8]),
            created_at: now,
            updated_at: now,
            model,
            workspace,
            mode,
            allow_shell,
            trust_mode,
            auto_approve,
            latest_turn_id: None,
            latest_response_bookmark: None,
            archived: req.archived,
            system_prompt: req.system_prompt,
        };
        self.store.save_thread(&thread)?;
        self.emit_event(
            &thread.id,
            None,
            None,
            "thread.started",
            json!({ "thread": thread }),
        )
        .await?;
        Ok(thread)
    }

    pub async fn list_threads(
        &self,
        include_archived: bool,
        limit: Option<usize>,
    ) -> Result<Vec<ThreadRecord>> {
        let mut threads = self.store.list_threads()?;
        if !include_archived {
            threads.retain(|t| !t.archived);
        }
        if let Some(limit) = limit {
            threads.truncate(limit);
        }
        Ok(threads)
    }

    pub async fn get_thread(&self, id: &str) -> Result<ThreadRecord> {
        self.store
            .load_thread(id)
            .with_context(|| format!("Thread not found: {id}"))
    }

    pub async fn update_thread(&self, id: &str, req: UpdateThreadRequest) -> Result<ThreadRecord> {
        if req.archived.is_none() {
            bail!("At least one thread field is required");
        }

        let mut thread = self.get_thread(id).await?;
        let mut changed = false;

        if let Some(archived) = req.archived
            && thread.archived != archived
        {
            thread.archived = archived;
            changed = true;
        }

        if changed {
            thread.updated_at = Utc::now();
            self.store.save_thread(&thread)?;
            self.emit_event(
                &thread.id,
                None,
                None,
                "thread.updated",
                json!({
                    "thread": thread.clone(),
                    "changes": {
                        "archived": thread.archived
                    }
                }),
            )
            .await?;
        }

        Ok(thread)
    }

    pub async fn get_thread_detail(&self, id: &str) -> Result<ThreadDetail> {
        let thread = self.get_thread(id).await?;
        let turns = self.store.list_turns_for_thread(id)?;
        let mut items = Vec::new();
        for turn in &turns {
            items.extend(self.store.list_items_for_turn(&turn.id)?);
        }
        let latest_seq = self.store.current_seq().await;
        Ok(ThreadDetail {
            thread,
            turns,
            items,
            latest_seq,
        })
    }

    pub async fn resume_thread(&self, id: &str) -> Result<ThreadRecord> {
        let thread = self.get_thread(id).await?;
        self.ensure_engine_loaded(&thread).await?;
        Ok(thread)
    }

    pub async fn fork_thread(&self, id: &str) -> Result<ThreadRecord> {
        let source = self.get_thread(id).await?;
        let mut forked = source.clone();
        let now = Utc::now();
        forked.id = format!("thr_{}", &Uuid::new_v4().to_string()[..8]);
        forked.created_at = now;
        forked.updated_at = now;
        forked.latest_turn_id = None;
        forked.archived = false;
        self.store.save_thread(&forked)?;

        let source_turns = self.store.list_turns_for_thread(&source.id)?;
        for source_turn in source_turns {
            let mut cloned_turn = source_turn.clone();
            cloned_turn.id = format!("turn_{}", &Uuid::new_v4().to_string()[..8]);
            cloned_turn.thread_id = forked.id.clone();
            cloned_turn.item_ids.clear();
            self.store.save_turn(&cloned_turn)?;

            let items = self.store.list_items_for_turn(&source_turn.id)?;
            for item in items {
                let mut cloned_item = item.clone();
                cloned_item.id = format!("item_{}", &Uuid::new_v4().to_string()[..8]);
                cloned_item.turn_id = cloned_turn.id.clone();
                self.store.save_item(&cloned_item)?;
                cloned_turn.item_ids.push(cloned_item.id.clone());
            }
            self.store.save_turn(&cloned_turn)?;
            forked.latest_turn_id = Some(cloned_turn.id.clone());
            forked.updated_at = now;
            self.store.save_thread(&forked)?;
        }

        self.emit_event(
            &forked.id,
            None,
            None,
            "thread.forked",
            json!({
                "thread": forked,
                "source_thread_id": source.id,
            }),
        )
        .await?;
        Ok(forked)
    }

    /// Seed a thread with messages from a saved session so subsequent turns
    /// continue with the prior conversation context.
    pub async fn seed_thread_from_messages(
        &self,
        thread_id: &str,
        messages: &[Message],
    ) -> Result<()> {
        let mut thread = self.get_thread(thread_id).await?;
        let now = Utc::now();

        let mut user_buf: Vec<String> = Vec::new();
        let mut pending_pairs: Vec<(String, Option<String>)> = Vec::new();

        for msg in messages {
            let text = msg
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if text.trim().is_empty() {
                continue;
            }
            if msg.role == "user" {
                user_buf.push(text);
            } else if msg.role == "assistant" {
                let user_text = if user_buf.is_empty() {
                    String::new()
                } else {
                    std::mem::take(&mut user_buf).join("\n")
                };
                pending_pairs.push((user_text, Some(text)));
            }
        }
        if !user_buf.is_empty() {
            let user_text = std::mem::take(&mut user_buf).join("\n");
            pending_pairs.push((user_text, None));
        }

        for (user_text, assistant_text) in pending_pairs {
            let turn_id = format!("turn_{}", &Uuid::new_v4().to_string()[..8]);
            let summary = if user_text.len() > SUMMARY_LIMIT {
                format!("{}...", &user_text[..SUMMARY_LIMIT.saturating_sub(3)])
            } else {
                user_text.clone()
            };
            let mut item_ids = Vec::new();

            if !user_text.is_empty() {
                let item_id = format!("item_{}", &Uuid::new_v4().to_string()[..8]);
                self.store.save_item(&TurnItemRecord {
                    schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                    id: item_id.clone(),
                    turn_id: turn_id.clone(),
                    kind: TurnItemKind::UserMessage,
                    status: TurnItemLifecycleStatus::Completed,
                    summary: summary.clone(),
                    detail: Some(user_text),
                    artifact_refs: Vec::new(),
                    started_at: Some(now),
                    ended_at: Some(now),
                })?;
                item_ids.push(item_id);
            }

            if let Some(assistant_text) = assistant_text {
                let asst_summary = if assistant_text.len() > SUMMARY_LIMIT {
                    format!("{}...", &assistant_text[..SUMMARY_LIMIT.saturating_sub(3)])
                } else {
                    assistant_text.clone()
                };
                let item_id = format!("item_{}", &Uuid::new_v4().to_string()[..8]);
                self.store.save_item(&TurnItemRecord {
                    schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                    id: item_id.clone(),
                    turn_id: turn_id.clone(),
                    kind: TurnItemKind::AgentMessage,
                    status: TurnItemLifecycleStatus::Completed,
                    summary: asst_summary,
                    detail: Some(assistant_text),
                    artifact_refs: Vec::new(),
                    started_at: Some(now),
                    ended_at: Some(now),
                })?;
                item_ids.push(item_id);
            }

            self.store.save_turn(&TurnRecord {
                schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                id: turn_id.clone(),
                thread_id: thread_id.to_string(),
                status: RuntimeTurnStatus::Completed,
                input_summary: summary,
                created_at: now,
                started_at: Some(now),
                ended_at: Some(now),
                duration_ms: Some(0),
                usage: None,
                error: None,
                item_ids,
                steer_count: 0,
            })?;

            thread.latest_turn_id = Some(turn_id);
            thread.updated_at = now;
        }

        self.store.save_thread(&thread)?;
        self.emit_event(
            thread_id,
            None,
            None,
            "thread.updated",
            json!({ "thread": thread, "reason": "session_resume" }),
        )
        .await?;
        Ok(())
    }

    pub async fn start_turn(&self, thread_id: &str, req: StartTurnRequest) -> Result<TurnRecord> {
        let prompt = req.prompt.trim().to_string();
        if prompt.is_empty() {
            bail!("prompt is required");
        }

        let mut thread = self.get_thread(thread_id).await?;
        let engine = self.ensure_engine_loaded(&thread).await?;

        {
            let active = self.active.lock().await;
            if let Some(active_thread) = active.engines.get(thread_id)
                && active_thread.active_turn.is_some()
            {
                bail!("Thread already has an active turn");
            }
        }

        let now = Utc::now();
        let turn_id = format!("turn_{}", &Uuid::new_v4().to_string()[..8]);
        let mut turn = TurnRecord {
            schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
            id: turn_id.clone(),
            thread_id: thread_id.to_string(),
            status: RuntimeTurnStatus::InProgress,
            input_summary: req
                .input_summary
                .unwrap_or_else(|| summarize_text(&prompt, SUMMARY_LIMIT)),
            created_at: now,
            started_at: Some(now),
            ended_at: None,
            duration_ms: None,
            usage: None,
            error: None,
            item_ids: Vec::new(),
            steer_count: 0,
        };

        let user_item_id = format!("item_{}", &Uuid::new_v4().to_string()[..8]);
        let user_item = TurnItemRecord {
            schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
            id: user_item_id.clone(),
            turn_id: turn_id.clone(),
            kind: TurnItemKind::UserMessage,
            status: TurnItemLifecycleStatus::Completed,
            summary: summarize_text(&prompt, SUMMARY_LIMIT),
            detail: Some(prompt.clone()),
            artifact_refs: Vec::new(),
            started_at: Some(now),
            ended_at: Some(now),
        };

        turn.item_ids.push(user_item_id.clone());
        self.store.save_item(&user_item)?;
        self.store.save_turn(&turn)?;

        thread.latest_turn_id = Some(turn_id.clone());
        thread.updated_at = now;
        self.store.save_thread(&thread)?;

        self.emit_event(
            thread_id,
            Some(&turn_id),
            None,
            "turn.started",
            json!({ "turn": turn.clone() }),
        )
        .await?;
        self.emit_event(
            thread_id,
            Some(&turn_id),
            Some(&user_item_id),
            "item.started",
            json!({ "item": user_item.clone() }),
        )
        .await?;
        self.emit_event(
            thread_id,
            Some(&turn_id),
            Some(&user_item_id),
            "item.completed",
            json!({ "item": user_item }),
        )
        .await?;

        {
            let mut active = self.active.lock().await;
            let Some(state) = active.engines.get_mut(thread_id) else {
                bail!("Thread engine not loaded");
            };
            state.active_turn = Some(ActiveTurnState {
                turn_id: turn_id.clone(),
                interrupt_requested: false,
                auto_approve: req.auto_approve.unwrap_or(thread.auto_approve),
                trust_mode: req.trust_mode.unwrap_or(thread.trust_mode),
            });
            touch_lru(&mut active.lru, thread_id);
        }

        let mode = parse_mode(req.mode.as_deref().unwrap_or(&thread.mode));
        let model = req.model.unwrap_or_else(|| thread.model.clone());
        let allow_shell = req.allow_shell.unwrap_or(thread.allow_shell);
        let trust_mode = req.trust_mode.unwrap_or(thread.trust_mode);

        engine
            .send(Op::send(
                prompt,
                mode,
                model.clone(),
                allow_shell,
                trust_mode,
            ))
            .await
            .map_err(|e| anyhow!("Failed to start turn: {e}"))?;

        let manager = Arc::new(self.clone());
        let thread_id_owned = thread_id.to_string();
        let turn_id_owned = turn_id.clone();
        let engine_clone = engine.clone();
        let cancel_token = self.cancel_token.clone();
        tokio::spawn(async move {
            if cancel_token.is_cancelled() {
                tracing::debug!("Skipping turn monitor: shutdown requested");
                return;
            }
            use futures_util::FutureExt;
            let result = std::panic::AssertUnwindSafe(manager.monitor_turn(
                thread_id_owned,
                turn_id_owned,
                engine_clone,
            ))
            .catch_unwind()
            .await;
            match result {
                Ok(res) => {
                    if let Err(err) = res {
                        tracing::error!("Failed to monitor turn: {err}");
                    }
                }
                Err(panic_err) => {
                    if let Some(msg) = panic_err.downcast_ref::<&str>() {
                        tracing::error!("Turn monitor panicked: {}", msg);
                    } else if let Some(msg) = panic_err.downcast_ref::<String>() {
                        tracing::error!("Turn monitor panicked: {}", msg);
                    } else {
                        tracing::error!("Turn monitor panicked with unknown error");
                    }
                }
            }
        });

        Ok(turn)
    }

    pub async fn interrupt_turn(&self, thread_id: &str, turn_id: &str) -> Result<TurnRecord> {
        {
            let mut active = self.active.lock().await;
            let Some(active_thread) = active.engines.get_mut(thread_id) else {
                bail!("Thread is not loaded");
            };
            let Some(active_turn) = active_thread.active_turn.as_mut() else {
                bail!("No active turn on thread {thread_id}");
            };
            if active_turn.turn_id != turn_id {
                bail!("Turn {turn_id} is not active on thread {thread_id}");
            }
            active_turn.interrupt_requested = true;
            active_thread.engine.cancel();
            touch_lru(&mut active.lru, thread_id);
        }

        self.emit_event(
            thread_id,
            Some(turn_id),
            None,
            "turn.interrupt_requested",
            json!({ "thread_id": thread_id, "turn_id": turn_id }),
        )
        .await?;

        self.store.load_turn(turn_id)
    }

    pub async fn steer_turn(
        &self,
        thread_id: &str,
        turn_id: &str,
        req: SteerTurnRequest,
    ) -> Result<TurnRecord> {
        let prompt = req.prompt.trim().to_string();
        if prompt.is_empty() {
            bail!("prompt is required");
        }

        let engine = {
            let mut active = self.active.lock().await;
            let engine = {
                let Some(active_thread) = active.engines.get_mut(thread_id) else {
                    bail!("Thread is not loaded");
                };
                let Some(active_turn) = active_thread.active_turn.as_mut() else {
                    bail!("No active turn on thread {thread_id}");
                };
                if active_turn.turn_id != turn_id {
                    bail!("Turn {turn_id} is not active on thread {thread_id}");
                }
                active_thread.engine.clone()
            };
            touch_lru(&mut active.lru, thread_id);
            engine
        };

        engine
            .steer(prompt.clone())
            .await
            .map_err(|e| anyhow!("Failed to steer turn: {e}"))?;

        let now = Utc::now();
        let mut turn = self.store.load_turn(turn_id)?;
        turn.steer_count = turn.steer_count.saturating_add(1);
        self.store.save_turn(&turn)?;

        let item = TurnItemRecord {
            schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
            id: format!("item_{}", &Uuid::new_v4().to_string()[..8]),
            turn_id: turn_id.to_string(),
            kind: TurnItemKind::UserMessage,
            status: TurnItemLifecycleStatus::Completed,
            summary: summarize_text(&prompt, SUMMARY_LIMIT),
            detail: Some(prompt.clone()),
            artifact_refs: Vec::new(),
            started_at: Some(now),
            ended_at: Some(now),
        };
        turn.item_ids.push(item.id.clone());
        self.store.save_item(&item)?;
        self.store.save_turn(&turn)?;

        self.emit_event(
            thread_id,
            Some(turn_id),
            Some(&item.id),
            "turn.steered",
            json!({
                "thread_id": thread_id,
                "turn_id": turn_id,
                "input": prompt,
            }),
        )
        .await?;
        self.emit_event(
            thread_id,
            Some(turn_id),
            Some(&item.id),
            "item.completed",
            json!({ "item": item }),
        )
        .await?;

        Ok(turn)
    }

    pub async fn compact_thread(
        &self,
        thread_id: &str,
        req: CompactThreadRequest,
    ) -> Result<TurnRecord> {
        let mut thread = self.get_thread(thread_id).await?;
        let engine = self.ensure_engine_loaded(&thread).await?;

        {
            let active = self.active.lock().await;
            if let Some(active_thread) = active.engines.get(thread_id)
                && active_thread.active_turn.is_some()
            {
                bail!("Thread already has an active turn");
            }
        }

        let now = Utc::now();
        let turn_id = format!("turn_{}", &Uuid::new_v4().to_string()[..8]);
        let turn = TurnRecord {
            schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
            id: turn_id.clone(),
            thread_id: thread_id.to_string(),
            status: RuntimeTurnStatus::InProgress,
            input_summary: req
                .reason
                .as_deref()
                .map(|s| summarize_text(s, SUMMARY_LIMIT))
                .unwrap_or_else(|| "Manual context compaction".to_string()),
            created_at: now,
            started_at: Some(now),
            ended_at: None,
            duration_ms: None,
            usage: None,
            error: None,
            item_ids: Vec::new(),
            steer_count: 0,
        };
        self.store.save_turn(&turn)?;

        thread.latest_turn_id = Some(turn_id.clone());
        thread.updated_at = now;
        self.store.save_thread(&thread)?;

        {
            let mut active = self.active.lock().await;
            let Some(state) = active.engines.get_mut(thread_id) else {
                bail!("Thread engine not loaded");
            };
            state.active_turn = Some(ActiveTurnState {
                turn_id: turn_id.clone(),
                interrupt_requested: false,
                auto_approve: true,
                trust_mode: thread.trust_mode,
            });
            touch_lru(&mut active.lru, thread_id);
        }

        self.emit_event(
            thread_id,
            Some(&turn_id),
            None,
            "turn.started",
            json!({ "turn": turn.clone(), "manual_compaction": true }),
        )
        .await?;

        engine
            .send(Op::CompactContext)
            .await
            .map_err(|e| anyhow!("Failed to trigger compaction: {e}"))?;

        let manager = Arc::new(self.clone());
        let thread_id_owned = thread_id.to_string();
        let turn_id_owned = turn_id.clone();
        let engine_clone = engine.clone();
        let cancel_token = self.cancel_token.clone();
        tokio::spawn(async move {
            if cancel_token.is_cancelled() {
                tracing::debug!("Skipping compaction monitor: shutdown requested");
                return;
            }
            use futures_util::FutureExt;
            let result = std::panic::AssertUnwindSafe(manager.monitor_turn(
                thread_id_owned,
                turn_id_owned,
                engine_clone,
            ))
            .catch_unwind()
            .await;
            match result {
                Ok(res) => {
                    if let Err(err) = res {
                        tracing::error!("Failed to monitor compaction turn: {err}");
                    }
                }
                Err(panic_err) => {
                    if let Some(msg) = panic_err.downcast_ref::<&str>() {
                        tracing::error!("Compaction monitor panicked: {}", msg);
                    } else if let Some(msg) = panic_err.downcast_ref::<String>() {
                        tracing::error!("Compaction monitor panicked: {}", msg);
                    } else {
                        tracing::error!("Compaction monitor panicked with unknown error");
                    }
                }
            }
        });

        Ok(turn)
    }

    pub fn events_since(
        &self,
        thread_id: &str,
        since_seq: Option<u64>,
    ) -> Result<Vec<RuntimeEventRecord>> {
        self.store.events_since(thread_id, since_seq)
    }

    async fn ensure_engine_loaded(&self, thread: &ThreadRecord) -> Result<EngineHandle> {
        {
            let mut active = self.active.lock().await;
            if let Some(engine) = active
                .engines
                .get(thread.id.as_str())
                .map(|state| state.engine.clone())
            {
                touch_lru(&mut active.lru, &thread.id);
                return Ok(engine);
            }
        }

        let mut compaction = CompactionConfig::default();
        compaction.enabled = true;
        compaction.model = thread.model.clone();
        compaction.token_threshold = compaction_threshold_for_model(&thread.model);
        compaction.message_threshold = compaction_message_threshold_for_model(&thread.model);
        let engine_cfg = EngineConfig {
            model: thread.model.clone(),
            workspace: thread.workspace.clone(),
            allow_shell: thread.allow_shell,
            trust_mode: thread.trust_mode,
            notes_path: self.config.notes_path(),
            mcp_config_path: self.config.mcp_config_path(),
            max_steps: 100,
            max_subagents: self.config.max_subagents().clamp(1, MAX_SUBAGENTS),
            features: self.config.features(),
            compaction,
            capacity: crate::core::capacity::CapacityControllerConfig::from_app_config(
                &self.config,
            ),
            todos: new_shared_todo_list(),
            plan_state: new_shared_plan_state(),
        };

        let engine = spawn_engine(engine_cfg, &self.config);

        let turns = self.store.list_turns_for_thread(&thread.id)?;
        let session_messages = self.reconstruct_messages_from_turns(&turns)?;
        let sys_prompt = thread
            .system_prompt
            .as_ref()
            .map(|s| SystemPrompt::Text(s.clone()));
        if !session_messages.is_empty() || sys_prompt.is_some() {
            engine
                .send(Op::SyncSession {
                    messages: session_messages,
                    system_prompt: sys_prompt,
                    model: thread.model.clone(),
                    workspace: thread.workspace.clone(),
                })
                .await
                .map_err(|e| anyhow!("Failed to sync thread session: {e}"))?;
        }

        let mut active = self.active.lock().await;
        let evicted = enforce_lru_capacity(&mut active, self.manager_cfg.max_active_threads);
        active.engines.insert(
            thread.id.clone(),
            ActiveThreadState {
                engine: engine.clone(),
                active_turn: None,
            },
        );
        touch_lru(&mut active.lru, &thread.id);
        drop(active);
        for handle in evicted {
            let _ = handle.send(Op::Shutdown).await;
        }
        Ok(engine)
    }

    fn reconstruct_messages_from_turns(&self, turns: &[TurnRecord]) -> Result<Vec<Message>> {
        let mut messages = Vec::new();
        for turn in turns {
            let items = self.store.list_items_for_turn(&turn.id)?;
            for item in items {
                match item.kind {
                    TurnItemKind::UserMessage => {
                        let text = item.detail.unwrap_or(item.summary);
                        messages.push(Message {
                            role: "user".to_string(),
                            content: vec![ContentBlock::Text {
                                text,
                                cache_control: None,
                            }],
                        });
                    }
                    TurnItemKind::AgentMessage => {
                        let text = item.detail.unwrap_or(item.summary);
                        messages.push(Message {
                            role: "assistant".to_string(),
                            content: vec![ContentBlock::Text {
                                text,
                                cache_control: None,
                            }],
                        });
                    }
                    _ => {}
                }
            }
        }
        Ok(messages)
    }

    async fn monitor_turn(
        &self,
        thread_id: String,
        turn_id: String,
        engine: EngineHandle,
    ) -> Result<()> {
        let mut current_message_item: Option<(String, String)> = None;
        let mut tool_items: HashMap<String, String> = HashMap::new();
        let mut compaction_items: HashMap<String, String> = HashMap::new();
        let mut turn_usage: Option<Usage> = None;
        let mut turn_status = RuntimeTurnStatus::Completed;
        let mut turn_error: Option<String> = None;

        loop {
            let event = {
                let mut rx = engine.rx_event.write().await;
                rx.recv().await
            };
            let Some(event) = event else {
                if self
                    .is_interrupt_requested(&thread_id, &turn_id)
                    .await
                    .unwrap_or(false)
                {
                    turn_status = RuntimeTurnStatus::Interrupted;
                }
                break;
            };

            match event {
                EngineEvent::TurnStarted { .. } => {
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        None,
                        "turn.lifecycle",
                        json!({ "status": "in_progress" }),
                    )
                    .await?;
                }
                EngineEvent::MessageStarted { .. } => {
                    let item_id = format!("item_{}", &Uuid::new_v4().to_string()[..8]);
                    let item = TurnItemRecord {
                        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                        id: item_id.clone(),
                        turn_id: turn_id.clone(),
                        kind: TurnItemKind::AgentMessage,
                        status: TurnItemLifecycleStatus::InProgress,
                        summary: String::new(),
                        detail: Some(String::new()),
                        artifact_refs: Vec::new(),
                        started_at: Some(Utc::now()),
                        ended_at: None,
                    };
                    self.store.save_item(&item)?;
                    self.attach_item_to_turn(&turn_id, &item.id)?;
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        Some(&item_id),
                        "item.started",
                        json!({ "item": item }),
                    )
                    .await?;
                    current_message_item = Some((item_id, String::new()));
                }
                EngineEvent::MessageDelta { content, .. } => {
                    if let Some((item_id, text)) = current_message_item.as_mut() {
                        text.push_str(&content);
                        self.emit_event(
                            &thread_id,
                            Some(&turn_id),
                            Some(item_id),
                            "item.delta",
                            json!({ "delta": content, "kind": "agent_message" }),
                        )
                        .await?;
                    }
                }
                EngineEvent::MessageComplete { .. } => {
                    if let Some((item_id, text)) = current_message_item.take() {
                        let mut item = self.store.load_item(&item_id)?;
                        item.status = TurnItemLifecycleStatus::Completed;
                        item.summary = summarize_text(&text, SUMMARY_LIMIT);
                        item.detail = Some(text);
                        item.ended_at = Some(Utc::now());
                        self.store.save_item(&item)?;
                        self.emit_event(
                            &thread_id,
                            Some(&turn_id),
                            Some(&item_id),
                            "item.completed",
                            json!({ "item": item }),
                        )
                        .await?;
                    }
                }
                EngineEvent::ToolCallStarted { id, name, input } => {
                    let item_id = format!("item_{}", &Uuid::new_v4().to_string()[..8]);
                    tool_items.insert(id.clone(), item_id.clone());
                    let kind = tool_kind_for_name(&name);
                    let summary = summarize_text(&format!("{name} started"), SUMMARY_LIMIT);
                    let item = TurnItemRecord {
                        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                        id: item_id.clone(),
                        turn_id: turn_id.clone(),
                        kind,
                        status: TurnItemLifecycleStatus::InProgress,
                        summary,
                        detail: Some(serde_json::to_string(&input).unwrap_or_default()),
                        artifact_refs: Vec::new(),
                        started_at: Some(Utc::now()),
                        ended_at: None,
                    };
                    self.store.save_item(&item)?;
                    self.attach_item_to_turn(&turn_id, &item.id)?;
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        Some(&item_id),
                        "item.started",
                        json!({ "item": item, "tool": { "id": id, "name": name, "input": input } }),
                    )
                    .await?;
                }
                EngineEvent::ToolCallProgress { id, output } => {
                    if let Some(item_id) = tool_items.get(&id) {
                        self.emit_event(
                            &thread_id,
                            Some(&turn_id),
                            Some(item_id),
                            "item.delta",
                            json!({ "delta": output, "kind": "tool_call" }),
                        )
                        .await?;
                    }
                }
                EngineEvent::ToolCallComplete { id, name, result } => {
                    if let Some(item_id) = tool_items.remove(&id) {
                        let mut item = self.store.load_item(&item_id)?;
                        let now = Utc::now();
                        item.ended_at = Some(now);
                        match result {
                            Ok(output) => {
                                item.status = if output.success {
                                    TurnItemLifecycleStatus::Completed
                                } else {
                                    TurnItemLifecycleStatus::Failed
                                };
                                item.summary = summarize_text(
                                    &format!("{name}: {}", output.content),
                                    SUMMARY_LIMIT,
                                );
                                item.detail = Some(output.content.clone());
                            }
                            Err(err) => {
                                item.status = TurnItemLifecycleStatus::Failed;
                                item.summary =
                                    summarize_text(&format!("{name} failed: {err}"), SUMMARY_LIMIT);
                                item.detail = Some(err.to_string());
                            }
                        }
                        self.store.save_item(&item)?;
                        self.emit_event(
                            &thread_id,
                            Some(&turn_id),
                            Some(&item_id),
                            if item.status == TurnItemLifecycleStatus::Completed {
                                "item.completed"
                            } else {
                                "item.failed"
                            },
                            json!({ "item": item }),
                        )
                        .await?;
                    }
                }
                EngineEvent::CompactionStarted { id, auto, message } => {
                    let item_id = format!("item_{}", &Uuid::new_v4().to_string()[..8]);
                    compaction_items.insert(id.clone(), item_id.clone());
                    let item = TurnItemRecord {
                        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                        id: item_id.clone(),
                        turn_id: turn_id.clone(),
                        kind: TurnItemKind::ContextCompaction,
                        status: TurnItemLifecycleStatus::InProgress,
                        summary: summarize_text(&message, SUMMARY_LIMIT),
                        detail: Some(message.clone()),
                        artifact_refs: Vec::new(),
                        started_at: Some(Utc::now()),
                        ended_at: None,
                    };
                    self.store.save_item(&item)?;
                    self.attach_item_to_turn(&turn_id, &item.id)?;
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        Some(&item_id),
                        "item.started",
                        json!({ "item": item, "auto": auto }),
                    )
                    .await?;
                }
                EngineEvent::CompactionCompleted { id, auto, message } => {
                    if let Some(item_id) = compaction_items.remove(&id) {
                        let mut item = self.store.load_item(&item_id)?;
                        item.status = TurnItemLifecycleStatus::Completed;
                        item.summary = summarize_text(&message, SUMMARY_LIMIT);
                        item.detail = Some(message);
                        item.ended_at = Some(Utc::now());
                        self.store.save_item(&item)?;
                        self.emit_event(
                            &thread_id,
                            Some(&turn_id),
                            Some(&item_id),
                            "item.completed",
                            json!({ "item": item, "auto": auto }),
                        )
                        .await?;
                    }
                }
                EngineEvent::CompactionFailed { id, auto, message } => {
                    if let Some(item_id) = compaction_items.remove(&id) {
                        let mut item = self.store.load_item(&item_id)?;
                        item.status = TurnItemLifecycleStatus::Failed;
                        item.summary = summarize_text(&message, SUMMARY_LIMIT);
                        item.detail = Some(message);
                        item.ended_at = Some(Utc::now());
                        self.store.save_item(&item)?;
                        self.emit_event(
                            &thread_id,
                            Some(&turn_id),
                            Some(&item_id),
                            "item.failed",
                            json!({ "item": item, "auto": auto }),
                        )
                        .await?;
                    }
                }
                EngineEvent::CapacityDecision {
                    risk_band,
                    action,
                    reason,
                    ..
                } => {
                    let message = format!(
                        "Capacity decision: risk={risk_band} action={action} reason={reason}"
                    );
                    let item = TurnItemRecord {
                        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                        id: format!("item_{}", &Uuid::new_v4().to_string()[..8]),
                        turn_id: turn_id.clone(),
                        kind: TurnItemKind::Status,
                        status: TurnItemLifecycleStatus::Completed,
                        summary: summarize_text(&message, SUMMARY_LIMIT),
                        detail: Some(message),
                        artifact_refs: Vec::new(),
                        started_at: Some(Utc::now()),
                        ended_at: Some(Utc::now()),
                    };
                    self.store.save_item(&item)?;
                    self.attach_item_to_turn(&turn_id, &item.id)?;
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        Some(&item.id),
                        "item.completed",
                        json!({ "item": item }),
                    )
                    .await?;
                }
                EngineEvent::CapacityIntervention {
                    action,
                    before_prompt_tokens,
                    after_prompt_tokens,
                    replay_outcome,
                    replan_performed,
                    ..
                } => {
                    let message = format!(
                        "Capacity intervention: {action} (~{before_prompt_tokens} -> ~{after_prompt_tokens}) replay={:?} replan={replan_performed}",
                        replay_outcome
                    );
                    let item = TurnItemRecord {
                        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                        id: format!("item_{}", &Uuid::new_v4().to_string()[..8]),
                        turn_id: turn_id.clone(),
                        kind: TurnItemKind::Status,
                        status: TurnItemLifecycleStatus::Completed,
                        summary: summarize_text(&message, SUMMARY_LIMIT),
                        detail: Some(message),
                        artifact_refs: Vec::new(),
                        started_at: Some(Utc::now()),
                        ended_at: Some(Utc::now()),
                    };
                    self.store.save_item(&item)?;
                    self.attach_item_to_turn(&turn_id, &item.id)?;
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        Some(&item.id),
                        "item.completed",
                        json!({ "item": item }),
                    )
                    .await?;
                }
                EngineEvent::CapacityMemoryPersistFailed { action, error, .. } => {
                    let message =
                        format!("Capacity memory persist failed: action={action} error={error}");
                    let item = TurnItemRecord {
                        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                        id: format!("item_{}", &Uuid::new_v4().to_string()[..8]),
                        turn_id: turn_id.clone(),
                        kind: TurnItemKind::Status,
                        status: TurnItemLifecycleStatus::Failed,
                        summary: summarize_text(&message, SUMMARY_LIMIT),
                        detail: Some(message),
                        artifact_refs: Vec::new(),
                        started_at: Some(Utc::now()),
                        ended_at: Some(Utc::now()),
                    };
                    self.store.save_item(&item)?;
                    self.attach_item_to_turn(&turn_id, &item.id)?;
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        Some(&item.id),
                        "item.failed",
                        json!({ "item": item }),
                    )
                    .await?;
                }
                EngineEvent::ApprovalRequired {
                    id,
                    tool_name,
                    description,
                } => {
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        None,
                        "approval.required",
                        json!({
                            "id": id,
                            "tool_name": tool_name,
                            "description": description,
                        }),
                    )
                    .await?;

                    let (auto_approve, trust_mode) = self
                        .active_turn_flags(&thread_id, &turn_id)
                        .await
                        .unwrap_or((true, false));
                    if auto_approve {
                        let _ = engine.approve_tool_call(id).await;
                    } else {
                        let _ = engine.deny_tool_call(id).await;
                    }
                    if trust_mode {
                        let _ = trust_mode;
                    }
                }
                EngineEvent::ElevationRequired {
                    tool_id,
                    tool_name,
                    denial_reason,
                    ..
                } => {
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        None,
                        "sandbox.denied",
                        json!({
                            "tool_id": tool_id,
                            "tool_name": tool_name,
                            "reason": denial_reason,
                        }),
                    )
                    .await?;
                    let (auto_approve, trust_mode) = self
                        .active_turn_flags(&thread_id, &turn_id)
                        .await
                        .unwrap_or((true, false));
                    if auto_approve && trust_mode {
                        let _ = engine
                            .retry_tool_with_policy(
                                tool_id,
                                crate::sandbox::SandboxPolicy::DangerFullAccess,
                            )
                            .await;
                    } else {
                        let _ = engine.deny_tool_call(tool_id).await;
                    }
                }
                EngineEvent::Status { message } => {
                    let item = TurnItemRecord {
                        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                        id: format!("item_{}", &Uuid::new_v4().to_string()[..8]),
                        turn_id: turn_id.clone(),
                        kind: TurnItemKind::Status,
                        status: TurnItemLifecycleStatus::Completed,
                        summary: summarize_text(&message, SUMMARY_LIMIT),
                        detail: Some(message.clone()),
                        artifact_refs: Vec::new(),
                        started_at: Some(Utc::now()),
                        ended_at: Some(Utc::now()),
                    };
                    self.store.save_item(&item)?;
                    self.attach_item_to_turn(&turn_id, &item.id)?;
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        Some(&item.id),
                        "item.completed",
                        json!({ "item": item }),
                    )
                    .await?;
                }
                EngineEvent::Error { message, .. } => {
                    turn_status = RuntimeTurnStatus::Failed;
                    turn_error = Some(message.clone());
                    let item = TurnItemRecord {
                        schema_version: CURRENT_RUNTIME_SCHEMA_VERSION,
                        id: format!("item_{}", &Uuid::new_v4().to_string()[..8]),
                        turn_id: turn_id.clone(),
                        kind: TurnItemKind::Error,
                        status: TurnItemLifecycleStatus::Failed,
                        summary: summarize_text(&message, SUMMARY_LIMIT),
                        detail: Some(message),
                        artifact_refs: Vec::new(),
                        started_at: Some(Utc::now()),
                        ended_at: Some(Utc::now()),
                    };
                    self.store.save_item(&item)?;
                    self.attach_item_to_turn(&turn_id, &item.id)?;
                    self.emit_event(
                        &thread_id,
                        Some(&turn_id),
                        Some(&item.id),
                        "item.failed",
                        json!({ "item": item }),
                    )
                    .await?;
                }
                EngineEvent::TurnComplete {
                    usage,
                    status,
                    error,
                } => {
                    turn_usage = Some(usage);
                    turn_status = match status {
                        TurnOutcomeStatus::Completed => RuntimeTurnStatus::Completed,
                        TurnOutcomeStatus::Interrupted => RuntimeTurnStatus::Interrupted,
                        TurnOutcomeStatus::Failed => RuntimeTurnStatus::Failed,
                    };
                    if let Some(err) = error {
                        turn_error = Some(err);
                    }
                    break;
                }
                _ => {}
            }
        }

        if self
            .is_interrupt_requested(&thread_id, &turn_id)
            .await
            .unwrap_or(false)
        {
            turn_status = RuntimeTurnStatus::Interrupted;
        }

        if let Some((item_id, text)) = current_message_item.take() {
            let mut item = self.store.load_item(&item_id)?;
            if turn_status == RuntimeTurnStatus::Interrupted {
                item.status = TurnItemLifecycleStatus::Interrupted;
            } else {
                item.status = TurnItemLifecycleStatus::Completed;
            }
            item.summary = summarize_text(&text, SUMMARY_LIMIT);
            item.detail = Some(text);
            item.ended_at = Some(Utc::now());
            self.store.save_item(&item)?;
            self.emit_event(
                &thread_id,
                Some(&turn_id),
                Some(&item_id),
                if item.status == TurnItemLifecycleStatus::Interrupted {
                    "item.interrupted"
                } else {
                    "item.completed"
                },
                json!({ "item": item }),
            )
            .await?;
        }

        let ended_at = Utc::now();
        let mut turn = self.store.load_turn(&turn_id)?;
        turn.status = turn_status;
        turn.ended_at = Some(ended_at);
        turn.duration_ms = turn.started_at.map(|start| duration_ms(start, ended_at));
        turn.usage = turn_usage;
        turn.error = turn_error;
        self.store.save_turn(&turn)?;

        let mut thread = self.get_thread(&thread_id).await?;
        thread.latest_turn_id = Some(turn_id.clone());
        thread.updated_at = Utc::now();
        self.store.save_thread(&thread)?;

        self.emit_event(
            &thread_id,
            Some(&turn_id),
            None,
            "turn.completed",
            json!({ "turn": turn.clone() }),
        )
        .await?;

        {
            let mut active = self.active.lock().await;
            if let Some(state) = active.engines.get_mut(&thread_id)
                && state
                    .active_turn
                    .as_ref()
                    .is_some_and(|t| t.turn_id == turn_id)
            {
                state.active_turn = None;
            }
            touch_lru(&mut active.lru, &thread_id);
        }

        Ok(())
    }

    fn attach_item_to_turn(&self, turn_id: &str, item_id: &str) -> Result<()> {
        let mut turn = self.store.load_turn(turn_id)?;
        if !turn.item_ids.iter().any(|id| id == item_id) {
            turn.item_ids.push(item_id.to_string());
            self.store.save_turn(&turn)?;
        }
        Ok(())
    }

    async fn is_interrupt_requested(&self, thread_id: &str, turn_id: &str) -> Result<bool> {
        let active = self.active.lock().await;
        let Some(state) = active.engines.get(thread_id) else {
            return Ok(false);
        };
        let Some(turn) = state.active_turn.as_ref() else {
            return Ok(false);
        };
        Ok(turn.turn_id == turn_id && turn.interrupt_requested)
    }

    async fn active_turn_flags(&self, thread_id: &str, turn_id: &str) -> Option<(bool, bool)> {
        let active = self.active.lock().await;
        let state = active.engines.get(thread_id)?;
        let turn = state.active_turn.as_ref()?;
        if turn.turn_id != turn_id {
            return None;
        }
        Some((turn.auto_approve, turn.trust_mode))
    }

    #[cfg(test)]
    pub(crate) async fn install_test_engine(
        &self,
        thread_id: &str,
        engine: EngineHandle,
    ) -> Result<()> {
        let _ = self.get_thread(thread_id).await?;
        let mut active = self.active.lock().await;
        active.engines.insert(
            thread_id.to_string(),
            ActiveThreadState {
                engine,
                active_turn: None,
            },
        );
        touch_lru(&mut active.lru, thread_id);
        Ok(())
    }
}

fn touch_lru(lru: &mut VecDeque<String>, thread_id: &str) {
    if let Some(idx) = lru.iter().position(|id| id == thread_id) {
        lru.remove(idx);
    }
    lru.push_back(thread_id.to_string());
}

fn enforce_lru_capacity(
    active: &mut ActiveThreads,
    max_active_threads: usize,
) -> Vec<EngineHandle> {
    let mut evicted = Vec::new();
    if max_active_threads == 0 || active.engines.len() < max_active_threads {
        return evicted;
    }
    let protected = active
        .engines
        .iter()
        .filter_map(|(thread_id, state)| {
            if state.active_turn.is_some() {
                Some(thread_id.clone())
            } else {
                None
            }
        })
        .collect::<HashSet<_>>();

    let scan_limit = active.lru.len();
    for _ in 0..scan_limit {
        let Some(candidate) = active.lru.pop_front() else {
            break;
        };
        if protected.contains(&candidate) {
            active.lru.push_back(candidate);
            continue;
        }
        if let Some(state) = active.engines.remove(&candidate) {
            evicted.push(state.engine);
        }
        break;
    }
    evicted
}

fn parse_mode(mode: &str) -> AppMode {
    match mode.trim().to_ascii_lowercase().as_str() {
        "normal" => AppMode::Normal,
        "plan" => AppMode::Plan,
        "yolo" => AppMode::Yolo,
        _ => AppMode::Agent,
    }
}

fn tool_kind_for_name(name: &str) -> TurnItemKind {
    let lower = name.to_ascii_lowercase();
    if lower == "exec_shell" || lower == "exec_shell_wait" || lower == "exec_shell_interact" {
        return TurnItemKind::CommandExecution;
    }
    if lower.contains("patch") || lower.contains("write") || lower.contains("edit") {
        return TurnItemKind::FileChange;
    }
    TurnItemKind::ToolCall
}

pub fn summarize_text(text: &str, limit: usize) -> String {
    let take = limit.saturating_sub(3);
    let mut count = 0;
    let mut out = String::new();
    for ch in text.chars() {
        if count >= take {
            out.push_str("...");
            return out;
        }
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        }
        out.push(ch);
        count += 1;
    }
    out
}

fn duration_ms(start: DateTime<Utc>, end: DateTime<Utc>) -> u64 {
    let millis = (end - start).num_milliseconds();
    if millis.is_negative() {
        0
    } else {
        u64::try_from(millis).unwrap_or(u64::MAX)
    }
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    let payload = serde_json::to_string_pretty(value)?;
    let tmp_name = format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("runtime_state")
    );
    let tmp_path = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(tmp_name);
    fs::write(&tmp_path, payload)
        .with_context(|| format!("Failed to write temp file {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "Failed to rename {} -> {}",
            tmp_path.display(),
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::engine::mock_engine_handle;
    use crate::core::events::{Event as EngineEvent, TurnOutcomeStatus};
    use std::time::{Duration, Instant};
    use tokio::sync::oneshot;
    use tokio::time::sleep;
    use uuid::Uuid;

    fn test_runtime_dir() -> PathBuf {
        std::env::temp_dir().join(format!("deepseek-runtime-threads-{}", Uuid::new_v4()))
    }

    fn test_manager_config(data_dir: PathBuf) -> RuntimeThreadManagerConfig {
        RuntimeThreadManagerConfig {
            data_dir,
            max_active_threads: 4,
        }
    }

    fn test_manager(data_dir: PathBuf) -> Result<RuntimeThreadManager> {
        RuntimeThreadManager::open(
            Config::default(),
            PathBuf::from("."),
            test_manager_config(data_dir),
        )
    }

    async fn install_mock_engine(
        manager: &RuntimeThreadManager,
        thread_id: &str,
    ) -> crate::core::engine::MockEngineHandle {
        let harness = mock_engine_handle();
        let mut active = manager.active.lock().await;
        active.engines.insert(
            thread_id.to_string(),
            ActiveThreadState {
                engine: harness.handle.clone(),
                active_turn: None,
            },
        );
        touch_lru(&mut active.lru, thread_id);
        harness
    }

    async fn wait_for_terminal_turn(
        manager: &RuntimeThreadManager,
        turn_id: &str,
        timeout: Duration,
    ) -> Result<TurnRecord> {
        let deadline = Instant::now() + timeout;
        loop {
            let turn = manager.store.load_turn(turn_id)?;
            if matches!(
                turn.status,
                RuntimeTurnStatus::Completed
                    | RuntimeTurnStatus::Failed
                    | RuntimeTurnStatus::Interrupted
                    | RuntimeTurnStatus::Canceled
            ) {
                return Ok(turn);
            }
            if Instant::now() >= deadline {
                bail!("Timed out waiting for turn {turn_id}");
            }
            sleep(Duration::from_millis(20)).await;
        }
    }

    #[test]
    fn enforce_lru_capacity_does_not_loop_when_all_threads_are_active() {
        let mut active = ActiveThreads::default();
        let harness_a = mock_engine_handle();
        let harness_b = mock_engine_handle();

        active.engines.insert(
            "thr_a".to_string(),
            ActiveThreadState {
                engine: harness_a.handle,
                active_turn: Some(ActiveTurnState {
                    turn_id: "turn_a".to_string(),
                    interrupt_requested: false,
                    auto_approve: true,
                    trust_mode: false,
                }),
            },
        );
        active.engines.insert(
            "thr_b".to_string(),
            ActiveThreadState {
                engine: harness_b.handle,
                active_turn: Some(ActiveTurnState {
                    turn_id: "turn_b".to_string(),
                    interrupt_requested: false,
                    auto_approve: true,
                    trust_mode: false,
                }),
            },
        );
        active.lru.push_back("thr_a".to_string());
        active.lru.push_back("thr_b".to_string());

        let evicted = enforce_lru_capacity(&mut active, 2);
        assert!(evicted.is_empty(), "no idle threads should be evicted");
        assert_eq!(active.engines.len(), 2);
        assert_eq!(active.lru.len(), 2);
    }

    #[tokio::test]
    async fn thread_lifecycle_persists_across_restart() -> Result<()> {
        let runtime_dir = test_runtime_dir();
        let manager = test_manager(runtime_dir.clone())?;
        let thread = manager
            .create_thread(CreateThreadRequest {
                model: None,
                workspace: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                archived: false,
                system_prompt: None,
            })
            .await?;

        let harness = install_mock_engine(&manager, &thread.id).await;
        let mut rx_op = harness.rx_op;
        let tx_event = harness.tx_event;
        tokio::spawn(async move {
            if matches!(rx_op.recv().await, Some(Op::SendMessage { .. })) {
                let _ = tx_event
                    .send(EngineEvent::TurnStarted {
                        turn_id: "engine_turn_1".to_string(),
                    })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::MessageStarted { index: 0 })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::MessageDelta {
                        index: 0,
                        content: "mock response".to_string(),
                    })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::MessageComplete { index: 0 })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::TurnComplete {
                        usage: Usage {
                            input_tokens: 10,
                            output_tokens: 12,
                            server_tool_use: None,
                        },
                        status: TurnOutcomeStatus::Completed,
                        error: None,
                    })
                    .await;
            }
        });

        let turn = manager
            .start_turn(
                &thread.id,
                StartTurnRequest {
                    prompt: "first prompt".to_string(),
                    input_summary: None,
                    model: None,
                    mode: None,
                    allow_shell: None,
                    trust_mode: None,
                    auto_approve: None,
                },
            )
            .await?;
        let completed = wait_for_terminal_turn(&manager, &turn.id, Duration::from_secs(2)).await?;
        assert_eq!(completed.status, RuntimeTurnStatus::Completed);

        drop(manager);

        let reopened = test_manager(runtime_dir)?;
        let detail = reopened.get_thread_detail(&thread.id).await?;
        assert_eq!(detail.thread.id, thread.id);
        assert_eq!(detail.turns.len(), 1);
        assert!(detail.latest_seq >= 1);
        assert!(!detail.items.is_empty());
        let events = reopened.events_since(&thread.id, None)?;
        assert!(
            events.iter().any(|ev| ev.event == "turn.completed"),
            "expected turn.completed event after restart"
        );
        Ok(())
    }

    #[tokio::test]
    async fn multi_turn_continuity_same_thread() -> Result<()> {
        let manager = test_manager(test_runtime_dir())?;
        let thread = manager
            .create_thread(CreateThreadRequest {
                model: None,
                workspace: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                archived: false,
                system_prompt: None,
            })
            .await?;

        let harness = install_mock_engine(&manager, &thread.id).await;
        let mut rx_op = harness.rx_op;
        let tx_event = harness.tx_event;
        tokio::spawn(async move {
            let mut turn_index = 0u8;
            while let Some(op) = rx_op.recv().await {
                if !matches!(op, Op::SendMessage { .. }) {
                    continue;
                }
                turn_index = turn_index.saturating_add(1);
                let _ = tx_event
                    .send(EngineEvent::TurnStarted {
                        turn_id: format!("engine_turn_{turn_index}"),
                    })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::MessageStarted { index: 0 })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::MessageDelta {
                        index: 0,
                        content: format!("reply {turn_index}"),
                    })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::MessageComplete { index: 0 })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::TurnComplete {
                        usage: Usage {
                            input_tokens: 5,
                            output_tokens: 5,
                            server_tool_use: None,
                        },
                        status: TurnOutcomeStatus::Completed,
                        error: None,
                    })
                    .await;
                if turn_index >= 2 {
                    break;
                }
            }
        });

        let turn_1 = manager
            .start_turn(
                &thread.id,
                StartTurnRequest {
                    prompt: "first".to_string(),
                    input_summary: None,
                    model: None,
                    mode: None,
                    allow_shell: None,
                    trust_mode: None,
                    auto_approve: None,
                },
            )
            .await?;
        let turn_1 = wait_for_terminal_turn(&manager, &turn_1.id, Duration::from_secs(2)).await?;
        assert_eq!(turn_1.status, RuntimeTurnStatus::Completed);

        let turn_2 = manager
            .start_turn(
                &thread.id,
                StartTurnRequest {
                    prompt: "second".to_string(),
                    input_summary: None,
                    model: None,
                    mode: None,
                    allow_shell: None,
                    trust_mode: None,
                    auto_approve: None,
                },
            )
            .await?;
        let turn_2 = wait_for_terminal_turn(&manager, &turn_2.id, Duration::from_secs(2)).await?;
        assert_eq!(turn_2.status, RuntimeTurnStatus::Completed);

        let detail = manager.get_thread_detail(&thread.id).await?;
        assert_eq!(
            detail.thread.latest_turn_id.as_deref(),
            Some(turn_2.id.as_str())
        );
        assert_eq!(detail.turns.len(), 2);
        assert!(detail.items.iter().any(|item| {
            item.kind == TurnItemKind::UserMessage && item.detail.as_deref() == Some("first")
        }));
        assert!(detail.items.iter().any(|item| {
            item.kind == TurnItemKind::UserMessage && item.detail.as_deref() == Some("second")
        }));

        let events = manager.events_since(&thread.id, None)?;
        let started = events
            .iter()
            .filter(|ev| ev.event == "turn.started")
            .count();
        let completed = events
            .iter()
            .filter(|ev| ev.event == "turn.completed")
            .count();
        assert_eq!(started, 2);
        assert_eq!(completed, 2);
        Ok(())
    }

    #[tokio::test]
    async fn interrupt_turn_marks_interrupted_after_cleanup() -> Result<()> {
        let manager = test_manager(test_runtime_dir())?;
        let thread = manager
            .create_thread(CreateThreadRequest {
                model: None,
                workspace: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                archived: false,
                system_prompt: None,
            })
            .await?;

        let harness = install_mock_engine(&manager, &thread.id).await;
        let mut rx_op = harness.rx_op;
        let tx_event = harness.tx_event;
        let cancel_token = harness.cancel_token;
        let cleanup_delay = Duration::from_millis(140);
        tokio::spawn(async move {
            if matches!(rx_op.recv().await, Some(Op::SendMessage { .. })) {
                let _ = tx_event
                    .send(EngineEvent::TurnStarted {
                        turn_id: "engine_turn_interrupt".to_string(),
                    })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::MessageStarted { index: 0 })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::MessageDelta {
                        index: 0,
                        content: "partial".to_string(),
                    })
                    .await;
                cancel_token.cancelled().await;
                sleep(cleanup_delay).await;
            }
        });

        let turn = manager
            .start_turn(
                &thread.id,
                StartTurnRequest {
                    prompt: "interrupt me".to_string(),
                    input_summary: None,
                    model: None,
                    mode: None,
                    allow_shell: None,
                    trust_mode: None,
                    auto_approve: None,
                },
            )
            .await?;

        sleep(Duration::from_millis(20)).await;
        let interrupted_at = Instant::now();
        let interrupt_result = manager.interrupt_turn(&thread.id, &turn.id).await?;
        assert_eq!(interrupt_result.status, RuntimeTurnStatus::InProgress);

        let final_turn = wait_for_terminal_turn(&manager, &turn.id, Duration::from_secs(3)).await?;
        assert_eq!(final_turn.status, RuntimeTurnStatus::Interrupted);
        assert!(
            interrupted_at.elapsed() >= cleanup_delay,
            "turn transitioned before cleanup finished"
        );

        let events = manager.events_since(&thread.id, None)?;
        let interrupt_seq = events
            .iter()
            .find(|ev| ev.event == "turn.interrupt_requested")
            .map(|ev| ev.seq)
            .context("missing turn.interrupt_requested event")?;
        let completed = events
            .iter()
            .find(|ev| ev.event == "turn.completed")
            .context("missing turn.completed event")?;
        assert!(completed.seq > interrupt_seq);
        assert_eq!(
            completed
                .payload
                .get("turn")
                .and_then(|turn| turn.get("status"))
                .and_then(Value::as_str),
            Some("interrupted")
        );
        Ok(())
    }

    #[tokio::test]
    async fn steer_turn_on_active_turn_records_item_and_event() -> Result<()> {
        let manager = test_manager(test_runtime_dir())?;
        let thread = manager
            .create_thread(CreateThreadRequest {
                model: None,
                workspace: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                archived: false,
                system_prompt: None,
            })
            .await?;

        let harness = install_mock_engine(&manager, &thread.id).await;
        let mut rx_op = harness.rx_op;
        let mut rx_steer = harness.rx_steer;
        let tx_event = harness.tx_event;
        let (steer_seen_tx, steer_seen_rx) = oneshot::channel::<String>();
        tokio::spawn(async move {
            if matches!(rx_op.recv().await, Some(Op::SendMessage { .. })) {
                let _ = tx_event
                    .send(EngineEvent::TurnStarted {
                        turn_id: "engine_turn_steer".to_string(),
                    })
                    .await;
                if let Some(steer) = rx_steer.recv().await {
                    let _ = steer_seen_tx.send(steer);
                }
                let _ = tx_event
                    .send(EngineEvent::MessageStarted { index: 0 })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::MessageDelta {
                        index: 0,
                        content: "steered response".to_string(),
                    })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::MessageComplete { index: 0 })
                    .await;
                let _ = tx_event
                    .send(EngineEvent::TurnComplete {
                        usage: Usage {
                            input_tokens: 8,
                            output_tokens: 9,
                            server_tool_use: None,
                        },
                        status: TurnOutcomeStatus::Completed,
                        error: None,
                    })
                    .await;
            }
        });

        let turn = manager
            .start_turn(
                &thread.id,
                StartTurnRequest {
                    prompt: "initial".to_string(),
                    input_summary: None,
                    model: None,
                    mode: None,
                    allow_shell: None,
                    trust_mode: None,
                    auto_approve: None,
                },
            )
            .await?;

        let steer_text = "add bullet list".to_string();
        let steered_turn = manager
            .steer_turn(
                &thread.id,
                &turn.id,
                SteerTurnRequest {
                    prompt: steer_text.clone(),
                },
            )
            .await?;
        assert_eq!(steered_turn.steer_count, 1);
        let observed_steer = steer_seen_rx
            .await
            .context("driver did not receive steer")?;
        assert_eq!(observed_steer, steer_text);

        let final_turn = wait_for_terminal_turn(&manager, &turn.id, Duration::from_secs(2)).await?;
        assert_eq!(final_turn.status, RuntimeTurnStatus::Completed);
        assert_eq!(final_turn.steer_count, 1);

        let events = manager.events_since(&thread.id, None)?;
        assert!(events.iter().any(|ev| ev.event == "turn.steered"));
        assert!(events.iter().any(|ev| {
            ev.event == "item.completed"
                && ev
                    .payload
                    .get("item")
                    .and_then(|item| item.get("detail"))
                    .and_then(Value::as_str)
                    == Some("add bullet list")
        }));
        Ok(())
    }

    #[tokio::test]
    async fn compaction_lifecycle_emits_item_events_for_auto_and_manual() -> Result<()> {
        let manager = test_manager(test_runtime_dir())?;
        let thread = manager
            .create_thread(CreateThreadRequest {
                model: None,
                workspace: None,
                mode: None,
                allow_shell: None,
                trust_mode: None,
                auto_approve: None,
                archived: false,
                system_prompt: None,
            })
            .await?;

        let harness = install_mock_engine(&manager, &thread.id).await;
        let mut rx_op = harness.rx_op;
        let tx_event = harness.tx_event;
        tokio::spawn(async move {
            let mut op_count = 0usize;
            while let Some(op) = rx_op.recv().await {
                match op {
                    Op::SendMessage { .. } => {
                        op_count = op_count.saturating_add(1);
                        let _ = tx_event
                            .send(EngineEvent::TurnStarted {
                                turn_id: "engine_turn_auto".to_string(),
                            })
                            .await;
                        let _ = tx_event
                            .send(EngineEvent::CompactionStarted {
                                id: "auto_compact_1".to_string(),
                                auto: true,
                                message: "auto compact begin".to_string(),
                            })
                            .await;
                        let _ = tx_event
                            .send(EngineEvent::CompactionCompleted {
                                id: "auto_compact_1".to_string(),
                                auto: true,
                                message: "auto compact done".to_string(),
                            })
                            .await;
                        let _ = tx_event
                            .send(EngineEvent::TurnComplete {
                                usage: Usage {
                                    input_tokens: 3,
                                    output_tokens: 3,
                                    server_tool_use: None,
                                },
                                status: TurnOutcomeStatus::Completed,
                                error: None,
                            })
                            .await;
                    }
                    Op::CompactContext => {
                        op_count = op_count.saturating_add(1);
                        let _ = tx_event
                            .send(EngineEvent::CompactionStarted {
                                id: "manual_compact_1".to_string(),
                                auto: false,
                                message: "manual compact begin".to_string(),
                            })
                            .await;
                        let _ = tx_event
                            .send(EngineEvent::CompactionCompleted {
                                id: "manual_compact_1".to_string(),
                                auto: false,
                                message: "manual compact done".to_string(),
                            })
                            .await;
                        let _ = tx_event
                            .send(EngineEvent::TurnComplete {
                                usage: Usage {
                                    input_tokens: 1,
                                    output_tokens: 1,
                                    server_tool_use: None,
                                },
                                status: TurnOutcomeStatus::Completed,
                                error: None,
                            })
                            .await;
                    }
                    _ => {}
                }
                if op_count >= 2 {
                    break;
                }
            }
        });

        let auto_turn = manager
            .start_turn(
                &thread.id,
                StartTurnRequest {
                    prompt: "trigger auto".to_string(),
                    input_summary: None,
                    model: None,
                    mode: None,
                    allow_shell: None,
                    trust_mode: None,
                    auto_approve: None,
                },
            )
            .await?;
        let auto_turn =
            wait_for_terminal_turn(&manager, &auto_turn.id, Duration::from_secs(2)).await?;
        assert_eq!(auto_turn.status, RuntimeTurnStatus::Completed);

        let manual_turn = manager
            .compact_thread(
                &thread.id,
                CompactThreadRequest {
                    reason: Some("manual request".to_string()),
                },
            )
            .await?;
        let manual_turn =
            wait_for_terminal_turn(&manager, &manual_turn.id, Duration::from_secs(2)).await?;
        assert_eq!(manual_turn.status, RuntimeTurnStatus::Completed);

        let events = manager.events_since(&thread.id, None)?;
        assert!(events.iter().any(|ev| {
            ev.event == "item.started"
                && ev
                    .payload
                    .get("item")
                    .and_then(|item| item.get("kind"))
                    .and_then(Value::as_str)
                    == Some("context_compaction")
                && ev.payload.get("auto").and_then(Value::as_bool) == Some(true)
        }));
        assert!(events.iter().any(|ev| {
            ev.event == "item.completed"
                && ev
                    .payload
                    .get("item")
                    .and_then(|item| item.get("kind"))
                    .and_then(Value::as_str)
                    == Some("context_compaction")
                && ev.payload.get("auto").and_then(Value::as_bool) == Some(false)
        }));
        Ok(())
    }

    #[test]
    fn summarize_text_truncates() {
        let out = summarize_text("abcdefghijklmnopqrstuvwxyz", 10);
        assert_eq!(out, "abcdefg...");
    }

    #[test]
    fn parse_mode_defaults_to_agent() {
        assert_eq!(parse_mode("unknown"), AppMode::Agent);
        assert_eq!(parse_mode("plan"), AppMode::Plan);
    }
}
