//! Onboarding flow rendering and helpers.

pub mod api_key;
pub mod language;
pub mod trust_directory;
pub mod welcome;

use std::path::{Path, PathBuf};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
};

use crate::palette;
use crate::tui::app::{App, OnboardingState};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().style(Style::default().bg(palette::DEEPSEEK_INK));
    f.render_widget(block, area);

    let content_width = 76.min(area.width.saturating_sub(4));
    let content_height = 20.min(area.height.saturating_sub(4));
    let content_area = Rect {
        x: (area.width - content_width) / 2,
        y: (area.height - content_height) / 2,
        width: content_width,
        height: content_height,
    };

    let lines = match app.onboarding {
        OnboardingState::Welcome => welcome::lines(app),
        OnboardingState::Language => language::lines(app),
        OnboardingState::ApiKey => api_key::lines(app),
        OnboardingState::TrustDirectory => trust_directory::lines(app),
        OnboardingState::Tips => tips_lines(app),
        OnboardingState::None => Vec::new(),
    };

    if !lines.is_empty() {
        let (step, total) = onboarding_step(app);
        let tr_title = |key: &str, fallback: &str| -> String {
            crate::json_locale::tr_ui_label(app.ui_locale, key)
                .unwrap_or(fallback)
                .to_string()
        };
        let title = tr_title("onboarding_title", " DeepSeek TUI ");
        let step_text = tr_title("onboarding_step", " Step {step}/{total} ")
            .replace("{step}", &step.to_string())
            .replace("{total}", &total.to_string());

        let panel = Block::default()
            .title(Line::from(Span::styled(
                title,
                Style::default()
                    .fg(palette::DEEPSEEK_BLUE)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_bottom(Line::from(Span::styled(
                step_text,
                Style::default()
                    .fg(palette::TEXT_MUTED)
                    .add_modifier(Modifier::BOLD),
            )))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::DEEPSEEK_SLATE))
            .padding(Padding::new(2, 2, 1, 1));
        let inner = panel.inner(content_area);
        f.render_widget(panel, content_area);
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        f.render_widget(paragraph, inner);
    }
}

fn onboarding_step(app: &App) -> (usize, usize) {
    let needs_trust = !app.trust_mode && needs_trust(&app.workspace);
    // Welcome + Language + Tips are always shown.
    let mut total = 3;
    if app.onboarding_needs_api_key {
        total += 1;
    }
    if needs_trust {
        total += 1;
    }

    let step = match app.onboarding {
        OnboardingState::Language => 0,
        OnboardingState::Welcome => 1,
        OnboardingState::ApiKey => 2,
        OnboardingState::TrustDirectory => {
            // Language (0) + Welcome (1) + optional ApiKey
            if app.onboarding_needs_api_key { 3 } else { 2 }
        }
        OnboardingState::Tips => total,
        OnboardingState::None => total,
    };

    (step, total)
}

pub fn tips_lines(app: &App) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::style::Modifier;
    use ratatui::text::{Line, Span};

    let tr = |key: &str, fallback: &str| -> String {
        crate::json_locale::tr_ui_label(app.ui_locale, key)
            .unwrap_or(fallback)
            .to_string()
    };

    vec![
        Line::from(Span::styled(
            tr("onboarding_tips_title", "Start Simple"),
            Style::default()
                .fg(palette::DEEPSEEK_SKY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::raw(tr("onboarding_tips_line1", "Write the task in plain language. Use /help or Ctrl+K when you want a command."))),
        Line::from(Span::raw(tr("onboarding_tips_line2", "The bottom composer is multi-line: Enter sends, Alt+Enter or Ctrl+J adds a new line."))),
        Line::from(Span::raw(tr("onboarding_tips_line3", "Switch modes only when the job changes: Plan for review-first work, Agent for execution, YOLO when you want auto-approval."))),
        Line::from(Span::raw(tr("onboarding_tips_line4", "Ctrl+R resumes earlier sessions, and Esc backs out of the current draft or overlay."))),
        Line::from(vec![
            Span::styled(tr("onboarding_tips_press", "Press "), Style::default().fg(palette::TEXT_MUTED)),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(palette::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                tr("onboarding_tips_enter_hint", " to open the workspace"),
                Style::default().fg(palette::TEXT_MUTED),
            ),
        ]),
    ]
}

pub fn default_marker_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".deepseek").join(".onboarded"))
}

pub fn is_onboarded() -> bool {
    default_marker_path().is_some_and(|path| path.exists())
}

pub fn mark_onboarded() -> std::io::Result<PathBuf> {
    let path = default_marker_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Home directory not found")
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, "")?;
    Ok(path)
}

pub fn needs_trust(workspace: &Path) -> bool {
    let markers = [
        workspace.join(".deepseek").join("trusted"),
        workspace.join(".deepseek").join("trust.json"),
    ];
    !markers.iter().any(|path| path.exists())
}

pub fn mark_trusted(workspace: &Path) -> std::io::Result<PathBuf> {
    let dir = workspace.join(".deepseek");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("trusted");
    std::fs::write(&path, "")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::{App, OnboardingState, TuiOptions};
    use std::path::PathBuf;

    fn make_app() -> App {
        let options = TuiOptions {
            model: "deepseek-v4-flash".to_string(),
            workspace: PathBuf::from("."),
            config_path: None,
            config_profile: None,
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: true,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
            initial_input: None,
        };
        let mut app = App::new(options, &Config::default());
        app.model = "deepseek-v4-flash".to_string();
        app
    }

    #[test]
    fn onboarding_step_language_is_zero() {
        let mut app = make_app();
        app.onboarding = OnboardingState::Language;
        app.onboarding_needs_api_key = false;
        app.trust_mode = true; // no trust needed
        let (step, total) = onboarding_step(&app);
        assert_eq!(step, 0, "Language should be step 0");
        assert_eq!(total, 3, "Total should be 3 (Language + Welcome + Tips)");
    }

    #[test]
    fn onboarding_step_welcome_is_one() {
        let mut app = make_app();
        app.onboarding = OnboardingState::Welcome;
        app.onboarding_needs_api_key = false;
        app.trust_mode = true;
        let (step, total) = onboarding_step(&app);
        assert_eq!(step, 1, "Welcome should be step 1");
        assert_eq!(total, 3);
    }

    #[test]
    fn onboarding_step_tips_is_last() {
        let mut app = make_app();
        app.onboarding = OnboardingState::Tips;
        app.onboarding_needs_api_key = false;
        app.trust_mode = true;
        let (step, total) = onboarding_step(&app);
        assert_eq!(step, total, "Tips should be the last step");
        assert_eq!(total, 3);
    }

    #[test]
    fn onboarding_step_with_api_key_increases_total() {
        let mut app = make_app();
        app.onboarding = OnboardingState::ApiKey;
        app.onboarding_needs_api_key = true;
        app.trust_mode = true;
        let (step, total) = onboarding_step(&app);
        assert_eq!(step, 2, "ApiKey should be step 2");
        assert_eq!(total, 4, "Total should be 4 with ApiKey step");
    }

    #[test]
    fn onboarding_step_trust_directory() {
        let mut app = make_app();
        app.onboarding = OnboardingState::TrustDirectory;
        app.onboarding_needs_api_key = true;
        app.trust_mode = false; // needs trust
        let (step, total) = onboarding_step(&app);
        assert_eq!(step, 3, "TrustDirectory should be after Language(0)+Welcome(1)+ApiKey(2)=3");
        assert_eq!(total, 5, "Total = Language + Welcome + ApiKey + Trust + Tips = 5");
    }

    #[test]
    fn onboarding_step_trust_without_api_key() {
        let mut app = make_app();
        app.onboarding = OnboardingState::TrustDirectory;
        app.onboarding_needs_api_key = false;
        app.trust_mode = false; // needs trust
        let (step, total) = onboarding_step(&app);
        assert_eq!(step, 2, "TrustDirectory without ApiKey = Language(0)+Welcome(1)=2");
        assert_eq!(total, 4, "Total = Language + Welcome + Trust + Tips = 4");
    }

    #[test]
    fn onboarding_step_none_equals_total() {
        let mut app = make_app();
        app.onboarding = OnboardingState::None;
        app.onboarding_needs_api_key = false;
        app.trust_mode = true;
        let (step, total) = onboarding_step(&app);
        assert_eq!(step, total);
    }
}
