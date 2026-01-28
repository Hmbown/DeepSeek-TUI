//! Repo-aware working set tracking and prompt context packing.
//!
//! The goal of this module is to keep a small, high-signal list of
//! "active" paths that the assistant should prioritize. It observes
//! user messages and tool calls, extracts likely paths, and produces:
//! - a compact working-set summary block for the system prompt
//! - pinned message indices that compaction should preserve

use crate::models::{ContentBlock, Message};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Configuration for working-set tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingSetConfig {
    /// Maximum number of entries to keep.
    pub max_entries: usize,
    /// Maximum number of paths to pin during compaction.
    pub max_pinned_paths: usize,
    /// Maximum characters to scan per text block when pinning messages.
    pub max_scan_chars: usize,
    /// Maximum entries to show in the system prompt block.
    pub max_prompt_entries: usize,
}

impl Default for WorkingSetConfig {
    fn default() -> Self {
        Self {
            max_entries: 16,
            max_pinned_paths: 8,
            max_scan_chars: 2_000,
            max_prompt_entries: 8,
        }
    }
}

/// The source that most recently updated an entry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkingSetSource {
    UserMessage,
    ToolInput,
    ToolOutput,
    Rebuild,
}

/// A single working-set entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingSetEntry {
    /// Workspace-relative path string.
    pub path: String,
    /// Whether the path is a directory (best-effort).
    pub is_dir: bool,
    /// Whether the path exists on disk (best-effort).
    pub exists: bool,
    /// Number of times this path was observed.
    pub touches: u32,
    /// The last observed turn index.
    pub last_turn: u64,
    /// The last update source.
    pub last_source: WorkingSetSource,
}

impl WorkingSetEntry {
    fn new(path: String, exists: bool, is_dir: bool, turn: u64, source: WorkingSetSource) -> Self {
        Self {
            path,
            is_dir,
            exists,
            touches: 1,
            last_turn: turn,
            last_source: source,
        }
    }
}

/// Repo-aware working-set state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkingSet {
    /// Tracking configuration.
    pub config: WorkingSetConfig,
    /// Monotonic turn counter (increments on user messages).
    pub turn: u64,
    /// Path entries keyed by workspace-relative path.
    pub entries: HashMap<String, WorkingSetEntry>,
}

impl WorkingSet {
    /// Advance to the next turn.
    pub fn next_turn(&mut self) {
        self.turn = self.turn.saturating_add(1);
    }

    /// Observe a user message and update the working set.
    pub fn observe_user_message(&mut self, text: &str, workspace: &Path) {
        self.next_turn();
        let paths = extract_paths_from_text(text);
        self.record_candidates(paths, workspace, WorkingSetSource::UserMessage);
    }

    /// Observe a tool call (input and optional output).
    pub fn observe_tool_call(
        &mut self,
        tool_name: &str,
        input: &Value,
        output: Option<&str>,
        workspace: &Path,
    ) {
        let input_candidates = extract_paths_from_value(input, Some(tool_name));
        self.record_candidates(input_candidates, workspace, WorkingSetSource::ToolInput);

        if let Some(text) = output {
            let output_candidates = extract_paths_from_text(text);
            self.record_candidates(output_candidates, workspace, WorkingSetSource::ToolOutput);
        }
    }

    /// Rebuild the working set from existing messages (best effort).
    ///
    /// This is used when syncing a resumed session.
    pub fn rebuild_from_messages(&mut self, messages: &[Message], workspace: &Path) {
        self.entries.clear();
        self.turn = 0;

        for message in messages {
            if message.role == "user" {
                self.next_turn();
            }
            let candidates = extract_paths_from_message(message);
            if candidates.is_empty() {
                continue;
            }
            self.record_candidates(candidates, workspace, WorkingSetSource::Rebuild);
        }
    }

    /// Render a compact working-set block for the system prompt.
    pub fn summary_block(&self, workspace: &Path) -> Option<String> {
        let entries = self.sorted_entries();
        let prompt_entries: Vec<&WorkingSetEntry> = entries
            .into_iter()
            .take(self.config.max_prompt_entries)
            .collect();

        let repo_summary = summarize_repo_root(workspace);

        if repo_summary.is_none() && prompt_entries.is_empty() {
            return None;
        }

        let mut lines: Vec<String> = Vec::new();
        lines.push("## Repo Working Set".to_string());
        lines.push(format!("Workspace: {}", workspace.display()));

        if let Some(summary) = repo_summary {
            lines.push(summary);
        }

        if !prompt_entries.is_empty() {
            lines.push("Active paths (prioritize these):".to_string());
            for entry in prompt_entries {
                let age = self.turn.saturating_sub(entry.last_turn);
                let kind = if entry.is_dir { "dir" } else { "file" };
                lines.push(format!(
                    "- {} ({kind}, touches: {}, last seen: {} turn(s) ago)",
                    entry.path, entry.touches, age
                ));
            }
        }

        lines.push(
            "When in doubt, use tools to verify and keep changes focused on the working set."
                .to_string(),
        );

        Some(lines.join("\n"))
    }

    /// Return the most relevant paths in score order.
    pub fn top_paths(&self, limit: usize) -> Vec<String> {
        self.sorted_entries()
            .into_iter()
            .take(limit)
            .map(|entry| entry.path.clone())
            .collect()
    }

    /// Identify message indices that should be pinned during compaction.
    pub fn pinned_message_indices(&self, messages: &[Message], workspace: &Path) -> Vec<usize> {
        if messages.is_empty() || self.entries.is_empty() {
            return Vec::new();
        }

        let pinned_paths: Vec<&WorkingSetEntry> = self
            .sorted_entries()
            .into_iter()
            .take(self.config.max_pinned_paths)
            .collect();
        if pinned_paths.is_empty() {
            return Vec::new();
        }

        let needles = build_search_needles(&pinned_paths, workspace);
        if needles.is_empty() {
            return Vec::new();
        }

        let mut pinned: Vec<usize> = Vec::new();
        for (idx, message) in messages.iter().enumerate() {
            if message_mentions_any_path(message, &needles, self.config.max_scan_chars) {
                pinned.push(idx);
            }
        }
        pinned
    }

    fn record_candidates(
        &mut self,
        candidates: Vec<String>,
        workspace: &Path,
        source: WorkingSetSource,
    ) {
        if candidates.is_empty() {
            return;
        }

        let workspace_canon = workspace.canonicalize().ok();

        for raw in candidates {
            let Some(normalized) = normalize_candidate(&raw) else {
                continue;
            };
            let Some((rel, exists, is_dir)) =
                relativize_candidate(&normalized, workspace, workspace_canon.as_deref())
            else {
                continue;
            };
            self.record_path(rel, exists, is_dir, source);
        }

        self.prune();
    }

    fn record_path(&mut self, rel: String, exists: bool, is_dir: bool, source: WorkingSetSource) {
        match self.entries.get_mut(&rel) {
            Some(entry) => {
                entry.exists |= exists;
                entry.is_dir |= is_dir;
                entry.touches = entry.touches.saturating_add(1);
                entry.last_turn = self.turn;
                entry.last_source = source;
            }
            None => {
                let entry = WorkingSetEntry::new(rel.clone(), exists, is_dir, self.turn, source);
                let _ = self.entries.insert(rel, entry);
            }
        }
    }

    fn prune(&mut self) {
        let max_entries = self.config.max_entries;
        if self.entries.len() <= max_entries {
            return;
        }

        // Rank by score ascending and drop the lowest until within bounds.
        let mut ranked: Vec<(String, i64)> = self
            .entries
            .values()
            .map(|entry| (entry.path.clone(), score_entry(entry, self.turn)))
            .collect();
        ranked.sort_by(|a, b| a.1.cmp(&b.1));

        let to_remove = self.entries.len().saturating_sub(max_entries);
        for (path, _) in ranked.into_iter().take(to_remove) {
            let _ = self.entries.remove(&path);
        }
    }

    fn sorted_entries(&self) -> Vec<&WorkingSetEntry> {
        let mut entries: Vec<&WorkingSetEntry> = self.entries.values().collect();
        entries.sort_by(|a, b| {
            let sb = score_entry(b, self.turn);
            let sa = score_entry(a, self.turn);
            sb.cmp(&sa).then_with(|| a.path.cmp(&b.path))
        });
        entries
    }
}

fn score_entry(entry: &WorkingSetEntry, current_turn: u64) -> i64 {
    let age = current_turn.saturating_sub(entry.last_turn);
    let recency_bonus = match age {
        0 => 6,
        1 => 4,
        2 => 3,
        3..=5 => 2,
        6..=10 => 1,
        _ => 0,
    };
    i64::from(entry.touches) * 4 + recency_bonus
}

fn normalize_candidate(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '`' | ',' | ';' | ':' | '(' | ')' | '[' | ']'
        )
    });
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn relativize_candidate(
    candidate: &str,
    workspace: &Path,
    workspace_canon: Option<&Path>,
) -> Option<(String, bool, bool)> {
    let candidate_path = Path::new(candidate);

    // Reject obvious URLs and non-paths early.
    if candidate.contains("://") {
        return None;
    }

    let (rel_path, abs_path) = if candidate_path.is_absolute() {
        let within_workspace = workspace_canon
            .map(|ws| candidate_path.starts_with(ws))
            .unwrap_or_else(|| candidate_path.starts_with(workspace));
        if !within_workspace {
            return None;
        }
        let rel = candidate_path.strip_prefix(workspace).ok()?.to_path_buf();
        (rel, candidate_path.to_path_buf())
    } else {
        if starts_with_parent_dir(candidate_path) {
            return None;
        }
        let rel = clean_relative(candidate_path);
        let abs = workspace.join(&rel);
        (rel, abs)
    };

    let metadata = fs::metadata(&abs_path).ok();
    let exists = metadata.is_some();
    let is_dir = metadata
        .as_ref()
        .map(fs::Metadata::is_dir)
        .unwrap_or_else(|| candidate.ends_with('/'));

    let rel_string = path_to_string(&rel_path)?;
    Some((rel_string, exists, is_dir))
}

fn starts_with_parent_dir(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(std::path::Component::ParentDir)
    )
}

fn clean_relative(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut parts: Vec<PathBuf> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = parts.pop();
            }
            Component::Normal(p) => parts.push(PathBuf::from(p)),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    let mut out = PathBuf::new();
    for part in parts {
        out.push(part);
    }
    out
}

fn path_to_string(path: &Path) -> Option<String> {
    path.as_os_str().to_str().map(|s| s.replace('\\', "/"))
}

fn extract_paths_from_message(message: &Message) -> Vec<String> {
    let mut paths = Vec::new();
    for block in &message.content {
        match block {
            ContentBlock::Text { text, .. } => {
                paths.extend(extract_paths_from_text(text));
            }
            ContentBlock::ToolUse { input, .. } => {
                paths.extend(extract_paths_from_value(input, None));
            }
            ContentBlock::ToolResult { content, .. } => {
                paths.extend(extract_paths_from_text(content));
            }
            ContentBlock::Thinking { .. } => {}
        }
    }
    paths
}

fn extract_paths_from_value(value: &Value, tool_hint: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    extract_paths_from_value_inner(value, tool_hint, None, &mut out);
    out
}

fn extract_paths_from_value_inner(
    value: &Value,
    tool_hint: Option<&str>,
    key_hint: Option<&str>,
    out: &mut Vec<String>,
) {
    match value {
        Value::String(s) => {
            let key_suggests_path = key_hint.map(key_is_path_like).unwrap_or(false);
            if key_suggests_path || looks_like_path(s) {
                out.extend(extract_paths_from_text(s));
                if key_suggests_path && !s.contains('/') && !s.contains('\\') {
                    out.push(s.to_string());
                }
            } else if tool_hint == Some("exec_shell") && s.len() < 400 {
                out.extend(extract_paths_from_text(s));
            }
        }
        Value::Array(arr) => {
            for item in arr {
                extract_paths_from_value_inner(item, tool_hint, key_hint, out);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                extract_paths_from_value_inner(v, tool_hint, Some(k.as_str()), out);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn key_is_path_like(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("path")
        || lower.contains("file")
        || lower.contains("dir")
        || lower.contains("cwd")
        || lower.contains("workspace")
        || lower.contains("root")
        || lower == "target"
}

fn looks_like_path(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return true;
    }
    match Path::new(trimmed).extension().and_then(OsStr::to_str) {
        Some(ext) => COMMON_EXTENSIONS.contains(&ext),
        None => false,
    }
}

const COMMON_EXTENSIONS: &[&str] = &[
    "rs", "toml", "md", "txt", "json", "yaml", "yml", "ts", "tsx", "js", "jsx", "py", "go", "java",
    "c", "cc", "cpp", "h", "hpp", "sh", "bash", "zsh", "sql", "html", "css", "scss",
];

fn extract_paths_from_text(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let re = path_regex();
    re.find_iter(text)
        .map(|m| m.as_str().to_string())
        .filter(|s| looks_like_path(s))
        .collect()
}

fn path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Path-ish tokens with separators or file extensions.
        Regex::new(
            r#"(?x)
            (?:
                (?:[A-Za-z]:\\)?                # optional Windows drive
                (?:\./|\../|/)?                 # optional leading
                [A-Za-z0-9._-]+
                (?:[/\\][A-Za-z0-9._-]+)+
                (?:\.[A-Za-z0-9]{1,8})?         # optional extension
            )
            |
            (?:
                [A-Za-z0-9._-]+\.[A-Za-z0-9]{1,8}
            )
            "#,
        )
        .expect("path regex should compile")
    })
}

fn truncate_chars(text: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => &text[..idx],
        None => text,
    }
}

fn build_search_needles(entries: &[&WorkingSetEntry], workspace: &Path) -> Vec<String> {
    let mut needles: HashSet<String> = HashSet::new();
    for entry in entries {
        let rel = entry.path.clone();
        if rel.is_empty() {
            continue;
        }
        let abs = workspace.join(&rel);
        let abs_str = abs.as_os_str().to_str().map(ToOwned::to_owned);

        let _ = needles.insert(rel.clone());
        if let Some(abs_str) = abs_str {
            let _ = needles.insert(abs_str);
        }
    }
    needles.into_iter().collect()
}

fn message_mentions_any_path(message: &Message, needles: &[String], max_scan_chars: usize) -> bool {
    if needles.is_empty() {
        return false;
    }
    for block in &message.content {
        match block {
            ContentBlock::Text { text, .. } => {
                let snippet = truncate_chars(text, max_scan_chars);
                if contains_any(snippet, needles) {
                    return true;
                }
            }
            ContentBlock::ToolUse { input, .. } => {
                if let Ok(json) = serde_json::to_string(input)
                    && contains_any(&json, needles)
                {
                    return true;
                }
            }
            ContentBlock::ToolResult { content, .. } => {
                let snippet = truncate_chars(content, max_scan_chars);
                if contains_any(snippet, needles) {
                    return true;
                }
            }
            ContentBlock::Thinking { .. } => {}
        }
    }
    false
}

fn contains_any(text: &str, needles: &[String]) -> bool {
    needles
        .iter()
        .any(|needle| !needle.is_empty() && text.contains(needle))
}

fn summarize_repo_root(workspace: &Path) -> Option<String> {
    let key_files = detect_key_files(workspace);
    let top_dirs = list_top_level_dirs(workspace, 8);

    if key_files.is_empty() && top_dirs.is_empty() {
        return None;
    }

    let mut parts: Vec<String> = Vec::new();
    if !key_files.is_empty() {
        parts.push(format!("Key files: {}", key_files.join(", ")));
    }
    if !top_dirs.is_empty() {
        parts.push(format!("Top-level dirs: {}", top_dirs.join(", ")));
    }
    Some(parts.join("\n"))
}

fn detect_key_files(workspace: &Path) -> Vec<String> {
    const CANDIDATES: &[&str] = &[
        "Cargo.toml",
        "README.md",
        "AGENTS.md",
        "CLAUDE.md",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "Makefile",
    ];

    CANDIDATES
        .iter()
        .filter_map(|name| {
            let path = workspace.join(name);
            if path.exists() {
                Some((*name).to_string())
            } else {
                None
            }
        })
        .collect()
}

fn list_top_level_dirs(workspace: &Path, limit: usize) -> Vec<String> {
    let mut dirs = Vec::new();
    let entries = match fs::read_dir(workspace) {
        Ok(entries) => entries,
        Err(_) => return dirs,
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };

        if name.starts_with('.') || IGNORED_ROOT_DIRS.contains(&name) {
            continue;
        }

        if let Ok(meta) = entry.metadata()
            && meta.is_dir()
        {
            dirs.push(name.to_string());
        }

        if dirs.len() >= limit {
            break;
        }
    }

    dirs.sort();
    dirs
}

const IGNORED_ROOT_DIRS: &[&str] = &["target", "node_modules", "dist", "build", ".git"];

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_message(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
        }
    }

    #[test]
    fn observe_user_message_tracks_paths() {
        let tmp = TempDir::new().expect("tempdir");
        let src = tmp.path().join("src");
        let file = src.join("lib.rs");
        fs::create_dir_all(&src).expect("mkdir");
        fs::write(&file, "pub fn x() {}").expect("write");

        let mut ws = WorkingSet::default();
        ws.observe_user_message("Please check src/lib.rs", tmp.path());

        assert!(ws.entries.contains_key("src/lib.rs"));
        let entry = ws.entries.get("src/lib.rs").expect("entry");
        assert!(entry.exists);
        assert!(!entry.is_dir);
    }

    #[test]
    fn observe_tool_call_extracts_paths_from_input() {
        let tmp = TempDir::new().expect("tempdir");
        let file = tmp.path().join("Cargo.toml");
        fs::write(&file, "[package]\nname = \"x\"").expect("write");

        let mut ws = WorkingSet::default();
        let input = serde_json::json!({ "path": "Cargo.toml" });
        ws.observe_tool_call("read_file", &input, None, tmp.path());

        assert!(ws.entries.contains_key("Cargo.toml"));
    }

    #[test]
    fn pinned_message_indices_respects_working_set() {
        let tmp = TempDir::new().expect("tempdir");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("mkdir");
        let file = src.join("main.rs");
        fs::write(&file, "fn main() {}").expect("write");

        let mut ws = WorkingSet::default();
        ws.observe_user_message("Edit src/main.rs", tmp.path());

        let messages = vec![
            make_message("user", "Unrelated text"),
            make_message("assistant", "I will read src/main.rs next."),
            make_message("user", "More unrelated text"),
        ];

        let pinned = ws.pinned_message_indices(&messages, tmp.path());
        assert_eq!(pinned, vec![1]);
    }

    #[test]
    fn summary_block_includes_repo_and_working_set() {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("Cargo.toml"), "[package]\nname = \"x\"").expect("write");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("mkdir");
        fs::write(src.join("lib.rs"), "pub fn x() {}").expect("write");

        let mut ws = WorkingSet::default();
        ws.observe_user_message("src/lib.rs", tmp.path());
        let block = ws.summary_block(tmp.path()).expect("block");

        assert!(block.contains("Repo Working Set"));
        assert!(block.contains("Cargo.toml"));
        assert!(block.contains("src"));
        assert!(block.contains("src/lib.rs"));
    }

    #[test]
    fn extract_paths_from_message_picks_up_tool_results() {
        let msg = Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: "Changed src/compaction.rs".to_string(),
            }],
        };

        let paths = extract_paths_from_message(&msg);
        assert!(paths.iter().any(|p| p.contains("src/compaction.rs")));
    }

    #[test]
    fn pinning_prefers_high_signal_paths() {
        let tmp = TempDir::new().expect("tempdir");
        fs::create_dir_all(tmp.path().join("src")).expect("mkdir");
        fs::write(tmp.path().join("src/a.rs"), "a").expect("write");
        fs::write(tmp.path().join("src/b.rs"), "b").expect("write");

        let mut ws = WorkingSet::default();
        ws.observe_user_message("src/a.rs", tmp.path());
        ws.observe_tool_call(
            "read_file",
            &serde_json::json!({ "path": "src/a.rs" }),
            Some("src/a.rs"),
            tmp.path(),
        );
        ws.observe_user_message("src/b.rs", tmp.path());

        let a_score = score_entry(ws.entries.get("src/a.rs").expect("a"), ws.turn);
        let b_score = score_entry(ws.entries.get("src/b.rs").expect("b"), ws.turn);
        assert!(a_score >= b_score);
    }

    #[test]
    fn estimate_tokens_is_available_for_future_budgeting() {
        use crate::compaction::estimate_tokens;
        let messages = vec![make_message("user", "src/main.rs")];
        assert!(estimate_tokens(&messages) > 0);
    }
}
