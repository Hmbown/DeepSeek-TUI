//! Background file-watcher for the `/watch` command.
//!
//! Uses the `notify` crate to monitor filesystem changes and relay
//! them back to the TUI event loop via a `std::sync::mpsc` channel.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// A single file-change event from the watcher.
#[derive(Debug, Clone)]
pub struct WatchEvent {
    /// Path of the file that changed.
    pub path: PathBuf,
}

/// A running file-watcher instance.
///
/// Dropping this stops watching (the background thread in `RecommendedWatcher`
/// shuts down when the watcher is dropped).
pub struct WatchInstance {
    /// Receiver for file-change events. Poll with `try_recv()`.
    pub rx: mpsc::Receiver<WatchEvent>,
    /// The watcher must live as long as we want events.
    _watcher: RecommendedWatcher,
}

impl std::fmt::Debug for WatchInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchInstance")
            .field("rx", &"mpsc::Receiver")
            .field("_watcher", &"RecommendedWatcher")
            .finish()
    }
}

impl WatchInstance {
    /// Start watching `path` recursively for file modifications and creation.
    ///
    /// Returns an error if the path does not exist or the platform watcher
    /// cannot be initialised.
    pub fn start(path: impl Into<PathBuf>) -> Result<Self, WatchError> {
        let path = path.into();
        let canonical = path.canonicalize().map_err(|e| WatchError::Io {
            context: format!("cannot resolve path `{}`", path.display()),
            source: e,
        })?;

        if !canonical.exists() {
            return Err(WatchError::NotFound(canonical));
        }

        let (tx, rx) = mpsc::channel();

        let mut watcher = RecommendedWatcher::new(
            move |event: Result<Event, notify::Error>| {
                if let Ok(event) = event {
                    // Debounce: only surface modify and create events.
                    let is_modify = matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Create(_)
                    );
                    if !is_modify {
                        return;
                    }
                    for path in &event.paths {
                        // Skip temporary / swap files.
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("");
                        if name.starts_with('.')
                            || name.ends_with('~')
                            || name.ends_with(".swp")
                            || name.ends_with(".swx")
                        {
                            continue;
                        }
                        let _ = tx.send(WatchEvent { path: path.clone() });
                    }
                }
            },
            Config::default(),
        )
        .map_err(|e| WatchError::Notify(e))?;

        watcher
            .watch(&canonical, RecursiveMode::Recursive)
            .map_err(|e| WatchError::Notify(e))?;

        Ok(Self {
            rx,
            _watcher: watcher,
        })
    }

    /// Non-blocking poll for the next file-change event.
    pub fn poll(&self) -> Option<WatchEvent> {
        self.rx.try_recv().ok()
    }
}

/// Errors from `WatchInstance::start`.
#[derive(Debug)]
pub enum WatchError {
    /// The path does not exist.
    NotFound(PathBuf),
    /// Underlying notify crate error.
    #[allow(dead_code)]
    Notify(notify::Error),
    /// I/O error during path resolution.
    Io {
        context: String,
        source: std::io::Error,
    },
}

impl std::fmt::Display for WatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(p) => write!(f, "path not found: {}", p.display()),
            Self::Notify(e) => write!(f, "notify error: {e}"),
            Self::Io { context, source } => {
                write!(f, "{context}: {source}")
            }
        }
    }
}

impl std::error::Error for WatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Notify(e) => Some(e),
            Self::NotFound(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: show diff between current and a previous file state
// ---------------------------------------------------------------------------

/// Attempt to read a file and show a brief unified diff against a previous
/// snapshot. Returns `None` if the file is binary, too large, or missing.
pub fn show_diff(path: &Path, old_content: &str) -> Option<String> {
    let new_content = std::fs::read_to_string(path).ok()?;
    if new_content.len() > 200_000 {
        return None; // too large for inline diff
    }
    let diff = similar::TextDiff::from_lines(old_content, &new_content);
    let mut out = Vec::new();
    let mut changed = 0;
    for change in diff.iter_all_changes() {
        let tag = match change.tag() {
            similar::ChangeTag::Delete => '-',
            similar::ChangeTag::Insert => '+',
            similar::ChangeTag::Equal => ' ',
        };
        let line = format!("{}{}", tag, change.value());
        if tag != ' ' {
            changed += 1;
        }
        // Cap output at 40 diff lines
        if out.len() >= 40 {
            out.push("  ... (truncated)".to_string());
            break;
        }
        out.push(line);
    }
    if changed == 0 {
        return None; // no meaningful change
    }
    Some(out.join(""))
}

/// Look up a previously cached file content for diff display.
///
/// Returns a best-effort snapshot from the filesystem (the first read).
/// The caller should cache this before the watcher overwrites it.
pub fn read_snapshot(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().filter(|s| s.len() < 200_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_start_watches_existing_path() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello").unwrap();

        let instance = WatchInstance::start(&file).expect("should start watching");
        assert!(instance.rx.try_recv().is_err()); // no events yet
    }

    #[test]
    fn test_start_fails_on_nonexistent_path() {
        let err = WatchInstance::start("/nonexistent/path/xyz").unwrap_err();
        // On most platforms canonicalize() returns Io error, so we check
        // that the error is not a success.
        assert!(
            matches!(&err, WatchError::NotFound(_) | WatchError::Io { .. }),
            "expected NotFound or Io error, got: {err:?}"
        );
    }

    #[test]
    fn test_show_diff_detects_changes() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline2 modified\nline3\n";
        let dir = tempdir().unwrap();
        let file = dir.path().join("diff.txt");
        fs::write(&file, new).unwrap();

        let diff = show_diff(&file, old);
        assert!(diff.is_some());
        let d = diff.unwrap();
        assert!(d.contains("-line2"));
        assert!(d.contains("+line2 modified"));
    }

    #[test]
    fn test_show_diff_returns_none_for_identical() {
        let content = "same\ncontent\n";
        let dir = tempdir().unwrap();
        let file = dir.path().join("same.txt");
        fs::write(&file, content).unwrap();

        let diff = show_diff(&file, content);
        assert!(diff.is_none());
    }
}
