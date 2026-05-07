//! Structured user-level memory file with decay scoring and per-entry management.
//!
//! v0.8.8 ships an MVP that lets the user keep persistent personal
//! notes the model sees on every turn. v0.9.0 upgrades the format
//! from flat markdown to JSONL (one JSON entry per line), adding
//! per-entry IDs, timestamps, access tracking, and decay scoring.
//!
//! - **Storage**: `~/.deepseek/memory.jsonl` (path is configurable via
//!   `memory_path` in `config.toml` and `DEEPSEEK_MEMORY_PATH` env).
//! - **`# foo`** typed in the composer appends `foo` as a new entry
//!   with an auto-generated ID — fast capture without leaving the TUI.
//! - **`/memory`** shows the resolved file path and current contents.
//! - **`/memory forget <id>`** removes a specific entry by ID.
//! - **`/memory list`** lists all entries with their age and decay score.
//! - **`remember` tool** lets the model itself append an entry when it
//!   notices a durable preference or convention.
//!
//! ## Decay scoring
//!
//! Each entry tracks `last_accessed`. The decay score is computed as:
//!
//! ```text
//! score = exp(-hours_since_last_access / decay_halflife_hours)
//! ```
//!
//! Default half-life: 168 hours (7 days). Entries accessed recently
//! score near 1.0; untouched entries approach 0.0. The system prompt
//! `<user_memory>` block sorts entries by descending score so the
//! model sees the most relevant notes first.
//!
//! Default behavior is **opt-in**: load + use the memory file only when
//! `[memory] enabled = true` in `config.toml` or `DEEPSEEK_MEMORY=on`.
//! That keeps existing users on zero-overhead behavior and makes the
//! feature explicit.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Maximum size of the user memory file. Larger files are loaded but the
/// `<user_memory>` block carries a "(truncated)" marker so the user knows
/// the model only saw a slice. Mirrors `project_context::MAX_CONTEXT_SIZE`.
const MAX_MEMORY_SIZE: usize = 100 * 1024;

/// Decay half-life in hours (default: 7 days). Used in `decay_score()`.
const DECAY_HALFLIFE_HOURS: f64 = 168.0;

/// A single memory entry persisted as one JSONL line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique ID (e.g. `mem_a1b2c3d4`).
    pub id: String,
    /// The note content.
    pub note: String,
    /// RFC 3339 timestamp of creation.
    pub created: String,
    /// RFC 3339 timestamp of last access.
    pub last_accessed: String,
    /// Number of times this entry has been loaded into the system prompt.
    pub access_count: u64,
}

impl MemoryEntry {
    /// Create a new entry with a generated ID and current timestamps.
    #[must_use]
    pub fn new(note: &str) -> Self {
        let id = format!("mem_{}", &Uuid::new_v4().to_string()[..8]);
        let now = Utc::now().to_rfc3339();
        Self {
            id,
            note: note.trim_start_matches('#').trim().to_string(),
            created: now.clone(),
            last_accessed: now,
            access_count: 0,
        }
    }

    /// Compute the decay score (0.0 to 1.0) based on hours since last access.
    /// An entry accessed just now scores ~1.0; an entry untouched for one
    /// half-life scores ~0.5; untouched for 2 * half-life scores ~0.25.
    #[must_use]
    pub fn decay_score(&self) -> f64 {
        let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&self.last_accessed) else {
            return 0.0;
        };
        let elapsed = Utc::now().signed_duration_since(parsed);
        let hours = elapsed.num_hours() as f64;
        if hours <= 0.0 {
            return 1.0;
        }
        (-hours / DECAY_HALFLIFE_HOURS).exp()
    }

    /// Human-readable age string (e.g. "2h", "3d", "2w").
    #[must_use]
    pub fn age_string(&self) -> String {
        let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&self.created) else {
            return "unknown".to_string();
        };
        let elapsed = Utc::now().signed_duration_since(parsed);
        let hours = elapsed.num_hours();
        let days = elapsed.num_days();
        let weeks = days / 7;
        if weeks > 0 {
            format!("{weeks}w")
        } else if days > 0 {
            format!("{days}d")
        } else if hours > 0 {
            format!("{hours}h")
        } else {
            let mins = elapsed.num_minutes().max(0);
            format!("{mins}m")
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Generate a short memory entry ID.
fn generate_id() -> String {
    format!("mem_{}", &Uuid::new_v4().to_string()[..8])
}

/// Parse an RFC 3339 timestamp string, falling back to now.
fn parse_or_now(ts: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| Utc::now())
}

/// Resolve the JSONL path: if `path` ends with `.md`, substitute `.jsonl`.
/// This provides seamless migration from the old markdown format.
fn jsonl_path(path: &Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if s.ends_with(".md") {
        let mut p = s.to_string();
        p.truncate(p.len() - 3);
        p.push_str(".jsonl");
        std::path::PathBuf::from(p)
    } else {
        path.to_path_buf()
    }
}

/// Resolve the legacy markdown path for backward-compat reads.
fn legacy_md_path(path: &Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if s.ends_with(".md") {
        path.to_path_buf()
    } else {
        let mut p = s.to_string();
        p.push_str(".md");
        std::path::PathBuf::from(p)
    }
}

/// Read entries from a JSONL file. Returns an empty vec on missing file.
fn load_entries(path: &Path) -> Vec<MemoryEntry> {
    let jpath = jsonl_path(path);

    // Try JSONL first.
    if let Ok(file) = fs::File::open(&jpath) {
        let reader = BufReader::new(file);
        let entries: Vec<MemoryEntry> = reader
            .lines()
            .filter_map(|line| {
                let line = line.ok()?;
                if line.trim().is_empty() {
                    return None;
                }
                serde_json::from_str(&line).ok()
            })
            .collect();
        if !entries.is_empty() {
            return entries;
        }
    }

    // Fall back: try legacy markdown format for migration.
    let md_path = legacy_md_path(path);
    let content = fs::read_to_string(&md_path).unwrap_or_default();
    let lines: Vec<&str> = content.lines().filter(|l| l.trim().starts_with("- (")).collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Convert each markdown bullet to a MemoryEntry.
    let now_ts = Utc::now().to_rfc3339();
    let entries: Vec<MemoryEntry> = lines
        .into_iter()
        .filter_map(|line| {
            // Format: `- (2026-05-03 22:14 UTC) note text`
            let rest = line.trim_start_matches("- (").trim();
            let (ts_part, note) = rest.split_once(')').unwrap_or(("", rest));
            let ts_part = ts_part.trim();
            let note = note.trim();

            if note.is_empty() {
                return None;
            }

            // Convert the old timestamp format to RFC 3339.
            let rfc3339 = if let Ok(dt) =
                chrono::NaiveDateTime::parse_from_str(ts_part, "%Y-%m-%d %H:%M UTC")
            {
                format!("{}Z", dt.and_utc().to_rfc3339())
            } else {
                now_ts.clone()
            };

            Some(MemoryEntry {
                id: generate_id(),
                note: note.to_string(),
                created: rfc3339.clone(),
                last_accessed: rfc3339,
                access_count: 0,
            })
        })
        .collect();

    // Migrate: write the markdown content as JSONL so next load is fast.
    if !entries.is_empty() {
        if let Some(parent) = jpath.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(file) = fs::File::create(&jpath) {
            let mut writer = io::BufWriter::new(file);
            for entry in &entries {
                if let Ok(line) = serde_json::to_string(entry) {
                    let _ = writeln!(writer, "{line}");
                }
            }
        }
    }

    entries
}

/// Write entries to a JSONL file, replacing existing content.
fn save_entries(path: &Path, entries: &[MemoryEntry]) -> io::Result<()> {
    let jpath = jsonl_path(path);
    if let Some(parent) = jpath.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(&jpath)?;
    let mut writer = io::BufWriter::new(file);
    for entry in entries {
        let line = serde_json::to_string(entry).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, e.to_string())
        })?;
        writeln!(writer, "{line}")?;
    }
    writer.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Read the user memory file at `path`, returning `None` when the file
/// doesn't exist or is empty after trimming.
#[must_use]
pub fn load(path: &Path) -> Option<String> {
    let entries = load_entries(path);
    if entries.is_empty() {
        return None;
    }
    // Format as markdown for the system prompt block.
    let mut lines: Vec<String> = Vec::new();
    for entry in &entries {
        lines.push(format!("- ({}) {}", entry.created, entry.note));
    }
    Some(lines.join("\n"))
}

/// Wrap memory content in a `<user_memory>` block ready to prepend to the
/// system prompt. The `source` value is rendered verbatim into a
/// `source="…"` attribute — pass the path so the model can see where the
/// memory came from. Returns `None` for empty content.
#[must_use]
pub fn as_system_block(content: &str, source: &Path) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let display = source.display();
    let payload = if content.len() > MAX_MEMORY_SIZE {
        let mut head = content[..MAX_MEMORY_SIZE].to_string();
        head.push_str("\n…(truncated, raise [memory].max_size or trim memory file)");
        head
    } else {
        trimmed.to_string()
    };

    Some(format!(
        "<user_memory source=\"{display}\">\n{payload}\n</user_memory>"
    ))
}

/// Compose the `<user_memory>` block for the system prompt, honouring the
/// opt-in toggle. Returns `None` when the feature is disabled or the file
/// is missing / empty so the caller doesn't have to check both conditions.
///
/// This function also **touches** every entry (updates `last_accessed`) when
/// the block is composed, so the decay score reflects that the model is
/// actively seeing these notes. Callers that hold a `&Config` should pass
/// `config.memory_enabled()` and `config.memory_path()` directly.
#[must_use]
pub fn compose_block(enabled: bool, path: &Path) -> Option<String> {
    if !enabled {
        return None;
    }
    let content = load(path)?;
    // Touch entries: update last_accessed and save.
    let entries = load_entries(path);
    if !entries.is_empty() {
        let now = Utc::now().to_rfc3339();
        let updated: Vec<MemoryEntry> = entries
            .into_iter()
            .map(|mut e| {
                e.last_accessed = now.clone();
                e.access_count = e.access_count.saturating_add(1);
                e
            })
            .collect();
        let _ = save_entries(path, &updated);
    }
    as_system_block(&content, path)
}

/// Append `entry` to the memory file at `path`, creating it (and its
/// parent directory) if needed. The entry gets a unique ID and timestamp.
/// Returns the generated ID on success.
pub fn append_entry(path: &Path, entry: &str) -> io::Result<String> {
    let trimmed = entry.trim_start_matches('#').trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "memory entry is empty after stripping `#` prefix",
        ));
    }

    // Load existing entries, add new one, save.
    let mut entries = load_entries(path);
    let mem_entry = MemoryEntry::new(trimmed);
    let id = mem_entry.id.clone();
    entries.push(mem_entry);
    save_entries(path, &entries)?;
    Ok(id)
}

/// Forget (delete) a memory entry by ID. Returns the removed note text on
/// success, or a descriptive error if the ID wasn't found.
#[derive(Debug)]
pub enum ForgetError {
    NotFound(String),
    Io(io::Error),
}

impl std::fmt::Display for ForgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "no memory entry with id `{id}`"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for ForgetError {}

/// Delete an entry by ID. Returns the note text of the removed entry.
pub fn forget_entry(path: &Path, id: &str) -> Result<String, ForgetError> {
    let entries = load_entries(path);
    let mut removed = None;
    let kept: Vec<MemoryEntry> = entries
        .into_iter()
        .filter(|e| {
            if e.id == id {
                removed = Some(e.note.clone());
                false
            } else {
                true
            }
        })
        .collect();

    match removed {
        Some(note) => {
            save_entries(path, &kept).map_err(ForgetError::Io)?;
            Ok(note)
        }
        None => Err(ForgetError::NotFound(id.to_string())),
    }
}

/// List all entries with their decay scores, sorted by score descending
/// (most relevant first). Each entry has its ID, note, age, and decay score.
#[must_use]
pub fn list_entries(path: &Path) -> Vec<(MemoryEntry, f64)> {
    let mut entries: Vec<(MemoryEntry, f64)> = load_entries(path)
        .into_iter()
        .map(|e| {
            let score = e.decay_score();
            (e, score)
        })
        .collect();
    // Sort by score descending (most-relevant first).
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    entries
}

/// Touch a specific entry by ID: update its `last_accessed` and increment
/// its `access_count`. No-op when the ID is not found.
pub fn touch_entry(path: &Path, id: &str) {
    let entries = load_entries(path);
    let now = Utc::now().to_rfc3339();
    let updated: Vec<MemoryEntry> = entries
        .into_iter()
        .map(|mut e| {
            if e.id == id {
                e.last_accessed = now.clone();
                e.access_count = e.access_count.saturating_add(1);
            }
            e
        })
        .collect();
    let _ = save_entries(path, &updated);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_returns_none_for_missing_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("never-existed.jsonl");
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_returns_none_for_empty_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        fs::write(&path, "").unwrap();
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_returns_content_for_real_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        let entry = MemoryEntry {
            id: "mem_test".to_string(),
            note: "remember the milk".to_string(),
            created: Utc::now().to_rfc3339(),
            last_accessed: Utc::now().to_rfc3339(),
            access_count: 1,
        };
        let line = serde_json::to_string(&entry).unwrap();
        fs::write(&path, &line).unwrap();
        let content = load(&path).unwrap();
        assert!(content.contains("remember the milk"));
    }

    #[test]
    fn load_migrates_from_markdown() {
        let tmp = tempdir().unwrap();
        let md_path = tmp.path().join("memory.md");
        let jpath = jsonl_path(&md_path);
        // Write a legacy markdown file.
        fs::write(
            &md_path,
            "- (2026-05-03 22:14 UTC) prefer pytest\n- (2026-05-04 09:02 UTC) use 4-space indent\n",
        )
        .unwrap();

        // Load via jsonl_path — should migrate.
        let content = load(&md_path).unwrap();
        assert!(content.contains("prefer pytest"));
        assert!(content.contains("use 4-space indent"));

        // Confirm the JSONL file was created.
        assert!(jpath.exists(), "JSONL migration file should exist");

        // Confirm the migrated JSONL contains valid entries.
        let entries = load_entries(&md_path);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].note, "prefer pytest");
        assert_eq!(entries[1].note, "use 4-space indent");
    }

    #[test]
    fn as_system_block_produces_xml_wrapper() {
        let block = as_system_block("note 1", Path::new("/tmp/m.jsonl")).unwrap();
        assert!(block.contains("<user_memory source=\"/tmp/m.jsonl\">"));
        assert!(block.contains("note 1"));
        assert!(block.ends_with("</user_memory>"));
    }

    #[test]
    fn as_system_block_returns_none_for_empty_content() {
        assert!(as_system_block("   ", Path::new("/tmp/m.jsonl")).is_none());
    }

    #[test]
    fn as_system_block_truncates_oversize_input() {
        let big = "x".repeat(MAX_MEMORY_SIZE + 100);
        let block = as_system_block(&big, Path::new("/tmp/m.jsonl")).unwrap();
        assert!(block.contains("(truncated"));
    }

    #[test]
    fn append_entry_creates_file_and_writes_one_entry() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        let id = append_entry(&path, "# remember the milk").unwrap();
        assert!(id.starts_with("mem_"), "id should start with mem_: {id}");

        let entries = load_entries(&path);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].note, "remember the milk");
        assert_eq!(entries[0].id, id);
        assert_eq!(entries[0].access_count, 0);
    }

    #[test]
    fn append_entry_appends_subsequent_entries() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        append_entry(&path, "# first").unwrap();
        append_entry(&path, "second").unwrap();
        let entries = load_entries(&path);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].note, "first");
        assert_eq!(entries[1].note, "second");
    }

    #[test]
    fn append_entry_rejects_empty_after_strip() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        let err = append_entry(&path, "###").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn forget_entry_removes_by_id() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        let id1 = append_entry(&path, "first").unwrap();
        let _id2 = append_entry(&path, "second").unwrap();

        let note = forget_entry(&path, &id1).unwrap();
        assert_eq!(note, "first");

        let entries = load_entries(&path);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].note, "second");
    }

    #[test]
    fn forget_entry_returns_error_for_missing_id() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        append_entry(&path, "first").unwrap();
        let err = forget_entry(&path, "nonexistent").unwrap_err();
        assert!(
            matches!(&err, ForgetError::NotFound(id) if id == "nonexistent"),
            "expected NotFound: {err}"
        );
    }

    #[test]
    fn forget_entry_on_empty_file_returns_error() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        let err = forget_entry(&path, "mem_00000000").unwrap_err();
        assert!(matches!(&err, ForgetError::NotFound(_)));
    }

    #[test]
    fn list_entries_returns_sorted_by_decay() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");

        // Entry with old timestamp
        append_entry(&path, "old note").unwrap();
        // Force an old last_accessed on the first entry.
        let old_ts = "2025-01-01T00:00:00+00:00";
        let entries = load_entries(&path);
        let updated: Vec<MemoryEntry> = entries
            .into_iter()
            .map(|mut e| {
                e.last_accessed = old_ts.to_string();
                e
            })
            .collect();
        save_entries(&path, &updated).unwrap();

        // Fresh entry
        append_entry(&path, "fresh note").unwrap();

        let listed = list_entries(&path);
        assert_eq!(listed.len(), 2);
        // Fresh note should have higher decay score.
        assert!(
            listed[0].1 > listed[1].1,
            "fresh note should have higher decay: {:.3} vs {:.3}",
            listed[0].1,
            listed[1].1
        );
    }

    #[test]
    fn decay_score_decreases_with_age() {
        let old_ts = "2025-01-01T00:00:00+00:00";
        let entry = MemoryEntry {
            id: "mem_old".to_string(),
            note: "old".to_string(),
            created: old_ts.to_string(),
            last_accessed: old_ts.to_string(),
            access_count: 0,
        };
        let score = entry.decay_score();
        assert!(
            score < 0.01,
            "old entry should have near-zero decay: {score}"
        );

        let now = Utc::now().to_rfc3339();
        let fresh = MemoryEntry {
            id: "mem_fresh".to_string(),
            note: "fresh".to_string(),
            created: now.clone(),
            last_accessed: now,
            access_count: 0,
        };
        let fresh_score = fresh.decay_score();
        assert!(
            fresh_score > 0.99,
            "fresh entry should have near-1.0 decay: {fresh_score}"
        );
    }

    #[test]
    fn age_string_produces_readable_format() {
        let now = Utc::now().to_rfc3339();
        let entry = MemoryEntry {
            id: "mem_test".to_string(),
            note: "test".to_string(),
            created: now,
            last_accessed: Utc::now().to_rfc3339(),
            access_count: 0,
        };
        let age = entry.age_string();
        assert!(age == "0m" || age == "1m", "unexpected age: {age}");
    }

    #[test]
    fn touch_entry_updates_last_accessed() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        let id = append_entry(&path, "note").unwrap();

        // Wait a moment, then touch.
        std::thread::sleep(std::time::Duration::from_millis(50));
        touch_entry(&path, &id);

        let entries = load_entries(&path);
        assert_eq!(entries[0].access_count, 1);
        assert_ne!(entries[0].last_accessed, entries[0].created);
    }

    #[test]
    fn compose_block_touches_entries() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        append_entry(&path, "test note").unwrap();

        let entries_before = load_entries(&path);
        assert_eq!(entries_before[0].access_count, 0);

        // compose_block should touch it.
        let _block = compose_block(true, &path);
        let entries_after = load_entries(&path);
        assert_eq!(entries_after[0].access_count, 1);
    }

    #[test]
    fn compose_block_returns_none_when_disabled() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.jsonl");
        assert!(compose_block(false, &path).is_none());
    }

    #[test]
    fn jsonl_path_substitutes_extension() {
        let md = Path::new("/tmp/memory.md");
        assert!(jsonl_path(md).to_string_lossy().ends_with("memory.jsonl"));

        let already_jsonl = Path::new("/tmp/memory.jsonl");
        assert!(jsonl_path(already_jsonl).to_string_lossy().ends_with("memory.jsonl"));

        let no_ext = Path::new("/tmp/memory");
        let jpath = jsonl_path(no_ext);
        assert!(jpath.to_string_lossy().ends_with("memory"));
    }
}
