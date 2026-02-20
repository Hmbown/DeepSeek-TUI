//! Shared text helpers for TUI selection and clipboard workflows.

use ratatui::text::Line;

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

pub(super) fn slice_text(text: &str, start: usize, end: usize) -> String {
    let mut out = String::new();
    let mut idx = 0usize;
    for ch in text.chars() {
        if idx >= start && idx < end {
            out.push(ch);
        }
        idx += 1;
        if idx >= end {
            break;
        }
    }
    out
}
