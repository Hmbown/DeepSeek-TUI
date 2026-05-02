#![allow(dead_code)]

//! Deepseek-TUI brand palette — vendored override (forked v0.12.0).
//!
//! Upstream schemaui ships hardcoded `Color::Gray` / `Color::DarkGray` /
//! `Color::White` / `Color::Yellow` / `Color::Cyan` / `Color::Magenta`
//! references across its rendering components. The deepseek-tui fork
//! routes every render path through this module so the schemaui editor
//! visually matches the rest of the TUI (navy ink + sky accents).
//!
//! These are intentionally exact RGB values (not 16-color named indices)
//! so they read identically on every modern terminal regardless of
//! terminal-palette overrides. Sourced from
//! `crates/tui/src/palette.rs` in the deepseek-tui workspace — keep in
//! sync if the upstream brand palette ever shifts.

use ratatui::style::Color;

// === Surface backgrounds ===
/// Primary navy ink — chrome backgrounds, panel fills.
pub const SURFACE_INK: Color = Color::Rgb(11, 21, 38);
/// Slightly lifted navy — popup / overlay surfaces sit on top of INK
/// at a small luminance bump so they read as floating without being
/// off-brand.
pub const SURFACE_RAISED: Color = Color::Rgb(17, 30, 52);

// === Borders + chrome ===
pub const BORDER_DIM: Color = Color::Rgb(48, 64, 92);
pub const BORDER_ACTIVE: Color = Color::Rgb(106, 174, 242);

// === Text ===
/// Primary text color — high-contrast on the navy ink.
pub const TEXT_PRIMARY: Color = Color::Rgb(220, 230, 240);
/// Secondary text — labels, body prose at slightly reduced contrast.
pub const TEXT_MUTED: Color = Color::Rgb(160, 175, 195);
/// Tertiary text — placeholders, defaults, low-importance hints.
pub const TEXT_DIM: Color = Color::Rgb(110, 124, 148);

// === Brand accents ===
/// Sky-blue — the primary accent color (selected tab, focus marker).
pub const ACCENT_SKY: Color = Color::Rgb(106, 174, 242);
/// Saturated brand blue — links, descriptions, secondary accent.
pub const ACCENT_BLUE: Color = Color::Rgb(80, 132, 220);
/// Soft purple — used by upstream for description spans; kept as an
/// alternate accent for variety in the editor's hint text.
pub const ACCENT_PURPLE: Color = Color::Rgb(168, 132, 220);

// === Status ===
pub const STATUS_OK: Color = Color::Rgb(120, 200, 130);
pub const STATUS_WARN: Color = Color::Rgb(232, 172, 80);
pub const STATUS_ERROR: Color = Color::Rgb(232, 102, 102);

/// Default body style — text on the navy ink surface.
#[inline]
#[must_use]
pub fn body_style() -> ratatui::style::Style {
    ratatui::style::Style::default()
        .fg(TEXT_PRIMARY)
        .bg(SURFACE_INK)
}

/// Block / overlay style — slightly lifted surface for floating panels.
#[inline]
#[must_use]
pub fn surface_style() -> ratatui::style::Style {
    ratatui::style::Style::default()
        .fg(TEXT_PRIMARY)
        .bg(SURFACE_RAISED)
}
