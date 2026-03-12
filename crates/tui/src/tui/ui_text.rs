//! Shared text helpers for TUI selection and clipboard workflows.

use ratatui::text::Line;
use unicode_width::UnicodeWidthChar;

use crate::tui::history::HistoryCell;

pub(super) fn history_cell_to_text(cell: &HistoryCell, width: u16) -> String {
    cell.lines(width)
        .into_iter()
        .map(line_to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_to_string(line: Line<'static>) -> String {
    line.spans
        .into_iter()
        .map(|span| span.content.to_string())
        .collect::<String>()
}

pub(super) fn line_to_plain(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

pub(super) fn text_display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

pub(super) fn slice_text(text: &str, start: usize, end: usize) -> String {
    if end <= start {
        return String::new();
    }

    let mut out = String::new();
    let mut col = 0usize;
    for ch in text.chars() {
        let ch_width = char_display_width(ch);
        let ch_start = col;
        let ch_end = col.saturating_add(ch_width);
        if ch_end > start && ch_start < end {
            out.push(ch);
        }
        col = ch_end;
        if col >= end {
            break;
        }
    }
    out
}

fn char_display_width(ch: char) -> usize {
    if ch == '\t' {
        4
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0).max(1)
    }
}
