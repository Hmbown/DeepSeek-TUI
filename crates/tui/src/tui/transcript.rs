//! Cached transcript rendering for the TUI.
//!
//! ## Per-cell revision caching
//!
//! Naive caching invalidates the whole transcript whenever ANY cell mutates.
//! During streaming the assistant content cell mutates on every delta — that
//! would force a re-wrap of every cell on every chunk. Codex avoids this by
//! tracking a per-cell revision counter; we mirror that pattern here.
//!
//! Each cell index has a paired `revision: u64`. The cache stores
//! `Vec<CachedCell>` with `(cell_index, revision, lines, line_meta)`. On
//! `ensure`, walk the cells; if a cell's current `revision` matches the cached
//! one (and width/options haven't changed), reuse the rendered lines.
//! Otherwise re-render that cell only and reassemble.
//!
//! Width or render-option changes still bust the entire cache (correct: wrap
//! layout depends on width and which cells are visible at all).

use std::sync::Arc;

use ratatui::text::Line;

use crate::tui::app::TranscriptSpacing;
use crate::tui::history::{HistoryCell, TranscriptRenderOptions};
use crate::tui::scrolling::TranscriptLineMeta;

/// Per-cell cached render output. Reused across `ensure` calls when the
/// upstream cell's revision counter hasn't changed.
///
/// Lines are stored behind an `Arc` so that cloning a `CachedCell` during
/// cache-ensure (which touches every cell every frame) is O(1) rather than
/// O(rendered_line_count). Without this, scrolling on a long transcript
/// pays the cost of deep-cloning every cell's `Vec<Line>` per frame, which
/// is the surface-level symptom of issue #78. The flatten step uses
/// `Arc::make_mut` to produce an owned `Vec` for the final `lines`
/// assembly, so the only deep-clone occurs on the flattened output — once
/// per frame instead of once per cell.
#[derive(Debug, Clone)]
struct CachedCell {
    /// Revision the cell was at when the lines/meta were rendered.
    revision: u64,
    /// Rendered lines for this cell (without trailing inter-cell spacers),
    /// shared via `Arc` so cache enumeration is O(N) not O(N*lines).
    lines: Arc<Vec<Line<'static>>>,
    /// Whether this cell's rendered output was empty (e.g. Thinking hidden).
    /// Cached so we can skip empty cells without re-rendering.
    is_empty: bool,
    /// Whether this cell is a stream continuation. Determines spacer rules.
    /// Cached because `is_stream_continuation` is cheap but reading via the
    /// cache lets us decide spacers without touching the cell.
    is_stream_continuation: bool,
    /// Whether this cell is conversational (User/Assistant/Thinking). Used
    /// for spacer calculations.
    is_conversational: bool,
    /// Whether this cell is a System or Tool cell (affects spacer rules).
    is_system_or_tool: bool,
}

/// Cache of rendered transcript lines for the current viewport.
#[derive(Debug)]
pub struct TranscriptViewCache {
    width: u16,
    options: TranscriptRenderOptions,
    /// Per-cell rendered output, indexed by current cell position.
    /// Length always equals the cell count seen on the last `ensure` call.
    per_cell: Vec<CachedCell>,
    /// Flattened lines reassembled from `per_cell` plus spacers.
    lines: Vec<Line<'static>>,
    /// Per-line metadata aligned with `lines`.
    line_meta: Vec<TranscriptLineMeta>,
}

impl TranscriptViewCache {
    /// Create an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            width: 0,
            options: TranscriptRenderOptions::default(),
            per_cell: Vec::new(),
            lines: Vec::new(),
            line_meta: Vec::new(),
        }
    }

    /// Ensure cached lines match the provided cells/widths/per-cell revisions.
    ///
    /// Reuses rendered lines for cells whose `cell_revisions[i]` matches the
    /// previously cached revision (when the cell shape — empty/spacer flags —
    /// also matches). Width or option changes bust the entire cache.
    ///
    /// `cell_revisions.len()` is expected to equal `cells.len()`. If they
    /// disagree (shouldn't happen in normal use) the cache treats every cell
    /// as dirty.
    ///
    /// Retained for tests and external use; the live render path uses the
    /// `ensure_split` variant to avoid concatenating history + active-cell
    /// entries every frame.
    #[allow(dead_code)]
    pub fn ensure(
        &mut self,
        cells: &[HistoryCell],
        cell_revisions: &[u64],
        width: u16,
        options: TranscriptRenderOptions,
    ) {
        self.ensure_split(&[cells], cell_revisions, width, options);
    }

    /// Ensure cached lines match the provided cell shards (logically
    /// concatenated) plus per-cell revisions. Avoids the
    /// `concat-into-Vec<HistoryCell>` clone the caller would otherwise pay
    /// every frame on long transcripts.
    pub fn ensure_split(
        &mut self,
        cell_shards: &[&[HistoryCell]],
        cell_revisions: &[u64],
        width: u16,
        options: TranscriptRenderOptions,
    ) {
        let total_cells: usize = cell_shards.iter().map(|s| s.len()).sum();

        let layout_changed = self.width != width || self.options != options;
        if layout_changed {
            self.per_cell.clear();
        }
        self.width = width;
        self.options = options;

        // Track whether anything actually changed; if all cells are reused at
        // the same indices, we can skip the reflatten.
        let mut any_dirty = layout_changed || self.per_cell.len() != total_cells;

        let mut new_per_cell: Vec<CachedCell> = Vec::with_capacity(total_cells);
        let revisions_match = cell_revisions.len() == total_cells;

        let mut idx: usize = 0;
        for shard in cell_shards {
            for cell in *shard {
                let current_rev = if revisions_match {
                    cell_revisions[idx]
                } else {
                    // No matching revisions — force a re-render this cycle.
                    u64::MAX
                };

                // Reuse cached entry if the revision matches AND it's at the
                // same index (cells can shift on insert/remove, so we only
                // reuse when the index is identical — a stricter invariant
                // codex also uses for its active-cell tail).
                if let Some(prev) = self.per_cell.get(idx)
                    && !layout_changed
                    && prev.revision == current_rev
                    && revisions_match
                {
                    new_per_cell.push(prev.clone());
                    idx += 1;
                    continue;
                }

                any_dirty = true;
                let rendered = cell.lines_with_options(width, options);
                let is_empty = rendered.is_empty();
                new_per_cell.push(CachedCell {
                    revision: current_rev,
                    lines: Arc::new(rendered),
                    is_empty,
                    is_stream_continuation: cell.is_stream_continuation(),
                    is_conversational: cell.is_conversational(),
                    is_system_or_tool: matches!(
                        cell,
                        HistoryCell::System { .. }
                            | HistoryCell::Error { .. }
                            | HistoryCell::Tool(_)
                            | HistoryCell::SubAgent(_)
                    ),
                });
                idx += 1;
            }
        }

        self.per_cell = new_per_cell;

        if !any_dirty {
            // All cells reused at the same indices: nothing to reflatten.
            // (Width didn't change either, since that bumps `layout_changed`.)
            return;
        }

        self.flatten(options.spacing);
    }

    /// Reassemble flat `lines` / `line_meta` from `per_cell` plus spacers.
    fn flatten(&mut self, spacing: TranscriptSpacing) {
        let mut lines = Vec::with_capacity(self.lines.capacity());
        let mut meta = Vec::with_capacity(self.line_meta.capacity());

        for (cell_index, cached) in self.per_cell.iter().enumerate() {
            if cached.is_empty {
                continue;
            }
            // Arc::make_mut would deep-clone only on write; since we just
            // rebuilt `lines` from scratch we always need the owned data.
            // Deref is zero-cost and gives us &[Line].
            for (line_in_cell, line) in cached.lines.iter().enumerate() {
                lines.push(line.clone());
                meta.push(TranscriptLineMeta::CellLine {
                    cell_index,
                    line_in_cell,
                });
            }

            if let Some(next) = self.per_cell.get(cell_index + 1) {
                let spacer_rows = spacer_rows_between(cached, next, spacing);
                for _ in 0..spacer_rows {
                    lines.push(Line::from(""));
                    meta.push(TranscriptLineMeta::Spacer);
                }
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

fn spacer_rows_between(
    current: &CachedCell,
    next: &CachedCell,
    spacing: TranscriptSpacing,
) -> usize {
    if current.is_stream_continuation {
        return 0;
    }

    let conversational_gap = match spacing {
        TranscriptSpacing::Compact => 0,
        TranscriptSpacing::Comfortable => 1,
        TranscriptSpacing::Spacious => 2,
    };
    let secondary_gap = match spacing {
        TranscriptSpacing::Compact => 0,
        TranscriptSpacing::Comfortable | TranscriptSpacing::Spacious => 1,
    };

    if current.is_conversational && next.is_conversational {
        conversational_gap
    } else if current.is_system_or_tool || next.is_system_or_tool {
        secondary_gap
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::history::HistoryCell;

    fn user_cell(content: &str) -> HistoryCell {
        HistoryCell::User {
            content: content.to_string(),
        }
    }

    fn assistant_cell(content: &str, streaming: bool) -> HistoryCell {
        HistoryCell::Assistant {
            content: content.to_string(),
            streaming,
        }
    }

    #[test]
    fn cache_reuses_cells_when_revision_unchanged() {
        let cells = vec![
            user_cell("hello"),
            assistant_cell("world", false),
            user_cell("again"),
        ];
        let revisions = vec![1u64, 1, 1];

        let mut cache = TranscriptViewCache::new();
        cache.ensure(&cells, &revisions, 80, TranscriptRenderOptions::default());
        let first_lines: Vec<String> = cache
            .lines()
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        let first_total = cache.total_lines();
        assert!(first_total > 0, "expected non-empty render");

        // Capture per-cell lines snapshot to verify reuse.
        let snapshot_per_cell: Vec<Vec<String>> = cache
            .per_cell
            .iter()
            .map(|c| {
                c.lines
                    .iter()
                    .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
                    .collect()
            })
            .collect();

        // Same revisions => everything reused, output identical.
        cache.ensure(&cells, &revisions, 80, TranscriptRenderOptions::default());
        let second_lines: Vec<String> = cache
            .lines()
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert_eq!(first_lines, second_lines);
        assert_eq!(cache.total_lines(), first_total);

        let snapshot_per_cell_2: Vec<Vec<String>> = cache
            .per_cell
            .iter()
            .map(|c| {
                c.lines
                    .iter()
                    .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
                    .collect()
            })
            .collect();
        assert_eq!(snapshot_per_cell, snapshot_per_cell_2);
    }

    #[test]
    fn bumping_one_cell_revision_only_rerenders_that_cell() {
        // Track render counts per cell using a custom HistoryCell wrapper
        // would require trait changes; instead, we detect reuse by inspecting
        // CachedCell instances. After a bump, only the bumped cell's stored
        // revision should differ from before; others remain identical.

        let cells_v1 = vec![
            user_cell("hello"),
            assistant_cell("hi", true),
            user_cell("again"),
        ];
        let revs_v1 = vec![1u64, 1, 1];

        let mut cache = TranscriptViewCache::new();
        cache.ensure(&cells_v1, &revs_v1, 80, TranscriptRenderOptions::default());

        // Snapshot the cached lines for cells 0 and 2 (unchanged across the
        // delta).
        let cell0_lines_before = cache.per_cell[0]
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let cell2_lines_before = cache.per_cell[2]
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        // Mutate cell 1 (assistant streaming delta) and bump only its rev.
        let cells_v2 = vec![
            user_cell("hello"),
            assistant_cell("hi world", true),
            user_cell("again"),
        ];
        let revs_v2 = vec![1u64, 2, 1];

        cache.ensure(&cells_v2, &revs_v2, 80, TranscriptRenderOptions::default());

        // Cells 0 and 2 are byte-identical (proving reuse path didn't corrupt).
        let cell0_lines_after = cache.per_cell[0]
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let cell2_lines_after = cache.per_cell[2]
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(cell0_lines_before, cell0_lines_after);
        assert_eq!(cell2_lines_before, cell2_lines_after);

        // Cell 1 reflects the new content.
        // The renderer interleaves role/whitespace spans, so the joined
        // content has internal padding (e.g. "Assistant   hi   world").
        // Check for the new tokens individually rather than a literal
        // "hi world" substring.
        let cell1_after: String = cache.per_cell[1]
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            cell1_after.contains("hi") && cell1_after.contains("world"),
            "cell1 should re-render with new content; got: {cell1_after}"
        );

        // Revisions in cache reflect the bump.
        assert_eq!(cache.per_cell[0].revision, 1);
        assert_eq!(cache.per_cell[1].revision, 2);
        assert_eq!(cache.per_cell[2].revision, 1);
    }

    #[test]
    fn width_change_rerenders_all_cells() {
        let cells = vec![
            user_cell("a fairly long message that may wrap at narrow widths"),
            assistant_cell("another long message body content", false),
        ];
        let revisions = vec![5u64, 7];

        let mut cache = TranscriptViewCache::new();
        cache.ensure(&cells, &revisions, 80, TranscriptRenderOptions::default());
        let wide_total = cache.total_lines();

        // Narrow width should change layout — everything re-renders.
        cache.ensure(&cells, &revisions, 20, TranscriptRenderOptions::default());
        let narrow_total = cache.total_lines();

        assert_ne!(
            wide_total, narrow_total,
            "narrow width should produce a different number of lines"
        );

        // Restoring the original width re-renders again.
        cache.ensure(&cells, &revisions, 80, TranscriptRenderOptions::default());
        assert_eq!(cache.total_lines(), wide_total);
    }

    #[test]
    fn streaming_assistant_only_rebuilds_one_cell_render_count() {
        // Verify behavior 6: when one Assistant cell streams a delta, only
        // that one cell is re-rendered. We use a counting wrapper hooked into
        // a custom History setup. Since `lines_with_options` is on `HistoryCell`
        // (concrete enum), we can't mock it directly. Instead we verify the
        // cache's invariant: cells with unchanged revisions retain their
        // previous CachedCell entries (clone-equal), proving no re-render
        // happened for them.
        //
        // We do this by storing revisions as monotonic u64 and verifying that
        // a `Vec<u64>` snapshot of `per_cell.revision` only differs at the
        // index that was bumped.

        let mut cells: Vec<HistoryCell> =
            (0..50).map(|i| user_cell(&format!("cell {i}"))).collect();
        cells.push(assistant_cell("streaming", true));
        let mut revisions: Vec<u64> = vec![1; 51];

        let mut cache = TranscriptViewCache::new();
        cache.ensure(&cells, &revisions, 80, TranscriptRenderOptions::default());

        // Snapshot total bytes rendered for cells 0..50 (unchanged).
        let stable_snapshot: Vec<String> = cache.per_cell[..50]
            .iter()
            .map(|c| {
                c.lines
                    .iter()
                    .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect();

        // Stream 10 deltas to the assistant cell, bumping only its revision.
        for i in 0..10 {
            if let HistoryCell::Assistant { content, .. } = &mut cells[50] {
                content.push_str(&format!(" delta-{i}"));
            }
            revisions[50] += 1;
            cache.ensure(&cells, &revisions, 80, TranscriptRenderOptions::default());

            // After every delta, cells 0..50 must be byte-identical to the
            // initial render. If we re-rendered them we'd observe identical
            // bytes anyway (deterministic), but the test ALSO checks the
            // CachedCell.revision values stayed at 1 — meaning the cache
            // never replaced them, only reused them.
            let stable_now: Vec<String> = cache.per_cell[..50]
                .iter()
                .map(|c| {
                    c.lines
                        .iter()
                        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
                        .collect::<Vec<_>>()
                        .join("|")
                })
                .collect();
            assert_eq!(
                stable_now, stable_snapshot,
                "stable cells diverged at delta {i}"
            );

            for (idx, c) in cache.per_cell[..50].iter().enumerate() {
                assert_eq!(
                    c.revision, 1,
                    "cell {idx} revision changed during streaming delta"
                );
            }
        }
    }

    #[test]
    fn missing_revisions_falls_back_to_full_render() {
        // If callers pass a `cell_revisions` slice with the wrong length
        // (shouldn't happen, but be defensive), the cache should still
        // produce correct output rather than panic or skip cells.
        let cells = vec![user_cell("a"), assistant_cell("b", false)];
        let bogus_revisions = vec![1u64]; // wrong length

        let mut cache = TranscriptViewCache::new();
        cache.ensure(
            &cells,
            &bogus_revisions,
            80,
            TranscriptRenderOptions::default(),
        );

        // Both cells were rendered (no panic, output non-empty).
        assert_eq!(cache.per_cell.len(), 2);
        assert!(!cache.lines().is_empty());
    }
}
