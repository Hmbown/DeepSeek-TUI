//! Side-git repository wrapper for workspace snapshots.
//!
//! `SnapshotRepo` shells out to the system `git` binary (we deliberately
//! avoid `git2` to dodge its LGPL surface). The two paths that matter:
//!
//! - `git_dir`  → `~/.deepseek/snapshots/<project_hash>/<worktree_hash>/.git`
//! - `work_tree` → the user's actual workspace
//!
//! Every git invocation passes both `--git-dir` AND `--work-tree`. That is
//! the single biggest safety mechanism: it guarantees we never accidentally
//! mutate the user's own `.git` directory. If git can't find the side
//! repo, the command fails fast instead of falling back to "current
//! directory".

use std::collections::HashSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::paths::{ensure_snapshot_dir, snapshot_git_dir};

/// Identifier for a snapshot — currently the underlying git commit SHA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotId(pub String);

impl SnapshotId {
    /// Borrow the SHA as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A single snapshot record (one row in `git log`).
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Commit SHA inside the side repo.
    pub id: SnapshotId,
    /// Subject line — the label passed to [`SnapshotRepo::snapshot`].
    pub label: String,
    /// Author timestamp (Unix seconds).
    pub timestamp: i64,
}

/// Wrapper around the per-workspace side-git repo.
pub struct SnapshotRepo {
    git_dir: PathBuf,
    work_tree: PathBuf,
}

impl SnapshotRepo {
    /// Open or initialize the snapshot repo for `workspace`.
    ///
    /// On first use this:
    /// 1. Creates the `~/.deepseek/snapshots/<…>/.git` dir.
    /// 2. Runs `git init --bare=false --quiet`.
    /// 3. Sets a fixed `user.name` / `user.email` so commits don't pick up
    ///    the user's global git identity (we don't want our snapshots to
    ///    look like they came from the user).
    pub fn open_or_init(workspace: &Path) -> io::Result<Self> {
        let work_tree = workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.to_path_buf());

        let _ = ensure_snapshot_dir(&work_tree)?;
        let git_dir = snapshot_git_dir(&work_tree);

        let needs_init = !git_dir.exists();
        if needs_init {
            let parent = git_dir.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "snapshot dir has no parent")
            })?;
            std::fs::create_dir_all(parent)?;
            // `git init` here uses the parent directory as the work tree
            // and stores metadata in `.git`. We then continue to use
            // explicit `--git-dir` / `--work-tree` flags for every other
            // command so behaviour is invariant of cwd.
            let init = Command::new("git")
                .arg("init")
                .arg("--quiet")
                .arg(parent)
                .output()
                .map_err(|e| io_other(format!("failed to spawn git init: {e}")))?;
            if !init.status.success() {
                return Err(io_other(format!(
                    "git init failed: {}",
                    String::from_utf8_lossy(&init.stderr).trim()
                )));
            }

            // Pin a stable identity so snapshot commits are recognisable
            // and don't bleed into the user's git config.
            let _ = run_git(
                &git_dir,
                &work_tree,
                &["config", "user.name", "deepseek-snapshots"],
            );
            let _ = run_git(
                &git_dir,
                &work_tree,
                &["config", "user.email", "snapshots@deepseek-tui.local"],
            );
            // Don't auto-gc on every commit; we manage pruning ourselves.
            let _ = run_git(&git_dir, &work_tree, &["config", "gc.auto", "0"]);
            // Ignore CRLF rewriting — we want byte-for-byte fidelity.
            let _ = run_git(&git_dir, &work_tree, &["config", "core.autocrlf", "false"]);
        }

        Ok(Self { git_dir, work_tree })
    }

    /// Take a snapshot of the current working tree.
    ///
    /// Internally: `git add -A`, `git write-tree`, `git commit-tree`, then
    /// `git update-ref HEAD <commit>`.
    /// `git add -A` honours the user's workspace ignore rules while staging
    /// into the side repo's index.
    ///
    /// Returns the snapshot's commit SHA.
    pub fn snapshot(&self, label: &str) -> io::Result<SnapshotId> {
        // Stage every tracked + untracked path the workspace exposes.
        // `--all` here means `add` + `update` + `remove` — the same set
        // `git status` would show.
        let add = run_git(&self.git_dir, &self.work_tree, &["add", "-A"])?;
        if !add.status.success() {
            return Err(io_other(format!(
                "git add -A failed: {}",
                String::from_utf8_lossy(&add.stderr).trim()
            )));
        }

        let tree = run_git(&self.git_dir, &self.work_tree, &["write-tree"])?;
        if !tree.status.success() {
            return Err(io_other(format!(
                "git write-tree failed: {}",
                String::from_utf8_lossy(&tree.stderr).trim()
            )));
        }
        let tree = String::from_utf8_lossy(&tree.stdout).trim().to_string();

        let parent = run_git(
            &self.git_dir,
            &self.work_tree,
            &["rev-parse", "--verify", "HEAD"],
        )?;
        let parent = parent
            .status
            .success()
            .then(|| String::from_utf8_lossy(&parent.stdout).trim().to_string())
            .filter(|s| !s.is_empty());

        let mut args = vec!["commit-tree".to_string(), tree];
        if let Some(parent) = parent {
            args.push("-p".to_string());
            args.push(parent);
        }
        args.push("-m".to_string());
        args.push(label.to_string());
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        // `commit-tree` creates marker commits even when the tree matches its
        // parent, and it does not run user/global commit hooks.
        let commit = run_git(&self.git_dir, &self.work_tree, &arg_refs)?;
        if !commit.status.success() {
            return Err(io_other(format!(
                "git commit-tree failed: {}",
                String::from_utf8_lossy(&commit.stderr).trim()
            )));
        }
        let sha = String::from_utf8_lossy(&commit.stdout).trim().to_string();

        let update = run_git(
            &self.git_dir,
            &self.work_tree,
            &["update-ref", "HEAD", &sha],
        )?;
        if !update.status.success() {
            return Err(io_other(format!(
                "git update-ref HEAD failed: {}",
                String::from_utf8_lossy(&update.stderr).trim()
            )));
        }

        Ok(SnapshotId(sha))
    }

    /// Restore the workspace to the state at `id`.
    ///
    /// Uses `git checkout <sha> -- :/` which checks out every path in the
    /// snapshot tree relative to the workspace root. We do NOT touch the
    /// user's own `.git` — snapshots only contain working-tree files.
    pub fn restore(&self, id: &SnapshotId) -> io::Result<()> {
        let current_paths = self.tree_paths("HEAD")?;
        let target_paths = self.tree_paths(id.as_str())?;
        let checkout = run_git(
            &self.git_dir,
            &self.work_tree,
            &["checkout", id.as_str(), "--", ":/"],
        )?;
        if !checkout.status.success() {
            return Err(io_other(format!(
                "git checkout failed: {}",
                String::from_utf8_lossy(&checkout.stderr).trim()
            )));
        }
        self.remove_paths_missing_from_target(&current_paths, &target_paths)?;
        Ok(())
    }

    fn tree_paths(&self, treeish: &str) -> io::Result<HashSet<PathBuf>> {
        let ls = run_git(
            &self.git_dir,
            &self.work_tree,
            &["ls-tree", "-r", "-z", "--name-only", treeish],
        )?;
        if !ls.status.success() {
            return Err(io_other(format!(
                "git ls-tree failed: {}",
                String::from_utf8_lossy(&ls.stderr).trim()
            )));
        }
        Ok(parse_nul_paths(&ls.stdout))
    }

    fn remove_paths_missing_from_target(
        &self,
        current_paths: &HashSet<PathBuf>,
        target_paths: &HashSet<PathBuf>,
    ) -> io::Result<()> {
        for rel in current_paths.difference(target_paths) {
            if !is_safe_relative_path(rel) {
                continue;
            }
            let path = self.work_tree.join(rel);
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_dir() {
                let _ = std::fs::remove_dir(&path);
            } else {
                std::fs::remove_file(&path)?;
            }
            self.prune_empty_parent_dirs(path.parent());
        }
        Ok(())
    }

    fn prune_empty_parent_dirs(&self, mut dir: Option<&Path>) {
        while let Some(path) = dir {
            if path == self.work_tree {
                break;
            }
            if std::fs::remove_dir(path).is_err() {
                break;
            }
            dir = path.parent();
        }
    }

    /// List up to `limit` most-recent snapshots, newest first.
    pub fn list(&self, limit: usize) -> io::Result<Vec<Snapshot>> {
        // `git log -<n>` is the short form of `--max-count=<n>`; if `limit`
        // is `usize::MAX` (caller asked for "everything") we pass an empty
        // count so git defaults to no upper bound.
        let mut args: Vec<String> = vec!["log".to_string()];
        if limit < usize::MAX {
            args.push(format!("--max-count={limit}"));
        }
        args.push("--pretty=format:%H%x09%at%x09%s".to_string());
        args.push("--no-color".to_string());
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let log = run_git(&self.git_dir, &self.work_tree, &arg_refs)?;
        if !log.status.success() {
            // No commits yet → empty list.
            return Ok(Vec::new());
        }
        let stdout = String::from_utf8_lossy(&log.stdout);
        let mut out = Vec::new();
        for line in stdout.lines() {
            let mut parts = line.splitn(3, '\t');
            let sha = parts.next().unwrap_or("").to_string();
            let ts = parts
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            let subject = parts.next().unwrap_or("").to_string();
            if sha.is_empty() {
                continue;
            }
            out.push(Snapshot {
                id: SnapshotId(sha),
                label: subject,
                timestamp: ts,
            });
        }
        Ok(out)
    }

    /// Drop snapshots older than `max_age`, returning the count removed.
    ///
    /// Strategy: identify keepable commits (younger than the cutoff),
    /// reset HEAD to the oldest survivor, then `git reflog expire` +
    /// `git gc --prune=now` to actually reclaim space. Cheap and avoids
    /// rewriting history when nothing has aged out.
    pub fn prune_older_than(&self, max_age: Duration) -> io::Result<usize> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| io_other(format!("clock error: {e}")))?
            .as_secs() as i64;
        let cutoff = now - max_age.as_secs() as i64;

        let snapshots = self.list(usize::MAX)?;
        if snapshots.is_empty() {
            return Ok(0);
        }

        // Snapshots are newest-first. Find the index of the first one
        // at-or-older than the cutoff — every entry from that index
        // onward is a candidate for removal. We use `<=` so a 0-second
        // retention drops same-second commits (otherwise tests calling
        // `prune_older_than(Duration::ZERO)` immediately after creating
        // a snapshot would never prune anything).
        let cut_index = snapshots.iter().position(|s| s.timestamp <= cutoff);
        let Some(cut) = cut_index else {
            return Ok(0);
        };
        let removed = snapshots.len() - cut;
        if removed == 0 {
            return Ok(0);
        }

        if cut == 0 {
            // Every snapshot is older than the cutoff — wipe the repo
            // entirely so the next snapshot starts a fresh history.
            // Removing `.git/refs/heads/*` is enough to orphan the old
            // commits, then gc reclaims them.
            let refs_dir = self.git_dir.join("refs").join("heads");
            if refs_dir.exists() {
                for entry in std::fs::read_dir(&refs_dir)? {
                    let path = entry?.path();
                    if path.is_file() {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
            // Also drop HEAD's packed refs so `git log` returns nothing.
            let packed = self.git_dir.join("packed-refs");
            if packed.exists() {
                let _ = std::fs::remove_file(&packed);
            }
        } else {
            // Reset HEAD to the youngest commit older-than-cutoff's
            // *predecessor* — i.e. the oldest surviving snapshot.
            let survivor = &snapshots[cut - 1];
            let reset = run_git(
                &self.git_dir,
                &self.work_tree,
                &["update-ref", "HEAD", survivor.id.as_str()],
            )?;
            if !reset.status.success() {
                return Err(io_other(format!(
                    "git update-ref failed: {}",
                    String::from_utf8_lossy(&reset.stderr).trim()
                )));
            }
        }

        // Reclaim space.
        let _ = run_git(
            &self.git_dir,
            &self.work_tree,
            &["reflog", "expire", "--expire=now", "--all"],
        );
        let _ = run_git(
            &self.git_dir,
            &self.work_tree,
            &["gc", "--prune=now", "--quiet"],
        );

        Ok(removed)
    }

    /// Return the side-repo's `.git` directory (for diagnostics).
    #[allow(dead_code)]
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// Return the work tree path (for diagnostics).
    #[allow(dead_code)]
    pub fn work_tree(&self) -> &Path {
        &self.work_tree
    }
}

fn run_git(git_dir: &Path, work_tree: &Path, args: &[&str]) -> io::Result<Output> {
    Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .arg("--work-tree")
        .arg(work_tree)
        .args(args)
        .output()
}

fn io_other(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

fn parse_nul_paths(bytes: &[u8]) -> HashSet<PathBuf> {
    bytes
        .split(|b| *b == 0)
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| PathBuf::from(String::from_utf8_lossy(chunk).into_owned()))
        .collect()
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_test_env;
    use std::sync::MutexGuard;
    use tempfile::tempdir;

    /// Holds HOME pinned to a tempdir for the lifetime of a test. Also
    /// owns the process-wide env-var mutex so tests across modules
    /// don't trample each other's `HOME`.
    pub(super) struct ScopedHome {
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
    pub(super) fn scoped_home(home: &Path) -> ScopedHome {
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

    /// Build a side-repo whose snapshot dir lives under the same
    /// tempdir we're using for `HOME` — so the inner `dirs::home_dir()`
    /// lookup stays inside our sandbox. Returns the guard alongside so
    /// the caller can keep HOME pinned for the rest of the test.
    fn make_repo(tmp: &Path) -> (SnapshotRepo, ScopedHome) {
        let workspace = tmp.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let guard = scoped_home(tmp);
        let repo = SnapshotRepo::open_or_init(&workspace).expect("open_or_init");
        (repo, guard)
    }

    #[test]
    fn snapshot_creates_commit_in_side_repo_only() {
        let tmp = tempdir().unwrap();
        let (repo, _home) = make_repo(tmp.path());
        std::fs::write(repo.work_tree().join("a.txt"), b"alpha").unwrap();

        let id = repo.snapshot("pre-turn:1").expect("snapshot");
        assert_eq!(id.as_str().len(), 40);

        let list = repo.list(10).expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label, "pre-turn:1");

        // The user's workspace must NOT have a real `.git` because we
        // never created one in their workspace — only in the side dir.
        assert!(!repo.work_tree().join(".git").exists());
    }

    #[test]
    fn restore_reverts_workspace_files() {
        let tmp = tempdir().unwrap();
        let (repo, _home) = make_repo(tmp.path());
        let f = repo.work_tree().join("file.txt");

        std::fs::write(&f, b"original").unwrap();
        let id = repo.snapshot("pre-turn:1").expect("snapshot");

        std::fs::write(&f, b"clobbered").unwrap();
        repo.snapshot("post-turn:1").expect("snapshot 2");

        repo.restore(&id).expect("restore");
        let after = std::fs::read_to_string(&f).unwrap();
        assert_eq!(after, "original");
    }

    #[test]
    fn restore_removes_files_added_after_target_snapshot() {
        let tmp = tempdir().unwrap();
        let (repo, _home) = make_repo(tmp.path());
        let original = repo.work_tree().join("original.txt");
        let added = repo.work_tree().join("added.txt");

        std::fs::write(&original, b"original").unwrap();
        let id = repo.snapshot("pre-turn:1").expect("snapshot");

        std::fs::write(&added, b"new file").unwrap();
        repo.snapshot("post-turn:1").expect("snapshot 2");

        repo.restore(&id).expect("restore");
        assert!(original.exists());
        assert!(!added.exists(), "restore must remove tracked added files");
    }

    #[test]
    fn snapshot_and_restore_do_not_move_user_git_head() {
        let tmp = tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .arg("init")
            .arg("--quiet")
            .status()
            .unwrap();
        std::fs::write(workspace.join("tracked.txt"), b"committed").unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .arg("add")
            .arg("tracked.txt")
            .status()
            .unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .arg("-c")
            .arg("user.name=user")
            .arg("-c")
            .arg("user.email=user@example.test")
            .arg("commit")
            .arg("--quiet")
            .arg("-m")
            .arg("init")
            .status()
            .unwrap();
        let user_head_before = Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;

        let _home = scoped_home(tmp.path());
        let repo = SnapshotRepo::open_or_init(&workspace).unwrap();
        std::fs::write(workspace.join("tracked.txt"), b"dirty-before").unwrap();
        let id = repo.snapshot("pre-turn:1").unwrap();
        std::fs::write(workspace.join("tracked.txt"), b"dirty-after").unwrap();
        repo.snapshot("post-turn:1").unwrap();
        repo.restore(&id).unwrap();

        let user_head_after = Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;
        assert_eq!(user_head_after, user_head_before);
        assert_eq!(
            std::fs::read_to_string(workspace.join("tracked.txt")).unwrap(),
            "dirty-before"
        );
    }

    #[test]
    fn list_respects_limit() {
        let tmp = tempdir().unwrap();
        let (repo, _home) = make_repo(tmp.path());
        for i in 0..5 {
            std::fs::write(repo.work_tree().join("f.txt"), format!("v{i}")).unwrap();
            repo.snapshot(&format!("turn:{i}")).unwrap();
        }
        let three = repo.list(3).unwrap();
        assert_eq!(three.len(), 3);
        // Newest first.
        assert_eq!(three[0].label, "turn:4");
    }

    #[test]
    fn prune_drops_snapshots_older_than_threshold() {
        let tmp = tempdir().unwrap();
        let (repo, _home) = make_repo(tmp.path());
        std::fs::write(repo.work_tree().join("f.txt"), "v0").unwrap();
        repo.snapshot("turn:0").unwrap();

        // Wait one second so the snapshot's commit timestamp is strictly
        // in the past relative to the prune call's "now" — otherwise
        // same-second comparisons make the assertion flaky.
        std::thread::sleep(Duration::from_millis(1100));

        let removed = repo.prune_older_than(Duration::from_secs(0)).unwrap();
        assert!(removed >= 1, "expected at least 1 pruned, got {removed}");

        // After pruning everything, the next snapshot should start a
        // fresh history.
        std::fs::write(repo.work_tree().join("f.txt"), "v1").unwrap();
        repo.snapshot("turn:1").unwrap();
        let list = repo.list(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label, "turn:1");
    }

    #[test]
    fn snapshot_respects_workspace_gitignore() {
        let tmp = tempdir().unwrap();
        let (repo, _home) = make_repo(tmp.path());
        std::fs::write(repo.work_tree().join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(repo.work_tree().join("ignored.txt"), b"secret").unwrap();
        std::fs::write(repo.work_tree().join("kept.txt"), b"public").unwrap();

        let id = repo.snapshot("pre-turn:1").expect("snapshot");

        // `git ls-tree` against the snapshot's commit shouldn't list ignored.txt.
        let ls = run_git(
            repo.git_dir(),
            repo.work_tree(),
            &["ls-tree", "-r", "--name-only", id.as_str()],
        )
        .expect("ls-tree");
        let names = String::from_utf8_lossy(&ls.stdout);
        assert!(names.contains("kept.txt"), "kept.txt missing: {names}");
        assert!(
            !names.contains("ignored.txt"),
            "ignored.txt should not be in snapshot: {names}",
        );
    }

    #[test]
    fn open_or_init_is_idempotent() {
        let tmp = tempdir().unwrap();
        let (_r, _h) = make_repo(tmp.path());
        // Second open should not panic and should reuse the existing
        // `.git`. We re-open via the public API rather than make_repo to
        // avoid double-acquiring HOME (the guard would deadlock).
        drop((_r, _h));
        let (_r2, _h2) = make_repo(tmp.path());
    }
}
