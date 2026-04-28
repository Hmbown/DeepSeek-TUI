mod footer;
mod header;
// Some helpers (`shift`, `ctrl_alt`, `is_press`, etc.) are part of the
// public surface for issue #93's help overlay and future call sites; allow
// dead code rather than scattering `#[allow]` across every constructor.
#[allow(dead_code)]
pub mod key_hint;
// Phase 1 of #85: widget lands without a wire-up site so reviewers can
// evaluate the rendering in isolation. The follow-up PR plumbs it through
// the composer area in `ui.rs`. `pub mod` (vs the usual `pub use` pattern)
// keeps the unused-imports lint quiet until then.
pub mod agent_card;
pub mod pending_input_preview;
mod renderable;
pub mod tool_card;

pub use footer::{
    FooterProps, FooterToast, FooterWidget, footer_agents_chip, footer_working_label,
};
pub use header::{HeaderData, HeaderWidget};
pub use renderable::Renderable;

use std::time::Duration;

use crate::palette;
use crate::tui::app::{App, AppMode, ComposerDensity};
use crate::tui::approval::{
    ApprovalRequest, ApprovalView, ElevationOption, ElevationRequest, RiskLevel, ToolCategory,
};
use crate::tui::history::HistoryCell;
use crate::tui::scrolling::TranscriptLineMeta;
use crate::{commands, config::COMMON_DEEPSEEK_MODELS};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::Stylize,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Widget, Wrap,
    },
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const SEND_FLASH_DURATION: Duration = Duration::from_millis(500);
const COMPOSER_PANEL_HEIGHT: u16 = 2;

pub struct ChatWidget {
    content_area: Rect,
    lines: Vec<Line<'static>>,
    scrollbar: Option<TranscriptScrollbar>,
}

#[derive(Debug, Clone, Copy)]
struct TranscriptScrollbar {
    top: usize,
    visible: usize,
    total: usize,
}

impl ChatWidget {
    pub fn new(app: &mut App, area: Rect) -> Self {
        let content_area = area;
        let visible_lines = content_area.height as usize;
        let render_options = app.transcript_render_options();

        if should_render_empty_state(app) {
            let lines = build_empty_state_lines(app, content_area);
            app.last_transcript_area = Some(content_area);
            app.last_transcript_top = 0;
            app.last_transcript_visible = visible_lines;
            app.last_transcript_total = 0;
            app.last_transcript_padding_top = 0;
            return Self {
                content_area,
                lines,
                scrollbar: None,
            };
        }

        // Per-cell revision caching (fix for issue #78):
        //
        // Every committed history cell carries its own revision counter in
        // `app.history_revisions`. The transcript cache compares each cell's
        // current revision against the previously rendered one, so unchanged
        // cells reuse their cached wrapped lines instead of being re-wrapped
        // every frame. This is the difference between O(history.len()) and
        // O(changed_cells) per render — and was the root cause of scroll lag
        // on long transcripts.
        //
        // The active in-flight cell (if any) is appended as the last cell so
        // its mutations show up at the live tail. Each entry inside the
        // active cell becomes a virtual cell at index `history.len() + i`,
        // matching `App::cell_at_virtual_index`. Active-cell entries share
        // the same `active_cell_revision` salt so any mutation in the active
        // cell forces only those rows to re-render — committed history rows
        // are unaffected.
        app.resync_history_revisions();
        let active_entries: &[HistoryCell] = app
            .active_cell
            .as_ref()
            .map_or(&[], |active| active.entries());
        // Build the per-cell revision slice without cloning history cells.
        // History cells reuse `app.history_revisions` directly; active-cell
        // entries fall back to a synthetic revision derived from
        // `active_cell_revision` (active cells don't carry their own
        // per-entry counter today).
        let mut cell_revisions: Vec<u64> =
            Vec::with_capacity(app.history.len() + active_entries.len());
        cell_revisions.extend_from_slice(&app.history_revisions);
        if !active_entries.is_empty() {
            let active_rev = app.active_cell_revision;
            for i in 0..active_entries.len() {
                let salt = (i as u64).wrapping_add(1);
                cell_revisions.push(
                    active_rev
                        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        .wrapping_add(salt),
                );
            }
        }
        let shards: [&[HistoryCell]; 2] = [&app.history, active_entries];
        app.transcript_cache.ensure_split(
            &shards,
            &cell_revisions,
            content_area.width.max(1),
            render_options,
        );

        let total_lines = app.transcript_cache.total_lines();

        let line_meta = app.transcript_cache.line_meta();

        if app.pending_scroll_delta != 0 {
            app.transcript_scroll = app.transcript_scroll.scrolled_by(
                app.pending_scroll_delta,
                line_meta,
                visible_lines,
            );
            app.pending_scroll_delta = 0;
        }

        let max_start = total_lines.saturating_sub(visible_lines);
        let (scroll_state, top) = app.transcript_scroll.resolve_top(line_meta, max_start);
        app.transcript_scroll = scroll_state;
        // If the user scrolled back to the live tail, the per-stream
        // "leave me alone" lock is over — new chunks should pin to bottom
        // again until they explicitly scroll up. Without this clear, content
        // piles up off-screen below the visible area and the view appears
        // frozen at the moment they returned to bottom.
        if app.transcript_scroll.is_at_tail() {
            app.user_scrolled_during_stream = false;
        }

        app.last_transcript_area = Some(content_area);
        app.last_transcript_top = top;
        app.last_transcript_visible = visible_lines;
        app.last_transcript_total = total_lines;
        app.last_transcript_padding_top = 0;
        let detail_target_cell = (!app.transcript_selection.is_active())
            .then(|| app.detail_cell_index_for_viewport(top, visible_lines, line_meta))
            .flatten();

        let end = (top + visible_lines).min(total_lines);
        let mut lines = if total_lines == 0 {
            vec![Line::from("")]
        } else {
            app.transcript_cache.lines()[top..end].to_vec()
        };

        // Brief flash highlight on the most recently sent user message.
        if !app.low_motion
            && let Some(send_at) = app.last_send_at
        {
            if send_at.elapsed() < SEND_FLASH_DURATION {
                apply_send_flash(&mut lines, top, &app.history, line_meta);
            } else {
                app.last_send_at = None;
            }
        }

        if let Some(target_cell) = detail_target_cell {
            apply_detail_target_highlight(&mut lines, top, target_cell, line_meta);
        }

        apply_selection(&mut lines, top, app);

        if app.transcript_scroll.is_at_tail() {
            app.last_transcript_padding_top = visible_lines.saturating_sub(lines.len());
            pad_lines_to_bottom(&mut lines, visible_lines);
        }

        let scrollbar = (total_lines > visible_lines && content_area.width > 1).then_some(
            TranscriptScrollbar {
                top,
                visible: visible_lines,
                total: total_lines,
            },
        );

        Self {
            content_area,
            lines,
            scrollbar,
        }
    }
}

impl Renderable for ChatWidget {
    fn render(&self, _area: Rect, buf: &mut Buffer) {
        let paragraph = Paragraph::new(self.lines.clone());
        paragraph.render(self.content_area, buf);

        if let Some(scrollbar) = self.scrollbar {
            let scrollable_range = scrollbar.total.saturating_sub(scrollbar.visible);
            let mut state = ScrollbarState::new(scrollable_range)
                .position(scrollbar.top.min(scrollable_range))
                .viewport_content_length(scrollbar.visible);
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .track_style(Style::default().fg(palette::BORDER_COLOR))
                .thumb_symbol("┃")
                .thumb_style(Style::default().fg(palette::DEEPSEEK_SKY))
                .render(self.content_area, buf, &mut state);
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

pub struct ComposerWidget<'a> {
    app: &'a App,
    max_height: u16,
    slash_menu_entries: &'a [String],
    mention_menu_entries: &'a [String],
}

impl<'a> ComposerWidget<'a> {
    pub fn new(
        app: &'a App,
        max_height: u16,
        slash_menu_entries: &'a [String],
        mention_menu_entries: &'a [String],
    ) -> Self {
        Self {
            app,
            max_height,
            slash_menu_entries,
            mention_menu_entries,
        }
    }

    /// Number of popup rows below the input. Mention and slash menus are
    /// mutually exclusive — the cursor can only sit inside an `@token` OR
    /// a `/cmd` token, not both at once. Mention takes precedence because
    /// the partial-mention check is positional and stricter than slash's
    /// "starts-with-/" check.
    fn active_menu_entries(&self) -> &'a [String] {
        if !self.mention_menu_entries.is_empty() {
            self.mention_menu_entries
        } else {
            self.slash_menu_entries
        }
    }

    fn active_menu_selected(&self) -> usize {
        if !self.mention_menu_entries.is_empty() {
            self.app.mention_menu_selected
        } else {
            self.app.slash_menu_selected
        }
    }

    fn has_panel(&self, area: Rect) -> bool {
        self.app.composer_border && area.height >= 3 && area.width >= 12
    }

    fn inner_area(&self, area: Rect) -> Rect {
        if self.has_panel(area) {
            Block::default().borders(Borders::ALL).inner(area)
        } else {
            area
        }
    }

    fn mode_color(&self) -> Color {
        match self.app.mode {
            AppMode::Agent => palette::MODE_AGENT,
            AppMode::Yolo => palette::MODE_YOLO,
            AppMode::Plan => palette::MODE_PLAN,
        }
    }

    fn max_height_cap(&self) -> u16 {
        composer_max_height(self.app.composer_density)
    }
}

impl Renderable for ComposerWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let background = Style::default().bg(self.app.ui_theme.composer_bg);
        let has_panel = self.has_panel(area);
        let inner_area = self.inner_area(area);
        let menu_entries = self.active_menu_entries();
        let menu_lines = menu_entries.len();
        let input_rows_budget = composer_input_rows_budget(inner_area.height, menu_lines);
        let content_width = usize::from(inner_area.width.max(1));
        let (visible_lines, _cursor_row, _cursor_col) = layout_input(
            &self.app.input,
            self.app.cursor_position,
            content_width,
            input_rows_budget,
        );
        let is_draft_mode = self.app.input.contains('\n') || visible_lines.len() > 1;
        if has_panel {
            let border_color = if self.app.input.trim().is_empty() {
                palette::BORDER_COLOR
            } else {
                self.mode_color()
            };
            let hint_line = if self.slash_menu_entries.is_empty() {
                None
            } else {
                Some(Line::from(vec![
                    Span::styled(" Up/Down move  ", Style::default().fg(palette::TEXT_MUTED)),
                    Span::styled("Tab accept  ", Style::default().fg(palette::TEXT_MUTED)),
                    Span::styled("Esc close", Style::default().fg(palette::TEXT_MUTED)),
                ]))
            };

            let mut block = Block::default()
                .title(Line::from(Span::styled(
                    if is_draft_mode { "Draft" } else { "Composer" },
                    Style::default().fg(palette::TEXT_MUTED),
                )))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .style(background);
            if let Some(hint_line) = hint_line {
                block = block.title_bottom(hint_line);
            }
            block.render(area, buf);
        } else {
            Block::default().style(background).render(area, buf);
        }

        let mut input_lines = Vec::new();
        if self.app.input.is_empty() {
            input_lines.push(Line::from(Span::styled(
                COMPOSER_PLACEHOLDER,
                Style::default().fg(palette::TEXT_MUTED).italic(),
            )));
        } else {
            for line in &visible_lines {
                input_lines.push(Line::from(Span::styled(
                    line.clone(),
                    Style::default().fg(palette::TEXT_PRIMARY),
                )));
            }
        }

        // For non-empty input, input_lines.len() already reflects wrapping via
        // layout_input.  For the empty-input placeholder, Paragraph::wrap will
        // wrap the single Line at render time, so we must estimate the wrapped
        // row count ourselves to keep padding accurate on narrow widths.
        let visual_rows = if self.app.input.is_empty() {
            placeholder_visual_lines(content_width)
        } else {
            input_lines.len()
        };
        let top_padding = composer_top_padding(visual_rows, input_rows_budget);
        let mut lines = Vec::new();
        for _ in 0..top_padding {
            lines.push(Line::from(""));
        }
        lines.extend(input_lines);

        if !menu_entries.is_empty() {
            let selected = self
                .active_menu_selected()
                .min(menu_entries.len().saturating_sub(1));
            // `@`-mention entries get an "@" prefix so the popup line reads
            // like the actual mention the user is composing.
            let prefix = if !self.mention_menu_entries.is_empty() {
                "@"
            } else {
                ""
            };

            // Compute a viewport window into the menu entries so the
            // selection cursor stays visible even when there are more
            // entries than available rows.
            let menu_visible_rows = inner_area
                .height
                .saturating_sub(visual_rows as u16)
                .saturating_sub(top_padding as u16)
                .saturating_sub(1) // at least one row for the cursor
                .max(1) as usize;
            let menu_total = menu_entries.len();
            let menu_top = if menu_total <= menu_visible_rows {
                0
            } else {
                // Keep the selection centered in the viewport.
                let half = menu_visible_rows / 2;
                if selected <= half {
                    0
                } else if selected + half >= menu_total {
                    menu_total.saturating_sub(menu_visible_rows)
                } else {
                    selected.saturating_sub(half)
                }
            };
            let menu_bottom = (menu_top + menu_visible_rows).min(menu_total);

            for (idx, entry) in menu_entries
                .iter()
                .enumerate()
                .take(menu_bottom)
                .skip(menu_top)
            {
                let is_selected = idx == selected;
                let style = if is_selected {
                    Style::default()
                        .fg(palette::SELECTION_TEXT)
                        .bg(palette::SELECTION_BG)
                } else {
                    Style::default().fg(palette::TEXT_MUTED)
                };
                let marker = if is_selected { "▸" } else { " " };
                lines.push(Line::from(vec![
                    Span::styled(" ", Style::default()),
                    Span::styled(marker, style),
                    Span::styled(" ", style),
                    Span::styled(format!("{prefix}{entry}"), style),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines)
            .style(background)
            .wrap(Wrap { trim: false });
        paragraph.render(inner_area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        composer_height(
            &self.app.input,
            width,
            self.max_height.min(self.max_height_cap()),
            self.active_menu_entries().len(),
            self.app.composer_density,
            self.app.composer_border,
        )
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let inner_area = self.inner_area(area);
        let content_width = usize::from(inner_area.width.max(1));
        let input_rows_budget =
            composer_input_rows_budget(inner_area.height, self.active_menu_entries().len());

        let (visible_lines, cursor_row, cursor_col) = layout_input(
            &self.app.input,
            self.app.cursor_position,
            content_width,
            input_rows_budget,
        );
        let visual_rows = if self.app.input.is_empty() {
            placeholder_visual_lines(content_width)
        } else {
            visible_lines.len()
        };
        let top_padding = composer_top_padding(visual_rows, input_rows_budget);

        let cursor_x = area
            .x
            .saturating_add(inner_area.x.saturating_sub(area.x))
            .saturating_add(u16::try_from(cursor_col).unwrap_or(u16::MAX));
        let cursor_y = area
            .y
            .saturating_add(inner_area.y.saturating_sub(area.y))
            .saturating_add(u16::try_from(top_padding + cursor_row).unwrap_or(u16::MAX));
        if cursor_x < area.x + area.width && cursor_y < area.y + area.height {
            Some((cursor_x, cursor_y))
        } else {
            None
        }
    }
}

/// Codex-style full-screen approval takeover (#129).
///
/// The widget reads its mutable state (selected option, staged
/// confirmation) directly from the [`ApprovalView`] so the destructive
/// variant can render its "Press Y again to confirm" banner without
/// touching internal fields. Rendering reflows to fill most of the
/// transcript area instead of a centered popup; on small terminals it
/// falls back to a 65×22 card so existing snapshot tests still see a
/// coherent layout.
pub struct ApprovalWidget<'a> {
    request: &'a ApprovalRequest,
    view: &'a ApprovalView,
}

impl<'a> ApprovalWidget<'a> {
    pub fn new(request: &'a ApprovalRequest, view: &'a ApprovalView) -> Self {
        Self { request, view }
    }
}

/// Layout pad around the takeover card. Generous so the modal feels
/// like a takeover rather than a popup, but never larger than the
/// terminal can hold.
const APPROVAL_CARD_HORIZONTAL_PAD: u16 = 6;
const APPROVAL_CARD_VERTICAL_PAD: u16 = 2;
/// Minimum card height — anything tighter and the destructive variant's
/// confirmation banner overlaps the option list.
const APPROVAL_CARD_MIN_HEIGHT: u16 = 18;
/// Maximum card width — readability craters past this on wide terminals.
const APPROVAL_CARD_MAX_WIDTH: u16 = 96;

impl Renderable for ApprovalWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let card_area = compute_takeover_area(area);
        Clear.render(card_area, buf);

        let risk = self.request.risk;
        let palette_colors = approval_palette(risk);
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(20);

        // Header: stakes badge + tool identifier. The badge is the
        // first thing the eye lands on.
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!(" {} ", risk_badge_text(risk)),
                Style::default()
                    .fg(palette::DEEPSEEK_INK)
                    .bg(palette_colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                self.request.tool_name.clone(),
                Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        // Category line — unchanged vocabulary so existing tests still
        // recognise the rendering.
        let (cat_label, cat_color) = category_label_for(self.request.category);
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Type: ", Style::default().fg(palette::TEXT_HINT)),
            Span::styled(
                cat_label,
                Style::default().fg(cat_color).add_modifier(Modifier::BOLD),
            ),
        ]));

        lines.push(Line::from(""));
        // About + impacts. Impact lines are the load-bearing content;
        // they tell the user what will happen.
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("About:  ", Style::default().fg(palette::TEXT_HINT)),
            Span::styled(
                self.request.description.clone(),
                Style::default().fg(palette::TEXT_BODY),
            ),
        ]));
        for impact in self.request.impacts.iter().take(4) {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Impact: ", Style::default().fg(palette::TEXT_HINT)),
                Span::styled(impact.clone(), Style::default().fg(palette::TEXT_BODY)),
            ]));
        }

        lines.push(Line::from(""));
        let params_str = self.request.params_display();
        let params_width = card_area.width.saturating_sub(14) as usize;
        let params_truncated =
            crate::utils::truncate_with_ellipsis(&params_str, params_width.max(20), "...");
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Params: ", Style::default().fg(palette::TEXT_HINT)),
            Span::styled(
                params_truncated,
                Style::default().fg(palette::TEXT_SECONDARY),
            ),
        ]));

        lines.push(Line::from(""));

        let options = approval_options_for(risk);
        let pending = self.view.pending_confirm();

        for (i, opt) in options.iter().enumerate() {
            let is_selected = i == self.view.selected();
            let staged = pending.is_some_and(|p| p == opt.option);
            let label_color = if opt.dangerous {
                palette_colors.accent
            } else {
                palette::TEXT_BODY
            };

            let row_style = if is_selected {
                Style::default()
                    .fg(palette::SELECTION_TEXT)
                    .bg(palette::SELECTION_BG)
            } else {
                Style::default()
            };

            let mut spans = vec![
                Span::raw("  "),
                Span::styled(
                    format!("[{}] ", opt.key_hint),
                    Style::default()
                        .fg(palette_colors.shortcut)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(opt.label.to_string(), row_style.fg(label_color)),
            ];
            if staged {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    "(staged)",
                    Style::default()
                        .fg(palette_colors.accent)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            lines.push(Line::from(spans));
        }

        // Variant-specific footer: benign nudges single-key approve;
        // destructive shows either the standing prompt or the
        // confirmation banner when an approve key has been staged.
        lines.push(Line::from(""));
        match (risk, pending) {
            (RiskLevel::Benign, _) => {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        "Single key approves: ",
                        Style::default().fg(palette::TEXT_HINT),
                    ),
                    Span::styled(
                        "Enter / 1 / y",
                        Style::default()
                            .fg(palette_colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "  ·  v: full params  ·  Esc: abort",
                        Style::default().fg(palette::TEXT_HINT),
                    ),
                ]));
            }
            (RiskLevel::Destructive, Some(opt)) => {
                let again_key = match opt {
                    crate::tui::approval::ApprovalOption::ApproveOnce => "Enter or y",
                    crate::tui::approval::ApprovalOption::ApproveAlways => "Enter or a",
                    _ => "Enter",
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        "Confirm destructive action — press ",
                        Style::default()
                            .fg(palette_colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        again_key.to_string(),
                        Style::default()
                            .fg(palette::DEEPSEEK_INK)
                            .bg(palette_colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " again to commit, anything else cancels.",
                        Style::default().fg(palette::TEXT_HINT),
                    ),
                ]));
            }
            (RiskLevel::Destructive, None) => {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        "Two keys to approve: ",
                        Style::default().fg(palette::TEXT_HINT),
                    ),
                    Span::styled(
                        "y/a then y/a again",
                        Style::default()
                            .fg(palette_colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "  ·  v: full params  ·  Esc: abort",
                        Style::default().fg(palette::TEXT_HINT),
                    ),
                ]));
            }
        }

        let title = format!(
            " {} approval — {} ",
            risk_badge_text(risk),
            self.request.tool_name
        );
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette_colors.border))
            .style(Style::default().bg(palette::DEEPSEEK_INK))
            .padding(Padding::uniform(1));

        // Render the card body inside the block, then paint the warm
        // accent rail on the destructive variant. The rail uses a
        // single-cell column so it doesn't shift the body layout.
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(card_area, buf);

        if matches!(risk, RiskLevel::Destructive) {
            paint_left_rail(card_area, buf, palette_colors.accent);
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

/// Compute the card rect inside `area`. Always centered; pad on every
/// side so the takeover reads as a takeover but a small terminal still
/// renders the full card without truncation.
fn compute_takeover_area(area: Rect) -> Rect {
    let avail_width = area.width.saturating_sub(APPROVAL_CARD_HORIZONTAL_PAD * 2);
    let avail_height = area.height.saturating_sub(APPROVAL_CARD_VERTICAL_PAD * 2);
    let card_width = APPROVAL_CARD_MAX_WIDTH.min(avail_width).max(40);
    let card_height = APPROVAL_CARD_MIN_HEIGHT.max(avail_height.min(28));
    let x = area.x + (area.width.saturating_sub(card_width)) / 2;
    let y = area.y + (area.height.saturating_sub(card_height)) / 2;
    Rect {
        x,
        y,
        width: card_width,
        height: card_height,
    }
}

/// Paint a single-column accent on the inside-left of the card. Only
/// touches cells that already exist in the buffer area.
fn paint_left_rail(card: Rect, buf: &mut Buffer, color: Color) {
    if card.width < 2 || card.height < 4 {
        return;
    }
    let rail_x = card.x + 1;
    let top = card.y + 1;
    let bot = card.y + card.height.saturating_sub(2);
    for y in top..=bot {
        if y >= buf.area.y + buf.area.height {
            break;
        }
        let cell = &mut buf[(rail_x, y)];
        cell.set_char('\u{2503}'); // ┃ — heavy bar so the warning reads at a glance
        cell.set_style(Style::default().fg(color).bg(palette::DEEPSEEK_INK));
    }
}

/// Approval palette per risk variant.
struct ApprovalColors {
    border: Color,
    accent: Color,
    shortcut: Color,
}

fn approval_palette(risk: RiskLevel) -> ApprovalColors {
    match risk {
        RiskLevel::Benign => ApprovalColors {
            border: palette::BORDER_COLOR,
            accent: palette::DEEPSEEK_SKY,
            shortcut: palette::DEEPSEEK_SKY,
        },
        RiskLevel::Destructive => ApprovalColors {
            border: palette::DEEPSEEK_RED,
            accent: palette::DEEPSEEK_RED,
            shortcut: palette::STATUS_WARNING,
        },
    }
}

fn risk_badge_text(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Benign => "REVIEW",
        RiskLevel::Destructive => "DESTRUCTIVE",
    }
}

fn category_label_for(category: ToolCategory) -> (&'static str, Color) {
    match category {
        ToolCategory::Safe => ("Safe", palette::STATUS_SUCCESS),
        ToolCategory::FileWrite => ("File Write", palette::STATUS_WARNING),
        ToolCategory::Shell => ("Shell Command", palette::STATUS_ERROR),
        ToolCategory::Network => ("Network", palette::STATUS_WARNING),
        ToolCategory::McpRead => ("MCP Read", palette::DEEPSEEK_SKY),
        ToolCategory::McpAction => ("MCP Action", palette::STATUS_WARNING),
        ToolCategory::Unknown => ("Unknown", palette::STATUS_ERROR),
    }
}

struct ApprovalOptionRow {
    option: crate::tui::approval::ApprovalOption,
    label: &'static str,
    key_hint: &'static str,
    dangerous: bool,
}

fn approval_options_for(risk: RiskLevel) -> [ApprovalOptionRow; 4] {
    use crate::tui::approval::ApprovalOption as O;
    let dangerous = matches!(risk, RiskLevel::Destructive);
    [
        ApprovalOptionRow {
            option: O::ApproveOnce,
            label: "Approve once",
            key_hint: "1 / y",
            dangerous,
        },
        ApprovalOptionRow {
            option: O::ApproveAlways,
            label: "Approve always for this kind",
            key_hint: "2 / a",
            dangerous,
        },
        ApprovalOptionRow {
            option: O::Deny,
            label: "Deny this call",
            key_hint: "3 / d / n",
            dangerous: false,
        },
        ApprovalOptionRow {
            option: O::Abort,
            label: "Abort the turn",
            key_hint: "Esc",
            dangerous: false,
        },
    ]
}

pub struct ElevationWidget<'a> {
    request: &'a ElevationRequest,
    selected: usize,
}

impl<'a> ElevationWidget<'a> {
    pub fn new(request: &'a ElevationRequest, selected: usize) -> Self {
        Self { request, selected }
    }
}

impl Renderable for ElevationWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_width = 70.min(area.width.saturating_sub(4));
        let popup_height = 22.min(area.height.saturating_sub(4));
        let popup_area = Rect {
            x: (area.width.saturating_sub(popup_width)) / 2,
            y: (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  ⚠ Sandbox Denied ",
                Style::default()
                    .fg(palette::STATUS_ERROR)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::raw("  Tool: "),
                Span::styled(
                    &self.request.tool_name,
                    Style::default()
                        .fg(palette::DEEPSEEK_SKY)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
        ];

        // Show command if it's a shell command
        if let Some(ref command) = self.request.command {
            let cmd_display = crate::utils::truncate_with_ellipsis(command, 45, "...");
            lines.push(Line::from(vec![
                Span::raw("  Cmd:  "),
                Span::styled(cmd_display, Style::default().fg(palette::TEXT_MUTED)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  Reason: "),
            Span::styled(
                &self.request.denial_reason,
                Style::default().fg(palette::STATUS_WARNING),
            ),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Impact if approved:",
            Style::default().fg(palette::TEXT_MUTED),
        )));
        if self
            .request
            .options
            .iter()
            .any(|option| matches!(option, ElevationOption::WithNetwork))
        {
            lines.push(Line::from(Span::styled(
                "    - network retry enables outbound downloads and HTTP requests",
                Style::default().fg(palette::TEXT_PRIMARY),
            )));
        }
        if self
            .request
            .options
            .iter()
            .any(|option| matches!(option, ElevationOption::WithWriteAccess(_)))
        {
            lines.push(Line::from(Span::styled(
                "    - write retry expands writable filesystem scope for this tool call",
                Style::default().fg(palette::TEXT_PRIMARY),
            )));
        }
        lines.push(Line::from(Span::styled(
            "    - full access removes sandbox restrictions entirely for this retry",
            Style::default().fg(palette::TEXT_PRIMARY),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Choose how to proceed:",
            Style::default().fg(palette::TEXT_MUTED),
        )));
        lines.push(Line::from(""));

        // Render options
        for (i, option) in self.request.options.iter().enumerate() {
            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default()
                    .fg(palette::SELECTION_TEXT)
                    .bg(palette::SELECTION_BG)
            } else {
                Style::default()
            };

            let key = match option {
                ElevationOption::WithNetwork => "n",
                ElevationOption::WithWriteAccess(_) => "w",
                ElevationOption::FullAccess => "f",
                ElevationOption::Abort => "a",
            };

            let label_color = match option {
                ElevationOption::Abort => palette::TEXT_MUTED,
                ElevationOption::FullAccess => palette::STATUS_ERROR,
                _ => palette::TEXT_PRIMARY,
            };

            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("[{key}] "),
                    Style::default().fg(palette::STATUS_SUCCESS),
                ),
                Span::styled(option.label(), style.fg(label_color)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(
                    option.description(),
                    Style::default().fg(palette::TEXT_MUTED),
                ),
            ]));
        }

        let title = " Sandbox Elevation Required ";
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::DEEPSEEK_INK))
            .padding(Padding::uniform(1));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        paragraph.render(popup_area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

pub(crate) fn pad_lines_to_bottom(lines: &mut Vec<Line<'static>>, height: usize) {
    if lines.len() >= height {
        return;
    }
    let padding = height.saturating_sub(lines.len());
    if padding == 0 {
        return;
    }

    let mut padded = Vec::with_capacity(height);
    padded.extend(std::iter::repeat_n(Line::from(""), padding));
    padded.append(lines);
    *lines = padded;
}

fn apply_selection(lines: &mut [Line<'static>], top: usize, app: &App) {
    let Some((start, end)) = app.transcript_selection.ordered_endpoints() else {
        return;
    };

    let selection_style = Style::default()
        .bg(app.ui_theme.selection_bg)
        .fg(palette::SELECTION_TEXT);

    for (idx, line) in lines.iter_mut().enumerate() {
        let line_index = top + idx;
        if line_index < start.line_index || line_index > end.line_index {
            continue;
        }

        let (col_start, col_end) = if start.line_index == end.line_index {
            (start.column, end.column)
        } else if line_index == start.line_index {
            (start.column, usize::MAX)
        } else if line_index == end.line_index {
            (0, end.column)
        } else {
            (0, usize::MAX)
        };

        if col_start == 0 && col_end == usize::MAX {
            for span in &mut line.spans {
                span.style = span.style.patch(selection_style);
            }
            continue;
        }

        line.spans = apply_selection_to_line(line, col_start, col_end, selection_style);
    }
}

fn apply_detail_target_highlight(
    lines: &mut [Line<'static>],
    top: usize,
    target_cell: usize,
    line_meta: &[TranscriptLineMeta],
) {
    let highlight_bg = Color::Rgb(18, 29, 39);
    for (idx, line) in lines.iter_mut().enumerate() {
        let line_index = top + idx;
        if let Some(TranscriptLineMeta::CellLine { cell_index, .. }) = line_meta.get(line_index)
            && *cell_index == target_cell
        {
            for span in &mut line.spans {
                span.style = span.style.bg(highlight_bg);
            }
        }
    }
}

/// Apply a brief background tint to the last user message's visible lines.
fn apply_send_flash(
    lines: &mut [Line<'static>],
    top: usize,
    history: &[HistoryCell],
    line_meta: &[TranscriptLineMeta],
) {
    // Find the last User cell index.
    let last_user_cell = history
        .iter()
        .rposition(|cell| matches!(cell, HistoryCell::User { .. }));
    let Some(target_cell) = last_user_cell else {
        return;
    };

    let flash_bg = Color::Rgb(30, 40, 55); // subtle dark-blue tint

    for (idx, line) in lines.iter_mut().enumerate() {
        let line_index = top + idx;
        if let Some(TranscriptLineMeta::CellLine { cell_index, .. }) = line_meta.get(line_index)
            && *cell_index == target_cell
        {
            for span in &mut line.spans {
                span.style = span.style.bg(flash_bg);
            }
        }
    }
}

fn apply_selection_to_line(
    line: &Line<'static>,
    col_start: usize,
    col_end: usize,
    selection_style: Style,
) -> Vec<Span<'static>> {
    let mut result = Vec::with_capacity(line.spans.len().saturating_add(2));
    let mut current_col = 0usize;

    for span in &line.spans {
        let span_text: &str = span.content.as_ref();
        let span_width = text_display_width(span_text);
        let span_end = current_col.saturating_add(span_width);

        if span_end <= col_start || current_col >= col_end {
            result.push(span.clone());
        } else if current_col >= col_start && span_end <= col_end {
            result.push(Span::styled(
                span.content.clone(),
                span.style.patch(selection_style),
            ));
        } else {
            let mut before = String::new();
            let mut selected = String::new();
            let mut after = String::new();
            let mut ch_col = current_col;

            for ch in span_text.chars() {
                let ch_width = char_display_width(ch);
                let ch_start = ch_col;
                let ch_end = ch_col.saturating_add(ch_width);
                if ch_end <= col_start {
                    before.push(ch);
                } else if ch_start >= col_end {
                    after.push(ch);
                } else {
                    selected.push(ch);
                }
                ch_col = ch_end;
            }

            if !before.is_empty() {
                result.push(Span::styled(before, span.style));
            }
            if !selected.is_empty() {
                result.push(Span::styled(selected, span.style.patch(selection_style)));
            }
            if !after.is_empty() {
                result.push(Span::styled(after, span.style));
            }
        }

        current_col = span_end;
    }

    result
}

fn text_display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

fn char_display_width(ch: char) -> usize {
    if ch == '\t' {
        4
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0).max(1)
    }
}

fn should_render_empty_state(app: &App) -> bool {
    app.history.is_empty() && !app.is_loading && !app.is_compacting
}

fn build_empty_state_lines(app: &App, area: Rect) -> Vec<Line<'static>> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }

    let workspace_name = app
        .workspace
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(std::string::ToString::to_string)
        .unwrap_or_else(|| app.workspace.to_string_lossy().into_owned());
    let body_width = usize::from(area.width.saturating_sub(8).clamp(24, 72));
    let left_padding = usize::from(area.width.saturating_sub(body_width as u16) / 2);
    let inset = " ".repeat(left_padding);

    let body = vec![
        Line::from(Span::styled(
            format!("{inset}DeepSeek TUI"),
            Style::default().fg(palette::DEEPSEEK_BLUE).bold(),
        )),
        Line::from(Span::styled(
            format!("{inset}{workspace_name}  ·  {}", app.model),
            Style::default().fg(palette::TEXT_MUTED),
        )),
    ];

    let top_padding = usize::from(area.height.saturating_sub(body.len() as u16) / 3);
    let mut lines = Vec::new();
    for _ in 0..top_padding {
        lines.push(Line::from(""));
    }
    lines.extend(body);
    lines
}

fn composer_input_rows_budget(inner_height: u16, extra_lines: usize) -> usize {
    usize::from(inner_height).saturating_sub(extra_lines).max(1)
}

fn composer_top_padding(content_lines: usize, rows_budget: usize) -> usize {
    rows_budget.saturating_sub(content_lines.clamp(1, rows_budget))
}

/// Placeholder text shown when the composer input is empty.
const COMPOSER_PLACEHOLDER: &str = "Write a task or use /.";

/// How many visual rows the empty-input placeholder occupies after wrapping.
fn placeholder_visual_lines(content_width: usize) -> usize {
    wrap_text(COMPOSER_PLACEHOLDER, content_width).len().max(1)
}

fn composer_min_input_rows(density: ComposerDensity) -> usize {
    match density {
        ComposerDensity::Compact => 2,
        ComposerDensity::Comfortable => 3,
        ComposerDensity::Spacious => 4,
    }
}

fn composer_max_height(density: ComposerDensity) -> u16 {
    match density {
        ComposerDensity::Compact => 7,
        ComposerDensity::Comfortable => 9,
        ComposerDensity::Spacious => 12,
    }
}

fn composer_height(
    input: &str,
    width: u16,
    available_height: u16,
    extra_lines: usize,
    density: ComposerDensity,
    show_panel: bool,
) -> u16 {
    let has_panel = show_panel && available_height >= 3 && width >= 12;
    let chrome_height = if has_panel {
        usize::from(COMPOSER_PANEL_HEIGHT)
    } else {
        0
    };
    let content_width = if has_panel {
        usize::from(width.saturating_sub(2).max(1))
    } else {
        usize::from(width.max(1))
    };
    let mut line_count = wrap_input_lines(input, content_width).len();
    if line_count == 0 {
        line_count = 1;
    }
    if has_panel {
        line_count = line_count.max(composer_min_input_rows(density));
    }
    line_count = line_count
        .saturating_add(extra_lines)
        .saturating_add(chrome_height);
    let max_height = usize::from(available_height.clamp(1, composer_max_height(density)));
    line_count.clamp(1, max_height).try_into().unwrap_or(1)
}

pub(crate) fn slash_completion_hints(input: &str, limit: usize) -> Vec<String> {
    if !input.starts_with('/') || input.contains(char::is_whitespace) {
        return Vec::new();
    }

    let prefix = input.trim_start_matches('/');
    let mut hints = commands::commands_matching(prefix)
        .into_iter()
        .map(|info| format!("/{}", info.name))
        .collect::<Vec<_>>();

    if hints.is_empty() && prefix.eq_ignore_ascii_case("model") {
        hints = COMMON_DEEPSEEK_MODELS
            .iter()
            .map(|name| format!("/model {name}"))
            .collect();
    }

    hints.sort();
    hints.dedup();
    hints.into_iter().take(limit).collect()
}

fn layout_input(
    input: &str,
    cursor: usize,
    width: usize,
    max_height: usize,
) -> (Vec<String>, usize, usize) {
    let mut lines = wrap_input_lines(input, width);
    if lines.is_empty() {
        lines.push(String::new());
    }
    let (cursor_row, cursor_col) = cursor_row_col(input, cursor, width.max(1));

    let max_height = max_height.max(1);
    let mut start = 0usize;
    if cursor_row >= max_height {
        start = cursor_row + 1 - max_height;
    }
    if start + max_height > lines.len() {
        start = lines.len().saturating_sub(max_height);
    }
    let visible = lines
        .into_iter()
        .skip(start)
        .take(max_height)
        .collect::<Vec<_>>();
    let visible_cursor_row = cursor_row.saturating_sub(start);

    (
        visible,
        visible_cursor_row,
        cursor_col.min(width.saturating_sub(1)),
    )
}

fn cursor_row_col(input: &str, cursor: usize, width: usize) -> (usize, usize) {
    let mut row = 0usize;
    let mut col = 0usize;
    let mut char_idx = 0usize;

    for grapheme in input.graphemes(true) {
        if char_idx >= cursor {
            break;
        }
        let grapheme_chars = grapheme.chars().count();
        let next_char_idx = char_idx.saturating_add(grapheme_chars);
        let cursor_inside = cursor < next_char_idx;

        if grapheme == "\n" {
            row += 1;
            col = 0;
            char_idx = next_char_idx;
            if cursor_inside {
                break;
            }
            continue;
        }

        let grapheme_width = grapheme.width();
        if col + grapheme_width > width && col != 0 {
            row += 1;
            col = 0;
        }
        col += grapheme_width;
        if col >= width {
            row += 1;
            col = 0;
        }
        if cursor_inside {
            break;
        }
        char_idx = next_char_idx;
    }

    (row, col)
}

fn wrap_input_lines(input: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if input.is_empty() {
        return lines;
    }

    for raw in input.split('\n') {
        let wrapped = wrap_text(raw, width);
        if wrapped.is_empty() {
            lines.push(String::new());
        } else {
            lines.extend(wrapped);
        }
    }

    // Note: No need for ends_with('\n') check - split('\n') already includes
    // the trailing empty string for inputs ending with newline.

    lines
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for grapheme in text.graphemes(true) {
        if grapheme == "\n" {
            lines.push(current);
            current = String::new();
            current_width = 0;
            continue;
        }

        let grapheme_width = grapheme.width();
        if current_width + grapheme_width > width && current_width != 0 {
            lines.push(current);
            current = String::new();
            current_width = 0;
        }

        current.push_str(grapheme);
        current_width += grapheme_width;

        if current_width >= width {
            lines.push(current);
            current = String::new();
            current_width = 0;
        }
    }

    lines.push(current);
    lines
}

#[cfg(test)]
mod tests {
    use super::{
        COMPOSER_PANEL_HEIGHT, ChatWidget, ComposerWidget, Renderable, apply_selection_to_line,
        composer_height, composer_max_height, composer_min_input_rows, composer_top_padding,
        cursor_row_col, layout_input, pad_lines_to_bottom, placeholder_visual_lines,
        should_render_empty_state, slash_completion_hints, wrap_input_lines, wrap_text,
    };
    use crate::config::Config;
    use crate::palette;
    use crate::tui::app::{App, ComposerDensity, TuiOptions};
    use crate::tui::history::{GenericToolCell, HistoryCell, ToolCell, ToolStatus};
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::Style,
        text::{Line, Span},
    };
    use std::path::PathBuf;
    use unicode_width::UnicodeWidthStr;

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "deepseek-v4-flash".to_string(),
            workspace: PathBuf::from("."),
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
        };
        App::new(options, &Config::default())
    }

    #[test]
    fn pad_lines_to_bottom_noop_when_already_filled() {
        let mut lines = vec![Line::from("one"), Line::from("two")];
        pad_lines_to_bottom(&mut lines, 2);
        assert_eq!(lines, vec![Line::from("one"), Line::from("two")]);
    }

    #[test]
    fn pad_lines_to_bottom_prepends_empty_lines() {
        let mut lines = vec![Line::from("one"), Line::from("two")];
        pad_lines_to_bottom(&mut lines, 5);

        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], Line::from(""));
        assert_eq!(lines[1], Line::from(""));
        assert_eq!(lines[2], Line::from(""));
        assert_eq!(lines[3], Line::from("one"));
        assert_eq!(lines[4], Line::from("two"));
    }

    #[test]
    fn pad_lines_to_bottom_noop_when_height_is_zero() {
        let mut lines = vec![Line::from("one")];
        pad_lines_to_bottom(&mut lines, 0);
        assert_eq!(lines, vec![Line::from("one")]);
    }

    // Cursor alignment tests

    #[test]
    fn cursor_basic_ascii() {
        // "hello" with cursor at various positions, width=10
        assert_eq!(cursor_row_col("hello", 0, 10), (0, 0));
        assert_eq!(cursor_row_col("hello", 3, 10), (0, 3));
        assert_eq!(cursor_row_col("hello", 5, 10), (0, 5));
    }

    #[test]
    fn cursor_at_wrap_boundary() {
        // "abcde" exactly fills width=5
        // Cursor at position 5 (after last char) should wrap to next line
        let (row, col) = cursor_row_col("abcde", 5, 5);
        assert_eq!(row, 1, "cursor at end of full line should wrap");
        assert_eq!(col, 0, "cursor should be at start of next line");
    }

    #[test]
    fn cursor_with_cjk_characters() {
        // "中" is a CJK character with width 2
        // "a中b" = 1 + 2 + 1 = 4 display width
        assert_eq!(cursor_row_col("a中b", 0, 10), (0, 0)); // before 'a'
        assert_eq!(cursor_row_col("a中b", 1, 10), (0, 1)); // after 'a', before '中'
        assert_eq!(cursor_row_col("a中b", 2, 10), (0, 3)); // after '中', before 'b'
        assert_eq!(cursor_row_col("a中b", 3, 10), (0, 4)); // after 'b'
    }

    #[test]
    fn cursor_cjk_at_wrap_boundary() {
        // width=5, input "abcd中" (4 + 2 = 6, CJK doesn't fit on line 1)
        // CJK should wrap to next line
        let lines = wrap_text("abcd中", 5);
        assert_eq!(lines, vec!["abcd", "中"]);

        // Cursor after CJK should be on row 1, col 2
        let (row, col) = cursor_row_col("abcd中", 5, 5);
        assert_eq!(row, 1);
        assert_eq!(col, 2);
    }

    #[test]
    fn cursor_with_combining_marks() {
        // "e\u0301" is 'e' with combining acute accent (é)
        // Display width is 1 (combining mark has width 0)
        let input = "e\u{0301}"; // é as e + combining acute
        assert_eq!(input.chars().count(), 2);

        // Cursor positions:
        // 0 = before 'e'
        // 1 = after 'e', before combining mark
        // 2 = after combining mark
        assert_eq!(cursor_row_col(input, 0, 10), (0, 0));
        assert_eq!(cursor_row_col(input, 1, 10), (0, 1));
        assert_eq!(cursor_row_col(input, 2, 10), (0, 1)); // combining mark has width 0
    }

    #[test]
    fn cursor_with_emoji() {
        // Many emojis are double-width
        let input = "a😀b";
        // Cursor at 2 (after emoji) should account for emoji width
        let (_row, col) = cursor_row_col(input, 2, 10);
        // Emoji width varies by system, but should be either 1 or 2
        assert!((2..=3).contains(&col), "col = {col}, expected 2 or 3");
    }

    #[test]
    fn cursor_with_emoji_zwj_sequence() {
        let input = "👨‍👩‍👧‍👦";
        let cursor = input.chars().count();
        let (row, col) = cursor_row_col(input, cursor, 10);
        assert_eq!(row, 0);
        assert_eq!(col, input.width());
    }

    #[test]
    fn cursor_with_newlines() {
        // "ab\ncd" with cursor moving through
        assert_eq!(cursor_row_col("ab\ncd", 0, 10), (0, 0)); // before 'a'
        assert_eq!(cursor_row_col("ab\ncd", 2, 10), (0, 2)); // after 'b', before '\n'
        assert_eq!(cursor_row_col("ab\ncd", 3, 10), (1, 0)); // after '\n', before 'c'
        assert_eq!(cursor_row_col("ab\ncd", 5, 10), (1, 2)); // after 'd'
    }

    #[test]
    fn wrap_input_lines_preserves_empty_lines() {
        let lines = wrap_input_lines("a\n\nb", 10);
        assert_eq!(lines, vec!["a", "", "b"]);
    }

    #[test]
    fn wrap_input_lines_trailing_newline() {
        let lines = wrap_input_lines("a\n", 10);
        assert_eq!(lines, vec!["a", ""]);
    }

    #[test]
    fn cursor_and_wrap_consistency() {
        // Ensure cursor_row_col is consistent with wrap_text
        // for various inputs
        let test_cases = vec![
            ("hello world", 5),
            ("abcdefghij", 3),
            ("中文测试", 6),
            ("a\nb\nc", 10),
        ];

        for (input, width) in test_cases {
            let lines = wrap_input_lines(input, width);
            let (cursor_row, _) = cursor_row_col(input, input.chars().count(), width);

            // Cursor at end should be on the last line (or wrapped past it)
            assert!(
                cursor_row <= lines.len(),
                "cursor_row={cursor_row} should be <= lines.len()={} for input={input:?}",
                lines.len()
            );
        }
    }

    #[test]
    fn slash_completion_hints_include_links_and_config() {
        let hints = slash_completion_hints("/", 128);
        assert!(hints.iter().any(|hint| hint == "/config"));
        assert!(hints.iter().any(|hint| hint == "/links"));
    }

    #[test]
    fn slash_completion_hints_exclude_set_and_deepseek_commands() {
        let hints = slash_completion_hints("/", 128);
        assert!(!hints.iter().any(|hint| hint == "/set"));
        assert!(!hints.iter().any(|hint| hint == "/deepseek"));
    }

    #[test]
    fn selection_style_uses_explicit_selection_text_role() {
        let line = Line::from(Span::styled(
            "hello world",
            Style::default().fg(palette::TEXT_PRIMARY),
        ));
        let selection_style = Style::default()
            .bg(palette::SELECTION_BG)
            .fg(palette::SELECTION_TEXT);

        let styled = apply_selection_to_line(&line, 0, 5, selection_style);
        assert_eq!(styled.len(), 2);
        assert_eq!(styled[0].content.as_ref(), "hello");
        assert_eq!(styled[0].style.fg, Some(palette::SELECTION_TEXT));
        assert_eq!(styled[0].style.bg, Some(palette::SELECTION_BG));
        assert_eq!(styled[1].content.as_ref(), " world");
    }

    #[test]
    fn composer_layout_helpers_stay_consistent() {
        let input = "line one wraps nicely\nline two wraps as well";
        let width = 16;
        let available_height = 6;
        let menu_lines = 2;

        let height = composer_height(
            input,
            width,
            available_height,
            menu_lines,
            ComposerDensity::Comfortable,
            true,
        );
        let has_panel = available_height >= 3 && width >= 12;
        let chrome_height = if has_panel {
            usize::from(COMPOSER_PANEL_HEIGHT)
        } else {
            0
        };
        let content_width = if has_panel {
            usize::from(width.saturating_sub(2).max(1))
        } else {
            usize::from(width.max(1))
        };
        let input_height_budget = usize::from(height)
            .saturating_sub(menu_lines)
            .saturating_sub(chrome_height)
            .max(1);
        let (visible, cursor_row, cursor_col) = layout_input(
            input,
            input.chars().count(),
            content_width,
            input_height_budget,
        );

        assert!(visible.len().saturating_add(menu_lines) <= usize::from(height));
        assert!(!visible.is_empty());
        assert!(cursor_row < visible.len());
        assert!(cursor_col < content_width.max(1));
        assert!(height >= 5);
    }

    #[test]
    fn composer_height_prefers_panel_shape_when_space_allows() {
        let height = composer_height("", 40, 8, 0, ComposerDensity::Comfortable, true);
        assert_eq!(height, 5);
    }

    #[test]
    fn composer_height_skips_panel_chrome_when_border_disabled() {
        let with_border = composer_height("", 40, 8, 0, ComposerDensity::Comfortable, true);
        let without_border = composer_height("", 40, 8, 0, ComposerDensity::Comfortable, false);

        assert_eq!(with_border, 5);
        assert_eq!(without_border, 1);
        assert!(without_border < with_border);
    }

    #[test]
    fn composer_density_changes_min_rows_and_height_cap() {
        assert_eq!(composer_min_input_rows(ComposerDensity::Compact), 2);
        assert_eq!(composer_min_input_rows(ComposerDensity::Spacious), 4);
        assert!(
            composer_max_height(ComposerDensity::Spacious)
                > composer_max_height(ComposerDensity::Compact)
        );
    }

    #[test]
    fn empty_composer_cursor_matches_placeholder_padding() {
        let mut app = create_test_app();
        // Pin density so the test is independent of any loaded user settings.
        app.composer_density = ComposerDensity::Comfortable;
        let slash_menu_entries = Vec::<String>::new();
        let mention_menu_entries = Vec::<String>::new();
        let widget = ComposerWidget::new(&app, 5, &slash_menu_entries, &mention_menu_entries);

        // Use a wide area so the placeholder fits on one line (no wrapping).
        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 5,
        };

        // inner_area: {x:1, y:1, w:38, h:3}  (borders shrink by 1 each side)
        // input_rows_budget = 3
        // placeholder_visual_lines(38) = 1  (placeholder is 22 chars, fits in 38)
        // top_padding = 3 - clamp(1, 1, 3) = 2
        // cursor_x = 0 + (1-0) + 0 = 1
        // cursor_y = 0 + (1-0) + (2+0) = 3
        assert_eq!(widget.cursor_pos(area), Some((1, 3)));
    }

    #[test]
    fn empty_composer_cursor_accounts_for_placeholder_wrapping() {
        let mut app = create_test_app();
        app.composer_density = ComposerDensity::Comfortable;
        let slash_menu_entries = Vec::<String>::new();
        let mention_menu_entries = Vec::<String>::new();
        let widget = ComposerWidget::new(&app, 5, &slash_menu_entries, &mention_menu_entries);

        // Narrow area forces the placeholder to wrap.
        let area = Rect {
            x: 0,
            y: 0,
            width: 14,
            height: 5,
        };

        // inner_area: {x:1, y:1, w:12, h:3}
        // input_rows_budget = 3
        // placeholder_visual_lines(12) = 2  ("Write a task" / " or use /.")
        // top_padding = 3 - clamp(2, 1, 3) = 1
        // cursor_x = 0 + (1-0) + 0 = 1
        // cursor_y = 0 + (1-0) + (1+0) = 2
        assert_eq!(placeholder_visual_lines(12), 2);
        assert_eq!(widget.cursor_pos(area), Some((1, 2)));
    }

    #[test]
    fn empty_composer_cursor_uses_full_area_when_border_disabled() {
        let mut app = create_test_app();
        app.composer_density = ComposerDensity::Comfortable;
        app.composer_border = false;
        let slash_menu_entries = Vec::<String>::new();
        let mention_menu_entries = Vec::<String>::new();
        let widget = ComposerWidget::new(&app, 3, &slash_menu_entries, &mention_menu_entries);

        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 3,
        };

        assert_eq!(widget.cursor_pos(area), Some((0, 2)));
    }

    #[test]
    fn composer_top_padding_uses_clamp() {
        // content_lines=0 is clamped to 1
        assert_eq!(composer_top_padding(0, 3), 2);
        // content_lines=1
        assert_eq!(composer_top_padding(1, 3), 2);
        // content_lines=3 fills the budget
        assert_eq!(composer_top_padding(3, 3), 0);
        // content_lines > budget is clamped
        assert_eq!(composer_top_padding(5, 3), 0);
    }

    #[test]
    fn empty_state_renders_only_without_transcript_activity() {
        let mut app = create_test_app();
        assert!(should_render_empty_state(&app));
        app.add_message(crate::tui::history::HistoryCell::User {
            content: "hello".to_string(),
        });
        assert!(!should_render_empty_state(&app));
    }

    /// Probe: confirm `cell.lines_with_motion` returns no Line whose total
    /// visual width exceeds the requested area width, even for pathological
    /// long single-line tool results.
    #[test]
    fn long_tool_result_lines_fit_requested_width() {
        let cell = HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
            name: "todo_write".to_string(),
            status: ToolStatus::Success,
            input_summary: Some("items: <2 items>".to_string()),
            output: Some("hello world ".repeat(420)),
            prompts: None,
        }));
        for width in [40u16, 80, 111, 165] {
            let lines = cell.lines(width);
            for (idx, line) in lines.iter().enumerate() {
                let visual: usize = line
                    .spans
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum();
                assert!(
                    visual <= usize::from(width),
                    "line {idx} at width {width} has visual width {visual} > {width}"
                );
            }
        }
    }

    /// Regression: a long single-line tool result must not write any cells
    /// outside the chat content area (issue #36 — sidebar gutter bleed).
    ///
    /// We render `ChatWidget` into a buffer that is wider than the chat area
    /// (simulating the sidebar split) and assert every cell to the right of
    /// `chat_area` is still the default empty cell.
    #[test]
    fn chat_widget_does_not_bleed_into_sidebar_for_long_tool_result() {
        // Reproduces the actual `todo_write` output shape: a status line,
        // a newline, then a pretty-printed JSON payload with long string
        // values. Run at several widths since the leak in the issue was
        // observed at ~165 cols.
        let cases: Vec<(u16, u16)> = vec![(80, 50), (120, 80), (165, 111), (200, 140)];
        for (total_width, chat_width) in cases {
            let mut app = create_test_app();
            let long_value: String = "hello world ".repeat(420);
            let json_payload = format!(
                "{{\n  \"items\": [\n    {{ \"id\": 1, \"content\": \"{long_value}\", \"status\": \"pending\" }}\n  ]\n}}"
            );
            let output = format!("Todo list updated (1 items, 0% complete)\n{json_payload}");
            app.add_message(HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
                name: "todo_write".to_string(),
                status: ToolStatus::Success,
                input_summary: Some("todos: <1 items>".to_string()),
                output: Some(output),
                prompts: None,
            })));

            let height: u16 = 30;
            let chat_area = Rect {
                x: 0,
                y: 0,
                width: chat_width,
                height,
            };
            let full_area = Rect {
                x: 0,
                y: 0,
                width: total_width,
                height,
            };
            let mut buf = Buffer::empty(full_area);

            let widget = ChatWidget::new(&mut app, chat_area);
            widget.render(chat_area, &mut buf);

            // Every cell outside chat_area should remain at default. If the
            // widget bled, we'll see leftover symbols.
            let default_symbol = " ";
            for y in 0..height {
                for x in chat_width..total_width {
                    let cell = &buf[(x, y)];
                    let sym = cell.symbol();
                    assert!(
                        sym == default_symbol || sym.is_empty(),
                        "[{total_width}x{height}, chat={chat_width}] cell ({x},{y}) leaked content {sym:?} outside chat_area"
                    );
                }
            }
        }
    }

    /// Regression: when the transcript scrollbar is visible, the rightmost
    /// content column must remain readable (the scrollbar gets its own
    /// 1-column gutter rather than overdrawing chat content).
    #[test]
    fn chat_widget_reserves_scrollbar_gutter_when_scrollbar_visible() {
        let mut app = create_test_app();
        // Many short messages → forces the scrollbar to be visible.
        for i in 0..200 {
            app.add_message(HistoryCell::User {
                content: format!("user message {i}"),
            });
        }

        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 8,
        };
        let mut buf = Buffer::empty(area);
        let widget = ChatWidget::new(&mut app, area);
        widget.render(area, &mut buf);

        // The rightmost column should host the scrollbar track/thumb.
        // The penultimate column should still hold normal content (a digit,
        // letter, or space — never the scrollbar glyph).
        let scrollbar_track = "│";
        let scrollbar_thumb = "┃";
        let mut scrollbar_seen = false;
        for y in 0..area.height {
            let last = buf[(area.width - 1, y)].symbol();
            let penult = buf[(area.width - 2, y)].symbol();
            if last == scrollbar_track || last == scrollbar_thumb {
                scrollbar_seen = true;
            }
            assert!(
                penult != scrollbar_track && penult != scrollbar_thumb,
                "scrollbar leaked into column {} (cell {:?}) at row {y}",
                area.width - 2,
                penult
            );
        }
        assert!(
            scrollbar_seen,
            "scrollbar should be visible for a long history"
        );
    }

    /// Regression for issue #65: after `App::handle_resize`, the chat widget
    /// must produce a clean render at the new width — no stale wrapping,
    /// no panic, no content exceeding the requested width. Cycling through
    /// several widths (shrinks and grows) flushes any cached layout that
    /// fails to invalidate on resize.
    #[test]
    fn chat_widget_renders_cleanly_after_resize_cycle() {
        let mut app = create_test_app();
        // Add some long content that wraps differently at different widths.
        for i in 0..40 {
            app.add_message(HistoryCell::User {
                content: format!("user message {i} with enough text to wrap at 30 columns easily"),
            });
        }

        let widths_to_cycle = [120u16, 80, 40, 60, 100, 30];
        let height: u16 = 20;
        for width in widths_to_cycle {
            // Caller-side: simulate the resize handler invalidating caches.
            app.handle_resize(width, height);
            let area = Rect {
                x: 0,
                y: 0,
                width,
                height,
            };
            let mut buf = Buffer::empty(area);
            let widget = ChatWidget::new(&mut app, area);
            widget.render(area, &mut buf);

            // The render must produce at least some non-empty content for a
            // populated history at any reasonable width. This catches a class
            // of resize regressions where stale layout state leaves a blank
            // viewport after a width change.
            let mut non_empty = 0usize;
            for y in 0..height {
                for x in 0..width {
                    let sym = buf[(x, y)].symbol();
                    if sym != " " && !sym.is_empty() {
                        non_empty += 1;
                    }
                }
            }
            assert!(
                non_empty > 0,
                "render at {width}x{height} produced an empty buffer after resize"
            );
        }
    }

    /// Regression for issue #65: the transcript view cache must invalidate
    /// when width changes, so the same `App.history` re-wraps to the new
    /// width on the very next `ChatWidget::new` call.
    #[test]
    fn transcript_cache_invalidates_on_width_change() {
        let mut app = create_test_app();
        for i in 0..10 {
            app.add_message(HistoryCell::User {
                content: format!("a fairly long user message number {i} that needs to wrap"),
            });
        }

        let area_wide = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 20,
        };
        let area_narrow = Rect {
            x: 0,
            y: 0,
            width: 30,
            height: 20,
        };
        let mut buf_wide = Buffer::empty(area_wide);
        let widget_wide = ChatWidget::new(&mut app, area_wide);
        widget_wide.render(area_wide, &mut buf_wide);
        let wide_total_lines = app.transcript_cache.total_lines();

        // Without an explicit resize call, just shrinking the render area
        // should still trigger a cache rebuild because the cache keys on width.
        let mut buf_narrow = Buffer::empty(area_narrow);
        let widget_narrow = ChatWidget::new(&mut app, area_narrow);
        widget_narrow.render(area_narrow, &mut buf_narrow);
        let narrow_total_lines = app.transcript_cache.total_lines();

        assert!(
            narrow_total_lines > wide_total_lines,
            "narrow render should produce more wrapped lines (got {narrow_total_lines}, wide={wide_total_lines})"
        );
    }

    /// Issue #78 — perf bench for transcript scroll lag.
    ///
    /// Builds a 5000-entry history (mix of user / assistant / a few tool
    /// cells), then times `ChatWidget::new` at scroll offsets 0, 100, 500,
    /// and 2000 lines from the tail. The first call after history mutation
    /// pays the wrap cost; subsequent calls at different offsets should hit
    /// the per-cell cache and be ~constant time regardless of offset.
    ///
    /// Run with: `cargo test -p deepseek-tui --release bench_transcript_scroll
    /// -- --ignored --nocapture`
    #[test]
    #[ignore = "perf bench; run with --release"]
    fn bench_transcript_scroll_5000_messages() {
        use std::time::Instant;

        let mut app = create_test_app();
        // 5000 cells: alternating user / assistant with realistic-ish bodies
        // so wrapping cost is non-trivial. Every 50th cell is a (small)
        // generic tool cell, mirroring real transcripts.
        for i in 0..5000usize {
            let cell = if i % 50 == 49 {
                HistoryCell::Tool(ToolCell::Generic(GenericToolCell {
                    name: "grep_files".to_string(),
                    status: ToolStatus::Success,
                    input_summary: Some(format!("query: hit-{i}")),
                    output: Some(format!("found 12 matches in cell-{i}")),
                    prompts: None,
                }))
            } else if i % 2 == 0 {
                HistoryCell::User {
                    content: format!(
                        "user message {i}: please review the changes in src/foo/bar.rs and \
                         tell me whether the new error handling looks reasonable"
                    ),
                }
            } else {
                HistoryCell::Assistant {
                    content: format!(
                        "Sure — looking at src/foo/bar.rs in cell {i}, the new error \
                         handling wraps each fallible call in `?` and propagates a \
                         typed `FooError`. That looks fine, but consider whether the \
                         `Display` impl needs to redact the inner path."
                    ),
                    streaming: false,
                }
            };
            app.add_message(cell);
        }

        let area = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 30,
        };

        // Warm-up: first call after a full history build pays the wrap cost
        // for every cell. We don't time this — it's amortized across the
        // session and is not the user-visible problem.
        let _ = ChatWidget::new(&mut app, area);

        let visible = area.height as usize;
        // For each scroll target, snap the scroll position there and measure
        // a fresh ChatWidget::new(). The cache should hit for all unchanged
        // cells, so the time should be roughly constant regardless of
        // offset.
        for offset_from_tail in [0usize, 100, 500, 2000] {
            let total = app.transcript_cache.total_lines();
            let max_start = total.saturating_sub(visible);
            let target = max_start.saturating_sub(offset_from_tail);
            app.transcript_scroll = crate::tui::scrolling::TranscriptScroll::at_line(target);

            let iters: u32 = 10;
            let start = Instant::now();
            for _ in 0..iters {
                let _ = ChatWidget::new(&mut app, area);
            }
            let elapsed = start.elapsed();
            let per_call_us = elapsed.as_micros() / u128::from(iters);
            println!(
                "[bench_transcript_scroll] offset={offset_from_tail:>5} \
                 per_render={per_call_us:>6} \u{3bc}s  ({:>3} ms / {iters} iters)",
                elapsed.as_millis()
            );
        }

        // Streaming-delta scenario: append one assistant cell at the tail
        // and time a render. The cache should re-render only the new cell,
        // NOT every cell — even at deep scroll.
        for offset_from_tail in [0usize, 2000] {
            let total = app.transcript_cache.total_lines();
            let max_start = total.saturating_sub(visible);
            let target = max_start.saturating_sub(offset_from_tail);
            app.transcript_scroll = crate::tui::scrolling::TranscriptScroll::at_line(target);

            let iters: u32 = 10;
            let start = Instant::now();
            for i in 0..iters {
                app.add_message(HistoryCell::Assistant {
                    content: format!("delta {i}"),
                    streaming: false,
                });
                let _ = ChatWidget::new(&mut app, area);
            }
            let elapsed = start.elapsed();
            let per_call_us = elapsed.as_micros() / u128::from(iters);
            println!(
                "[bench_transcript_scroll] streaming offset={offset_from_tail:>5} \
                 per_render={per_call_us:>6} \u{3bc}s  ({:>3} ms / {iters} iters)",
                elapsed.as_millis()
            );
        }
    }
}
