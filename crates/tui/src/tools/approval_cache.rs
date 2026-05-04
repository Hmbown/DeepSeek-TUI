#![allow(dead_code)]
//! Per‑call approval cache with fingerprint keys (§5.A).
//!
//! Instead of caching by tool name alone (which would let an approved
//! `exec_shell "cat foo"` silently pass `exec_shell "rm -rf /"`), the
//! cache keys off a **call fingerprint** — a digest of the tool name and
//! the semantically‑relevant portion of its arguments.
//!
//! ## Fingerprint shape (per‑call, [`build_approval_key`])
//!
//! | Tool           | Key                                      |
//! |---------------|------------------------------------------|
//! | `apply_patch`  | `patch:<hash of file paths>`             |
//! | `exec_shell`   | `shell:<command prefix (first 3 tokens)>` |
//! | `fetch_url`    | `net:<hostname>`                         |
//! | `read_file`    | `"file:<parent_dir_hash>"`               |
//! | `write_file`   | `"file:<parent_dir_hash>"`               |
//! | `edit_file`    | `"file:<parent_dir_hash>"`               |
//! | everything else| `tool:<tool_name>`                       |
//!
//! ## Fingerprint shape (session‑broad, [`build_session_approval_key`])
//!
//! | Tool           | Key                                      |
//! |---------------|------------------------------------------|
//! | `read_file`    | `"read:dir:<parent_dir_hash>"`           |
//! | `write_file`   | `"write:dir:<parent_dir_hash>"`          |
//! | `edit_file`    | `"edit:dir:<parent_dir_hash>"`           |
//! | `apply_patch`  | `"patch:dir:<common_parent_dir_hash>"`   |
//! | `exec_shell`   | `"shell:<command_prefix>"`               |
//! | `fetch_url`    | `"net:<hostname>"`                       |
//! | everything else| `"tool:<tool_name>"`                     |
//!
//! The cache is **session‑keyed**: entries carry an
//! `ApprovedForSession` flag. When true, the approval is reused for the
//! remainder of the session; when false, it is a one‑shot grant (future
//! calls with the same fingerprint still prompt).

use std::collections::HashMap;
use std::time::Instant;

use crate::command_safety::classify_command;

/// The fingerprint of a tool call — stable enough to match repeated
/// calls but specific enough to avoid privilege confusion.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApprovalKey(pub String);

/// Status of a previously‑rendered approval decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalCacheStatus {
    /// Call fingerprint matched and the session‑level flag says reuse.
    Approved,
    /// Call fingerprint matched but the grant was one‑shot (already consumed).
    Denied,
    /// No match — requires fresh approval.
    Unknown,
}

/// A single cache entry.
#[derive(Debug, Clone)]
struct ApprovalCacheEntry {
    /// When this entry was created.
    created: Instant,
    /// Whether the approval should be reused across the session.
    approved_for_session: bool,
}

/// An approval cache backed by tool‑call fingerprints.
#[derive(Debug, Default)]
pub struct ApprovalCache {
    entries: HashMap<ApprovalKey, ApprovalCacheEntry>,
}

impl ApprovalCache {
    /// Construct an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Look up a previously‑rendered approval decision.
    pub fn check(&self, key: &ApprovalKey) -> ApprovalCacheStatus {
        let Some(entry) = self.entries.get(key) else {
            return ApprovalCacheStatus::Unknown;
        };
        if entry.approved_for_session {
            ApprovalCacheStatus::Approved
        } else {
            ApprovalCacheStatus::Denied
        }
    }

    /// Record an approval decision under the given fingerprint.
    ///
    /// When `approved_for_session` is true, subsequent calls with the
    /// same key will auto‑approve for the remainder of the session.
    pub fn insert(&mut self, key: ApprovalKey, approved_for_session: bool) {
        self.entries.insert(
            key,
            ApprovalCacheEntry {
                created: Instant::now(),
                approved_for_session,
            },
        );
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Number of cached entries.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── Fingerprint helpers ────────────────────────────────────────────

/// Derive the parent-directory hash from a path value.
///
/// Returns the hash of the parent directory (or the workspace root if
/// the path has no parent). This is used by the session‑broad key to
/// match all reads/writes in the same directory tree.
fn parent_dir_hash(value: &serde_json::Value) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let path_str = match value {
        serde_json::Value::String(s) => s.clone(),
        _ => return "<no_path>".to_string(),
    };
    let path = std::path::Path::new(&path_str);
    let parent = path
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or(".");
    let mut hasher = DefaultHasher::new();
    parent.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Extract a `path` parameter from a JSON input value.
fn extract_path(input: &serde_json::Value) -> Option<&str> {
    input
        .get("path")
        .and_then(|v| v.as_str())
        .or_else(|| input.get("target").and_then(|v| v.as_str()))
        .or_else(|| input.get("destination").and_then(|v| v.as_str()))
}

/// Build the session‑level approval key — a **broader** fingerprint than
/// [`build_approval_key`] so that an "always allow" decision generalises
/// to related operations (e.g. all reads in the same directory).
///
/// | Tool                  | Key (session‑broad)                        |
/// |-----------------------|--------------------------------------------|
/// | `read_file`           | `"read:dir:<parent_dir_hash>"`             |
/// | `write_file`          | `"write:dir:<parent_dir_hash>"`            |
/// | `edit_file`           | `"edit:dir:<parent_dir_hash>"`             |
/// | `apply_patch`         | `"patch:dir:<common_parent_dir_hash>"`     |
/// | `exec_shell`          | `"shell:<command_prefix>"` (unchanged)     |
/// | `fetch_url`           | `"net:<hostname>"` (unchanged)             |
/// | everything else       | `"tool:<tool_name>"` (unchanged)           |
///
/// # Example
///
/// ```
/// # use crate::tools::approval_cache::build_session_approval_key;
/// # use serde_json::json;
/// // read_file("src/main.rs") → broad key based on "src/" directory
/// let key = build_session_approval_key("read_file", &json!({"path": "src/main.rs"}));
/// assert!(key.0.starts_with("read:dir:"), "expected read:dir: prefix, got {key:?}");
/// ```
#[must_use]
pub fn build_session_approval_key(tool_name: &str, input: &serde_json::Value) -> ApprovalKey {
    match tool_name {
        // File read tools — key on parent directory
        "read_file" => {
            if let Some(path) = extract_path(input) {
                let ph = parent_dir_hash(&serde_json::Value::String(path.to_string()));
                ApprovalKey(format!("read:dir:{ph}"))
            } else {
                ApprovalKey(format!("tool:{tool_name}"))
            }
        }
        // File write/edit tools — key on parent directory
        "write_file" | "edit_file" => {
            if let Some(path) = extract_path(input) {
                let ph = parent_dir_hash(&serde_json::Value::String(path.to_string()));
                ApprovalKey(format!("write:dir:{ph}"))
            } else {
                ApprovalKey(format!("tool:{tool_name}"))
            }
        }
        // Patch tool — key on common parent directory of all affected files
        "apply_patch" => {
            let paths = collect_patch_paths(input);
            if paths.is_empty() {
                return ApprovalKey("patch:no_files".to_string());
            }
            // Find the common ancestor directory across all patched files.
            let common = common_parent_dir(&paths);
            let ph = parent_dir_hash(&serde_json::Value::String(
                common.unwrap_or_else(|| ".".to_string()),
            ));
            ApprovalKey(format!("patch:dir:{ph}"))
        }
        // Shell — reuse the exact prefix (already broad enough)
        "exec_shell"
        | "exec_shell_wait"
        | "exec_shell_interact"
        | "exec_wait"
        | "exec_interact" => {
            let prefix = command_prefix(input);
            ApprovalKey(format!("shell:{prefix}"))
        }
        // Network — reuse host-level key (already broad enough)
        "fetch_url" | "web.fetch" | "web_fetch" => {
            let host = parse_host(input);
            ApprovalKey(format!("net:{host}"))
        }
        // Everything else — tool-wide key for session
        _ => ApprovalKey(format!("tool:{tool_name}")),
    }
}

/// Collect file paths referenced by a patch input into a Vec.
fn collect_patch_paths(input: &serde_json::Value) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();

    if let Some(changes) = input.get("changes").and_then(|v| v.as_array()) {
        for change in changes {
            if let Some(path) = change.get("path").and_then(|v| v.as_str()) {
                paths.push(path.to_string());
            }
        }
    } else if let Some(patch_text) = input.get("patch").and_then(|v| v.as_str()) {
        for line in patch_text.lines() {
            if let Some(rest) = line.strip_prefix("+++ b/") {
                paths.push(rest.trim().to_string());
            }
        }
    }

    paths
}

/// Find the common parent directory for a set of paths.
/// Returns `None` if the set is empty or paths have no common ancestor.
fn common_parent_dir(paths: &[String]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    if paths.len() == 1 {
        return std::path::Path::new(&paths[0])
            .parent()
            .and_then(|p| p.to_str())
            .map(|s| s.to_string());
    }
    // Split each path into components and find common prefix.
    let components: Vec<Vec<std::path::Component>> = paths
        .iter()
        .map(|p| std::path::Path::new(p).components().collect::<Vec<_>>())
        .collect();
    let first = &components[0];
    let mut common_len = 0;
    for i in 0..first.len() {
        let comp = first[i];
        if components[1..].iter().all(|c| c.get(i) == Some(&comp)) {
            common_len = i + 1;
        } else {
            break;
        }
    }
    if common_len == 0 {
        return Some(".".to_string());
    }
    let common: std::path::PathBuf = first[..common_len].iter().collect();
    common.to_str().map(|s| s.to_string())
}

/// Build the approval‑cache key for a tool call.
///
/// The key incorporates the tool name and a lossy digest of the
/// arguments so that the cache can distinguish `exec_shell "ls"`
/// from `exec_shell "rm -rf /"` while still recognising repeated
/// invocations of the same harmless command.
///
/// For file‑based tools (`read_file`, `write_file`, `edit_file`), the
/// key includes the parent‑directory hash so that repeated reads of
/// files in the same directory are recognised as the same operation.
#[must_use]
pub fn build_approval_key(tool_name: &str, input: &serde_json::Value) -> ApprovalKey {
    let fingerprint = match tool_name {
        "read_file" => {
            if let Some(path) = extract_path(input) {
                let ph = parent_dir_hash(&serde_json::Value::String(path.to_string()));
                format!("file:{ph}")
            } else {
                format!("tool:{tool_name}")
            }
        }
        "write_file" | "edit_file" => {
            if let Some(path) = extract_path(input) {
                let ph = parent_dir_hash(&serde_json::Value::String(path.to_string()));
                format!("file:{ph}")
            } else {
                format!("tool:{tool_name}")
            }
        }
        "apply_patch" => {
            let paths_hash = hash_patch_paths(input);
            format!("patch:{paths_hash}")
        }
        "exec_shell"
        | "exec_shell_wait"
        | "exec_shell_interact"
        | "exec_wait"
        | "exec_interact" => {
            let prefix = command_prefix(input);
            format!("shell:{prefix}")
        }
        "fetch_url" | "web.fetch" | "web_fetch" => {
            let host = parse_host(input);
            format!("net:{host}")
        }
        _ => format!("tool:{tool_name}"),
    };
    ApprovalKey(fingerprint)
}

/// Return the canonical command prefix for the shell command in `input`.
///
/// Uses [`classify_command`] from the arity dictionary so that
/// `auto_allow = ["git status"]` correctly matches `git status -s` and
/// `git status --porcelain` without also matching `git push`.
fn command_prefix(input: &serde_json::Value) -> String {
    let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if tokens.is_empty() {
        return "<empty>".to_string();
    }
    classify_command(&tokens)
}

/// Hash the sorted set of file paths referenced by a patch input.
fn hash_patch_paths(input: &serde_json::Value) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut paths: Vec<&str> = Vec::new();

    if let Some(changes) = input.get("changes").and_then(|v| v.as_array()) {
        for change in changes {
            if let Some(path) = change.get("path").and_then(|v| v.as_str()) {
                paths.push(path);
            }
        }
    } else if let Some(patch_text) = input.get("patch").and_then(|v| v.as_str()) {
        for line in patch_text.lines() {
            if let Some(rest) = line.strip_prefix("+++ b/") {
                paths.push(rest.trim());
            }
        }
    }

    paths.sort();
    paths.dedup();

    if paths.is_empty() {
        return "no_files".to_string();
    }

    let mut hasher = DefaultHasher::new();
    for path in &paths {
        path.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// Parse the host portion from a URL input.
fn parse_host(input: &serde_json::Value) -> String {
    let url = input.get("url").and_then(|v| v.as_str()).unwrap_or("");

    if let Ok(parsed) = reqwest::Url::parse(url) {
        parsed.host_str().unwrap_or(url).to_string()
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cache_hit_returns_approved_for_session() {
        let mut cache = ApprovalCache::new();
        let key = build_approval_key("exec_shell", &json!({"command": "ls -la"}));
        cache.insert(key.clone(), true);
        assert_eq!(cache.check(&key), ApprovalCacheStatus::Approved);
    }

    #[test]
    fn cache_one_shot_is_not_reused() {
        let mut cache = ApprovalCache::new();
        let key = build_approval_key("exec_shell", &json!({"command": "cargo build"}));
        cache.insert(key.clone(), false);
        assert_eq!(cache.check(&key), ApprovalCacheStatus::Denied);
    }

    #[test]
    fn cache_miss_is_unknown() {
        let cache = ApprovalCache::new();
        let key = build_approval_key("exec_shell", &json!({"command": "ls"}));
        assert_eq!(cache.check(&key), ApprovalCacheStatus::Unknown);
    }

    #[test]
    fn different_commands_different_keys() {
        let key_a = build_approval_key("exec_shell", &json!({"command": "ls"}));
        let key_b = build_approval_key("exec_shell", &json!({"command": "rm -rf /tmp"}));
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn same_command_same_key() {
        let key_a = build_approval_key("exec_shell", &json!({"command": "cargo build --release"}));
        let key_b = build_approval_key("exec_shell", &json!({"command": "cargo build --release"}));
        assert_eq!(key_a, key_b);
    }

    #[test]
    fn command_prefix_drops_flags() {
        let key_a = build_approval_key("exec_shell", &json!({"command": "cargo build"}));
        let key_b = build_approval_key("exec_shell", &json!({"command": "cargo build --release"}));
        assert_eq!(key_a, key_b);
    }

    #[test]
    fn patch_keys_differ_by_path() {
        let key_a = build_approval_key(
            "apply_patch",
            &json!({"changes": [{"path": "a.rs", "content": "x"}]}),
        );
        let key_b = build_approval_key(
            "apply_patch",
            &json!({"changes": [{"path": "b.rs", "content": "x"}]}),
        );
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn net_keys_differ_by_host() {
        let key_a = build_approval_key("fetch_url", &json!({"url": "https://example.com"}));
        let key_b = build_approval_key("fetch_url", &json!({"url": "https://other.org"}));
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn file_tools_key_on_parent_dir() {
        // Same parent dir → same key
        let key_a = build_approval_key("read_file", &json!({"path": "src/main.rs"}));
        let key_b = build_approval_key("read_file", &json!({"path": "src/lib.rs"}));
        assert_eq!(key_a, key_b);
        assert!(
            key_a.0.starts_with("file:"),
            "expected file: prefix, got {:?}",
            key_a.0
        );
        // Different parent dir → different key
        let key_c = build_approval_key("read_file", &json!({"path": "Cargo.toml"}));
        assert_ne!(key_a, key_c);
    }

    #[test]
    fn write_file_uses_same_file_prefix_as_read() {
        let key = build_approval_key("write_file", &json!({"path": "src/main.rs"}));
        assert!(
            key.0.starts_with("file:"),
            "expected file: prefix for write_file, got {:?}",
            key.0
        );
        let key_edit = build_approval_key("edit_file", &json!({"path": "src/lib.rs"}));
        assert!(
            key_edit.0.starts_with("file:"),
            "expected file: prefix for edit_file, got {:?}",
            key_edit.0
        );
    }

    // ── build_session_approval_key tests ───────────────────────────

    #[test]
    fn session_key_for_read_file_uses_read_dir_prefix() {
        let key = build_session_approval_key("read_file", &json!({"path": "src/main.rs"}));
        assert!(
            key.0.starts_with("read:dir:"),
            "expected read:dir: prefix, got {:?}",
            key.0
        );
    }

    #[test]
    fn session_key_same_dir_reads_match() {
        let key_a = build_session_approval_key("read_file", &json!({"path": "src/main.rs"}));
        let key_b = build_session_approval_key("read_file", &json!({"path": "src/lib.rs"}));
        assert_eq!(key_a, key_b, "same dir should produce same session key");
    }

    #[test]
    fn session_key_diff_dir_reads_differ() {
        let key_a = build_session_approval_key("read_file", &json!({"path": "src/main.rs"}));
        let key_b = build_session_approval_key("read_file", &json!({"path": "Cargo.toml"}));
        assert_ne!(key_a, key_b, "different dirs should produce different keys");
    }

    #[test]
    fn session_key_write_and_edit_use_write_dir_prefix() {
        let key_w = build_session_approval_key("write_file", &json!({"path": "src/main.rs"}));
        assert!(key_w.0.starts_with("write:dir:"), "{:?}", key_w.0);
        let key_e = build_session_approval_key("edit_file", &json!({"path": "src/lib.rs"}));
        assert!(key_e.0.starts_with("write:dir:"), "{:?}", key_e.0);
        // Same dir → same key
        assert_eq!(key_w, key_e);
    }

    #[test]
    fn session_key_apply_patch_uses_patch_dir_prefix() {
        let key = build_session_approval_key(
            "apply_patch",
            &json!({"changes": [{"path": "src/main.rs", "content": "x"}]}),
        );
        assert!(
            key.0.starts_with("patch:dir:"),
            "expected patch:dir: prefix, got {:?}",
            key.0
        );
        // Same dir → same key
        let key2 = build_session_approval_key(
            "apply_patch",
            &json!({"changes": [{"path": "src/lib.rs", "content": "y"}]}),
        );
        assert_eq!(key, key2, "same dir patches should match");
    }

    #[test]
    fn session_key_shell_unchanged() {
        let key = build_session_approval_key("exec_shell", &json!({"command": "cargo build --release"}));
        assert_eq!(key.0, "shell:cargo build");
    }

    #[test]
    fn session_key_network_unchanged() {
        let key = build_session_approval_key("fetch_url", &json!({"url": "https://example.com/data"}));
        assert_eq!(key.0, "net:example.com");
    }

    #[test]
    fn session_key_generic_tool_uses_tool_prefix() {
        let key = build_session_approval_key("list_dir", &json!({"path": "src"}));
        assert_eq!(key.0, "tool:list_dir");
    }
}
