//! Full-screen live transcript overlay with sticky-bottom auto-scroll (#94).
//!
//! Toggled with `Ctrl+T` while the engine is streaming. Behaviour:
//!
//! - At-bottom (`sticky_to_bottom = true`) — every refresh re-pins scroll to
//!   the new tail, so streaming output appears to flow off the bottom edge.
//! - Scroll up — `sticky_to_bottom` flips to `false`; subsequent refreshes
//!   leave scroll position alone so the user can read history without being
//!   yanked back down.
//! - Scroll back to bottom (End / G / paging past the tail) — `sticky` flips
//!   to `true` again; auto-tail resumes.
//! - Esc / `q` — close, returning to the normal view. The engine never
//!   pauses while the overlay is open; new chunks accumulate in the cells
//!   exactly as they would on the normal screen.
//!
//! Cache strategy: the overlay holds its own `TranscriptCache` keyed by
//! `(CellId, width, revision)`. Revisions come from the same per-cell
//! counters the main transcript already maintains (`App.history_revisions`
//! and `App.active_cell_revision`). Resize invalidates the cells whose width
//! key just changed; revision bumps invalidate only the cells that mutated;
//! cells that didn't change reuse their existing wrap.

use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget, Wrap},
};

use crate::palette;
use crate::tui::app::App;
use crate::tui::history::{HistoryCell, TranscriptRenderOptions};
use crate::tui::transcript_cache::{CellId, TranscriptCache};
use crate::tui::views::{ModalKind, ModalView, ViewAction};

/// Single-line footer hint. Kept short so it fits on narrow terminals.
const FOOTER_HINT: &str =
    " j/k scroll  Space/b page  g/G top/bottom  End=resume tail  q/Esc close ";

/// Snapshot of one cell, refreshed every frame from `App`. Owns the cell so
/// the overlay's `render(&self)` can wrap without re-borrowing `App`.
#[derive(Debug, Clone)]
struct CellSnapshot {
    id: CellId,
    revision: u64,
    cell: HistoryCell,
}

pub struct LiveTranscriptOverlay {
    /// Latest cell snapshots (history + active). Refreshed via
    /// `refresh_from_app` immediately before each render so streaming
    /// mutations show up on the next paint.
    snapshots: Vec<CellSnapshot>,
    /// Render options sampled from `App` at refresh time so toggles like
    /// `show_thinking` propagate into the overlay live.
    options: TranscriptRenderOptions,
    /// Wrapped-line cache. `RefCell` so `render(&self)` can write through.
    cache: RefCell<TranscriptCache>,
    /// Sticky-tail flag: when `true`, refresh re-pins scroll to the bottom.
    /// Flipped to `false` when the user scrolls up; flipped back to `true`
    /// when they scroll past the last visible line.
    sticky_to_bottom: bool,
    /// Current top-of-viewport line offset into the flattened line list.
    scroll: usize,
    /// Visible content height from the last render. Used by paging keys
    /// before the next render frame populates a fresh value.
    last_visible_height: RefCell<usize>,
    /// Last total line count after wrapping; cached so `handle_key` can
    /// clamp scroll without re-wrapping. Updated by `render`.
    last_total_lines: RefCell<usize>,
    /// Pending `gg` second keystroke for Vim-style jump-to-top.
    pending_g: bool,
}

impl LiveTranscriptOverlay {
    #[must_use]
    pub fn new() -> Self {
        Self {
            snapshots: Vec::new(),
            options: TranscriptRenderOptions::default(),
            cache: RefCell::new(TranscriptCache::new()),
            sticky_to_bottom: true,
            scroll: 0,
            last_visible_height: RefCell::new(0),
            last_total_lines: RefCell::new(0),
            pending_g: false,
        }
    }

    /// Pull the latest cells + revisions from `App` so the next `render` shows
    /// streaming mutations. Must be called before `view_stack.render` while
    /// this overlay is on top; otherwise the cells stay frozen at whatever
    /// state they were in when the overlay was first opened.
    pub fn refresh_from_app(&mut self, app: &mut App) {
        app.resync_history_revisions();
        let mut new_snapshots = Vec::with_capacity(
            app.history.len() + app.active_cell.as_ref().map_or(0, |a| a.entries().len()),
        );
        for (idx, cell) in app.history.iter().enumerate() {
            let rev = app.history_revisions.get(idx).copied().unwrap_or(0);
            new_snapshots.push(CellSnapshot {
                id: CellId::History(idx),
                revision: rev,
                cell: cell.clone(),
            });
        }
        if let Some(active) = app.active_cell.as_ref() {
            let active_rev = app.active_cell_revision;
            for (idx, cell) in active.entries().iter().enumerate() {
                let salt = (idx as u64).wrapping_add(1);
                // Salt mirrors the main-transcript scheme so cache keys are
                // stable across the two overlays for the same active entry.
                let revision = active_rev
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(salt);
                new_snapshots.push(CellSnapshot {
                    id: CellId::Active(idx),
                    revision,
                    cell: cell.clone(),
                });
            }
        }
        self.snapshots = new_snapshots;
        self.options = app.transcript_render_options();
    }

    /// Wrap each cell (using the cache) and return the flat line vector.
    fn flatten(&self, width: u16) -> Vec<Line<'static>> {
        let width = width.max(1);
        let mut out: Vec<Line<'static>> = Vec::new();
        let mut cache = self.cache.borrow_mut();
        for snap in &self.snapshots {
            let lines: Vec<Line<'static>> = match cache.get(snap.id, width, snap.revision) {
                Some(cached) => cached.to_vec(),
                None => {
                    let rendered = snap.cell.lines_with_options(width, self.options);
                    cache.insert(snap.id, width, snap.revision, rendered.clone());
                    rendered
                }
            };
            out.extend(lines);
        }
        out
    }

    fn page_height(&self) -> usize {
        let cached = *self.last_visible_height.borrow();
        if cached == 0 { 10 } else { cached }
    }

    fn half_page_height(&self) -> usize {
        self.page_height().div_ceil(2).max(1)
    }

    fn max_scroll(&self) -> usize {
        let total = *self.last_total_lines.borrow();
        let visible = self.page_height();
        total.saturating_sub(visible)
    }

    fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
        // Any upward motion exits sticky-tail; explicit user intent.
        self.sticky_to_bottom = false;
    }

    fn scroll_down(&mut self, amount: usize) {
        let max = self.max_scroll();
        self.scroll = (self.scroll + amount).min(max);
        if self.scroll >= max {
            self.sticky_to_bottom = true;
        }
    }

    fn jump_to_top(&mut self) {
        self.scroll = 0;
        self.sticky_to_bottom = false;
    }

    fn jump_to_bottom(&mut self) {
        self.scroll = self.max_scroll();
        self.sticky_to_bottom = true;
    }

    /// For tests: snapshot count.
    #[cfg(test)]
    fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// For tests: whether sticky-tail is currently armed.
    #[cfg(test)]
    pub fn is_sticky(&self) -> bool {
        self.sticky_to_bottom
    }

    /// For tests: current scroll offset.
    #[cfg(test)]
    pub fn scroll_offset(&self) -> usize {
        self.scroll
    }
}

impl Default for LiveTranscriptOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl ModalView for LiveTranscriptOverlay {
    fn kind(&self) -> ModalKind {
        ModalKind::LiveTranscript
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        if ctrl {
            match key.code {
                KeyCode::Char('d') | KeyCode::Char('D') => {
                    self.scroll_down(self.half_page_height());
                    self.pending_g = false;
                    return ViewAction::None;
                }
                KeyCode::Char('u') | KeyCode::Char('U') => {
                    self.scroll_up(self.half_page_height());
                    self.pending_g = false;
                    return ViewAction::None;
                }
                KeyCode::Char('f') | KeyCode::Char('F') => {
                    self.scroll_down(self.page_height());
                    self.pending_g = false;
                    return ViewAction::None;
                }
                KeyCode::Char('b') | KeyCode::Char('B') => {
                    self.scroll_up(self.page_height());
                    self.pending_g = false;
                    return ViewAction::None;
                }
                // Ctrl+T toggles the overlay closed when already open.
                KeyCode::Char('t') | KeyCode::Char('T') => return ViewAction::Close,
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => ViewAction::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up(1);
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down(1);
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::PageUp => {
                self.scroll_up(self.page_height());
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::PageDown => {
                self.scroll_down(self.page_height());
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::Char(' ') if shift => {
                self.scroll_up(self.page_height());
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::Char(' ') => {
                self.scroll_down(self.page_height());
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::Home => {
                self.jump_to_top();
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::End => {
                self.jump_to_bottom();
                self.pending_g = false;
                ViewAction::None
            }
            KeyCode::Char('g') => {
                if self.pending_g {
                    self.jump_to_top();
                    self.pending_g = false;
                } else {
                    self.pending_g = true;
                }
                ViewAction::None
            }
            KeyCode::Char('G') => {
                self.jump_to_bottom();
                self.pending_g = false;
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_width = area.width.saturating_sub(2).max(1);
        let popup_height = area.height.saturating_sub(2).max(1);
        let popup_area = Rect {
            x: 1,
            y: 1,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        // Compute inner content height once: borders eat 1 row top + 1 bottom,
        // padding eats 1 more on each side.
        let visible_height = popup_area.height.saturating_sub(4) as usize;
        *self.last_visible_height.borrow_mut() = visible_height;

        // Wrap content using the per-cell cache; subtract padding from width
        // so wrapped lines fit between the inner edges.
        let content_width = popup_width.saturating_sub(4);
        let lines = self.flatten(content_width);
        *self.last_total_lines.borrow_mut() = lines.len();

        let max_scroll = lines.len().saturating_sub(visible_height);
        // Sticky-tail: every render re-pins scroll to the bottom unless the
        // user has explicitly scrolled away. Without this, streaming new
        // content would push the visible window backwards as `scroll` stays
        // fixed against a growing total.
        let scroll = if self.sticky_to_bottom {
            max_scroll
        } else {
            self.scroll.min(max_scroll)
        };
        let end = (scroll + visible_height).min(lines.len());
        let visible_lines: Vec<Line<'static>> = if lines.is_empty() {
            vec![Line::from(Span::styled(
                "(no transcript yet)",
                Style::default().fg(palette::TEXT_DIM),
            ))]
        } else {
            lines[scroll..end].to_vec()
        };

        let title = if self.sticky_to_bottom {
            " Live transcript (tailing) "
        } else {
            " Live transcript (paused) "
        };

        let footer = Line::from(Span::styled(
            FOOTER_HINT,
            Style::default().fg(palette::TEXT_HINT),
        ));
        let block = Block::default()
            .title(title)
            .title_bottom(footer)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::DEEPSEEK_INK))
            .padding(Padding::uniform(1));

        let paragraph = Paragraph::new(visible_lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(popup_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::history::HistoryCell;

    fn user(s: &str) -> HistoryCell {
        HistoryCell::User {
            content: s.to_string(),
        }
    }

    fn assistant(s: &str, streaming: bool) -> HistoryCell {
        HistoryCell::Assistant {
            content: s.to_string(),
            streaming,
        }
    }

    /// Force a render so `last_visible_height` and `last_total_lines` are
    /// populated; otherwise paging keys use the constant fallback.
    fn prime_layout(view: &mut LiveTranscriptOverlay, height: u16) {
        let area = Rect::new(0, 0, 60, height);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
    }

    fn install_snapshots(view: &mut LiveTranscriptOverlay, cells: Vec<HistoryCell>) {
        view.snapshots = cells
            .into_iter()
            .enumerate()
            .map(|(idx, cell)| CellSnapshot {
                id: CellId::History(idx),
                revision: 1,
                cell,
            })
            .collect();
    }

    #[test]
    fn new_overlay_starts_sticky() {
        let v = LiveTranscriptOverlay::new();
        assert!(v.is_sticky());
        assert_eq!(v.scroll_offset(), 0);
        assert_eq!(v.snapshot_count(), 0);
    }

    #[test]
    fn scroll_up_breaks_sticky() {
        let mut v = LiveTranscriptOverlay::new();
        install_snapshots(
            &mut v,
            (0..50).map(|i| user(&format!("line {i}"))).collect(),
        );
        prime_layout(&mut v, 10);
        // Force scroll non-zero so scroll_up actually moves.
        v.scroll = 5;
        v.sticky_to_bottom = true;
        let _ = v.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert!(!v.is_sticky(), "scrolling up must release the sticky tail");
    }

    #[test]
    fn end_resumes_sticky_tail() {
        let mut v = LiveTranscriptOverlay::new();
        install_snapshots(
            &mut v,
            (0..50).map(|i| user(&format!("line {i}"))).collect(),
        );
        prime_layout(&mut v, 10);
        // Drop out of sticky mode by scrolling up.
        v.scroll = 10;
        v.sticky_to_bottom = false;
        let _ = v.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert!(
            v.is_sticky(),
            "End must re-arm the sticky tail so streaming continues to follow"
        );
    }

    #[test]
    fn scrolling_to_max_re_arms_sticky() {
        let mut v = LiveTranscriptOverlay::new();
        install_snapshots(
            &mut v,
            (0..50).map(|i| user(&format!("line {i}"))).collect(),
        );
        prime_layout(&mut v, 10);
        v.sticky_to_bottom = false;
        // PageDown once should not re-arm since we're not yet at the tail.
        let _ = v.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        // Now jump explicitly to bottom and verify re-arm.
        v.scroll = 0;
        v.sticky_to_bottom = false;
        let _ = v.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));
        assert!(v.is_sticky());
    }

    #[test]
    fn esc_closes() {
        let mut v = LiveTranscriptOverlay::new();
        let action = v.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(action, ViewAction::Close));
    }

    #[test]
    fn ctrl_t_closes_when_already_open() {
        let mut v = LiveTranscriptOverlay::new();
        let action = v.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(matches!(action, ViewAction::Close));
    }

    #[test]
    fn render_does_not_panic_on_empty() {
        let v = LiveTranscriptOverlay::new();
        let area = Rect::new(0, 0, 40, 12);
        let mut buf = Buffer::empty(area);
        v.render(area, &mut buf);
    }

    #[test]
    fn cache_reuses_unchanged_cells_across_renders() {
        // Same revisions across two renders should reuse cache entries; only
        // a "modified" cell (different revision) forces a new wrap. Verify by
        // counting cache size — it grows by 1 per unique (cell, width, rev).
        let mut v = LiveTranscriptOverlay::new();
        install_snapshots(&mut v, vec![user("a"), user("b"), assistant("c", false)]);
        let area = Rect::new(0, 0, 60, 16);
        let mut buf = Buffer::empty(area);
        v.render(area, &mut buf);
        let after_first = v.cache.borrow().len();
        v.render(area, &mut buf);
        let after_second = v.cache.borrow().len();
        assert_eq!(
            after_first, after_second,
            "second render should reuse every cell — no new cache entries"
        );
    }

    #[test]
    fn cache_invalidates_on_revision_bump() {
        let mut v = LiveTranscriptOverlay::new();
        install_snapshots(&mut v, vec![user("a"), assistant("b", true)]);
        let area = Rect::new(0, 0, 60, 16);
        let mut buf = Buffer::empty(area);
        v.render(area, &mut buf);
        let before = v.cache.borrow().len();
        // Bump the streaming assistant's revision (simulating a delta) and
        // re-render. We expect the cache to grow by one new entry — the new
        // (cell, width, new_rev) — while the user cell entry is reused.
        v.snapshots[1].revision = 2;
        v.render(area, &mut buf);
        let after = v.cache.borrow().len();
        assert!(
            after > before,
            "bumping a revision must add a new cache entry"
        );
    }

    #[test]
    fn resize_does_not_evict_unchanged_width_entries() {
        // Render at width=60, then again at width=80. Both wraps must
        // co-exist in the cache so flipping back to width=60 hits cache.
        let mut v = LiveTranscriptOverlay::new();
        install_snapshots(&mut v, vec![user("a"), user("b")]);
        let small = Rect::new(0, 0, 60, 16);
        let large = Rect::new(0, 0, 80, 16);
        let mut buf_s = Buffer::empty(small);
        let mut buf_l = Buffer::empty(large);
        v.render(small, &mut buf_s);
        let after_small = v.cache.borrow().len();
        v.render(large, &mut buf_l);
        let after_both = v.cache.borrow().len();
        assert!(
            after_both > after_small,
            "rendering at a new width must add new cache entries"
        );
        // Flip back to small — should NOT add any new entries (cache hits).
        v.render(small, &mut buf_s);
        let after_replay = v.cache.borrow().len();
        assert_eq!(
            after_replay, after_both,
            "replay at old width must hit cache"
        );
    }
}
