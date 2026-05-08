//! Boot-time snapshot pruning.
//!
//! Called from `session_manager` once per session start. Failure is
//! never fatal — old snapshots taking disk space is annoying but not
//! correctness-breaking, so we log and move on.

use std::io;
use std::path::Path;
use std::time::Duration;

use super::paths::snapshot_git_dir;
use super::repo::SnapshotRepo;

/// Default snapshot retention window: 7 days.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Prune snapshots older than `max_age` for the given workspace.
///
/// If no snapshot repo exists yet (first run) this is a cheap no-op.
/// Returns the number of snapshots removed.
pub fn prune_older_than(workspace: &Path, max_age: Duration) -> io::Result<usize> {
    let git_dir = snapshot_git_dir(workspace);
    if !git_dir.exists() {
        return Ok(0);
    }
    let repo = SnapshotRepo::open_or_init(workspace)?;
    let removed = repo.prune_older_than(max_age)?;
    repo.prune_unreachable_objects()?;
    Ok(removed)
}

/// Prune the oldest snapshots until the `.git` directory fits within
/// `max_size_bytes`.
///
/// This is the hard safety net against unbounded snapshot growth (#1112).
/// When the snapshot directory exceeds the budget, we progressively drop
/// the oldest snapshots (in chronological batches) until the total size
/// is within limits or there are no more snapshots to remove.
///
/// Returns the number of snapshots removed.
pub fn prune_over_size(workspace: &Path, max_size_bytes: u64) -> io::Result<usize> {
    if max_size_bytes == 0 {
        return Ok(0); // 0 means unlimited
    }
    let git_dir = snapshot_git_dir(workspace);
    if !git_dir.exists() {
        return Ok(0);
    }
    let repo = SnapshotRepo::open_or_init(workspace)?;

    let mut total_removed = 0;
    loop {
        let current_size = repo.total_size();
        if current_size <= max_size_bytes {
            break;
        }
        let snapshots = repo.list(usize::MAX)?;
        if snapshots.len() <= 1 {
            // Nothing left to remove (or only the newest remains).
            break;
        }
        // Drop the oldest half each round for logarithmic convergence.
        let oldest = snapshots.last().unwrap();
        let newest = snapshots.first().unwrap();
        let age_span = newest.timestamp.saturating_sub(oldest.timestamp);
        if age_span == 0 {
            // All snapshots share the same timestamp (common in tests
            // or rapid-fire turns). Age-based pruning can't distinguish
            // them, so use count-based pruning instead.
            let keep_count = (snapshots.len() / 2).max(1);
            let removed = repo.prune_to_count(keep_count)?;
            total_removed += removed;
            if removed == 0 {
                break;
            }
        } else {
            let mid_ts = oldest.timestamp + age_span / 2;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let max_age_secs = (now - mid_ts).max(0) as u64;
            let removed = repo.prune_older_than(Duration::from_secs(max_age_secs))?;
            total_removed += removed;
            if removed == 0 {
                break;
            }
        }
    }
    repo.prune_unreachable_objects()?;
    Ok(total_removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_test_env;
    use std::sync::MutexGuard;
    use tempfile::tempdir;

    /// Same guard shape as in `repo::tests` — pins HOME for the lifetime
    /// of one test under the process-wide env mutex.
    struct ScopedHome {
        prev: Option<std::ffi::OsString>,
        _guard: MutexGuard<'static, ()>,
    }
    impl Drop for ScopedHome {
        fn drop(&mut self) {
            // SAFETY: process-wide lock still held.
            unsafe {
                match self.prev.take() {
                    Some(v) => std::env::set_var("HOME", v),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }
    fn scoped_home(home: &std::path::Path) -> ScopedHome {
        let guard = lock_test_env();
        let prev = std::env::var_os("HOME");
        // SAFETY: serialised by the global env lock.
        unsafe {
            std::env::set_var("HOME", home);
        }
        ScopedHome {
            prev,
            _guard: guard,
        }
    }

    #[test]
    fn prune_no_repo_returns_zero() {
        let tmp = tempdir().unwrap();
        let _home = scoped_home(tmp.path());
        let removed = prune_older_than(tmp.path(), DEFAULT_MAX_AGE).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn prune_with_existing_repo_zero_age_clears_all() {
        let tmp = tempdir().unwrap();
        let _home = scoped_home(tmp.path());
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();
        std::fs::write(workspace.join("f.txt"), "x").unwrap();
        repo.snapshot("turn:0").unwrap();

        // Same-second flake guard: see `repo::tests`.
        std::thread::sleep(Duration::from_millis(1100));

        let removed = prune_older_than(&workspace, Duration::from_secs(0)).unwrap();
        assert!(removed >= 1);
    }

    // ── prune_over_size tests (#1112) ────────────────────────────────

    #[test]
    fn prune_over_size_no_repo_returns_zero() {
        let tmp = tempdir().unwrap();
        let _home = scoped_home(tmp.path());
        let removed = prune_over_size(tmp.path(), 1024).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn prune_over_size_zero_budget_means_unlimited() {
        let tmp = tempdir().unwrap();
        let _home = scoped_home(tmp.path());
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();
        std::fs::write(workspace.join("f.txt"), "x").unwrap();
        repo.snapshot("turn:0").unwrap();

        // max_size_bytes = 0 → unlimited, nothing should be pruned.
        let removed = prune_over_size(&workspace, 0).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(repo.list(10).unwrap().len(), 1);
    }

    #[test]
    fn prune_over_size_does_nothing_when_under_budget() {
        let tmp = tempdir().unwrap();
        let _home = scoped_home(tmp.path());
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();
        std::fs::write(workspace.join("f.txt"), "x").unwrap();
        repo.snapshot("turn:0").unwrap();

        let current = repo.total_size();
        // Set budget to 10x current — nothing should be pruned.
        let removed = prune_over_size(&workspace, current * 10).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(repo.list(10).unwrap().len(), 1);
    }

    #[test]
    fn prune_over_size_removes_oldest_when_over_budget() {
        let tmp = tempdir().unwrap();
        let _home = scoped_home(tmp.path());
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();

        // Create multiple snapshots with unique data to grow the repo.
        for i in 0..5 {
            std::fs::write(
                workspace.join("f.txt"),
                format!("content {i} {}", "x".repeat(200)),
            )
            .unwrap();
            repo.snapshot(&format!("turn:{i}")).unwrap();
        }
        let count_before = repo.list(10).unwrap().len();
        assert_eq!(count_before, 5);

        // Set budget to 1 byte — should force aggressive pruning.
        let removed = prune_over_size(&workspace, 1).unwrap();
        assert!(removed > 0, "expected pruning, removed={removed}");

        // After pruning, there should be fewer snapshots.
        let count_after = repo.list(usize::MAX).unwrap().len();
        assert!(
            count_after < count_before,
            "snapshot count should decrease: {count_after} vs {count_before}"
        );
    }
    #[test]
    fn prune_over_size_preserves_newest_snapshots() {
        let tmp = tempdir().unwrap();
        let _home = scoped_home(tmp.path());
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();

        // Create 10 snapshots.
        for i in 0..10 {
            std::fs::write(workspace.join("f.txt"), format!("v{i} {}", "y".repeat(300))).unwrap();
            repo.snapshot(&format!("turn:{i}")).unwrap();
        }

        let count_before = repo.list(usize::MAX).unwrap().len();
        assert_eq!(count_before, 10);

        // Set budget to 1 byte — force aggressive pruning.
        let _removed = prune_over_size(&workspace, 1).unwrap();

        let remaining = repo.list(usize::MAX).unwrap();
        // At least the newest snapshot must survive.
        assert!(
            !remaining.is_empty(),
            "at least one snapshot must survive even under pressure"
        );
        // The newest surviving label must be "turn:9".
        assert_eq!(
            remaining[0].label, "turn:9",
            "newest snapshot should be preserved"
        );
        // Some but not all were removed.
        assert!(
            remaining.len() < count_before,
            "some snapshots should have been pruned"
        );
    }

    #[test]
    fn prune_over_size_converges_on_tiny_budget() {
        let tmp = tempdir().unwrap();
        let _home = scoped_home(tmp.path());
        let workspace = tmp.path().join("ws");
        std::fs::create_dir_all(&workspace).unwrap();
        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();

        std::fs::write(workspace.join("f.txt"), "data").unwrap();
        repo.snapshot("turn:0").unwrap();

        // Budget of 1 byte — impossible to satisfy, but must not infinite loop.
        // The prune should terminate (possibly leaving the repo over budget
        // if there's nothing left to remove).
        let result = prune_over_size(&workspace, 1);
        assert!(result.is_ok(), "must not hang or error: {result:?}");
    }
}
