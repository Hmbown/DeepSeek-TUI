//! Cached transcript rendering for the TUI.

use ratatui::text::Line;

use crate::tui::history::{HistoryCell, TranscriptRenderOptions};
use crate::tui::scrolling::TranscriptLineMeta;

/// Cache of rendered transcript lines for the current viewport.
#[derive(Debug)]
pub struct TranscriptViewCache {
    width: u16,
    version: u64,
    options: TranscriptRenderOptions,
    lines: Vec<Line<'static>>,
    line_meta: Vec<TranscriptLineMeta>,
}

impl TranscriptViewCache {
    /// Create an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            width: 0,
            version: 0,
            options: TranscriptRenderOptions::default(),
            lines: Vec::new(),
            line_meta: Vec::new(),
        }
    }

    /// Ensure cached lines match the provided cells/width/version.
    pub fn ensure(
        &mut self,
        cells: &[HistoryCell],
        width: u16,
        version: u64,
        options: TranscriptRenderOptions,
    ) {
        if self.width == width && self.version == version && self.options == options {
            return;
        }
        self.width = width;
        self.version = version;
        self.options = options;

        let mut lines = Vec::new();
        let mut meta = Vec::new();

        for (cell_index, cell) in cells.iter().enumerate() {
            let cell_lines = cell.lines_with_options(width, options);
            if cell_lines.is_empty() {
                continue;
            }
            for (line_in_cell, line) in cell_lines.into_iter().enumerate() {
                lines.push(line);
                meta.push(TranscriptLineMeta::CellLine {
                    cell_index,
                    line_in_cell,
                });
            }

            if cell_index + 1 < cells.len() && !cell.is_stream_continuation() {
                lines.push(Line::from(""));
                meta.push(TranscriptLineMeta::Spacer);
            }
        }

        self.lines = lines;
        self.line_meta = meta;
    }

    /// Return cached lines.
    #[must_use]
    pub fn lines(&self) -> &[Line<'static>] {
        &self.lines
    }

    /// Return cached line metadata.
    #[must_use]
    pub fn line_meta(&self) -> &[TranscriptLineMeta] {
        &self.line_meta
    }

    /// Return total cached lines.
    #[must_use]
    pub fn total_lines(&self) -> usize {
        self.lines.len()
    }
}
