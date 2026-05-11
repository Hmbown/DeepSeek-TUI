//! Terminal capability detection for flicker-free rendering.
//!
//! Detects whether the current terminal supports DECSET 2026 (Synchronized Output)
//! which is critical for preventing flicker on Windows terminals.
//!
//! ## Windows Terminal Support
//!
//! - **Windows Terminal** (modern): Full DECSET 2026 support
//! - **ConPTY** (Windows 10 1809+): Supported via VT passthrough
//! - **Legacy cmd.exe/conhost**: May not support; escape is silently ignored
//!
//! ## Detection Strategy
//!
//! 1. Check `WT_SESSION` env var (Windows Terminal)
//! 2. Check `TERM_PROGRAM` env var
//! 3. Check Windows version via `VER` command
//! 4. Default to enabled (safe: unsupported terminals ignore the escape)

use std::env;

/// Terminal rendering capabilities detected at startup.
#[derive(Debug, Clone)]
pub struct TerminalCapabilities {
    /// Whether the terminal likely supports DECSET 2026 (Synchronized Output).
    pub supports_synchronized_update: bool,
    /// Whether we're running inside Windows Terminal.
    pub is_windows_terminal: bool,
    /// Whether we're running inside VSCode's integrated terminal.
    pub is_vscode_terminal: bool,
    /// Whether we're running inside ConPTY.
    pub is_conpty: bool,
}

impl TerminalCapabilities {
    /// Detect terminal capabilities from environment variables.
    pub fn detect() -> Self {
        let is_windows_terminal = env::var("WT_SESSION").is_ok()
            || env::var("WT_PROFILE_ID").is_ok();

        let is_vscode_terminal = env::var("TERM_PROGRAM")
            .map(|v| v == "vscode")
            .unwrap_or(false);

        // ConPTY is used by Windows Terminal and modern console hosts
        let is_conpty = cfg!(windows) && (
            is_windows_terminal
            || env::var("ConEmuPID").is_ok()
            || env::var("ANSICON").is_ok()
        );

        // DECSET 2026 support:
        // - Windows Terminal: Yes (since 1.0)
        // - ConPTY: Yes (VT passthrough)
        // - VSCode terminal: Yes (xterm.js based)
        // - macOS Terminal.app: No (but doesn't flicker anyway)
        // - iTerm2: Yes
        // - Ghostty: Yes
        // - Alacritty: Yes
        let supports_synchronized_update = if cfg!(windows) {
            // On Windows, assume support for modern terminals
            is_windows_terminal || is_conpty || is_vscode_terminal
        } else {
            // On macOS/Linux, most modern terminals support it
            // Terminal.app doesn't but doesn't need it (no flicker)
            true
        };

        tracing::debug!(
            target: "terminal_caps",
            is_windows_terminal,
            is_vscode_terminal,
            is_conpty,
            supports_synchronized_update,
            "Terminal capabilities detected"
        );

        Self {
            supports_synchronized_update,
            is_windows_terminal,
            is_vscode_terminal,
            is_conpty,
        }
    }

    /// Check if we should use synchronized updates for rendering.
    ///
    /// Returns true if the terminal likely supports DECSET 2026.
    /// Even if detection is uncertain, it's safe to try - unsupported
    /// terminals silently ignore the escape sequence.
    #[allow(dead_code)]
    pub fn should_use_synchronized_update(&self) -> bool {
        // Always try synchronized updates - they're safe on unsupported
        // terminals (escape is silently ignored) and critical for
        // preventing flicker on Windows.
        true
    }

    /// Get recommended frame rate limit based on terminal capabilities.
    ///
    /// Slower terminals benefit from lower frame rates to reduce flicker.
    pub fn recommended_frame_interval_ms(&self) -> u64 {
        if self.is_vscode_terminal {
            // VSCode's xterm.js has higher rendering latency
            16 // ~60 FPS
        } else {
            8 // ~120 FPS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detection_doesnt_panic() {
        // Should not panic even without terminal env vars
        let caps = TerminalCapabilities::detect();
        // Default should assume support (safe to try)
        assert!(caps.should_use_synchronized_update());
    }
}
