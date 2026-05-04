//! User-level memory file with per-scope size budget and smart truncation.
//!
//! v0.8.8 ships an MVP that lets the user keep a persistent personal
//! note file the model sees on every turn:
//!
//! - **Load** `~/.deepseek/memory.md` (path is configurable via
//!   `memory_path` in `config.toml` and `DEEPSEEK_MEMORY_PATH` env),
//!   wrap it in a `<user_memory>` block, and prepend it to the system
//!   prompt alongside the existing `<project_instructions>` block.
//! - **#495**: When the memory content exceeds `max_memory_tokens`,
//!   entries are sorted by recency + access frequency (most recently
//!   accessed first) and the lowest-priority entries are dropped.
//! - **`# foo`** typed in the composer appends `foo` to the memory
//!   file as a timestamped bullet — fast capture without leaving the TUI.
//! - **`/memory`** shows the resolved file path and current contents, and
//!   **`/memory edit`** prints a copy-pasteable `$VISUAL` / `$EDITOR`
//!   command for opening the file yourself.
//! - **`remember` tool** lets the model itself append a bullet when it
//!   notices a durable preference or convention worth keeping across
//!   sessions.
//!
//! Default behavior is **opt-in**: load + use the memory file only when
//! `[memory] enabled = true` in `config.toml` or `DEEPSEEK_MEMORY=on`.
//! That keeps existing users on zero-overhead behavior and makes the
//! feature explicit.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Maximum size of the user memory file. Larger files are loaded but the
/// `<user_memory>` block carries a "(truncated)" marker so the user knows
/// the model only saw a slice. Mirrors `project_context::MAX_CONTEXT_SIZE`.
const MAX_MEMORY_SIZE: usize = 100 * 1024;

/// Default maximum tokens of memory content injected into the system prompt.
const DEFAULT_MAX_MEMORY_TOKENS: u32 = 4000;

/// Sidecar file extension for access metadata (stored alongside `memory.md`).
const ACCESS_META_EXTENSION: &str = ".access.json";

// ── Entry types ──────────────────────────────────────────────────────────────

/// A single parsed entry from the user memory file.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryEntry {
    /// The raw text of this entry (including the `- (date)` prefix if present).
    pub raw: String,
    /// Parsed creation timestamp (from the `- (date)` prefix), if any.
    pub created: Option<chrono::DateTime<Utc>>,
}

/// Access-tracking metadata for an entry, keyed by an entry-identifying hash
/// (currently the first 64 characters of the entry content).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EntryAccessMeta {
    /// Number of times this entry has been included in a prompt.
    access_count: u64,
    /// ISO-8601 timestamp of the most recent access.
    last_accessed: String,
}

/// On-disk format for the access metadata sidecar.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AccessMetadataStore {
    /// Map from entry hash -> access metadata.
    entries: HashMap<String, EntryAccessMeta>,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Read the user memory file at `path`, returning `None` when the file
/// doesn't exist or is empty after trimming.
#[must_use]
pub fn load(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    if content.trim().is_empty() {
        return None;
    }
    Some(content)
}

/// Wrap memory content in a `<user_memory>` block ready to prepend to the
/// system prompt. The `source` value is rendered verbatim into a
/// `source="…"` attribute — pass the path so the model can see where the
/// memory came from. Returns `None` for empty content.
///
/// When `max_tokens` is `Some(n)`, entries are sorted by recency + access
/// frequency and truncated to stay within `n` tokens.
#[must_use]
pub fn as_system_block(content: &str, source: &Path, max_tokens: Option<u32>) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let display = source.display();
    let max_tokens = max_tokens.unwrap_or(DEFAULT_MAX_MEMORY_TOKENS) as usize;

    // Apply smart truncation: sort by recency + frequency, keep the top
    // entries within the budget.
    let payload = if content.len() > MAX_MEMORY_SIZE {
        let mut head = content[..MAX_MEMORY_SIZE].to_string();
        head.push_str("\n…(truncated, raise [memory].max_size or trim memory.md)");
        head
    } else {
        let entries = parse_entries(trimmed);
        let truncated = sort_and_truncate(&entries, max_tokens, source);
        if truncated < entries.len() {
            // Some entries were dropped — rebuild the text from the kept ones.
            let meta_path = access_meta_path(source);
            let metadata = load_access_metadata(&meta_path);
            let kept = select_entries(&entries, truncated, &metadata);
            let mut out = kept.join("\n");
            out.push_str(&format!(
                "\n\n…(memory truncated to ~{max_tokens} tokens; {dropped} of {total} entries shown)",
                dropped = kept.len(),
                total = entries.len(),
            ));
            out
        } else {
            trimmed.to_string()
        }
    };

    Some(format!(
        "<user_memory source=\"{display}\">\n{payload}\n</user_memory>"
    ))
}

/// Compose the `<user_memory>` block for the system prompt, honouring the
/// opt-in toggle. Returns `None` when the feature is disabled or the file
/// is missing / empty so the caller doesn't have to check both conditions.
///
/// Callers that hold a `&Config` should pass `config.memory_enabled()`,
/// `config.memory_path()`, and `config.memory_max_tokens()` directly. The
/// split keeps this module `Config`-free so it can be reused from sub-agent
/// / engine boundaries where the high-level `Config` isn't available.
#[must_use]
pub fn compose_block(enabled: bool, path: &Path, max_tokens: Option<u32>) -> Option<String> {
    if !enabled {
        return None;
    }
    let content = load(path)?;
    as_system_block(&content, path, max_tokens)
}

/// Append `entry` to the memory file at `path`, creating it (and its
/// parent directory) if needed. The entry is timestamped so the user can
/// later see when each note was added. The leading `#` from a `# foo`
/// quick-add is stripped so the file stays as readable Markdown.
pub fn append_entry(path: &Path, entry: &str) -> io::Result<()> {
    let trimmed = entry.trim_start_matches('#').trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "memory entry is empty after stripping `#` prefix",
        ));
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let timestamp = Utc::now().format("%Y-%m-%d %H:%M UTC");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "- ({timestamp}) {trimmed}")?;
    Ok(())
}

// ── Entry parsing ────────────────────────────────────────────────────────────

/// Parse the raw memory file content into individual entries, splitting on
/// lines that start with `- (` (timestamped bullets).
fn parse_entries(content: &str) -> Vec<MemoryEntry> {
    let mut entries: Vec<MemoryEntry> = Vec::new();
    let mut current = String::new();

    for line in content.lines() {
        if line.starts_with("- (") {
            if !current.is_empty() {
                let created = parse_timestamp(&current);
                entries.push(MemoryEntry {
                    raw: current,
                    created,
                });
            }
            current = line.to_string();
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }

    if !current.is_empty() {
        let created = parse_timestamp(&current);
        entries.push(MemoryEntry {
            raw: current,
            created,
        });
    }

    entries
}

/// Extract the timestamp from an entry starting with `- (YYYY-MM-DD HH:MM UTC)`.
fn parse_timestamp(entry: &str) -> Option<chrono::DateTime<Utc>> {
    let line = entry.lines().next()?;
    // Format: `- (2026-05-03 22:14 UTC) content`
    let rest = line.strip_prefix("- (")?;
    let (date_part, _) = rest.split_once(')')?;
    // Format: `- (2026-05-03 22:14 UTC) content`
    // Try parsing with format string that handles seconds.
    let trimmed = date_part.trim();
    // Try parsing with format string that handles seconds.
    match chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M UTC") {
        Ok(ndt) => Some(ndt.and_utc()),
        Err(_) => {
            // Fallback: strip trailing timezone text and reparse
            let cleaned = trimmed
                .strip_suffix("UTC")
                .unwrap_or(trimmed)
                .trim_end();
            chrono::NaiveDateTime::parse_from_str(cleaned, "%Y-%m-%d %H:%M")
                .ok()
                .map(|ndt| ndt.and_utc())
        }
    }
}

// ── Access metadata ──────────────────────────────────────────────────────────

/// Compute the sidecar path for access metadata, relative to the memory file.
fn access_meta_path(memory_path: &Path) -> PathBuf {
    let mut p = memory_path.to_path_buf();
    let name = p
        .file_name()
        .map(|n| format!("{}{}", n.to_string_lossy(), ACCESS_META_EXTENSION))
        .unwrap_or_else(|| format!("memory{}", ACCESS_META_EXTENSION));
    p.set_file_name(&name);
    p
}

/// Load access metadata from the sidecar file.
fn load_access_metadata(path: &Path) -> AccessMetadataStore {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Save access metadata to the sidecar file.
fn save_access_metadata(path: &Path, store: &AccessMetadataStore) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(store) {
        let _ = fs::write(path, &json);
    }
}

/// Compute a stable hash for an entry (first 64 chars of the raw text).
fn entry_hash(entry: &str) -> String {
    let clean = entry.trim();
    let prefix: String = clean.chars().take(64).collect();
    // Use a simple approach: take the first 64 chars as the key.
    // This is stable across runs since entry text rarely changes.
    prefix
}

/// Record that a set of entries was just accessed (used in the prompt).
/// Updates the sidecar metadata file with current timestamps.
pub fn record_access(memory_path: &Path, entries: &[MemoryEntry]) {
    let meta_path = access_meta_path(memory_path);
    let mut store = load_access_metadata(&meta_path);
    let now = Utc::now().to_rfc3339();

    for entry in entries {
        let hash = entry_hash(&entry.raw);
        let meta = store.entries.entry(hash).or_insert(EntryAccessMeta {
            access_count: 0,
            last_accessed: now.clone(),
        });
        meta.access_count = meta.access_count.saturating_add(1);
        meta.last_accessed = now.clone();
    }

    save_access_metadata(&meta_path, &store);
}

// ── Sorting and truncation ───────────────────────────────────────────────────

/// Compute a composite relevance score for an entry.
///
/// Score combines:
/// - Creation recency (how recently the entry was added)
/// - Access frequency (how often it's been included)
/// - Access recency (when it was last seen by the model)
///
/// Higher score = more relevant.
fn compute_score(
    entry: &MemoryEntry,
    meta: Option<&EntryAccessMeta>,
    now_ts: &chrono::DateTime<Utc>,
) -> f64 {
    let mut score = 0.0f64;

    // 1. Creation recency: entries created more recently get a boost.
    //    Normalize to [0, 10] based on age (cap at 180 days).
    if let Some(created) = &entry.created {
        let age_hours = (*now_ts - *created).num_hours().max(0) as f64;
        let age_days = age_hours / 24.0;
        // Exponential decay: entry created today = ~10, 7 days ago = ~3.7, 30 days ago = ~0.5
        score += 10.0 * (-age_days / 14.0).exp();
    }

    if let Some(meta) = meta {
        // 2. Access frequency: more accessed entries get a logarithmic boost.
        //    Cap at 10 accesses (log2(10) ≈ 3.3).
        let freq_score = (meta.access_count as f64 + 1.0).log2().min(3.3);
        score += freq_score;

        // 3. Access recency: entries accessed more recently get a boost.
        if let Ok(last_access) = chrono::DateTime::parse_from_rfc3339(&meta.last_accessed) {
            let last_access_utc = last_access.with_timezone(&Utc);
            let hours_since_access = (*now_ts - last_access_utc).num_hours().max(0) as f64;
            // Decay: accessed today = +5, 3 days ago = +0.5
            score += 5.0 * (-hours_since_access / 24.0).exp();
        }
    }

    score
}

/// Estimate the token count of a string (rough: ~4 chars per token).
fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4 // ceil(len / 4)
}

/// Select which entries to keep based on relevance score.
///
/// Returns the indices (in the original `entries` vec) of entries to keep.
fn select_entry_indices(
    entries: &[MemoryEntry],
    max_tokens: usize,
    metadata: &AccessMetadataStore,
) -> Vec<usize> {
    if entries.is_empty() {
        return Vec::new();
    }

    let now = Utc::now();

    // Score each entry.
    let mut scored: Vec<(usize, f64)> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            let hash = entry_hash(&entry.raw);
            let meta = metadata.entries.get(&hash);
            (idx, compute_score(entry, meta, &now))
        })
        .collect();

    // Sort by score descending.
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Greedily select entries until we hit the budget.
    let mut kept: Vec<usize> = Vec::new();
    let mut tokens_used = 0usize;

    // Always include the wrapping tokens overhead.
    // The `<user_memory source="...">\n...\n</user_memory>` wrapper costs
    // roughly 6-10 tokens. We leave a bit of headroom.
    let overhead_tokens = 16;

    for (idx, _score) in &scored {
        let entry_tokens = estimate_tokens(&entries[*idx].raw);
        if tokens_used + entry_tokens + overhead_tokens > max_tokens {
            // If nothing fits yet, include at least one entry.
            if kept.is_empty() {
                kept.push(*idx);
            }
            break;
        }
        kept.push(*idx);
        tokens_used += entry_tokens;
    }

    // Restore original order (keep the file's line ordering for readability).
    kept.sort();
    kept
}

/// Select which entries to keep (returns the text of kept entries in order).
fn select_entries(
    entries: &[MemoryEntry],
    keep_count: usize,
    metadata: &AccessMetadataStore,
) -> Vec<String> {
    let indices = select_entry_indices(entries, usize::MAX, metadata);
    // If keep_count < total, cap.
    let capped: Vec<usize> = indices.into_iter().take(keep_count).collect();
    let kept_set: std::collections::HashSet<usize> = capped.iter().copied().collect();

    entries
        .iter()
        .enumerate()
        .filter(|(idx, _)| kept_set.contains(idx))
        .map(|(_, e)| e.raw.clone())
        .collect()
}

/// Compare the total entries vs the truncated count. Returns the number of
/// entries that would fit within `max_tokens` after scoring.
fn sort_and_truncate(entries: &[MemoryEntry], max_tokens: usize, path: &Path) -> usize {
    let meta_path = access_meta_path(path);
    let metadata = load_access_metadata(&meta_path);
    let indices = select_entry_indices(entries, max_tokens, &metadata);

    // Record access for the kept entries.
    let kept: Vec<MemoryEntry> = indices
        .iter()
        .map(|i| entries[*i].clone())
        .collect();
    record_access(path, &kept);

    indices.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use tempfile::tempdir;

    #[test]
    fn load_returns_none_for_missing_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("never-existed.md");
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_returns_none_for_whitespace_only_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(&path, "   \n   \n").unwrap();
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_returns_content_for_real_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(&path, "remember the milk").unwrap();
        assert_eq!(load(&path).as_deref(), Some("remember the milk"));
    }

    #[test]
    fn as_system_block_produces_xml_wrapper() {
        let block = as_system_block("note 1", Path::new("/tmp/m.md"), None).unwrap();
        assert!(block.contains("<user_memory source=\"/tmp/m.md\">"));
        assert!(block.contains("note 1"));
        assert!(block.ends_with("</user_memory>"));
    }

    #[test]
    fn as_system_block_returns_none_for_empty_content() {
        assert!(as_system_block("   ", Path::new("/tmp/m.md"), None).is_none());
    }

    #[test]
    fn as_system_block_truncates_oversize_input() {
        let big = "x".repeat(MAX_MEMORY_SIZE + 100);
        let block = as_system_block(&big, Path::new("/tmp/m.md"), None).unwrap();
        assert!(block.contains("(truncated"));
    }

    #[test]
    fn as_system_block_truncates_by_token_budget() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");

        // Create entries with both old and recent timestamps.
        let old_date = "2024-01-01 00:00 UTC";
        let recent_date = "2026-06-01 00:00 UTC";
        let content = format!(
            "- ({old_date}) old entry that should be dropped first\n\
             - ({recent_date}) recent entry that should be kept\n\
             - ({old_date}) another old entry that might be useful\n"
        );
        fs::write(&path, &content).unwrap();
        // Set a very tight budget: only ~1 entry fits.
        let block = as_system_block(&content, &path, Some(1)).unwrap();
        assert!(block.contains("(memory truncated"));

        // The recent entry should be present.
        assert!(block.contains("recent entry"));
    }

    #[test]
    fn as_system_block_keeps_all_entries_when_budget_sufficient() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        let content = "- (2026-06-01 00:00 UTC) only entry\n";
        fs::write(&path, content).unwrap();

        let block = as_system_block(content, &path, Some(10000)).unwrap();
        // No truncation note since budget is generous.
        assert!(!block.contains("(memory truncated"));
        assert!(block.contains("only entry"));
    }

    #[test]
    fn append_entry_creates_file_and_writes_one_bullet() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        append_entry(&path, "# remember the milk").unwrap();

        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("remember the milk"), "{body}");
        assert!(
            body.starts_with("- ("),
            "should start with bullet + date: {body}"
        );
        assert!(body.trim_end().ends_with("remember the milk"));
    }

    #[test]
    fn append_entry_appends_subsequent_lines() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        append_entry(&path, "# first").unwrap();
        append_entry(&path, "second").unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("first"));
        assert!(body.contains("second"));
        // Two bullets means two lines of `- (date) entry`.
        assert_eq!(body.matches("- (").count(), 2);
    }

    #[test]
    fn append_entry_rejects_empty_after_strip() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        let err = append_entry(&path, "###").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn parse_entries_splits_on_timestamped_bullets() {
        let content = "- (2026-05-03 22:14 UTC) first\n\
                       - (2026-05-04 09:02 UTC) second\n\
                       - (2026-05-05 12:00 UTC) third\n";
        let entries = parse_entries(content);
        assert_eq!(entries.len(), 3);
        assert!(entries[0].raw.contains("first"));
        assert!(entries[1].raw.contains("second"));
        assert!(entries[2].raw.contains("third"));
    }

    #[test]
    fn parse_entries_handles_multiline_entries() {
        let content = "- (2026-05-03 22:14 UTC) first\n  continuation\n\
                       - (2026-05-04 09:02 UTC) second\n";
        let entries = parse_entries(content);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].raw.contains("continuation"));
    }

    #[test]
    fn parse_timestamp_extracts_date() {
        let entry = "- (2026-05-03 22:14 UTC) remember the milk";
        let ts = parse_timestamp(entry);
        assert!(ts.is_some());
        let ts = ts.unwrap();
        assert_eq!(ts.year(), 2026);
        assert_eq!(ts.month(), 5);
        assert_eq!(ts.day(), 3);
    }

    #[test]
    fn compute_score_prefers_recent_entries() {
        let now = Utc::now();
        let old = MemoryEntry {
            raw: "- (2024-01-01 00:00 UTC) old".to_string(),
            created: Some(
                chrono::NaiveDateTime::parse_from_str("2024-01-01 00:00:00", "%Y-%m-%d %H:%M:%S")
                    .unwrap()
                    .and_utc(),
            ),
        };
        let recent = MemoryEntry {
            raw: format!("- ({}) recent", now.format("%Y-%m-%d %H:%M UTC")),
            created: Some(now),
        };

        let score_old = compute_score(&old, None, &now);
        let score_recent = compute_score(&recent, None, &now);
        assert!(
            score_recent > score_old,
            "recent entry should score higher than old entry: {score_recent} vs {score_old}"
        );
    }

    #[test]
    fn compute_score_boosts_frequent_access() {
        let now = Utc::now();
        let entry = MemoryEntry {
            raw: "- (2026-06-01 00:00 UTC) test".to_string(),
            created: Some(
                chrono::NaiveDateTime::parse_from_str("2026-06-01 00:00:00", "%Y-%m-%d %H:%M:%S")
                    .unwrap()
                    .and_utc(),
            ),
        };

        let meta_few = EntryAccessMeta {
            access_count: 1,
            last_accessed: now.to_rfc3339(),
        };
        let meta_many = EntryAccessMeta {
            access_count: 20,
            last_accessed: now.to_rfc3339(),
        };

        let score_few = compute_score(&entry, Some(&meta_few), &now);
        let score_many = compute_score(&entry, Some(&meta_many), &now);
        assert!(
            score_many > score_few,
            "frequently-accessed entry should score higher: {score_many} vs {score_few}"
        );
    }

    #[test]
    fn access_metadata_roundtrip() {
        let tmp = tempdir().unwrap();
        let memory_path = tmp.path().join("memory.md");
        fs::write(&memory_path, "- (2026-06-01 00:00 UTC) test\n").unwrap();

        let entries = parse_entries("- (2026-06-01 00:00 UTC) test\n");

        // Record access twice.
        record_access(&memory_path, &entries);
        record_access(&memory_path, &entries);

        let meta_path = access_meta_path(&memory_path);
        let store = load_access_metadata(&meta_path);
        let hash = entry_hash(&entries[0].raw);
        assert_eq!(store.entries.len(), 1);
        assert_eq!(store.entries[&hash].access_count, 2);
    }

    #[test]
    fn select_entry_indices_respects_budget() {
        let now = Utc::now();
        let entries = vec![
            MemoryEntry {
                raw: format!("- ({}) entry a - short", now.format("%Y-%m-%d %H:%M UTC")),
                created: Some(now),
            },
            MemoryEntry {
                raw: "entry b - no timestamp".to_string(),
                created: None,
            },
        ];

        let metadata = AccessMetadataStore::default();
        // Budget of 5 tokens should only fit one entry (2-3 tokens each).
        let indices = select_entry_indices(&entries, 5, &metadata);
        assert_eq!(indices.len(), 1, "only 1 entry should fit in 5 tokens");
    }

    #[test]
    fn select_entry_indices_keeps_at_least_one_entry() {
        let entries = vec![MemoryEntry {
            raw: "short".to_string(),
            created: None,
        }];

        let metadata = AccessMetadataStore::default();
        // Even a very tight budget should keep at least one entry.
        let indices = select_entry_indices(&entries, 1, &metadata);
        assert_eq!(indices.len(), 1);
    }

    #[test]
    fn entry_hash_is_stable() {
        let text = "- (2026-06-01 00:00 UTC) remember the milk";
        let h1 = entry_hash(text);
        let h2 = entry_hash(text);
        assert_eq!(h1, h2);
    }

    #[test]
    fn compose_block_honours_disabled() {
        assert!(compose_block(false, Path::new("/does/not.exist"), None).is_none());
    }

    #[test]
    fn compose_block_returns_none_for_missing_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("nonexistent.md");
        assert!(compose_block(true, &path, None).is_none());
    }

    #[test]
    fn compose_block_returns_content_for_real_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(&path, "- (2026-06-01 00:00 UTC) test entry\n").unwrap();
        let block = compose_block(true, &path, None);
        assert!(block.is_some());
        assert!(block.unwrap().contains("test entry"));
    }
}
