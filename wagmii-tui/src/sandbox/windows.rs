//! Windows sandbox implementation (best-effort placeholder).
//!
//! Windows sandboxing can be implemented using:
//! - Windows Sandbox (full isolation)
//! - AppContainer (process isolation)
//! - Restricted tokens (reduced privileges)
//!
//! This module selects a preferred approach and exposes helpers used by the
//! sandbox manager. Full enforcement should be implemented in a helper binary.

use std::path::Path;

use super::SandboxPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsSandboxKind {
    WindowsSandbox,
    AppContainer,
    RestrictedToken,
}

impl std::fmt::Display for WindowsSandboxKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WindowsSandboxKind::WindowsSandbox => write!(f, "sandbox"),
            WindowsSandboxKind::AppContainer => write!(f, "appcontainer"),
            WindowsSandboxKind::RestrictedToken => write!(f, "restricted-token"),
        }
    }
}

pub fn is_available() -> bool {
    windows_sandbox_available() || appcontainer_available() || restricted_token_available()
}

pub fn select_best_kind(_policy: &SandboxPolicy, _cwd: &Path) -> WindowsSandboxKind {
    if windows_sandbox_available() {
        WindowsSandboxKind::WindowsSandbox
    } else if appcontainer_available() {
        WindowsSandboxKind::AppContainer
    } else {
        WindowsSandboxKind::RestrictedToken
    }
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

fn windows_sandbox_available() -> bool {
    let Ok(system_root) = std::env::var("SystemRoot") else {
        return false;
    };
    Path::new(&system_root)
        .join("System32")
        .join("WindowsSandbox.exe")
        .exists()
}

fn appcontainer_available() -> bool {
    true
}

fn restricted_token_available() -> bool {
    true
}
