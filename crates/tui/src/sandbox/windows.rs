//! Windows process-containment helpers.
//!
//! This module intentionally does **not** claim to implement filesystem or
//! network sandboxing on Windows. The first Windows backend is a helper-runner
//! that places the command in a Job Object with kill-on-close semantics. That
//! prevents leaked child process trees, but it does not enforce ReadOnly or
//! WorkspaceWrite access policies.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use super::SandboxPolicy;

const HELPER_BIN_NAME: &str = "deepseek-windows-sandbox-helper.exe";
static AVAILABLE_HELPER: OnceLock<Option<PathBuf>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsSandboxKind {
    /// Process-tree containment via Windows Job Objects.
    JobObjectContainment,
}

impl std::fmt::Display for WindowsSandboxKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WindowsSandboxKind::JobObjectContainment => write!(f, "job-object-containment"),
        }
    }
}

/// Check if a usable Windows helper backend is available.
pub fn is_available() -> bool {
    available_helper_path().is_some()
}

pub fn select_best_kind(_policy: &SandboxPolicy, _cwd: &Path) -> Option<WindowsSandboxKind> {
    if is_available() {
        Some(WindowsSandboxKind::JobObjectContainment)
    } else {
        None
    }
}

pub fn build_helper_command(
    spec_program: &str,
    spec_args: &[String],
    spec_timeout: Duration,
    cwd: &Path,
) -> Option<Vec<String>> {
    let helper = available_helper_path()?;

    let timeout_ms = spec_timeout.as_millis().min(u128::from(u64::MAX)) as u64;
    let mut command = vec![
        helper.to_string_lossy().into_owned(),
        "--contain".to_string(),
        "--cwd".to_string(),
        cwd.to_string_lossy().into_owned(),
        "--timeout-ms".to_string(),
        timeout_ms.to_string(),
        "--".to_string(),
        spec_program.to_string(),
    ];
    command.extend(spec_args.iter().cloned());
    Some(command)
}

pub fn detect_denial(exit_code: i32, stderr: &str) -> bool {
    if exit_code == 0 {
        return false;
    }

    let patterns = [
        "Access is denied",
        "access denied",
        "STATUS_ACCESS_DENIED",
        "privilege",
        "AppContainer",
        "sandbox",
    ];

    patterns.iter().any(|p| stderr.contains(p))
}

fn helper_path() -> Option<PathBuf> {
    if let Some(path) = helper_path_from_env() {
        return Some(path);
    }

    let current = std::env::current_exe().ok()?;
    helper_candidates(&current)
        .into_iter()
        .find(|candidate| candidate.is_file())
}

fn available_helper_path() -> Option<PathBuf> {
    AVAILABLE_HELPER
        .get_or_init(|| {
            let helper = helper_path()?;
            helper_self_test(&helper).then_some(helper)
        })
        .clone()
}

fn helper_path_from_env() -> Option<PathBuf> {
    std::env::var_os("DEEPSEEK_WINDOWS_SANDBOX_HELPER")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn helper_candidates(current_exe: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(dir) = current_exe.parent() {
        candidates.push(dir.join(HELPER_BIN_NAME));

        // Cargo test binaries live in target/{debug,release}/deps while helper
        // binaries live one directory up.
        if dir.file_name().is_some_and(|name| name == "deps")
            && let Some(parent) = dir.parent()
        {
            candidates.push(parent.join(HELPER_BIN_NAME));
        }
    }

    candidates
}

fn helper_self_test(helper: &Path) -> bool {
    Command::new(helper)
        .arg("--self-test")
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_candidates_include_current_dir() {
        let current = Path::new(r"C:\repo\target\debug\deepseek-tui.exe");
        let candidates = helper_candidates(current);
        assert_eq!(
            candidates,
            vec![PathBuf::from(
                r"C:\repo\target\debug\deepseek-windows-sandbox-helper.exe"
            )]
        );
    }

    #[test]
    fn helper_candidates_include_cargo_test_parent_dir() {
        let current = Path::new(r"C:\repo\target\debug\deps\deepseek_tui-test.exe");
        let candidates = helper_candidates(current);
        assert_eq!(
            candidates,
            vec![
                PathBuf::from(r"C:\repo\target\debug\deps\deepseek-windows-sandbox-helper.exe"),
                PathBuf::from(r"C:\repo\target\debug\deepseek-windows-sandbox-helper.exe"),
            ]
        );
    }
}
