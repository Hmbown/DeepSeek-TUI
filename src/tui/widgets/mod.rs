mod header;
mod renderable;

pub use header::{HeaderData, HeaderWidget};
pub use renderable::Renderable;

use crate::palette;
use crate::tui::app::App;
use crate::tui::approval::{ApprovalRequest, ElevationOption, ElevationRequest, ToolCategory};
use crate::tui::scrolling::TranscriptScroll;
use crate::{commands, config::COMMON_DEEPSEEK_MODELS};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::Stylize,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Widget, Wrap},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub struct ChatWidget {
    content_area: Rect,
    lines: Vec<Line<'static>>,
}

impl ChatWidget {
    pub fn new(app: &mut App, area: Rect) -> Self {
        let content_area = area;
        let visible_lines = content_area.height as usize;
        let render_options = app.transcript_render_options();

        app.transcript_cache.ensure(
            &app.history,
            content_area.width.max(1),
            app.history_version,
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

        app.last_transcript_area = Some(content_area);
        app.last_transcript_top = top;
        app.last_transcript_visible = visible_lines;
        app.last_transcript_total = total_lines;
        app.last_transcript_padding_top = 0;

        let end = (top + visible_lines).min(total_lines);
        let mut lines = if total_lines == 0 {
            vec![Line::from("")]
        } else {
            app.transcript_cache.lines()[top..end].to_vec()
        };

        apply_selection(&mut lines, top, app);

        if matches!(app.transcript_scroll, TranscriptScroll::ToBottom) {
            app.last_transcript_padding_top = visible_lines.saturating_sub(lines.len());
            pad_lines_to_bottom(&mut lines, visible_lines);
        }

        Self {
            content_area,
            lines,
        }
    }
}

impl Renderable for ChatWidget {
    fn render(&self, _area: Rect, buf: &mut Buffer) {
        let paragraph = Paragraph::new(self.lines.clone());
        paragraph.render(self.content_area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

pub struct ComposerWidget<'a> {
    app: &'a App,
    prompt: &'a str,
    max_height: u16,
}

impl<'a> ComposerWidget<'a> {
    pub fn new(app: &'a App, prompt: &'a str, max_height: u16) -> Self {
        Self {
            app,
            prompt,
            max_height,
        }
    }
}

impl Renderable for ComposerWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let slash_menu_entries = if self.app.slash_menu_hidden {
            Vec::new()
        } else {
            slash_completion_hints(&self.app.input, 6)
        };
        let prompt_width = self.prompt.width();
        let prompt_width_u16 = u16::try_from(prompt_width).unwrap_or(u16::MAX);
        let content_width = usize::from(area.width.saturating_sub(prompt_width_u16).max(1));
        let menu_lines = slash_menu_entries.len();
        let max_height = usize::from(area.height).saturating_sub(menu_lines).max(1);
        let continuation = " ".repeat(prompt_width);

        let (visible_lines, _cursor_row, _cursor_col) = layout_input(
            &self.app.input,
            self.app.cursor_position,
            content_width,
            max_height,
        );

        let background = Style::default().bg(self.app.ui_theme.composer_bg);
        let block = Block::default().style(background);
        block.render(area, buf);

        let mut lines = Vec::new();
        if self.app.input.is_empty() {
            let placeholder = "Type a message or /help for commands...";
            lines.push(Line::from(vec![
                Span::styled(
                    self.prompt,
                    Style::default().fg(palette::DEEPSEEK_SKY).bold(),
                ),
                Span::styled(
                    placeholder,
                    Style::default().fg(palette::TEXT_MUTED).italic(),
                ),
            ]));
        } else {
            for (idx, line) in visible_lines.iter().enumerate() {
                let prefix = if idx == 0 {
                    self.prompt
                } else {
                    continuation.as_str()
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(palette::DEEPSEEK_SKY).bold()),
                    Span::styled(line.clone(), Style::default().fg(palette::TEXT_PRIMARY)),
                ]));
            }
        }

        if !slash_menu_entries.is_empty() {
            let selected = self
                .app
                .slash_menu_selected
                .min(slash_menu_entries.len().saturating_sub(1));
            for (idx, entry) in slash_menu_entries.iter().enumerate() {
                let is_selected = idx == selected;
                let style = if is_selected {
                    Style::default()
                        .fg(palette::DEEPSEEK_SKY)
                        .bg(palette::SELECTION_BG)
                } else {
                    Style::default().fg(palette::TEXT_MUTED)
                };
                let marker = if is_selected { "▸" } else { " " };
                lines.push(Line::from(vec![
                    Span::styled(" ", Style::default()),
                    Span::styled(marker, style),
                    Span::styled(" ", style),
                    Span::styled(entry.clone(), style),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines).style(background);
        paragraph.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let menu_lines = if self.app.slash_menu_hidden {
            0
        } else {
            slash_completion_hints(&self.app.input, 6).len()
        };
        composer_height(
            &self.app.input,
            width,
            self.max_height,
            self.prompt,
            menu_lines,
        )
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let menu_lines = if self.app.slash_menu_hidden {
            0
        } else {
            slash_completion_hints(&self.app.input, 6).len()
        };
        let prompt_width = self.prompt.width();
        let prompt_width_u16 = u16::try_from(prompt_width).unwrap_or(u16::MAX);
        let content_width = usize::from(area.width.saturating_sub(prompt_width_u16).max(1));
        let max_height = usize::from(area.height).saturating_sub(menu_lines).max(1);

        let (_visible_lines, cursor_row, cursor_col) = layout_input(
            &self.app.input,
            self.app.cursor_position,
            content_width,
            max_height,
        );

        let cursor_x = area
            .x
            .saturating_add(prompt_width_u16)
            .saturating_add(u16::try_from(cursor_col).unwrap_or(u16::MAX));
        let cursor_y = area
            .y
            .saturating_add(u16::try_from(cursor_row).unwrap_or(u16::MAX));
        if cursor_x < area.x + area.width && cursor_y < area.y + area.height {
            Some((cursor_x, cursor_y))
        } else {
            None
        }
    }
}

pub struct ApprovalWidget<'a> {
    request: &'a ApprovalRequest,
    selected: usize,
}

impl<'a> ApprovalWidget<'a> {
    pub fn new(request: &'a ApprovalRequest, selected: usize) -> Self {
        Self { request, selected }
    }
}

impl Renderable for ApprovalWidget<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let popup_width = 65.min(area.width.saturating_sub(4));
        let popup_height = 18.min(area.height.saturating_sub(4));
        let popup_area = Rect {
            x: (area.width.saturating_sub(popup_width)) / 2,
            y: (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let mut lines = vec![
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

        let category_label = match self.request.category {
            ToolCategory::Safe => ("Safe", palette::STATUS_SUCCESS),
            ToolCategory::FileWrite => ("File Write", palette::STATUS_WARNING),
            ToolCategory::Shell => ("Shell Command", palette::STATUS_ERROR),
        };
        lines.push(Line::from(vec![
            Span::raw("  Type: "),
            Span::styled(
                category_label.0,
                Style::default()
                    .fg(category_label.1)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        if let Some(cost) = &self.request.estimated_cost {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  Cost: "),
                Span::styled(
                    cost.display(),
                    Style::default()
                        .fg(palette::STATUS_WARNING)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                format!("  {}", &cost.breakdown),
                Style::default().fg(palette::TEXT_MUTED),
            )));
        } else {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  No cost (free operation)",
                Style::default().fg(palette::TEXT_MUTED),
            )));
        }

        lines.push(Line::from(""));
        let params_str = self.request.params_display();
        let params_truncated = crate::utils::truncate_with_ellipsis(&params_str, 50, "...");
        lines.push(Line::from(Span::styled(
            format!("  Params: {params_truncated}"),
            Style::default().fg(palette::TEXT_MUTED),
        )));

        lines.push(Line::from(""));

        let options = [
            ("y", "Approve (this time)"),
            ("a", "Approve for session"),
            ("n", "Deny"),
            ("v", "View full params"),
            ("Esc", "Abort turn"),
        ];

        for (i, (key, label)) in options.iter().enumerate() {
            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .bg(palette::SELECTION_BG)
            } else {
                Style::default()
            };

            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("[{key}] "),
                    Style::default().fg(palette::STATUS_SUCCESS),
                ),
                Span::styled(*label, style),
            ]));
        }

        let title = format!(" Approve Tool: {} ", &self.request.tool_name);
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
        let popup_height = 20.min(area.height.saturating_sub(4));
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
            "  Choose how to proceed:",
            Style::default().fg(palette::TEXT_MUTED),
        )));
        lines.push(Line::from(""));

        // Render options
        for (i, option) in self.request.options.iter().enumerate() {
            let is_selected = i == self.selected;
            let style = if is_selected {
                Style::default()
                    .fg(palette::DEEPSEEK_SKY)
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

    let selection_style = Style::default().bg(app.ui_theme.selection_bg);

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

        let new_spans = apply_selection_to_line(line, col_start, col_end, selection_style);
        line.spans = new_spans;
    }
}

fn apply_selection_to_line(
    line: &Line<'static>,
    col_start: usize,
    col_end: usize,
    selection_style: Style,
) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut current_col = 0usize;

    for span in &line.spans {
        let span_text: &str = span.content.as_ref();
        let span_len = span_text.chars().count();
        let span_end = current_col + span_len;

        if span_end <= col_start || current_col >= col_end {
            result.push(span.clone());
        } else if current_col >= col_start && span_end <= col_end {
            result.push(Span::styled(
                span.content.clone(),
                span.style.patch(selection_style),
            ));
        } else {
            let chars: Vec<char> = span_text.chars().collect();
            let mut before = String::new();
            let mut selected = String::new();
            let mut after = String::new();

            for (i, &ch) in chars.iter().enumerate() {
                let char_col = current_col + i;
                if char_col < col_start {
                    before.push(ch);
                } else if char_col < col_end {
                    selected.push(ch);
                } else {
                    after.push(ch);
                }
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

fn composer_height(
    input: &str,
    width: u16,
    available_height: u16,
    prompt: &str,
    extra_lines: usize,
) -> u16 {
    let prompt_width = prompt.width();
    let prompt_width_u16 = u16::try_from(prompt_width).unwrap_or(u16::MAX);
    let content_width = usize::from(width.saturating_sub(prompt_width_u16).max(1));
    let mut line_count = wrap_input_lines(input, content_width).len();
    if line_count == 0 {
        line_count = 1;
    }
    line_count = line_count.saturating_add(extra_lines);
    let max_height = usize::from(available_height.clamp(1, 8));
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
    use super::{cursor_row_col, pad_lines_to_bottom, wrap_input_lines, wrap_text};
    use ratatui::text::Line;
    use unicode_width::UnicodeWidthStr;

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
        assert!(col >= 2 && col <= 3, "col = {col}, expected 2 or 3");
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
}
