//! Theme tokens for sidebar, plan, and tool chrome.
//!
//! This module keeps chrome-specific colors separate from the broader palette
//! while sharing the same normalized theme names.

use std::sync::atomic::{AtomicU8, Ordering};

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{BorderType, Borders, Padding};

use crate::palette;
use crate::tui::history::ToolStatus;

/// Visual variant exposed by the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Whale,
    Dark,
    Light,
}

/// Centralized visual tokens for sidebar, plan, and tool rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub variant: Variant,

    // Sidebar / section chrome
    pub section_borders: Borders,
    pub section_border_type: BorderType,
    pub section_border_color: Color,
    pub section_bg: Color,
    pub section_title_color: Color,
    pub section_padding: Padding,

    // Tool cell color tokens
    pub tool_title_color: Color,
    pub tool_value_color: Color,
    pub tool_label_color: Color,
    pub tool_running_accent: Color,
    pub tool_success_accent: Color,
    pub tool_failed_accent: Color,

    // Plan cell color tokens
    pub plan_progress_color: Color,
    pub plan_summary_color: Color,
    pub plan_explanation_color: Color,
    pub plan_pending_color: Color,
    pub plan_in_progress_color: Color,
    pub plan_completed_color: Color,
}

impl Theme {
    /// The current whale theme. Visible output today uses these values.
    #[must_use]
    pub const fn whale() -> Self {
        Self {
            variant: Variant::Whale,
            section_borders: Borders::ALL,
            section_border_type: BorderType::Plain,
            section_border_color: palette::BORDER_COLOR,
            section_bg: Color::Reset,
            section_title_color: palette::DEEPSEEK_BLUE,
            section_padding: Padding::horizontal(1),
            tool_title_color: palette::TEXT_SOFT,
            tool_value_color: palette::TEXT_MUTED,
            tool_label_color: palette::TEXT_DIM,
            tool_running_accent: palette::ACCENT_TOOL_LIVE,
            tool_success_accent: palette::TEXT_DIM,
            tool_failed_accent: palette::ACCENT_TOOL_ISSUE,
            plan_progress_color: palette::STATUS_SUCCESS,
            plan_summary_color: palette::TEXT_MUTED,
            plan_explanation_color: palette::TEXT_DIM,
            plan_pending_color: palette::TEXT_MUTED,
            plan_in_progress_color: palette::STATUS_WARNING,
            plan_completed_color: palette::STATUS_SUCCESS,
        }
    }

    #[must_use]
    pub const fn dark() -> Self {
        Self {
            variant: Variant::Dark,
            section_borders: Borders::ALL,
            section_border_type: BorderType::Plain,
            section_border_color: Color::Rgb(68, 85, 126),
            section_bg: Color::Rgb(15, 17, 21),
            section_title_color: Color::Rgb(148, 163, 184),
            section_padding: Padding::horizontal(1),
            tool_title_color: Color::Rgb(226, 232, 240),
            tool_value_color: Color::Rgb(148, 163, 184),
            tool_label_color: Color::Rgb(100, 116, 139),
            tool_running_accent: Color::Rgb(96, 165, 250),
            tool_success_accent: Color::Rgb(148, 163, 184),
            tool_failed_accent: Color::Rgb(248, 113, 113),
            plan_progress_color: Color::Rgb(96, 165, 250),
            plan_summary_color: Color::Rgb(203, 213, 225),
            plan_explanation_color: Color::Rgb(148, 163, 184),
            plan_pending_color: Color::Rgb(148, 163, 184),
            plan_in_progress_color: Color::Rgb(251, 191, 36),
            plan_completed_color: Color::Rgb(34, 197, 94),
        }
    }

    #[must_use]
    pub const fn light() -> Self {
        Self {
            variant: Variant::Light,
            section_borders: Borders::ALL,
            section_border_type: BorderType::Plain,
            section_border_color: Color::Rgb(148, 163, 184),
            section_bg: Color::Rgb(250, 250, 250),
            section_title_color: Color::Rgb(30, 41, 59),
            section_padding: Padding::horizontal(1),
            tool_title_color: Color::Rgb(15, 23, 42),
            tool_value_color: Color::Rgb(51, 65, 85),
            tool_label_color: Color::Rgb(100, 116, 139),
            tool_running_accent: Color::Rgb(37, 99, 235),
            tool_success_accent: Color::Rgb(71, 85, 105),
            tool_failed_accent: Color::Rgb(220, 38, 38),
            plan_progress_color: Color::Rgb(37, 99, 235),
            plan_summary_color: Color::Rgb(51, 65, 85),
            plan_explanation_color: Color::Rgb(100, 116, 139),
            plan_pending_color: Color::Rgb(100, 116, 139),
            plan_in_progress_color: Color::Rgb(180, 83, 9),
            plan_completed_color: Color::Rgb(22, 163, 74),
        }
    }

    #[must_use]
    pub fn for_name(name: &str) -> Self {
        match crate::palette::normalized_theme_or_default(name) {
            "dark" | "system" => Self::dark(),
            "light" => Self::light(),
            "default" | "whale" => Self::whale(),
            _ => Self::whale(),
        }
    }

    /// Pick the right tool accent for a given [`ToolStatus`].
    #[must_use]
    pub const fn tool_status_color(self, status: ToolStatus) -> Color {
        match status {
            ToolStatus::Running => self.tool_running_accent,
            ToolStatus::Success => self.tool_success_accent,
            ToolStatus::Failed => self.tool_failed_accent,
        }
    }

    /// Bold tool title style (e.g. "Plan", "Shell").
    #[must_use]
    pub fn tool_title_style(self) -> Style {
        Style::default()
            .fg(self.tool_title_color)
            .add_modifier(Modifier::BOLD)
    }

    /// Right-side status text ("running", "done", "issue") style.
    #[must_use]
    pub fn tool_status_style(self, status: ToolStatus) -> Style {
        Style::default().fg(self.tool_status_color(status))
    }

    /// Detail label style ("command:", "time:", step markers).
    #[must_use]
    pub fn tool_label_style(self) -> Style {
        Style::default().fg(self.tool_label_color)
    }

    /// Default value style for tool detail rows.
    #[must_use]
    pub fn tool_value_style(self) -> Style {
        Style::default().fg(self.tool_value_color)
    }
}

static ACTIVE_THEME: AtomicU8 = AtomicU8::new(0);

const THEME_WHALE: u8 = 0;
const THEME_DARK: u8 = 1;
const THEME_LIGHT: u8 = 2;

/// Set the globally active theme. Called by `App` when the theme changes.
pub fn set_active_theme(theme: Theme) {
    let val = match theme.variant {
        Variant::Whale => THEME_WHALE,
        Variant::Dark => THEME_DARK,
        Variant::Light => THEME_LIGHT,
    };
    ACTIVE_THEME.store(val, Ordering::Relaxed);
}

/// Returns the active theme used by the TUI today.
#[must_use]
pub fn active_theme() -> Theme {
    match ACTIVE_THEME.load(Ordering::Relaxed) {
        THEME_DARK => Theme::dark(),
        THEME_LIGHT => Theme::light(),
        _ => Theme::whale(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Theme, Variant, active_theme};
    use crate::palette;
    use crate::tui::history::ToolStatus;

    #[test]
    fn active_theme_returns_whale() {
        assert_eq!(active_theme(), Theme::whale());
    }

    #[test]
    fn whale_theme_matches_existing_palette_choices() {
        let theme = Theme::whale();
        assert_eq!(theme.variant, Variant::Whale);
        assert_eq!(theme.section_border_color, palette::BORDER_COLOR);
        assert_eq!(theme.section_bg, ratatui::style::Color::Reset);
        assert_eq!(theme.section_title_color, palette::DEEPSEEK_BLUE);
        assert_eq!(theme.tool_title_color, palette::TEXT_SOFT);
        assert_eq!(theme.tool_value_color, palette::TEXT_MUTED);
        assert_eq!(theme.tool_label_color, palette::TEXT_DIM);
        assert_eq!(theme.tool_running_accent, palette::ACCENT_TOOL_LIVE);
        assert_eq!(theme.tool_success_accent, palette::TEXT_DIM);
        assert_eq!(theme.tool_failed_accent, palette::ACCENT_TOOL_ISSUE);
    }

    #[test]
    fn dark_and_light_theme_have_distinct_variants() {
        assert_eq!(Theme::dark().variant, Variant::Dark);
        assert_eq!(Theme::light().variant, Variant::Light);
    }

    #[test]
    fn tool_status_color_maps_each_status() {
        let theme = Theme::whale();
        assert_eq!(
            theme.tool_status_color(ToolStatus::Running),
            theme.tool_running_accent
        );
        assert_eq!(
            theme.tool_status_color(ToolStatus::Success),
            theme.tool_success_accent
        );
        assert_eq!(
            theme.tool_status_color(ToolStatus::Failed),
            theme.tool_failed_accent
        );
    }
}
