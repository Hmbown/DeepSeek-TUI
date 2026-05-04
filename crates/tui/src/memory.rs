//! User-level memory file.
//!
//! v0.8.8 ships an MVP that lets the user keep a persistent personal
//! note file the model sees on every turn.
//!
//! v0.8.9 adds worktree-aware deduplication (#496): memory entries are
//! tagged with the git repo root they were created in, and when loading,
//! only entries matching the current repo (plus untagged global entries)
//! are returned. This prevents memories from one project bleeding into
//! another.
//!
//! - **Load** `~/.deepseek/memory.md` (path is configurable via
//!   `memory_path` in `config.toml` and `DEEPSEEK_MEMORY_PATH` env),
//!   filter by current git repo root, wrap remaining entries in a
//!   `<user_memory>` block, and prepend them to the system prompt.
//! - **`# foo`** typed in the composer appends `foo` to the memory
//!   file as a timestamped bullet — fast capture without leaving the TUI.
//!   When inside a git repo, the bullet is tagged with `[repo-root-path]`.
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

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;

/// Maximum size of the user memory file. Larger files are loaded but the
/// `<user_memory>` block carries a "(truncated)" marker so the user knows
/// the model only saw a slice. Mirrors `project_context::MAX_CONTEXT_SIZE`.
const MAX_MEMORY_SIZE: usize = 100 * 1024;

/// Regex: match a git-repo tag `[/path/to/root]` immediately after the
/// timestamp closing paren. Capture group 1 is the `(timestamp)` part,
/// group 2 is the `path` inside brackets.
///
/// A tagged line looks like:
///   - (2026-05-03 22:14 UTC) [/home/user/proj] note text
const REPO_TAG_RE: &str = r"^-\s*(\([^)]+\))\s+\[([^\]]+)\]\s*";

/// Discover the git repository root by walking up from `cwd` looking for a
/// `.git` directory (or file, in the case of worktrees and submodules).
/// Returns `None` when no git repository is found.
#[must_use]
pub fn discover_git_root(cwd: &Path) -> Option<PathBuf> {
    let mut current = if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        std::env::current_dir().ok().map(|c| c.join(cwd))?
    };

    // Canonicalize to resolve symlinks so repo comparisons are byte-identical.
    if let Ok(canon) = current.canonicalize() {
        current = canon;
    }

    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        match current.parent() {
            Some(parent) if parent != current => {
                current = parent.to_path_buf();
            }
            _ => return None,
        }
    }
}

/// Read the user memory file at `path`, filtering entries that belong to
/// the current git repo. When `git_root` is `Some`, entries tagged with a
/// different repo root are excluded; untagged entries pass through.
/// Returns `None` when the file doesn't exist or is empty after filtering.
#[must_use]
pub fn load(path: &Path, git_root: Option<&Path>) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let filtered = filter_by_repo(&content, git_root);
    if filtered.trim().is_empty() {
        return None;
    }
    Some(filtered)
}

/// Filter memory file content to only include entries relevant to the
/// current git repository. Lines are:
///
/// - **Tagged with another repo** — dropped (no bleed)
/// - **Tagged with current repo** — kept (entry belongs here)
/// - **Untagged** — kept (global / cross-project entries)
/// - **Non-entry lines** (headers, blank lines) — kept as-is
fn filter_by_repo(content: &str, git_root: Option<&Path>) -> String {
    let Some(root) = git_root else {
        // Not inside a git repo: return everything as-is (backward compatible).
        return content.to_string();
    };

    let root_str = root.to_string_lossy();
    let re = regex::Regex::new(REPO_TAG_RE).expect("static repo tag regex");
    let mut out = String::with_capacity(content.len());

    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            let tagged_path = caps
                .get(2)
                .map(|m| m.as_str())
                .unwrap_or("");
            if tagged_path == root_str.as_ref() {
                // Entry tagged with current repo: strip the tag prefix before adding.
                // Group 1 is the `(timestamp)` part; rebuild without the `[path]` tag.
                let ts = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                let rest = &line[caps.get(0).unwrap().len()..];
                out.push_str("- ");
                out.push_str(ts);
                out.push(' ');
                out.push_str(rest);
            } else {
                // Entry tagged with a different repo: skip.
            }
        } else {
            // Non-entry or untagged entry: pass through as-is.
            out.push_str(line);
        }
        out.push('\n');
    }

    out
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
        head.push_str("\n…(truncated, raise [memory].max_size or trim memory.md)");
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
/// `git_root` is used to filter entries by the current git repository
/// (worktree-aware deduplication, #496). Pass `None` to show all entries.
///
/// Callers that hold a `&Config` should pass `config.memory_enabled()` and
/// `config.memory_path()` directly. The split keeps this module
/// `Config`-free so it can be reused from sub-agent / engine boundaries
/// where the high-level `Config` isn't available.
#[must_use]
pub fn compose_block(enabled: bool, path: &Path, git_root: Option<&Path>) -> Option<String> {
    if !enabled {
        return None;
    }
    let content = load(path, git_root)?;
    as_system_block(&content, path)
}

/// Append `entry` to the memory file at `path`, creating it (and its
/// parent directory) if needed. The entry is timestamped so the user can
/// later see when each note was added. The leading `#` from a `# foo`
/// quick-add is stripped so the file stays as readable Markdown.
///
/// When `git_root` is `Some`, the entry is tagged with the repo root so
/// the worktree-aware loader can filter it by project (#496).
pub fn append_entry(path: &Path, entry: &str, git_root: Option<&Path>) -> io::Result<()> {
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

    if let Some(root) = git_root {
        let tag = root.to_string_lossy();
        writeln!(file, "- ({timestamp}) [{tag}] {trimmed}")?;
    } else {
        writeln!(file, "- ({timestamp}) {trimmed}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── discover_git_root ──────────────────────────────────────────────

    #[test]
    fn discover_git_root_finds_repo() {
        let tmp = tempdir().unwrap();
        // Create a minimal git repo
        let git_dir = tmp.path().join(".git");
        fs::create_dir(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();

        let found = discover_git_root(tmp.path());
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn discover_git_root_returns_none_outside_repo() {
        let tmp = tempdir().unwrap();
        assert!(discover_git_root(tmp.path()).is_none());
    }

    #[test]
    fn discover_git_root_finds_parent_repo() {
        let tmp = tempdir().unwrap();
        let git_dir = tmp.path().join(".git");
        fs::create_dir(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();

        let subdir = tmp.path().join("a").join("b").join("c");
        fs::create_dir_all(&subdir).unwrap();

        let found = discover_git_root(&subdir);
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap()
        );
    }

    // ── filter_by_repo ─────────────────────────────────────────────────

    #[test]
    fn filter_by_repo_without_git_root_returns_all() {
        let content = "- (2026-01-01) global note\n- (2026-01-02) [/repo/a] project note\n";
        let filtered = filter_by_repo(content, None);
        assert_eq!(filtered, content);
    }

    #[test]
    fn filter_by_repo_keeps_untagged_and_matching_tagged() {
        let content = "\
- (2026-01-01) global note
- (2026-01-02) [/home/user/proj-a] project note
- (2026-01-03) [/home/user/proj-b] other project note
some header";
        let git_root = Path::new("/home/user/proj-a");
        let filtered = filter_by_repo(content, Some(git_root));
        assert!(filtered.contains("global note"), "should keep untagged");
        assert!(
            filtered.contains("project note"),
            "should keep matching tagged"
        );
        assert!(
            !filtered.contains("other project note"),
            "should drop non-matching tagged"
        );
        assert!(filtered.contains("some header"), "should keep non-entry lines");
    }

    #[test]
    fn filter_by_repo_strips_repo_tag_from_matching_entries() {
        let content = "- (2026-01-01) [/tmp/repo] indentation convention";
        let git_root = Path::new("/tmp/repo");
        let filtered = filter_by_repo(content, Some(git_root));
        // The tag `[/tmp/repo]` should be stripped so the model sees a clean entry.
        assert!(filtered.contains("indentation convention"));
        assert!(!filtered.contains("[/tmp/repo]"), "tag should be stripped");
        // The timestamp and bullet should remain.
        assert!(filtered.starts_with("- (2026-01-01)"), "{filtered}");
    }

    #[test]
    fn filter_by_repo_empty_result_when_all_filtered() {
        let content = "- (2026-01-01) [/other/repo] only entry";
        let filtered = filter_by_repo(content, Some(Path::new("/this/repo")));
        assert_eq!(filtered.trim(), "");
    }

    // ── load ───────────────────────────────────────────────────────────

    #[test]
    fn load_returns_none_for_missing_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("never-existed.md");
        assert!(load(&path, None).is_none());
    }

    #[test]
    fn load_returns_none_for_whitespace_only_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(&path, "   \n   \n").unwrap();
        assert!(load(&path, None).is_none());
    }

    #[test]
    fn load_returns_content_for_real_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(&path, "remember the milk").unwrap();
        assert_eq!(load(&path, None).as_deref(), Some("remember the milk\n"));
    }

    #[test]
    fn load_filters_by_repo() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        fs::write(
            &path,
            "- (2026-01-01) global\n- (2026-01-02) [/my/repo] project note\n- (2026-01-03) [/other] other\n",
        )
        .unwrap();
        let git_root = Path::new("/my/repo");
        let result = load(&path, Some(git_root)).unwrap();
        assert!(result.contains("global"));
        assert!(result.contains("project note"));
        assert!(!result.contains("other"));
        // The repo tag should be stripped.
        assert!(!result.contains("[/my/repo]"));
    }

    // ── as_system_block ────────────────────────────────────────────────

    #[test]
    fn as_system_block_produces_xml_wrapper() {
        let block = as_system_block("note 1", Path::new("/tmp/m.md")).unwrap();
        assert!(block.contains("<user_memory source=\"/tmp/m.md\">"));
        assert!(block.contains("note 1"));
        assert!(block.ends_with("</user_memory>"));
    }

    #[test]
    fn as_system_block_returns_none_for_empty_content() {
        assert!(as_system_block("   ", Path::new("/tmp/m.md")).is_none());
    }

    #[test]
    fn as_system_block_truncates_oversize_input() {
        let big = "x".repeat(MAX_MEMORY_SIZE + 100);
        let block = as_system_block(&big, Path::new("/tmp/m.md")).unwrap();
        assert!(block.contains("(truncated"));
    }

    // ── append_entry ───────────────────────────────────────────────────

    #[test]
    fn append_entry_creates_file_and_writes_one_bullet() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        append_entry(&path, "# remember the milk", None).unwrap();

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
        append_entry(&path, "# first", None).unwrap();
        append_entry(&path, "second", None).unwrap();
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
        let err = append_entry(&path, "###", None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn append_entry_tags_with_git_root() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        let git_root = Path::new("/home/user/my-project");
        append_entry(&path, "use 4-space indentation", Some(git_root)).unwrap();

        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("[/home/user/my-project]"),
            "should contain repo tag: {body}"
        );
        assert!(body.contains("4-space indentation"));
    }

    #[test]
    fn append_entry_without_git_root_is_untagged() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("memory.md");
        append_entry(&path, "global preference", None).unwrap();

        let body = fs::read_to_string(&path).unwrap();
        assert!(
            !body.contains("[/"),
            "no tag should appear: {body}"
        );
    }
}
