//! Persistent memory storage backed by TOML files.
//!
//! Memories are stored under `~/.deepseek/memories/`:
//! - Per-project: `project/<project_hash>.toml`
//! - Global: `global.toml`
//!
//! Each file contains a `[[memories]]` array of TOML tables.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Confidence level for a memory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryConfidence {
    High,
    Medium,
    Low,
}

impl std::fmt::Display for MemoryConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
        }
    }
}

impl std::str::FromStr for MemoryConfidence {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "high" => Ok(Self::High),
            "medium" => Ok(Self::Medium),
            "low" => Ok(Self::Low),
            other => Err(format!(
                "unknown confidence level '{other}'; expected high, medium, or low"
            )),
        }
    }
}

/// A single persistent memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    /// Unique identifier (UUID v4).
    pub id: String,
    /// The memory content (free-form text).
    pub content: String,
    /// Origin of this memory: a session_id or "manual".
    pub source: String,
    /// When the memory was created.
    pub created: chrono::DateTime<chrono::Utc>,
    /// Confidence level.
    pub confidence: MemoryConfidence,
    /// Arbitrary tags for classification and search.
    pub tags: Vec<String>,
    /// If set, scoped to a specific project.
    pub project_hash: Option<String>,
}

/// Wrapper for the TOML file format.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MemoriesFile {
    memories: Vec<Memory>,
}

/// Max memories per file before pruning oldest.
const DEFAULT_MAX_MEMORIES: usize = 50;

/// Compute a stable hash of an absolute workspace path.
pub fn project_hash(workspace: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    workspace.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Build the base memories directory: `~/.deepseek/memories/`.
fn memories_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".deepseek").join("memories"))
}

/// Resolve the file path for a given project hash.
fn project_path(project_hash: &str) -> Option<PathBuf> {
    memories_dir().map(|base| base.join("project").join(format!("{project_hash}.toml")))
}

/// Resolve the global memories file path.
fn global_path() -> Option<PathBuf> {
    memories_dir().map(|base| base.join("global.toml"))
}

/// Load memories from a TOML file. Returns an empty vec when the file is
/// missing or unreadable.
fn load_from(path: &Path) -> Vec<Memory> {
    match fs::read_to_string(path) {
        Ok(content) => match toml::from_str::<MemoriesFile>(&content) {
            Ok(file) => file.memories,
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}

/// Save memories to a TOML file, pruning to `max_memories`.
fn save_to(path: &Path, mut memories: Vec<Memory>, max_memories: usize) -> io::Result<()> {
    // Sort by created descending, keep most recent.
    memories.sort_by(|a, b| b.created.cmp(&a.created));
    memories.truncate(max_memories);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = MemoriesFile { memories };
    let content = toml::to_string_pretty(&file).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("TOML serialization failed: {e}"))
    })?;
    fs::write(path, content)
}

/// Save a single memory entry.
///
/// Appends `memory` to the appropriate file (project-scoped or global) and
/// prunes to `max_memories`. The file is created if it doesn't exist.
pub fn save_memory(memory: Memory, _workspace: Option<&Path>, max_memories: usize) -> io::Result<()> {
    let max = if max_memories == 0 {
        DEFAULT_MAX_MEMORIES
    } else {
        max_memories
    };

    let path = if let Some(hash) = &memory.project_hash {
        project_path(hash)
    } else {
        global_path()
    }
    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory not found"))?;

    let mut all = load_from(&path);
    all.push(memory);
    save_to(&path, all, max)
}

/// Load all memories for a project (and optionally include global).
pub fn load_memories(project_hash: Option<&str>, include_global: bool) -> io::Result<Vec<Memory>> {
    let mut all = Vec::new();

    if include_global {
        if let Some(gp) = global_path() {
            all.extend(load_from(&gp));
        }
    }

    if let Some(hash) = project_hash {
        if let Some(pp) = project_path(hash) {
            all.extend(load_from(&pp));
        }
    }

    // Sort by created descending.
    all.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(all)
}

/// Simple keyword search across content and tags.
///
/// Splits `query` into tokens and scores each memory by how many tokens
/// match in the content (case-insensitive). Results are sorted by
/// relevance (descending), then by recency (descending).
pub fn search_memories(
    memories: &[Memory],
    query: &str,
) -> Vec<Memory> {
    let lower = query.to_ascii_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();

    if tokens.is_empty() {
        let mut result = memories.to_vec();
        result.sort_by(|a, b| b.created.cmp(&a.created));
        return result;
    }

    let mut scored: Vec<(usize, &Memory)> = memories
        .iter()
        .map(|m| {
            let content_lower = m.content.to_ascii_lowercase();
            let tag_lower: String = m.tags.join(" ").to_ascii_lowercase();
            let combined = format!("{content_lower} {tag_lower}");

            let score = tokens
                .iter()
                .filter(|t| combined.contains(*t))
                .count();
            (score, m)
        })
        .filter(|(score, _)| *score > 0)
        .collect();

    scored.sort_by(|(s1, m1), (s2, m2)| {
        s2.cmp(s1).then_with(|| m2.created.cmp(&m1.created))
    });

    scored.into_iter().map(|(_, m)| m.clone()).collect()
}

/// Delete a memory by id from its file.
///
/// Returns `true` if a memory was actually removed.
pub fn delete_memory(id: &str, project_hash: Option<&str>, max_memories: usize) -> io::Result<bool> {
    // Try project file first, then global.
    let candidates: Vec<Option<PathBuf>> = if let Some(hash) = project_hash {
        vec![project_path(hash), global_path()]
    } else {
        vec![global_path(), None]
    };

    for candidate in candidates.into_iter().flatten() {
        let mut all = load_from(&candidate);
        let before = all.len();
        all.retain(|m| m.id != id);
        if all.len() < before {
            let max = if max_memories == 0 {
                DEFAULT_MAX_MEMORIES
            } else {
                max_memories
            };
            save_to(&candidate, all, max)?;
            return Ok(true);
        }
    }

    Ok(false)
}

/// Create a new Memory with defaults filled in.
#[must_use]
pub fn new_memory(
    content: String,
    source: String,
    confidence: MemoryConfidence,
    tags: Vec<String>,
    project_hash: Option<String>,
) -> Memory {
    Memory {
        id: uuid::Uuid::new_v4().to_string(),
        content,
        source,
        created: Utc::now(),
        confidence,
        tags,
        project_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn project_hash_is_stable() {
        let a = project_hash(Path::new("/home/user/project"));
        let b = project_hash(Path::new("/home/user/project"));
        assert_eq!(a, b);
    }

    #[test]
    fn search_matches_content_and_tags() {
        let m1 = Memory {
            id: "1".into(),
            content: "this project uses Rust".into(),
            source: "manual".into(),
            created: Utc::now(),
            confidence: MemoryConfidence::High,
            tags: vec!["language".into()],
            project_hash: None,
        };
        let m2 = Memory {
            id: "2".into(),
            content: "prefers 4-space indentation".into(),
            source: "manual".into(),
            created: Utc::now(),
            confidence: MemoryConfidence::Medium,
            tags: vec!["style".into()],
            project_hash: None,
        };

        let results = search_memories(&[m1.clone(), m2.clone()], "rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "1");

        let results = search_memories(&[m1.clone(), m2.clone()], "style");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "2");

        let results = search_memories(&[m1, m2], "nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn confidence_parse_roundtrip() {
        assert_eq!("high".parse::<MemoryConfidence>().unwrap(), MemoryConfidence::High);
        assert_eq!("LOW".parse::<MemoryConfidence>().unwrap(), MemoryConfidence::Low);
        assert_eq!(MemoryConfidence::Medium.to_string(), "medium");
    }

    #[test]
    fn confidence_parse_rejects_unknown() {
        assert!("unknown".parse::<MemoryConfidence>().is_err());
    }

    #[test]
    fn new_memory_fills_defaults() {
        let m = new_memory(
            "test".into(),
            "session-1".into(),
            MemoryConfidence::High,
            vec!["tag".into()],
            Some("hash".into()),
        );
        assert!(!m.id.is_empty());
        assert_eq!(m.content, "test");
        assert_eq!(m.source, "session-1");
        assert_eq!(m.confidence, MemoryConfidence::High);
        assert_eq!(m.tags, vec!["tag"]);
        assert_eq!(m.project_hash, Some("hash".into()));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("test.toml");

        let m = new_memory(
            "roundtrip test".into(),
            "test".into(),
            MemoryConfidence::High,
            vec![],
            None,
        );

        save_to(&path, vec![m.clone()], 50).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, "roundtrip test");
        assert_eq!(loaded[0].id, m.id);
    }

    #[test]
    fn delete_removes_by_id() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("test.toml");

        let m1 = new_memory("keep".into(), "test".into(), MemoryConfidence::High, vec![], None);
        let m2 = new_memory("remove".into(), "test".into(), MemoryConfidence::High, vec![], None);
        let id2 = m2.id.clone();

        save_to(&path, vec![m1.clone(), m2], 50).unwrap();

        // Using direct save_to/load_from to test the mechanics.
        let mut all = load_from(&path);
        assert_eq!(all.len(), 2);
        all.retain(|m| m.id != id2);
        save_to(&path, all, 50).unwrap();

        let remaining = load_from(&path);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, m1.id);
    }

    #[test]
    fn prune_respects_max_memories() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("test.toml");

        let mut memories: Vec<Memory> = (0..10)
            .map(|i| new_memory(
                format!("memory {i}"),
                "test".into(),
                MemoryConfidence::Low,
                vec![],
                None,
            ))
            .collect();
        // Ensure deterministic order: oldest first.
        memories.sort_by(|a, b| a.created.cmp(&b.created));

        save_to(&path, memories, 3).unwrap();
        let loaded = load_from(&path);
        assert_eq!(loaded.len(), 3);
        // Should keep the 3 most recent.
        assert!(loaded[0].content.contains('9'));
        assert!(loaded[1].content.contains('8'));
        assert!(loaded[2].content.contains('7'));
    }
}
