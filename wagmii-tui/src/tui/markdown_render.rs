//! Simple markdown rendering for TUI transcript lines.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::palette;

pub fn render_markdown(content: &str, width: u16, base_style: Style) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let width = width.max(1) as usize;
    let mut in_code_block = false;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            let code_style = Style::default()
                .fg(palette::WAGMII_SKY)
                .add_modifier(Modifier::ITALIC);
            out.extend(render_wrapped_line(raw_line, width, code_style, true));
            continue;
        }

        if let Some((level, text)) = parse_heading(trimmed) {
            let style = Style::default()
                .fg(palette::WAGMII_SKY)
                .add_modifier(Modifier::BOLD);
            out.extend(render_wrapped_line(text, width, style, false));
            if level == 1 {
                out.push(Line::from(Span::styled(
                    "─".repeat(width.min(40)),
                    Style::default().fg(palette::TEXT_DIM),
                )));
            }
            continue;
        }

        if let Some((bullet, text)) = parse_list_item(trimmed) {
            let bullet_style = Style::default().fg(palette::WAGMII_SKY);
            let content_style = base_style;
            out.extend(render_list_line(
                &bullet,
                text,
                width,
                bullet_style,
                content_style,
            ));
            continue;
        }

        let link_style = Style::default()
            .fg(palette::WAGMII_BLUE)
            .add_modifier(Modifier::UNDERLINED);
        out.extend(render_line_with_links(
            trimmed, width, base_style, link_style,
        ));
        if raw_line.is_empty() {
            out.push(Line::from(""));
        }
    }

    if out.is_empty() {
        out.push(Line::from(""));
    }

    out
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 {
        return None;
    }
    let text = trimmed[hashes..].trim();
    if text.is_empty() {
        None
    } else {
        Some((hashes, text))
    }
}

fn parse_list_item(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return Some(("-".to_string(), trimmed[2..].trim()));
    }
    let bytes = trimmed.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == 0 || idx >= bytes.len() || bytes[idx] != b'.' {
        return None;
    }
    let rest = &trimmed[idx + 1..];
    if !rest.starts_with(' ') {
        return None;
    }
    Some((format!("{}.", &trimmed[..idx]), rest.trim_start()))
}

fn render_wrapped_line(
    line: &str,
    width: usize,
    style: Style,
    indent_code: bool,
) -> Vec<Line<'static>> {
    let prefix = if indent_code { "  " } else { "" };
    let prefix_width = prefix.width();
    let available = width.saturating_sub(prefix_width).max(1);
    let wrapped = wrap_text(line, available);
    let mut out = Vec::new();

    for (idx, chunk) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            out.push(Line::from(vec![
                Span::raw(prefix),
                Span::styled(chunk, style),
            ]));
        } else {
            out.push(Line::from(vec![
                Span::raw(" ".repeat(prefix_width)),
                Span::styled(chunk, style),
            ]));
        }
    }

    out
}

fn render_list_line(
    bullet: &str,
    text: &str,
    width: usize,
    bullet_style: Style,
    text_style: Style,
) -> Vec<Line<'static>> {
    let bullet_prefix = format!("{bullet} ");
    let bullet_width = bullet_prefix.width();
    let available = width.saturating_sub(bullet_width).max(1);
    let wrapped = render_line_with_links(text, available, text_style, link_style());

    let mut out = Vec::new();
    for (idx, line) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            let mut spans = vec![Span::styled(bullet_prefix.clone(), bullet_style)];
            spans.extend(line.spans);
            out.push(Line::from(spans));
        } else {
            let mut spans = vec![Span::raw(" ".repeat(bullet_width))];
            spans.extend(line.spans);
            out.push(Line::from(spans));
        }
    }
    out
}

fn render_line_with_links(
    line: &str,
    width: usize,
    base_style: Style,
    link_style: Style,
) -> Vec<Line<'static>> {
    if line.trim().is_empty() {
        return vec![Line::from("")];
    }

    let mut lines = Vec::new();
    let mut current_spans: Vec<Span> = Vec::new();
    let mut current_width = 0usize;

    for word in line.split_whitespace() {
        let style = if looks_like_link(word) {
            link_style
        } else {
            base_style
        };
        let word_width = word.width();
        let additional = if current_width == 0 {
            word_width
        } else {
            word_width + 1
        };

        if current_width + additional > width && !current_spans.is_empty() {
            lines.push(Line::from(current_spans));
            current_spans = Vec::new();
            current_width = 0;
        }

        if current_width > 0 {
            current_spans.push(Span::raw(" "));
            current_width += 1;
        }

        current_spans.push(Span::styled(word.to_string(), style));
        current_width += word_width;
    }

    if !current_spans.is_empty() {
        lines.push(Line::from(current_spans));
    }

    lines
}

fn looks_like_link(word: &str) -> bool {
    word.starts_with("http://") || word.starts_with("https://")
}

fn link_style() -> Style {
    Style::default()
        .fg(palette::WAGMII_BLUE)
        .add_modifier(Modifier::UNDERLINED)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = word.width();
        let additional = if current.is_empty() {
            word_width
        } else {
            word_width + 1
        };
        if current_width + additional > width && !current.is_empty() {
            lines.push(current);
            current = word.to_string();
            current_width = word_width;
        } else {
            if !current.is_empty() {
                current.push(' ');
                current_width += 1;
            }
            current.push_str(word);
            current_width += word_width;
        }
    }

    if current.is_empty() {
        lines.push(String::new());
    } else {
        lines.push(current);
    }

    lines
}
