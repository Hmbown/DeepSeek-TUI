//! Scroll state tracking for transcript rendering.

use std::time::{Duration, Instant};

// === Transcript Line Metadata ===

/// Metadata describing how rendered transcript lines map to history cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptLineMeta {
    CellLine {
        cell_index: usize,
        line_in_cell: usize,
    },
    Spacer,
}

impl TranscriptLineMeta {
    /// Return cell/line indices if this entry is a cell line.
    #[must_use]
    pub fn cell_line(&self) -> Option<(usize, usize)> {
        match *self {
            TranscriptLineMeta::CellLine {
                cell_index,
                line_in_cell,
            } => Some((cell_index, line_in_cell)),
            TranscriptLineMeta::Spacer => None,
        }
    }
}

// === Scroll Anchors ===

/// Scroll anchor for the transcript view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranscriptScroll {
    #[default]
    ToBottom,
    Scrolled {
        cell_index: usize,
        line_in_cell: usize,
    },
    ScrolledSpacerBeforeCell {
        cell_index: usize,
    },
}

impl TranscriptScroll {
    /// Resolve the anchor to a top line index.
    #[must_use]
    pub fn resolve_top(self, line_meta: &[TranscriptLineMeta], max_start: usize) -> (Self, usize) {
        match self {
            TranscriptScroll::ToBottom => (TranscriptScroll::ToBottom, max_start),
            TranscriptScroll::Scrolled {
                cell_index,
                line_in_cell,
            } => {
                let anchor = anchor_index(line_meta, cell_index, line_in_cell);
                match anchor {
                    Some(idx) => (self, idx.min(max_start)),
                    None => (TranscriptScroll::ToBottom, max_start),
                }
            }
            TranscriptScroll::ScrolledSpacerBeforeCell { cell_index } => {
                let anchor = spacer_before_cell_index(line_meta, cell_index);
                match anchor {
                    Some(idx) => (self, idx.min(max_start)),
                    None => (TranscriptScroll::ToBottom, max_start),
                }
            }
        }
    }

    /// Apply a delta scroll and return the updated anchor.
    #[must_use]
    pub fn scrolled_by(
        self,
        delta_lines: i32,
        line_meta: &[TranscriptLineMeta],
        visible_lines: usize,
    ) -> Self {
        if delta_lines == 0 {
            return self;
        }

        let total_lines = line_meta.len();
        if total_lines <= visible_lines {
            return TranscriptScroll::ToBottom;
        }

        let max_start = total_lines.saturating_sub(visible_lines);
        let current_top = match self {
            TranscriptScroll::ToBottom => max_start,
            TranscriptScroll::Scrolled {
                cell_index,
                line_in_cell,
            } => anchor_index(line_meta, cell_index, line_in_cell)
                .unwrap_or(max_start)
                .min(max_start),
            TranscriptScroll::ScrolledSpacerBeforeCell { cell_index } => {
                spacer_before_cell_index(line_meta, cell_index)
                    .unwrap_or(max_start)
                    .min(max_start)
            }
        };

        let new_top = if delta_lines < 0 {
            current_top.saturating_sub(delta_lines.unsigned_abs() as usize)
        } else {
            let delta = usize::try_from(delta_lines).unwrap_or(usize::MAX);
            current_top.saturating_add(delta).min(max_start)
        };

        if new_top == max_start {
            TranscriptScroll::ToBottom
        } else {
            TranscriptScroll::anchor_for(line_meta, new_top).unwrap_or(TranscriptScroll::ToBottom)
        }
    }

    /// Create an anchor from a top line index.
    #[must_use]
    pub fn anchor_for(line_meta: &[TranscriptLineMeta], start: usize) -> Option<Self> {
        if line_meta.is_empty() {
            return None;
        }

        let start = start.min(line_meta.len().saturating_sub(1));
        match line_meta[start] {
            TranscriptLineMeta::CellLine {
                cell_index,
                line_in_cell,
            } => Some(TranscriptScroll::Scrolled {
                cell_index,
                line_in_cell,
            }),
            TranscriptLineMeta::Spacer => {
                if let Some((cell_index, _)) = anchor_at_or_after(line_meta, start) {
                    Some(TranscriptScroll::ScrolledSpacerBeforeCell { cell_index })
                } else {
                    anchor_at_or_before(line_meta, start).map(|(cell_index, line_in_cell)| {
                        TranscriptScroll::Scrolled {
                            cell_index,
                            line_in_cell,
                        }
                    })
                }
            }
        }
    }
}

fn anchor_index(
    line_meta: &[TranscriptLineMeta],
    cell_index: usize,
    line_in_cell: usize,
) -> Option<usize> {
    line_meta
        .iter()
        .enumerate()
        .find_map(|(idx, entry)| match *entry {
            TranscriptLineMeta::CellLine {
                cell_index: ci,
                line_in_cell: li,
            } if ci == cell_index && li == line_in_cell => Some(idx),
            _ => None,
        })
}

fn spacer_before_cell_index(line_meta: &[TranscriptLineMeta], cell_index: usize) -> Option<usize> {
    line_meta.iter().enumerate().find_map(|(idx, entry)| {
        if matches!(entry, TranscriptLineMeta::Spacer)
            && line_meta
                .get(idx + 1)
                .and_then(TranscriptLineMeta::cell_line)
                .is_some_and(|(ci, _)| ci == cell_index)
        {
            Some(idx)
        } else {
            None
        }
    })
}

fn anchor_at_or_after(line_meta: &[TranscriptLineMeta], start: usize) -> Option<(usize, usize)> {
    line_meta
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(_, entry)| entry.cell_line())
}

fn anchor_at_or_before(line_meta: &[TranscriptLineMeta], start: usize) -> Option<(usize, usize)> {
    line_meta
        .iter()
        .enumerate()
        .take(start.saturating_add(1))
        .rev()
        .find_map(|(_, entry)| entry.cell_line())
}

/// Direction for mouse scroll input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

impl ScrollDirection {
    fn sign(self) -> i32 {
        match self {
            ScrollDirection::Up => -1,
            ScrollDirection::Down => 1,
        }
    }
}

/// Stateful tracker for mouse scroll accumulation.
#[derive(Debug, Default)]
pub struct MouseScrollState {
    last_event_at: Option<Instant>,
    pending_lines: i32,
}

/// A computed scroll delta from user input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollUpdate {
    pub delta_lines: i32,
}

impl MouseScrollState {
    /// Create a new scroll state tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a scroll event and return the resulting delta.
    pub fn on_scroll(&mut self, direction: ScrollDirection) -> ScrollUpdate {
        let now = Instant::now();
        let is_trackpad = self
            .last_event_at
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(35));
        self.last_event_at = Some(now);

        let lines_per_tick = if is_trackpad { 1 } else { 3 };
        self.pending_lines += direction.sign() * lines_per_tick;

        let delta = self.pending_lines;
        self.pending_lines = 0;
        ScrollUpdate { delta_lines: delta }
    }
}
