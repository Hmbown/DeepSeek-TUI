//! Worktree manager service (#452)
//!
//! Manages git worktrees for branch-isolated development.
//! Worktrees live under `<repo>/.worktrees/<branch>/`.
//!
//! # Usage
//!
//! ```bash
//! # Create a worktree and launch a TUI session in it
//! deepseek worktree create <branch>
//!
//! # List all active worktrees
//! deepseek worktree list
//!
//! # Remove a worktree by branch name
//! deepseek worktree rm <branch>
//! ```

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Run `git worktree add` to create a worktree for the given branch, then
/// re-invoke `deepseek` inside that worktree (launching the TUI).
pub fn run_worktree_create(branch: &str, workspace: &Path) -> Result<()> {
    if !is_git_repo(workspace) {
        bail!("Not a git repository: {}", workspace.display());
    }

    let worktree_dir = workspace.join(".worktrees").join(branch);

    if worktree_dir.exists() {
        bail!("Worktree already exists at {}", worktree_dir.display());
    }

    // Check if branch exists locally
    let branch_exists = git_has_local_branch(workspace, branch);

    // Create the worktree
    let mut cmd = Command::new("git");
    cmd.current_dir(workspace);
    cmd.arg("worktree").arg("add");
    if branch_exists {
        // Branch exists — check it out into the worktree
        cmd.arg(&worktree_dir).arg(branch);
    } else {
        // Create branch + worktree at once (creates branch from HEAD)
        cmd.arg("-B").arg(branch).arg(&worktree_dir);
    }

    let output = cmd
        .output()
        .context("Failed to execute `git worktree add`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree add failed:\n{stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("{stdout}");

    // Re-invoke `deepseek` inside the new worktree
    let self_exe = std::env::current_exe().context("Failed to get current executable path")?;

    eprintln!("Starting DeepSeek in worktree: {}", worktree_dir.display());

    let mut child = Command::new(&self_exe)
        .args(std::env::args().skip(2).skip_while(|a| a != "--")) // skip `worktree create <branch>`
        .current_dir(&worktree_dir)
        .spawn()
        .with_context(|| format!("Failed to launch deepseek in {}", worktree_dir.display()))?;

    let status = child.wait()?;
    if !status.success() {
        bail!("DeepSeek exited with status: {status}");
    }
    Ok(())
}

/// List all active git worktrees for the current repo.
pub fn run_worktree_list(workspace: &Path) -> Result<()> {
    if !is_git_repo(workspace) {
        bail!("Not a git repository: {}", workspace.display());
    }

    let output = Command::new("git")
        .args(["worktree", "list"])
        .current_dir(workspace)
        .output()
        .context("Failed to execute `git worktree list`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree list failed:\n{stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    print!("{stdout}");
    Ok(())
}

/// Remove a worktree by branch name. The worktree path is derived as
/// `.worktrees/<branch>` under the workspace root.
pub fn run_worktree_rm(branch: &str, workspace: &Path) -> Result<()> {
    if !is_git_repo(workspace) {
        bail!("Not a git repository: {}", workspace.display());
    }

    let worktree_dir = workspace.join(".worktrees").join(branch);

    if !worktree_dir.exists() {
        bail!("Worktree directory not found at {}", worktree_dir.display());
    }

    // First try `git worktree remove` (safe — rejects dirty trees)
    let output = Command::new("git")
        .args(["worktree", "remove", &worktree_dir.to_string_lossy()])
        .current_dir(workspace)
        .output()
        .context("Failed to execute `git worktree remove`")?;

    if output.status.success() {
        println!("Removed worktree: .worktrees/{branch}");
        // Also prune stale worktree metadata
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(workspace)
            .output();
        return Ok(());
    }

    // `git worktree remove` failed. Try force-remove.
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!(
        "`git worktree remove` failed ({}), trying force...",
        stderr.trim()
    );

    let output = Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            &worktree_dir.to_string_lossy(),
        ])
        .current_dir(workspace)
        .output()
        .context("Failed to execute `git worktree remove --force`")?;

    if output.status.success() {
        println!("Force-removed worktree: .worktrees/{branch}");
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(workspace)
            .output();
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree remove failed:\n{err}");
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Check if the given directory is inside a git repository.
fn is_git_repo(path: &Path) -> bool {
    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .output();
    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Check if a branch exists locally.
fn git_has_local_branch(repo: &Path, branch: &str) -> bool {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .current_dir(repo)
        .output();
    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}
