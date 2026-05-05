//! API key entry screen for onboarding.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette;
use crate::tui::app::App;

pub fn lines(app: &App) -> Vec<Line<'static>> {
    let tr = |key: &str, fallback: &str| -> String {
        crate::json_locale::tr_ui_label(app.ui_locale, key)
            .unwrap_or(fallback)
            .to_string()
    };

    let mut lines = vec![
        Line::from(Span::styled(
            tr("onboarding_api_key_title", "Connect your DeepSeek API key"),
            Style::default()
                .fg(palette::DEEPSEEK_SKY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            tr("onboarding_api_key_step1", "Step 1.  Open https://platform.deepseek.com/api_keys and create a key."),
            Style::default().fg(palette::TEXT_PRIMARY),
        )),
        Line::from(Span::styled(
            tr("onboarding_api_key_step2", "Step 2.  Paste it below and press Enter."),
            Style::default().fg(palette::TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            tr("onboarding_api_key_saved", "Saved to ~/.deepseek/config.toml so it works from any folder."),
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(Span::styled(
            tr("onboarding_api_key_paste_hint", "Paste the full key exactly as issued (no spaces or newlines)."),
            Style::default().fg(palette::TEXT_MUTED),
        )),
        Line::from(""),
    ];

    let masked = mask_key(&app.api_key_input);
    let placeholder = tr("onboarding_api_key_placeholder", "(paste key here)");
    let label = tr("onboarding_api_key_label", "Key: ");
    let display = if masked.is_empty() {
        placeholder
    } else {
        masked
    };
    lines.push(Line::from(vec![
        Span::styled(label, Style::default().fg(palette::TEXT_MUTED)),
        Span::styled(
            display,
            Style::default()
                .fg(palette::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    if let Some(message) = app.status_message.as_deref() {
        lines.push(Line::from(Span::styled(
            message.to_string(),
            Style::default().fg(palette::STATUS_WARNING),
        )));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        tr("onboarding_api_key_enter", "Press Enter to save, Esc to go back."),
        Style::default().fg(palette::TEXT_MUTED),
    )));

    lines
}

fn mask_key(input: &str) -> String {
    let trimmed = input.trim();
    let len = trimmed.chars().count();
    if len == 0 {
        return String::new();
    }
    if len <= 4 {
        return "*".repeat(len);
    }
    let visible: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{}{}", "*".repeat(len - 4), visible)
}
