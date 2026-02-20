//! Diff rendering helpers for TUI previews.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::palette;

const LINE_NUMBER_WIDTH: usize = 4;

pub fn render_diff(diff: &str, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut old_line: Option<usize> = None;
    let mut new_line: Option<usize> = None;

    for raw in diff.lines() {
        if raw.starts_with("diff --git") || raw.starts_with("index ") {
            lines.extend(render_header_line(raw, width));
            continue;
        }

        if raw.starts_with("--- ") || raw.starts_with("+++ ") {
            lines.extend(render_header_line(raw, width));
            continue;
        }

        if raw.starts_with("@@") {
            if let Some((old_start, new_start)) = parse_hunk_header(raw) {
                old_line = Some(old_start);
                new_line = Some(new_start);
            }
            lines.extend(render_hunk_header(raw, width));
            continue;
        }

        if raw.starts_with('+') && !raw.starts_with("+++") {
            let content = raw.trim_start_matches('+');
            lines.extend(render_diff_line(
                content,
                width,
                old_line,
                new_line,
                Style::default().fg(palette::STATUS_SUCCESS),
            ));
            if let Some(line) = new_line.as_mut() {
                *line = line.saturating_add(1);
            }
            continue;
        }

        if raw.starts_with('-') && !raw.starts_with("---") {
            let content = raw.trim_start_matches('-');
            lines.extend(render_diff_line(
                content,
                width,
                old_line,
                new_line,
                Style::default().fg(palette::STATUS_ERROR),
            ));
            if let Some(line) = old_line.as_mut() {
                *line = line.saturating_add(1);
            }
            continue;
        }

        if raw.starts_with(' ') {
            let content = raw.trim_start_matches(' ');
            lines.extend(render_diff_line(
                content,
                width,
                old_line,
                new_line,
                Style::default().fg(palette::TEXT_PRIMARY),
            ));
            if let Some(line) = old_line.as_mut() {
                *line = line.saturating_add(1);
            }
            if let Some(line) = new_line.as_mut() {
                *line = line.saturating_add(1);
            }
            continue;
        }

        lines.extend(render_header_line(raw, width));
    }

    lines
}

fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    let old = parts[1].trim_start_matches('-');
    let new = parts[2].trim_start_matches('+');
    let old_start = old.split(',').next()?.parse::<usize>().ok()?;
    let new_start = new.split(',').next()?.parse::<usize>().ok()?;
    Some((old_start, new_start))
}

fn render_header_line(line: &str, width: u16) -> Vec<Line<'static>> {
    let style = Style::default()
        .fg(palette::WAGMII_SKY)
        .add_modifier(Modifier::BOLD);
    wrap_with_style(line, style, width)
}

fn render_hunk_header(line: &str, width: u16) -> Vec<Line<'static>> {
    let style = Style::default().fg(palette::WAGMII_BLUE);
    wrap_with_style(line, style, width)
}

fn render_diff_line(
    content: &str,
    width: u16,
    old_line: Option<usize>,
    new_line: Option<usize>,
    style: Style,
) -> Vec<Line<'static>> {
    let prefix = format_line_numbers(old_line, new_line);
    let prefix_width = prefix.width();
    let available = width.saturating_sub(prefix_width as u16).max(1) as usize;
    let wrapped = wrap_text(content, available);

    let mut out = Vec::new();
    for (idx, chunk) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            out.push(Line::from(vec![
                Span::styled(prefix.clone(), Style::default().fg(palette::TEXT_MUTED)),
                Span::styled(chunk, style),
            ]));
        } else {
            out.push(Line::from(vec![
                Span::raw(" ".repeat(prefix_width)),
                Span::styled(chunk, style),
            ]));
        }
    }

    if out.is_empty() {
        out.push(Line::from(vec![Span::styled(
            prefix,
            Style::default().fg(palette::TEXT_MUTED),
        )]));
    }

    out
}

fn format_line_numbers(old_line: Option<usize>, new_line: Option<usize>) -> String {
    let old = old_line
        .map(|value| {
            format!(
                "{value:>LINE_NUMBER_WIDTH$}",
                LINE_NUMBER_WIDTH = LINE_NUMBER_WIDTH
            )
        })
        .unwrap_or_else(|| " ".repeat(LINE_NUMBER_WIDTH));
    let new = new_line
        .map(|value| {
            format!(
                "{value:>LINE_NUMBER_WIDTH$}",
                LINE_NUMBER_WIDTH = LINE_NUMBER_WIDTH
            )
        })
        .unwrap_or_else(|| " ".repeat(LINE_NUMBER_WIDTH));
    format!("{old} {new} | ")
}

fn wrap_with_style(text: &str, style: Style, width: u16) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for part in wrap_text(text, width.max(1) as usize) {
        out.push(Line::from(Span::styled(part, style)));
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled("", style)));
    }
    out
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
