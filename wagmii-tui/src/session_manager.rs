//! Session management for resuming conversations.
//!
//! This module provides functionality for:
//! - Saving sessions to disk
//! - Listing previous sessions
//! - Resuming sessions by ID
//! - Managing session lifecycle

#![allow(dead_code)] // Public API - session persistence functions for future TUI integration

use crate::models::{ContentBlock, Message, SystemPrompt};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Maximum number of sessions to retain
const MAX_SESSIONS: usize = 50;
const CURRENT_SESSION_SCHEMA_VERSION: u32 = 1;
const CURRENT_QUEUE_SCHEMA_VERSION: u32 = 1;

const fn default_session_schema_version() -> u32 {
    CURRENT_SESSION_SCHEMA_VERSION
}

const fn default_queue_schema_version() -> u32 {
    CURRENT_QUEUE_SCHEMA_VERSION
}

/// Persisted queued message for offline/degraded mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedSessionMessage {
    pub display: String,
    #[serde(default)]
    pub skill_instruction: Option<String>,
}

/// Persisted queue state for recovery after restart/crash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineQueueState {
    #[serde(default = "default_queue_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub messages: Vec<QueuedSessionMessage>,
    #[serde(default)]
    pub draft: Option<QueuedSessionMessage>,
}

impl Default for OfflineQueueState {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_QUEUE_SCHEMA_VERSION,
            messages: Vec::new(),
            draft: None,
        }
    }
}

/// Session metadata stored with each saved session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Unique session identifier
    pub id: String,
    /// Human-readable title (derived from first message)
    pub title: String,
    /// When the session was created
    pub created_at: DateTime<Utc>,
    /// When the session was last updated
    pub updated_at: DateTime<Utc>,
    /// Number of messages in the session
    pub message_count: usize,
    /// Total tokens used
    pub total_tokens: u64,
    /// Model used for the session
    pub model: String,
    /// Workspace directory
    pub workspace: PathBuf,
    /// Optional mode label (agent/plan/etc.)
    #[serde(default)]
    pub mode: Option<String>,
}

/// A saved session containing full conversation history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    /// Schema version for migration compatibility
    #[serde(default = "default_session_schema_version")]
    pub schema_version: u32,
    /// Session metadata
    pub metadata: SessionMetadata,
    /// Conversation messages
    pub messages: Vec<Message>,
    /// System prompt if any
    pub system_prompt: Option<String>,
}

/// Manager for session persistence operations
pub struct SessionManager {
    /// Directory where sessions are stored
    sessions_dir: PathBuf,
}

impl SessionManager {
    fn validated_session_path(&self, id: &str) -> std::io::Result<PathBuf> {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Session id cannot be empty",
            ));
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid session id '{id}'"),
            ));
        }
        Ok(self.sessions_dir.join(format!("{trimmed}.json")))
    }

    /// Create a new `SessionManager` with the specified sessions directory
    pub fn new(sessions_dir: PathBuf) -> std::io::Result<Self> {
        // Ensure the sessions directory exists
        fs::create_dir_all(&sessions_dir)?;
        Ok(Self { sessions_dir })
    }

    /// Create a `SessionManager` using the default location (~/.wagmii/sessions)
    pub fn default_location() -> std::io::Result<Self> {
        Self::new(default_sessions_dir()?)
    }

    /// Save a session to disk using atomic write (temp file + rename).
    pub fn save_session(&self, session: &SavedSession) -> std::io::Result<PathBuf> {
        let path = self.validated_session_path(&session.metadata.id)?;

        let content = serde_json::to_string_pretty(session)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Atomic write: write to temp file then rename to avoid corruption
        let tmp_filename = format!(".{}.tmp", session.metadata.id.trim());
        let tmp_path = self.sessions_dir.join(&tmp_filename);
        fs::write(&tmp_path, &content)?;
        fs::rename(&tmp_path, &path)?;

        // Clean up old sessions if we have too many
        self.cleanup_old_sessions()?;

        Ok(path)
    }

    /// Save a crash-recovery checkpoint for in-flight turns.
    pub fn save_checkpoint(&self, session: &SavedSession) -> std::io::Result<PathBuf> {
        let checkpoints = self.sessions_dir.join("checkpoints");
        fs::create_dir_all(&checkpoints)?;
        let path = checkpoints.join("latest.json");
        let content = serde_json::to_string_pretty(session)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp_path = checkpoints.join(".latest.tmp");
        fs::write(&tmp_path, &content)?;
        fs::rename(&tmp_path, &path)?;
        Ok(path)
    }

    /// Load the most recent crash-recovery checkpoint if present.
    pub fn load_checkpoint(&self) -> std::io::Result<Option<SavedSession>> {
        let path = self.sessions_dir.join("checkpoints").join("latest.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        let session: SavedSession = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if session.schema_version > CURRENT_SESSION_SCHEMA_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Checkpoint schema v{} is newer than supported v{}",
                    session.schema_version, CURRENT_SESSION_SCHEMA_VERSION
                ),
            ));
        }
        Ok(Some(session))
    }

    /// Clear any crash-recovery checkpoint.
    pub fn clear_checkpoint(&self) -> std::io::Result<()> {
        let path = self.sessions_dir.join("checkpoints").join("latest.json");
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Save offline queue state (queued + draft messages).
    pub fn save_offline_queue_state(&self, state: &OfflineQueueState) -> std::io::Result<PathBuf> {
        let checkpoints = self.sessions_dir.join("checkpoints");
        fs::create_dir_all(&checkpoints)?;
        let path = checkpoints.join("offline_queue.json");
        let content = serde_json::to_string_pretty(state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp_path = checkpoints.join(".offline_queue.tmp");
        fs::write(&tmp_path, &content)?;
        fs::rename(&tmp_path, &path)?;
        Ok(path)
    }

    /// Load offline queue state if present.
    pub fn load_offline_queue_state(&self) -> std::io::Result<Option<OfflineQueueState>> {
        let path = self
            .sessions_dir
            .join("checkpoints")
            .join("offline_queue.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        let state: OfflineQueueState = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if state.schema_version > CURRENT_QUEUE_SCHEMA_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Offline queue schema v{} is newer than supported v{}",
                    state.schema_version, CURRENT_QUEUE_SCHEMA_VERSION
                ),
            ));
        }
        Ok(Some(state))
    }

    /// Remove persisted offline queue state.
    pub fn clear_offline_queue_state(&self) -> std::io::Result<()> {
        let path = self
            .sessions_dir
            .join("checkpoints")
            .join("offline_queue.json");
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Load a session by ID
    pub fn load_session(&self, id: &str) -> std::io::Result<SavedSession> {
        let path = self.validated_session_path(id)?;

        let content = fs::read_to_string(&path)?;
        let session: SavedSession = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if session.schema_version > CURRENT_SESSION_SCHEMA_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Session schema v{} is newer than supported v{}",
                    session.schema_version, CURRENT_SESSION_SCHEMA_VERSION
                ),
            ));
        }

        Ok(session)
    }

    /// Load a session by partial ID prefix
    pub fn load_session_by_prefix(&self, prefix: &str) -> std::io::Result<SavedSession> {
        let sessions = self.list_sessions()?;

        let matches: Vec<_> = sessions
            .into_iter()
            .filter(|s| s.id.starts_with(prefix))
            .collect();

        match matches.len() {
            0 => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("No session found with prefix: {prefix}"),
            )),
            1 => self.load_session(&matches[0].id),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Ambiguous prefix '{}' matches {} sessions",
                    prefix,
                    matches.len()
                ),
            )),
        }
    }

    /// List all saved sessions, sorted by most recently updated
    pub fn list_sessions(&self) -> std::io::Result<Vec<SessionMetadata>> {
        let mut sessions = Vec::new();

        for entry in fs::read_dir(&self.sessions_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(session) = Self::load_session_metadata(&path)
            {
                sessions.push(session);
            }
        }

        // Sort by updated_at descending (most recent first)
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(sessions)
    }

    /// Load only the metadata from a session file (faster than loading full session)
    fn load_session_metadata(path: &Path) -> std::io::Result<SessionMetadata> {
        #[derive(Deserialize)]
        struct SavedSessionMetadata {
            metadata: SessionMetadata,
        }

        let file = fs::File::open(path)?;
        let session: SavedSessionMetadata = serde_json::from_reader(file)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(session.metadata)
    }

    /// Delete a session by ID
    pub fn delete_session(&self, id: &str) -> std::io::Result<()> {
        let path = self.validated_session_path(id)?;
        fs::remove_file(path)
    }

    /// Clean up old sessions to stay within `MAX_SESSIONS` limit
    fn cleanup_old_sessions(&self) -> std::io::Result<()> {
        let sessions = self.list_sessions()?;

        if sessions.len() > MAX_SESSIONS {
            // Delete oldest sessions
            for session in sessions.iter().skip(MAX_SESSIONS) {
                let _ = self.delete_session(&session.id);
            }
        }

        Ok(())
    }

    /// Get the most recent session
    pub fn get_latest_session(&self) -> std::io::Result<Option<SessionMetadata>> {
        let sessions = self.list_sessions()?;
        Ok(sessions.into_iter().next())
    }

    /// Search sessions by title
    pub fn search_sessions(&self, query: &str) -> std::io::Result<Vec<SessionMetadata>> {
        let query_lower = query.to_lowercase();
        let sessions = self.list_sessions()?;

        Ok(sessions
            .into_iter()
            .filter(|s| s.title.to_lowercase().contains(&query_lower))
            .collect())
    }
}

/// Resolve the default session directory path (`~/.wagmii/sessions`).
pub fn default_sessions_dir() -> std::io::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Home directory not found")
    })?;
    Ok(home.join(".wagmii").join("sessions"))
}

/// Create a new `SavedSession` from conversation state
pub fn create_saved_session(
    messages: &[Message],
    model: &str,
    workspace: &Path,
    total_tokens: u64,
    system_prompt: Option<&SystemPrompt>,
) -> SavedSession {
    create_saved_session_with_mode(
        messages,
        model,
        workspace,
        total_tokens,
        system_prompt,
        None,
    )
}

/// Create a new `SavedSession` from conversation state with optional mode label
pub fn create_saved_session_with_mode(
    messages: &[Message],
    model: &str,
    workspace: &Path,
    total_tokens: u64,
    system_prompt: Option<&SystemPrompt>,
    mode: Option<&str>,
) -> SavedSession {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();

    // Generate title from first user message
    let title = messages
        .iter()
        .find(|m| m.role == "user")
        .and_then(|m| {
            m.content.iter().find_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(truncate_title(text, 50)),
                _ => None,
            })
        })
        .unwrap_or_else(|| "New Session".to_string());

    SavedSession {
        schema_version: CURRENT_SESSION_SCHEMA_VERSION,
        metadata: SessionMetadata {
            id,
            title,
            created_at: now,
            updated_at: now,
            message_count: messages.len(),
            total_tokens,
            model: model.to_string(),
            workspace: workspace.to_path_buf(),
            mode: mode.map(str::to_string),
        },
        messages: messages.to_vec(),
        system_prompt: system_prompt_to_string(system_prompt),
    }
}

/// Update an existing session with new messages
pub fn update_session(
    mut session: SavedSession,
    messages: &[Message],
    total_tokens: u64,
    system_prompt: Option<&SystemPrompt>,
) -> SavedSession {
    session.schema_version = CURRENT_SESSION_SCHEMA_VERSION;
    session.messages = messages.to_vec();
    session.metadata.updated_at = Utc::now();
    session.metadata.message_count = messages.len();
    session.metadata.total_tokens = total_tokens;
    session.system_prompt = system_prompt_to_string(system_prompt).or(session.system_prompt);
    session
}

fn system_prompt_to_string(system_prompt: Option<&SystemPrompt>) -> Option<String> {
    match system_prompt {
        Some(SystemPrompt::Text(text)) => Some(text.clone()),
        Some(SystemPrompt::Blocks(blocks)) => Some(
            blocks
                .iter()
                .map(|b| b.text.clone())
                .collect::<Vec<_>>()
                .join("\n\n---\n\n"),
        ),
        None => None,
    }
}

/// Truncate a string to create a title (character-safe for UTF-8)
fn truncate_title(s: &str, max_len: usize) -> String {
    let s = s.trim();
    let first_line = s.lines().next().unwrap_or(s);

    let char_count = first_line.chars().count();
    if char_count <= max_len {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(max_len - 3).collect();
        format!("{truncated}...")
    }
}

/// Format a session for display in a picker
pub fn format_session_line(meta: &SessionMetadata) -> String {
    let age = format_age(&meta.updated_at);
    let truncated_title = truncate_title(&meta.title, 40);

    format!(
        "{} | {} | {} msgs | {}",
        &meta.id[..8],
        truncated_title,
        meta.message_count,
        age
    )
}

/// Format a datetime as relative age
fn format_age(dt: &DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(*dt);

    if duration.num_minutes() < 1 {
        "just now".to_string()
    } else if duration.num_hours() < 1 {
        format!("{}m ago", duration.num_minutes())
    } else if duration.num_days() < 1 {
        format!("{}h ago", duration.num_hours())
    } else if duration.num_weeks() < 1 {
        format!("{}d ago", duration.num_days())
    } else {
        format!("{}w ago", duration.num_weeks())
    }
}

// === Unit Tests ===

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ContentBlock;
    use std::fs;
    use tempfile::tempdir;

    fn make_test_message(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
        }
    }

    #[test]
    fn test_session_manager_new() {
        let tmp = tempdir().expect("tempdir");
        let manager = SessionManager::new(tmp.path().join("sessions")).expect("new");
        assert!(tmp.path().join("sessions").exists());
        let _ = manager;
    }

    #[test]
    fn test_save_and_load_session() {
        let tmp = tempdir().expect("tempdir");
        let manager = SessionManager::new(tmp.path().join("sessions")).expect("new");

        let messages = vec![
            make_test_message("user", "Hello!"),
            make_test_message("assistant", "Hi there!"),
        ];

        let session = create_saved_session(&messages, "test-model", tmp.path(), 100, None);
        let session_id = session.metadata.id.clone();

        manager.save_session(&session).expect("save");

        let loaded = manager.load_session(&session_id).expect("load");
        assert_eq!(loaded.metadata.id, session_id);
        assert_eq!(loaded.messages.len(), 2);
    }

    #[test]
    fn test_list_sessions() {
        let tmp = tempdir().expect("tempdir");
        let manager = SessionManager::new(tmp.path().join("sessions")).expect("new");

        // Create a few sessions
        for i in 0..3 {
            let messages = vec![make_test_message("user", &format!("Session {i}"))];
            let session = create_saved_session(&messages, "test-model", tmp.path(), 100, None);
            manager.save_session(&session).expect("save");
        }

        let sessions = manager.list_sessions().expect("list");
        assert_eq!(sessions.len(), 3);
    }

    #[test]
    fn test_load_by_prefix() {
        let tmp = tempdir().expect("tempdir");
        let manager = SessionManager::new(tmp.path().join("sessions")).expect("new");

        let messages = vec![make_test_message("user", "Test session")];
        let session = create_saved_session(&messages, "test-model", tmp.path(), 100, None);
        let prefix = session.metadata.id[..8].to_string();
        manager.save_session(&session).expect("save");

        let loaded = manager.load_session_by_prefix(&prefix).expect("load");
        assert_eq!(loaded.messages.len(), 1);
    }

    #[test]
    fn test_delete_session() {
        let tmp = tempdir().expect("tempdir");
        let manager = SessionManager::new(tmp.path().join("sessions")).expect("new");

        let messages = vec![make_test_message("user", "To be deleted")];
        let session = create_saved_session(&messages, "test-model", tmp.path(), 100, None);
        let session_id = session.metadata.id.clone();

        manager.save_session(&session).expect("save");
        assert!(manager.load_session(&session_id).is_ok());

        manager.delete_session(&session_id).expect("delete");
        assert!(manager.load_session(&session_id).is_err());
    }

    #[test]
    fn test_session_id_rejects_invalid_characters() {
        let tmp = tempdir().expect("tempdir");
        let manager = SessionManager::new(tmp.path().join("sessions")).expect("new");

        let err = manager
            .load_session("../outside")
            .expect_err("invalid id should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        let err = manager
            .delete_session("sess bad")
            .expect_err("invalid id should fail");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_truncate_title() {
        assert_eq!(truncate_title("Short", 50), "Short");
        assert_eq!(
            truncate_title("This is a very long title that should be truncated", 20),
            "This is a very lo..."
        );
        assert_eq!(truncate_title("Line 1\nLine 2", 50), "Line 1");
    }

    #[test]
    fn test_format_age() {
        let now = Utc::now();
        assert_eq!(format_age(&now), "just now");

        let hour_ago = now - chrono::Duration::hours(2);
        assert_eq!(format_age(&hour_ago), "2h ago");

        let day_ago = now - chrono::Duration::days(3);
        assert_eq!(format_age(&day_ago), "3d ago");
    }

    #[test]
    fn test_update_session() {
        let tmp = tempdir().expect("tempdir");

        let messages = vec![make_test_message("user", "Hello")];
        let session = create_saved_session(&messages, "test-model", tmp.path(), 50, None);

        let new_messages = vec![
            make_test_message("user", "Hello"),
            make_test_message("assistant", "Hi!"),
        ];

        let updated = update_session(session, &new_messages, 100, None);
        assert_eq!(updated.messages.len(), 2);
        assert_eq!(updated.metadata.total_tokens, 100);
    }

    #[test]
    fn test_checkpoint_round_trip_and_clear() {
        let tmp = tempdir().expect("tempdir");
        let manager = SessionManager::new(tmp.path().join("sessions")).expect("new");
        let messages = vec![make_test_message("user", "checkpoint me")];
        let session = create_saved_session(&messages, "test-model", tmp.path(), 12, None);

        manager.save_checkpoint(&session).expect("save checkpoint");
        let loaded = manager
            .load_checkpoint()
            .expect("load checkpoint")
            .expect("checkpoint exists");
        assert_eq!(loaded.metadata.id, session.metadata.id);

        manager.clear_checkpoint().expect("clear checkpoint");
        assert!(
            manager
                .load_checkpoint()
                .expect("load checkpoint")
                .is_none()
        );
    }

    #[test]
    fn test_offline_queue_round_trip_and_clear() {
        let tmp = tempdir().expect("tempdir");
        let manager = SessionManager::new(tmp.path().join("sessions")).expect("new");

        let state = OfflineQueueState {
            messages: vec![QueuedSessionMessage {
                display: "queued message".to_string(),
                skill_instruction: Some("Use skill".to_string()),
            }],
            draft: Some(QueuedSessionMessage {
                display: "draft message".to_string(),
                skill_instruction: None,
            }),
            ..OfflineQueueState::default()
        };

        manager
            .save_offline_queue_state(&state)
            .expect("save queue state");
        let loaded = manager
            .load_offline_queue_state()
            .expect("load queue state")
            .expect("queue state exists");
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].display, "queued message");
        assert!(loaded.draft.is_some());

        manager
            .clear_offline_queue_state()
            .expect("clear queue state");
        assert!(
            manager
                .load_offline_queue_state()
                .expect("load queue state")
                .is_none()
        );
    }

    #[test]
    fn test_checkpoint_rejects_newer_schema() {
        let tmp = tempdir().expect("tempdir");
        let manager = SessionManager::new(tmp.path().join("sessions")).expect("new");
        let checkpoints = tmp.path().join("sessions").join("checkpoints");
        fs::create_dir_all(&checkpoints).expect("create checkpoints dir");
        let path = checkpoints.join("latest.json");
        fs::write(
            &path,
            r#"{
                "schema_version": 999,
                "metadata": {
                    "id": "sid",
                    "title": "bad",
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z",
                    "message_count": 0,
                    "total_tokens": 0,
                    "model": "m",
                    "workspace": "/tmp",
                    "mode": null
                },
                "messages": [],
                "system_prompt": null
            }"#,
        )
        .expect("write checkpoint");

        let err = manager.load_checkpoint().expect_err("should reject schema");
        assert!(err.to_string().contains("newer than supported"));
    }
}
