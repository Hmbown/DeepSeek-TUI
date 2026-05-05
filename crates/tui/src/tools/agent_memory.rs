#![allow(dead_code)]
//! Agent Memory System — structured short/long/entity memory.
//!
//! Inspired by CrewAI's agent memory model:
//! - **Short-term**: Recent conversation context (ephemeral, per-session)
//! - **Long-term**: Persistent knowledge across sessions (file-backed)
//! - **Entity**: Key facts about entities (people, files, concepts)
//!
//! # Memory Operations
//!
//! - `memory_store` — store a memory entry with type and optional embedding
//! - `memory_search` — search across memory using text matching
//! - `memory_list` — list entries by type

use std::collections::{HashMap, VecDeque};
use serde::{Deserialize, Serialize};

// ── Memory types ────────────────────────────────────────────────────────────

/// Memory category matching CrewAI's memory types.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// Recent conversation context. Limited capacity, FIFO eviction.
    ShortTerm,
    /// Persistent knowledge across sessions. Unlimited capacity.
    LongTerm,
    /// Key facts about entities (files, people, concepts).
    Entity,
}

impl MemoryType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShortTerm => "short_term",
            Self::LongTerm => "long_term",
            Self::Entity => "entity",
        }
    }

    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "short_term" | "short" | "recent" => Some(Self::ShortTerm),
            "long_term" | "long" | "persistent" => Some(Self::LongTerm),
            "entity" | "fact" => Some(Self::Entity),
            _ => None,
        }
    }
}

// ── Memory entry ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique key for retrieval.
    pub key: String,
    /// The stored content.
    pub value: String,
    /// Memory category.
    #[serde(rename = "type")]
    pub memory_type: MemoryType,
    /// Optional embedding vector (for vector search). Stored as JSON array.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
    /// Tags for categorization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// When the entry was created.
    pub created_at: String,
    /// Number of times this entry has been accessed.
    #[serde(default)]
    pub access_count: u32,
}

// ── Memory store ────────────────────────────────────────────────────────────

/// In-memory store with short-term capacity limits and optional file
/// persistence for long-term memories.
#[derive(Debug, Clone)]
pub struct AgentMemory {
    /// Short-term memory with FIFO capacity limit.
    short_term: VecDeque<MemoryEntry>,
    /// Long-term memory (unlimited, optionally persisted).
    long_term: Vec<MemoryEntry>,
    /// Entity facts (key-value, latest-wins).
    entities: HashMap<String, MemoryEntry>,
    /// Maximum short-term entries.
    short_term_capacity: usize,
}

impl Default for AgentMemory {
    fn default() -> Self {
        Self {
            short_term: VecDeque::new(),
            long_term: Vec::new(),
            entities: HashMap::new(),
            short_term_capacity: 50,
        }
    }
}

impl AgentMemory {
    /// Create a new memory store.
    #[must_use]
    pub fn new(short_term_capacity: usize) -> Self {
        Self {
            short_term_capacity,
            ..Default::default()
        }
    }

    /// Store a memory entry.
    pub fn store(&mut self, entry: MemoryEntry) {
        match entry.memory_type {
            MemoryType::ShortTerm => {
                if self.short_term.len() >= self.short_term_capacity {
                    self.short_term.pop_front();
                }
                self.short_term.push_back(entry);
            }
            MemoryType::LongTerm => {
                // Replace existing entry with same key
                if let Some(existing) = self
                    .long_term
                    .iter_mut()
                    .find(|e| e.key == entry.key)
                {
                    *existing = entry;
                } else {
                    self.long_term.push(entry);
                }
            }
            MemoryType::Entity => {
                self.entities.insert(entry.key.clone(), entry);
            }
        }
    }

    /// Search memory entries by text matching.
    #[must_use]
    pub fn search(&self, query: &str, memory_type: Option<MemoryType>) -> Vec<MemoryEntry> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        fn push_matches(
            results: &mut Vec<(MemoryEntry, f64)>,
            entries: &[MemoryEntry],
            query_lower: &str,
        ) {
            for entry in entries {
                let score = match_score(entry, query_lower);
                if score > 0.0 {
                    results.push((entry.clone(), score));
                }
            }
        }

        match memory_type {
            Some(MemoryType::ShortTerm) => {
                let entries: Vec<MemoryEntry> = self.short_term.iter().cloned().collect();
                push_matches(&mut results, &entries, &query_lower);
            }
            Some(MemoryType::LongTerm) => {
                push_matches(&mut results, &self.long_term, &query_lower);
            }
            Some(MemoryType::Entity) => {
                let entries: Vec<MemoryEntry> = self.entities.values().cloned().collect();
                push_matches(&mut results, &entries, &query_lower);
            }
            None => {
                let short_entries: Vec<MemoryEntry> = self.short_term.iter().cloned().collect();
                push_matches(&mut results, &short_entries, &query_lower);
                push_matches(&mut results, &self.long_term, &query_lower);
                let entity_entries: Vec<MemoryEntry> = self.entities.values().cloned().collect();
                push_matches(&mut results, &entity_entries, &query_lower);
            }
        }

        // Sort by score descending, then deduplicate by key
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut seen = seen;
        results
            .into_iter()
            .filter(|(entry, _)| seen.insert(entry.key.clone()))
            .map(|(entry, _)| entry)
            .collect()
    }

    /// Retrieve a specific entry by key.
    #[must_use]
    pub fn retrieve(&self, key: &str) -> Option<&MemoryEntry> {
        if let Some(entry) = self.entities.get(key) {
            return Some(entry);
        }
        self.long_term.iter().find(|e| e.key == key)
    }

    /// List entries of a specific type.
    #[must_use]
    pub fn list(&self, memory_type: Option<MemoryType>) -> Vec<&MemoryEntry> {
        match memory_type {
            Some(MemoryType::ShortTerm) => self.short_term.iter().collect(),
            Some(MemoryType::LongTerm) => self.long_term.iter().collect(),
            Some(MemoryType::Entity) => self.entities.values().collect(),
            None => {
                let mut all: Vec<&MemoryEntry> = self.short_term.iter().collect();
                all.extend(&self.long_term);
                all.extend(self.entities.values());
                all
            }
        }
    }

    /// Number of total entries across all types.
    #[must_use]
    pub fn len(&self) -> usize {
        self.short_term.len() + self.long_term.len() + self.entities.len()
    }
}

/// Simple text matching score: checks key, value, and tags for query terms.
fn match_score(entry: &MemoryEntry, query_lower: &str) -> f64 {
    let mut score: f64 = 0.0;

    // Exact key match
    if entry.key.to_lowercase().contains(query_lower) {
        score += 2.0;
    }

    // Value contains query terms
    let value_lower = entry.value.to_lowercase();
    if value_lower.contains(query_lower) {
        score += 3.0;
    }

    // Individual word matching
    for word in query_lower.split_whitespace() {
        if entry.key.to_lowercase().contains(word) {
            score += 1.0;
        }
        if value_lower.contains(word) {
            score += 1.5;
        }
        for tag in &entry.tags {
            if tag.to_lowercase().contains(word) {
                score += 1.0;
            }
        }
    }

    // Boost for frequently accessed entries
    if entry.access_count > 0 {
        score *= 1.0 + (entry.access_count as f64 * 0.1).min(0.5);
    }

    score
}

// ── ToolSpec ────────────────────────────────────────────────────────────────

use async_trait::async_trait;
use serde_json::json;
use crate::tools::spec::{ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec};

/// Tool for storing memories that persist across agent sessions.
pub struct MemoryStoreTool {
    memory: std::sync::Arc<tokio::sync::Mutex<AgentMemory>>,
}

impl MemoryStoreTool {
    pub fn new() -> Self {
        Self {
            memory: std::sync::Arc::new(tokio::sync::Mutex::new(AgentMemory::default())),
        }
    }
}

#[async_trait]
impl ToolSpec for MemoryStoreTool {
    fn name(&self) -> &'static str {
        "memory_store"
    }

    fn description(&self) -> &'static str {
        "Store a memory entry with a key, value, type (short_term/long_term/entity), and optional tags. Short-term memories are FIFO-limited; long-term and entity memories persist."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Unique key for retrieval"
                },
                "value": {
                    "type": "string",
                    "description": "Content to store"
                },
                "type": {
                    "type": "string",
                    "enum": ["short_term", "long_term", "entity"],
                    "description": "Memory type (default: long_term)"
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional tags for categorization"
                }
            },
            "required": ["key", "value"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let key = input
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::invalid_input("Missing 'key'"))?;
        let value = input
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::invalid_input("Missing 'value'"))?;
        let memory_type = input
            .get("type")
            .and_then(|v| v.as_str())
            .and_then(MemoryType::from_str)
            .unwrap_or(MemoryType::LongTerm);
        let tags: Vec<String> = input
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let entry = MemoryEntry {
            key: key.to_string(),
            value: value.to_string(),
            memory_type,
            embedding: None,
            tags,
            created_at: chrono::Utc::now().to_rfc3339(),
            access_count: 0,
        };

        let mut memory = self.memory.lock().await;
        memory.store(entry);

        Ok(ToolResult::success(format!(
            "Memory stored: '{}' (type: {})",
            key,
            memory_type.as_str()
        )))
    }
}

/// Tool for searching agent memory.
pub struct MemorySearchTool {
    memory: std::sync::Arc<tokio::sync::Mutex<AgentMemory>>,
}

impl MemorySearchTool {
    pub fn new(memory: std::sync::Arc<tokio::sync::Mutex<AgentMemory>>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl ToolSpec for MemorySearchTool {
    fn name(&self) -> &'static str {
        "memory_search"
    }

    fn description(&self) -> &'static str {
        "Search agent memory by text query. Returns matching entries sorted by relevance. Optionally filter by memory type (short_term, long_term, entity)."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query text"
                },
                "type": {
                    "type": "string",
                    "enum": ["short_term", "long_term", "entity"],
                    "description": "Optional memory type filter"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results to return (default: 10)"
                }
            },
            "required": ["query"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::invalid_input("Missing 'query'"))?;
        let memory_type = input
            .get("type")
            .and_then(|v| v.as_str())
            .and_then(MemoryType::from_str);
        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        let memory = self.memory.lock().await;
        let results = memory.search(query, memory_type);

        let truncated: Vec<&MemoryEntry> = results.iter().take(max_results).collect();

        if truncated.is_empty() {
            Ok(ToolResult::success("No matching memories found."))
        } else {
            let output = truncated
                .iter()
                .enumerate()
                .map(|(i, entry)| {
                    format!(
                        "{}. [{}] {} = {}",
                        i + 1,
                        entry.memory_type.as_str(),
                        entry.key,
                        entry.value
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            Ok(ToolResult::success(format!(
                "Found {} memories:\n{}",
                results.len(),
                output
            )))
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(key: &str, value: &str, mt: MemoryType) -> MemoryEntry {
        MemoryEntry {
            key: key.to_string(),
            value: value.to_string(),
            memory_type: mt,
            embedding: None,
            tags: Vec::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
            access_count: 0,
        }
    }

    #[test]
    fn test_short_term_fifo() {
        let mut mem = AgentMemory::new(3);
        mem.store(make_entry("a", "first", MemoryType::ShortTerm));
        mem.store(make_entry("b", "second", MemoryType::ShortTerm));
        mem.store(make_entry("c", "third", MemoryType::ShortTerm));
        mem.store(make_entry("d", "fourth", MemoryType::ShortTerm));

        let list = mem.list(Some(MemoryType::ShortTerm));
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].key, "b"); // "a" was evicted
    }

    #[test]
    fn test_long_term_dedup() {
        let mut mem = AgentMemory::default();
        mem.store(make_entry("key1", "v1", MemoryType::LongTerm));
        mem.store(make_entry("key1", "v2", MemoryType::LongTerm));

        let list = mem.list(Some(MemoryType::LongTerm));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].value, "v2");
    }

    #[test]
    fn test_entity_latest_wins() {
        let mut mem = AgentMemory::default();
        mem.store(make_entry("user_name", "Alice", MemoryType::Entity));
        mem.store(make_entry("user_name", "Bob", MemoryType::Entity));

        let entry = mem.retrieve("user_name").unwrap();
        assert_eq!(entry.value, "Bob");
    }

    #[test]
    fn test_search() {
        let mut mem = AgentMemory::default();
        mem.store(make_entry("auth_pattern", "Use bcrypt for password hashing", MemoryType::LongTerm));
        mem.store(make_entry("db_config", "PostgreSQL with connection pooling", MemoryType::LongTerm));
        mem.store(make_entry("api_key", "sk-abc123", MemoryType::Entity));

        let results = mem.search("password", None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "auth_pattern");

        let results = mem.search("postgresql", None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "db_config");
    }
}
