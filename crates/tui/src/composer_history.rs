//! Cross-session composer input history (#366).
//!
//! Persists user-typed prompts to `~/.deepseek/composer_history.txt` so
//! pressing Up-arrow at the composer recalls submissions from previous
//! sessions, not just the current one. One entry per line, oldest first,
//! capped at [`MAX_HISTORY_ENTRIES`] entries (older entries are pruned
//! at append time).
//!
//! Entries that begin with `/` (slash commands) are NOT stored — they
//! pollute the recall stream and the fuzzy slash-menu already covers
//! them. Empty / whitespace-only inputs are also skipped.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

/// Hard cap on persisted history. Keeps the file small (typical entries
/// are < 200 chars, so 1000 entries ≈ 200 KB) and bounds startup load
/// time.
pub const MAX_HISTORY_ENTRIES: usize = 1000;

const HISTORY_FILE_NAME: &str = "composer_history.txt";

fn history_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".deepseek").join(HISTORY_FILE_NAME))
}

/// Read the persisted history into memory. Returns an empty vec if the
/// file doesn't exist or can't be parsed — this is best-effort.
#[must_use]
pub fn load_history() -> Vec<String> {
    let Some(path) = history_path() else {
        return Vec::new();
    };
    let Ok(file) = fs::File::open(&path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .collect()
}

/// Append an entry to the persisted history, pruning old entries to
/// stay within [`MAX_HISTORY_ENTRIES`]. Slash-commands and empty input
/// are skipped — those don't help recall.
///
/// Best-effort — failures are logged via `tracing` but not propagated
/// because composer history is a UX nicety, not a correctness concern.
pub fn append_history(entry: &str) {
    let trimmed = entry.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return;
    }
    let Some(path) = history_path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        tracing::warn!(
            "Failed to create composer history dir {}: {err}",
            parent.display()
        );
        return;
    }

    // Read existing entries, append the new one, prune from the front
    // until under the cap, then atomically rewrite.
    let mut entries = load_history();
    if entries.last().map(String::as_str) == Some(trimmed) {
        // De-dupe consecutive duplicates — repeated submission of the
        // same prompt shouldn't bloat the file.
        return;
    }
    entries.push(trimmed.to_string());
    if entries.len() > MAX_HISTORY_ENTRIES {
        let excess = entries.len() - MAX_HISTORY_ENTRIES;
        entries.drain(0..excess);
    }

    let payload = entries.join("\n") + "\n";
    if let Err(err) = crate::utils::write_atomic(&path, payload.as_bytes()) {
        tracing::warn!(
            "Failed to persist composer history at {}: {err}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<R>(f: impl FnOnce() -> R) -> R {
        // Use the crate-wide test env mutex so we don't race with other
        // tests (config, restore, etc.) that also mutate HOME.
        let _guard = crate::test_support::lock_test_env();
        let tmp = tempfile::tempdir().expect("tempdir");
        let prev = std::env::var_os("HOME");
        // SAFETY: env mutation is serialized by the lock above.
        unsafe { std::env::set_var("HOME", tmp.path()) };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        match prev {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        match result {
            Ok(r) => r,
            Err(p) => std::panic::resume_unwind(p),
        }
    }

    #[test]
    fn append_and_load_round_trip() {
        with_temp_home(|| {
            append_history("first");
            append_history("second");
            append_history("third");
            let history = load_history();
            assert_eq!(history, vec!["first", "second", "third"]);
        });
    }

    #[test]
    fn slash_commands_skipped() {
        with_temp_home(|| {
            append_history("/help");
            append_history("real prompt");
            append_history("/cost");
            let history = load_history();
            assert_eq!(history, vec!["real prompt"]);
        });
    }

    #[test]
    fn empty_and_whitespace_skipped() {
        with_temp_home(|| {
            append_history("");
            append_history("   ");
            append_history("\n\t");
            append_history("real");
            let history = load_history();
            assert_eq!(history, vec!["real"]);
        });
    }

    #[test]
    fn consecutive_duplicates_deduped() {
        with_temp_home(|| {
            append_history("same");
            append_history("same");
            append_history("same");
            append_history("different");
            append_history("same");
            let history = load_history();
            assert_eq!(history, vec!["same", "different", "same"]);
        });
    }

    #[test]
    fn pruned_to_cap_at_append_time() {
        with_temp_home(|| {
            for i in 0..(MAX_HISTORY_ENTRIES + 50) {
                append_history(&format!("entry {i}"));
            }
            let history = load_history();
            assert_eq!(history.len(), MAX_HISTORY_ENTRIES);
            // Newest entries survive; oldest 50 were pruned.
            assert_eq!(history.first().map(String::as_str), Some("entry 50"));
            assert_eq!(
                history.last().map(String::as_str),
                Some(format!("entry {}", MAX_HISTORY_ENTRIES + 49)).as_deref()
            );
        });
    }

    #[test]
    fn missing_file_loads_empty() {
        with_temp_home(|| {
            let history = load_history();
            assert!(history.is_empty());
        });
    }
}
