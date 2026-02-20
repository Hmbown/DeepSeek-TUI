//! Lightweight verbose logging helpers for the CLI.

use std::sync::atomic::{AtomicBool, Ordering};

use colored::Colorize;

use crate::palette;
static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Enable or disable verbose logging output.
pub fn set_verbose(enabled: bool) {
    VERBOSE.store(enabled, Ordering::SeqCst);
}

/// Check whether verbose logging is enabled.
#[must_use]
pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::SeqCst)
}

/// Emit a verbose info message (no-op when verbosity is disabled).
pub fn info(message: impl AsRef<str>) {
    if is_verbose() {
        let (r, g, b) = palette::WAGMII_SKY_RGB;
        eprintln!("{} {}", "info".truecolor(r, g, b).bold(), message.as_ref());
    }
}

/// Emit a verbose warning message (no-op when verbosity is disabled).
pub fn warn(message: impl AsRef<str>) {
    if is_verbose() {
        let (r, g, b) = palette::WAGMII_SKY_RGB;
        eprintln!("{} {}", "warn".truecolor(r, g, b).bold(), message.as_ref());
    }
}
