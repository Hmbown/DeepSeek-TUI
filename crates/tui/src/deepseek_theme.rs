//! Whale/DeepSeek terminal theme tokens.
//!
//! A small, deliberately flat module that names the color, border, and
//! padding choices the TUI is already making. All values match the dark
//! palette previously hard-coded against [`crate::palette`]; a single
//! source-of-truth change here can swap the skin later. Visible output
//! is not changed by introducing this module.
//!
//! The only consumers today are the plan and tool cell renderers in
//! [`crate::tui::history`] and the sidebar section chrome in
//! [`crate::tui::ui`]. All other call sites continue to use [`crate::palette`]
//! directly until they are migrated in a later slice.

use std::cell::Cell;

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{BorderType, Borders, Padding};

use crate::palette;
use crate::tui::history::ToolStatus;

/// Visual variant exposed by the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ThemeArg {
    Dark,
    Light,
}

/// Visual variant exposed by the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
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
    /// The current dark theme. Visible output today uses these values.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            variant: Variant::Dark,
            section_borders: Borders::ALL,
            section_border_type: BorderType::Plain,
            section_border_color: palette::BORDER_COLOR,
            section_bg: palette::DEEPSEEK_INK, // explicit dark bg instead of Reset (inherits terminal bg)
            section_title_color: palette::DEEPSEEK_BLUE,
            // Horizontal padding only. `Padding::uniform(1)` ate two rows of
            // each sidebar panel — for compact terminals where Plan/Todos/Tasks
            // get ~3 rows total via the 25% layout split, that left zero rows
            // for content (#63 follow-up: panels rendered as empty boxes even
            // when "No todos" / "No active plan" should have shown).
            section_padding: Padding::horizontal(1),
            tool_title_color: palette::TEXT_SOFT,
            tool_value_color: palette::TEXT_MUTED,
            tool_label_color: palette::TEXT_DIM,
            tool_running_accent: palette::ACCENT_TOOL_LIVE,
            tool_success_accent: palette::TEXT_DIM,
            tool_failed_accent: palette::ACCENT_TOOL_ISSUE,
            plan_progress_color: palette::DEEPSEEK_BLUE, // deeper blue instead of bright SKY for dark theme
            plan_summary_color: palette::TEXT_MUTED,
            plan_explanation_color: palette::TEXT_DIM,
            plan_pending_color: palette::TEXT_MUTED,
            plan_in_progress_color: palette::STATUS_WARNING,
            plan_completed_color: palette::DEEPSEEK_BLUE, // deeper blue instead of bright SKY for dark theme
        }
    }

    /// The light theme. White background, blue accents, dark text.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            variant: Variant::Light,
            section_borders: Borders::ALL,
            section_border_type: BorderType::Plain,
            section_border_color: Color::Rgb(180, 200, 230), // #B4C8E6 lighter blue-grey
            section_bg: Color::Rgb(248, 250, 252),           // #F8FAFC near-white
            section_title_color: Color::Rgb(53, 120, 229),   // DEEPSEEK_BLUE
            section_padding: Padding::horizontal(1),
            tool_title_color: Color::Rgb(30, 40, 60),        // dark text
            tool_value_color: Color::Rgb(80, 90, 110),       // muted text
            tool_label_color: Color::Rgb(120, 130, 150),    // dim text
            tool_running_accent: Color::Rgb(53, 120, 229),   // blue live
            tool_success_accent: Color::Rgb(34, 139, 90),   // forest green
            tool_failed_accent: Color::Rgb(200, 60, 60),    // soft red
            plan_progress_color: Color::Rgb(53, 120, 229),  // blue
            plan_summary_color: Color::Rgb(80, 90, 110),    // muted
            plan_explanation_color: Color::Rgb(100, 110, 130), // dim
            plan_pending_color: Color::Rgb(120, 130, 150), // muted
            plan_in_progress_color: Color::Rgb(255, 150, 50), // amber
            plan_completed_color: Color::Rgb(34, 139, 90),  // green
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

thread_local! {
    static ACTIVE_THEME: Cell<Theme> = Cell::new(Theme::dark());
}

/// Returns the currently active theme, set by `set_active_theme()`.
#[must_use]
pub fn active_theme() -> Theme {
    ACTIVE_THEME.with(|cell| cell.get())
}

/// Sets the active theme and persists to `TuiPrefs`.
pub fn set_active_theme(theme: Theme) {
    ACTIVE_THEME.with(|cell| cell.set(theme));
    // Persist asynchronously via TuiPrefs::save() — fire-and-forget
    let variant_str = match theme.variant {
        Variant::Dark => "dark",
        Variant::Light => "light",
    };
    std::thread::spawn(move || {
        if let Ok(mut prefs) = crate::settings::TuiPrefs::load() {
            prefs.theme = variant_str.to_string();
            let _ = prefs.save();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{Theme, Variant, active_theme};
    use crate::palette;
    use crate::tui::history::ToolStatus;

    #[test]
    fn active_theme_returns_dark() {
        assert_eq!(active_theme(), Theme::dark());
    }

    #[test]
    fn dark_theme_matches_existing_palette_choices() {
        let theme = Theme::dark();
        assert_eq!(theme.variant, Variant::Dark);
        assert_eq!(theme.section_border_color, palette::BORDER_COLOR);
        assert_eq!(theme.section_bg, palette::DEEPSEEK_INK);
        assert_eq!(theme.section_title_color, palette::DEEPSEEK_BLUE);
        assert_eq!(theme.tool_title_color, palette::TEXT_SOFT);
        assert_eq!(theme.tool_value_color, palette::TEXT_MUTED);
        assert_eq!(theme.tool_label_color, palette::TEXT_DIM);
        assert_eq!(theme.tool_running_accent, palette::ACCENT_TOOL_LIVE);
        assert_eq!(theme.tool_success_accent, palette::TEXT_DIM);
        assert_eq!(theme.tool_failed_accent, palette::ACCENT_TOOL_ISSUE);
    }

    #[test]
    fn tool_status_color_maps_each_status() {
        let theme = Theme::dark();
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
