use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::tui::model::{CompositeMode, FieldKind, FieldSchema};
use crate::tui::state::{FieldState, FormState, SectionState};

pub fn render_fields(
    frame: &mut Frame<'_>,
    area: Rect,
    form_state: &mut FormState,
    enable_cursor: bool,
) {
    let (section, selected_index) = match form_state.active_section_mut() {
        Some((section, selected)) => (section, selected),
        None => {
            let placeholder =
                Paragraph::new("No section selected").block(Block::default().borders(Borders::ALL));
            frame.render_widget(placeholder, area);
            return;
        }
    };

    let mut field_area = area;
    if let Some(description) = &section.description {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(2)])
            .split(area);
        let details = Paragraph::new(description.clone())
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .title(format!("{} Details", section.title))
                    .borders(Borders::ALL),
            );
        frame.render_widget(details, chunks[0]);
        field_area = chunks[1];
    }

    if section.fields.is_empty() {
        let placeholder = Paragraph::new("This section has no fields").block(
            Block::default()
                .title(section.title.clone())
                .borders(Borders::ALL),
        );
        frame.render_widget(placeholder, field_area);
        return;
    }

    let value_panel_width = field_area
        .width
        .saturating_sub(8 + highlight_symbol_width());
    let text_line_width = value_panel_width.saturating_add(
        (UnicodeWidthStr::width(VALUE_BORDER_PREFIX) + UnicodeWidthStr::width(VALUE_BORDER_SUFFIX))
            as u16,
    );
    let mut items = Vec::with_capacity(section.fields.len());
    let mut cursor_hint: Option<CursorHint> = None;
    let mut field_heights = Vec::with_capacity(section.fields.len());
    adjust_scroll_offset(section, selected_index, field_area.height);
    let viewport_top = section.scroll_offset;

    for (idx, field) in section.fields.iter().enumerate() {
        let render = build_field_render(
            field,
            idx == selected_index,
            value_panel_width,
            text_line_width,
        );
        if idx == selected_index
            && let Some(hint) = render.cursor_hint
        {
            cursor_hint = Some(hint);
        }
        field_heights.push(render.lines.len());
        items.push(ListItem::new(render.lines));
    }

    let mut list_state = ListState::default();
    if !section.fields.is_empty() {
        list_state.select(Some(selected_index));
        *list_state.offset_mut() = section.scroll_offset;
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title(section.title.clone())
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default())
        .highlight_symbol(HIGHLIGHT_SYMBOL);

    frame.render_stateful_widget(list, field_area, &mut list_state);

    if enable_cursor
        && let (Some(cursor), Some(height)) =
            (cursor_hint, field_heights.get(selected_index).copied())
        && selected_index >= viewport_top
    {
        let relative_y: usize = field_heights
            .iter()
            .take(selected_index)
            .skip(viewport_top)
            .copied()
            .sum();
        let caret_line = relative_y + cursor.line_in_field.min(height.saturating_sub(1));
        let max_visible = field_area.height.saturating_sub(3) as usize;
        #[cfg(feature = "debug")]
        println!(
            "[cursor-debug] selected={} scroll_offset={} relative_y={} caret_line={} max_visible={}",
            selected_index, section.scroll_offset, relative_y, caret_line, max_visible
        );
        if caret_line <= max_visible {
            let inner_y = field_area.y.saturating_add(2);
            #[cfg(feature = "debug")]
            println!(
                "[cursor-debug-xy] inner_y={} caret_line={} cursor_y={}",
                inner_y,
                caret_line,
                inner_y + caret_line as u16
            );
            let cursor_y = inner_y.saturating_add(caret_line as u16);
            let cursor_x = field_area.x.saturating_add(cursor.column_offset);
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

fn adjust_scroll_offset(section: &mut SectionState, selected: usize, height: u16) {
    let window = height.saturating_sub(4) as usize;
    if window == 0 {
        section.scroll_offset = 0;
        return;
    }
    if selected < section.scroll_offset {
        section.scroll_offset = selected;
    } else if selected >= section.scroll_offset + window {
        section.scroll_offset = selected + 1 - window;
    }
}

const VALUE_BORDER_PREFIX: &str = "│ ";
const VALUE_BORDER_SUFFIX: &str = " │";
const HIGHLIGHT_SYMBOL: &str = "» ";
const GUTTER_PADDING: u16 = 0;
const LIST_BORDER_OFFSET: u16 = 1;

fn highlight_symbol_width() -> u16 {
    UnicodeWidthStr::width(HIGHLIGHT_SYMBOL) as u16
}

struct FieldRender {
    lines: Vec<Line<'static>>,
    cursor_hint: Option<CursorHint>,
}

struct CursorHint {
    line_in_field: usize,
    column_offset: u16,
}

fn build_field_render(
    field: &FieldState,
    is_selected: bool,
    value_width: u16,
    text_width: u16,
) -> FieldRender {
    let mut lines = Vec::new();
    lines.push(info_line(field, is_selected));
    let (value_panel, cursor_hint) = value_panel_lines(field, is_selected, value_width);
    lines.extend(value_panel);
    lines.extend(meta_lines(field, is_selected, text_width));

    if is_selected {
        if let Some(selector_lines) = composite_selector_lines(field) {
            lines.extend(selector_lines);
        }

        if let Some(summary) = composite_summary_lines(field) {
            lines.extend(summary);
        }

        if let Some(summary) = repeatable_summary_lines(field, text_width) {
            lines.extend(summary);
        }
    }

    if let Some(error) = error_lines(field, text_width) {
        lines.extend(error);
    }

    FieldRender { lines, cursor_hint }
}

fn info_line(field: &FieldState, is_selected: bool) -> Line<'static> {
    let mut label = field.schema.display_label();
    if field.schema.required {
        label.push_str(" *");
    }

    let label_style = if is_selected {
        Style::default()
            .fg(crate::deepseek_palette::STATUS_WARN)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(crate::deepseek_palette::ACCENT_SKY)
            .add_modifier(Modifier::BOLD)
    };
    let mut spans = vec![Span::styled(label, label_style)];

    if field.dirty {
        spans.push(Span::styled("  ·dirty", Style::default().fg(crate::deepseek_palette::STATUS_WARN)));
    }

    if field.error.is_some() {
        spans.push(Span::styled(
            "  ·invalid",
            Style::default().fg(crate::deepseek_palette::STATUS_ERROR).add_modifier(Modifier::BOLD),
        ));
    }

    Line::from(spans)
}

fn value_panel_lines(
    field: &FieldState,
    is_selected: bool,
    max_width: u16,
) -> (Vec<Line<'static>>, Option<CursorHint>) {
    let clamp_width = max_width.max(4) as usize;
    let value_text = field.display_value_with_limit(clamp_width.saturating_mul(3));
    let cursor_offset = field
        .cursor_offset()
        .unwrap_or_else(|| value_text.chars().count());
    let mut wrapped_value = wrap_preserving_spaces(&value_text, clamp_width);
    if wrapped_value.is_empty() {
        wrapped_value.push(String::new());
    }
    let inner_width = wrapped_value
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .max()
        .unwrap_or(0);
    let mut cursor_hint = None;
    let mut lines = Vec::new();

    if is_selected {
        let border_width = inner_width.saturating_add(2);
        let border_line = "─".repeat(border_width);
        let border_style = Style::default().fg(crate::deepseek_palette::STATUS_WARN);
        let value_style = Style::default()
            .fg(crate::deepseek_palette::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD);

        lines.push(Line::from(Span::styled(
            format!("┌{}┐", border_line),
            border_style,
        )));
        let content_start = lines.len();
        for segment in &wrapped_value {
            let mut content = segment.clone();
            let mut width = UnicodeWidthStr::width(content.as_str());
            while width < inner_width {
                content.push(' ');
                width += 1;
            }
            lines.push(Line::from(vec![
                Span::styled(VALUE_BORDER_PREFIX, border_style),
                Span::styled(content, value_style),
                Span::styled(VALUE_BORDER_SUFFIX, border_style),
            ]));
        }
        lines.push(Line::from(Span::styled(
            format!("└{}┘", border_line),
            border_style,
        )));
        let (relative_line, caret_width) =
            wrapped_cursor_position(&value_text, clamp_width, cursor_offset);
        let caret_line = content_start + relative_line.min(wrapped_value.len().saturating_sub(1));
        let caret_width = caret_width.min(inner_width);
        let prefix_width = UnicodeWidthStr::width(VALUE_BORDER_PREFIX) as u16 + GUTTER_PADDING;
        let highlight_width = highlight_symbol_width();
        let column_offset = LIST_BORDER_OFFSET
            .saturating_add(highlight_width)
            .saturating_add(prefix_width)
            .saturating_add(caret_width as u16);
        cursor_hint = Some(CursorHint {
            line_in_field: caret_line,
            column_offset,
        });
    } else {
        for segment in wrapped_value {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(segment, Style::default().fg(crate::deepseek_palette::TEXT_PRIMARY)),
            ]));
        }
    }

    (lines, cursor_hint)
}

pub(crate) fn meta_lines(
    field: &FieldState,
    is_selected: bool,
    max_width: u16,
) -> Vec<Line<'static>> {
    let mut parts = Vec::new();
    parts.push(format!("type: {}", field_type_label(&field.schema.kind)));

    // 添加约束信息
    let constraints = extract_constraints(&field.schema);
    if !constraints.is_empty() {
        parts.push(format!("constraints: {}", constraints.join(", ")));
    }

    if let Some(desc) = field
        .schema
        .description
        .as_ref()
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
    {
        parts.push(format!("desc: {}", desc));
    }
    let content = parts.join(" | ");
    if content.is_empty() {
        return Vec::new();
    }

    let style = if is_selected {
        Style::default()
            .fg(crate::deepseek_palette::ACCENT_BLUE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(crate::deepseek_palette::TEXT_DIM)
    };

    wrap_with_prefix(&content, "  ", max_width, style, style)
}

pub(crate) fn error_lines(field: &FieldState, max_width: u16) -> Option<Vec<Line<'static>>> {
    field.error.as_ref().map(|message| {
        let mut lines = Vec::new();
        lines.push(Line::from(Span::styled(
            "  Error:",
            Style::default().fg(crate::deepseek_palette::STATUS_ERROR).add_modifier(Modifier::BOLD),
        )));
        let error_style = Style::default().fg(crate::deepseek_palette::STATUS_ERROR);
        lines.extend(wrap_with_prefix_limited(
            message,
            "    ",
            max_width,
            error_style,
            error_style,
            3,
        ));
        lines
    })
}

fn field_type_label(kind: &FieldKind) -> String {
    match kind {
        FieldKind::String => "string".to_string(),
        FieldKind::Integer => "integer".to_string(),
        FieldKind::Number => "number".to_string(),
        FieldKind::Boolean => "boolean".to_string(),
        FieldKind::Enum { .. } => "enum".to_string(),
        FieldKind::Nullable(inner) => format!("{} | null", field_type_label(inner)),
        FieldKind::Array(inner) => match inner.as_ref() {
            FieldKind::Json => "object[]".to_string(),
            FieldKind::Composite(comp) => {
                // Get more descriptive label for composite arrays
                match &comp.mode {
                    CompositeMode::OneOf => {
                        if comp.variants.len() == 1 {
                            format!("{}[]", comp.variants[0].title)
                        } else {
                            "choice[]".to_string()
                        }
                    }
                    CompositeMode::AnyOf => "multi-choice[]".to_string(),
                }
            }
            _ => format!("{}[]", field_type_label(inner)),
        },
        FieldKind::Json => "object".to_string(),
        FieldKind::Composite(comp) => match &comp.mode {
            CompositeMode::OneOf => "choice".to_string(),
            CompositeMode::AnyOf => "multi-choice".to_string(),
        },
        FieldKind::KeyValue(_) => "map".to_string(),
    }
}

fn extract_constraints(schema: &FieldSchema) -> Vec<String> {
    let mut constraints = Vec::new();

    // format 约束
    if let Some(format) = schema.metadata.get("format")
        && let Some(format_str) = format.as_str()
    {
        constraints.push(format!("format: {}", format_str));
    }

    // pattern 约束
    if let Some(pattern) = schema.metadata.get("pattern")
        && let Some(pattern_str) = pattern.as_str()
    {
        let truncated = if pattern_str.len() > 30 {
            format!("{}...", &pattern_str[..27])
        } else {
            pattern_str.to_string()
        };
        constraints.push(format!("pattern: {}", truncated));
    }

    // 字符串长度约束
    if matches!(schema.kind, FieldKind::String | FieldKind::Nullable(_)) {
        if let Some(min) = schema.metadata.get("minLength").and_then(|v| v.as_u64()) {
            if let Some(max) = schema.metadata.get("maxLength").and_then(|v| v.as_u64()) {
                constraints.push(format!("length: {}..{}", min, max));
            } else {
                constraints.push(format!("minLength: {}", min));
            }
        } else if let Some(max) = schema.metadata.get("maxLength").and_then(|v| v.as_u64()) {
            constraints.push(format!("maxLength: {}", max));
        }
    }

    // 数值范围约束
    if matches!(
        schema.kind,
        FieldKind::Integer | FieldKind::Number | FieldKind::Nullable(_)
    ) {
        let min = schema
            .metadata
            .get("minimum")
            .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)));
        let max = schema
            .metadata
            .get("maximum")
            .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)));

        if let (Some(min_val), Some(max_val)) = (min, max) {
            if matches!(schema.kind, FieldKind::Integer) {
                constraints.push(format!("range: {}..{}", min_val as i64, max_val as i64));
            } else {
                constraints.push(format!("range: {:.2}..{:.2}", min_val, max_val));
            }
        } else if let Some(min_val) = min {
            constraints.push(format!(
                "min: {}",
                if matches!(schema.kind, FieldKind::Integer) {
                    format!("{}", min_val as i64)
                } else {
                    format!("{:.2}", min_val)
                }
            ));
        } else if let Some(max_val) = max {
            constraints.push(format!(
                "max: {}",
                if matches!(schema.kind, FieldKind::Integer) {
                    format!("{}", max_val as i64)
                } else {
                    format!("{:.2}", max_val)
                }
            ));
        }

        // multipleOf 约束
        if let Some(multiple) = schema.metadata.get("multipleOf").and_then(|v| v.as_f64()) {
            constraints.push(format!("multipleOf: {}", multiple));
        }
    }

    // 数组项数约束
    if matches!(schema.kind, FieldKind::Array(_)) {
        if let Some(min) = schema.metadata.get("minItems").and_then(|v| v.as_u64()) {
            if let Some(max) = schema.metadata.get("maxItems").and_then(|v| v.as_u64()) {
                constraints.push(format!("items: {}..{}", min, max));
            } else {
                constraints.push(format!("minItems: {}", min));
            }
        } else if let Some(max) = schema.metadata.get("maxItems").and_then(|v| v.as_u64()) {
            constraints.push(format!("maxItems: {}", max));
        }

        if let Some(unique) = schema.metadata.get("uniqueItems").and_then(|v| v.as_bool())
            && unique
        {
            constraints.push("unique".to_string());
        }
    }

    constraints
}

fn composite_summary_lines(field: &FieldState) -> Option<Vec<Line<'static>>> {
    let summaries = field.composite_variant_summaries()?;
    if summaries.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    lines.push(Line::from("  Active variants:"));
    let max_render = 3usize;
    for summary in summaries.iter().take(max_render) {
        lines.push(Line::from(vec![
            Span::styled("  • ", Style::default().fg(crate::deepseek_palette::TEXT_MUTED)),
            Span::styled(
                summary.title.clone(),
                Style::default()
                    .fg(crate::deepseek_palette::ACCENT_PURPLE)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        if let Some(desc) = summary.description.as_ref()
            && !desc.is_empty()
        {
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(desc.clone(), Style::default().fg(crate::deepseek_palette::TEXT_MUTED)),
            ]));
        }
        for line in &summary.lines {
            lines.push(Line::from(format!("     {line}")));
        }
        lines.push(Line::from(" "));
    }
    if summaries.len() > max_render {
        lines.push(Line::from(format!(
            "    … ({} more active variants)",
            summaries.len() - max_render
        )));
    }
    Some(lines)
}

fn repeatable_summary_lines(field: &FieldState, max_width: u16) -> Option<Vec<Line<'static>>> {
    let entry_prefix = "  » ";
    let entry_width = max_width
        .saturating_sub(UnicodeWidthStr::width(entry_prefix) as u16)
        .max(1) as usize;

    if let Some((entries, selected)) = field.composite_list_panel_with_limit(entry_width) {
        if entries.is_empty() {
            return None;
        }
        let mut lines = Vec::new();
        lines.push(Line::from("  Entries:"));
        let max_render = 4usize;
        for (idx, entry) in entries.iter().enumerate().take(max_render) {
            let marker = if idx == selected { "»" } else { " " };
            lines.push(Line::from(format!(
                "  {marker} {}",
                truncate_line(entry, entry_width)
            )));
        }
        if entries.len() > max_render {
            lines.push(Line::from(format!(
                "    … {} more entries",
                entries.len() - max_render
            )));
        }
        return Some(lines);
    }
    None
}

fn composite_selector_lines(field: &FieldState) -> Option<Vec<Line<'static>>> {
    let view = field.composite_selector_view()?;
    let mut lines = Vec::new();
    if view.options.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  No variants available in this schema.",
            Style::default().fg(crate::deepseek_palette::TEXT_MUTED),
        )]));
        return Some(lines);
    }

    // 改进：更清晰的标签
    let label = if view.multi {
        "AnyOf (value satisfies at least one)"
    } else {
        "OneOf (select exactly one)"
    };
    lines.push(Line::from(vec![Span::styled(
        format!("  {label}"),
        Style::default()
            .fg(crate::deepseek_palette::ACCENT_PURPLE)
            .add_modifier(Modifier::BOLD),
    )]));

    // 改进：每个选项独立一行
    for (idx, option) in view.options.iter().enumerate() {
        let is_active = view.active.get(idx).copied().unwrap_or(false);
        let mark = if view.multi {
            if is_active { "[x]" } else { "[ ]" }
        } else if is_active {
            "(•)"
        } else {
            "( )"
        };

        let style = if is_active {
            Style::default()
                .fg(crate::deepseek_palette::STATUS_WARN)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(crate::deepseek_palette::TEXT_MUTED)
        };

        let mut spans = vec![
            Span::styled(format!("    {mark} "), Style::default().fg(crate::deepseek_palette::TEXT_DIM)),
            Span::styled(format!("#{} {}", idx + 1, option), style),
        ];

        // 改进：显示描述
        if let Some(Some(desc)) = view.descriptions.get(idx)
            && !desc.is_empty()
        {
            spans.push(Span::styled(
                format!(" - {}", desc),
                Style::default().fg(crate::deepseek_palette::TEXT_MUTED),
            ));
        }

        lines.push(Line::from(spans));
    }

    // 改进：更清晰的操作提示
    let hint = if view.multi {
        "  Press Enter to toggle • Ctrl+E to open editor"
    } else {
        "  Press Enter to select • Ctrl+E to open editor"
    };
    lines.push(Line::from(vec![Span::styled(
        hint,
        Style::default().fg(crate::deepseek_palette::TEXT_DIM),
    )]));

    // 改进：显示当前激活的变体
    if view.multi {
        let active_titles: Vec<_> = view
            .options
            .iter()
            .enumerate()
            .filter_map(|(idx, title)| {
                view.active
                    .get(idx)
                    .copied()
                    .filter(|flag| *flag)
                    .map(|_| format!("#{} {}", idx + 1, title))
            })
            .collect();

        if active_titles.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "  Active: <none selected>",
                Style::default().fg(crate::deepseek_palette::TEXT_MUTED),
            )]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("  Active: ", Style::default().fg(crate::deepseek_palette::TEXT_MUTED)),
                Span::styled(active_titles.join(", "), Style::default().fg(crate::deepseek_palette::TEXT_PRIMARY)),
            ]));
        }
    }

    Some(lines)
}

fn wrap_preserving_spaces(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1);
        if current_width + ch_width > width && !current.is_empty() {
            lines.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }
    lines.push(current);
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn wrapped_cursor_position(text: &str, width: usize, cursor_offset: usize) -> (usize, usize) {
    if width == 0 {
        return (0, cursor_offset);
    }

    let mut line = 0usize;
    let mut line_width = 0usize;
    for (seen, ch) in text.chars().enumerate() {
        if seen >= cursor_offset {
            break;
        }
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1);
        if line_width + ch_width > width && line_width > 0 {
            line += 1;
            line_width = 0;
        }
        line_width += ch_width;
    }
    (line, line_width)
}

fn wrap_with_prefix(
    text: &str,
    prefix: &str,
    max_width: u16,
    prefix_style: Style,
    content_style: Style,
) -> Vec<Line<'static>> {
    let prefix_width = UnicodeWidthStr::width(prefix) as u16;
    let available = max_width.saturating_sub(prefix_width).max(1) as usize;
    wrap_preserving_spaces(text, available)
        .into_iter()
        .map(|segment| {
            Line::from(vec![
                Span::styled(prefix.to_string(), prefix_style),
                Span::styled(segment, content_style),
            ])
        })
        .collect()
}

fn wrap_with_prefix_limited(
    text: &str,
    prefix: &str,
    max_width: u16,
    prefix_style: Style,
    content_style: Style,
    max_lines: usize,
) -> Vec<Line<'static>> {
    let prefix_width = UnicodeWidthStr::width(prefix) as u16;
    let available = max_width.saturating_sub(prefix_width).max(1) as usize;
    let mut wrapped = wrap_preserving_spaces(text, available);
    if wrapped.len() > max_lines {
        wrapped.truncate(max_lines);
        if let Some(last) = wrapped.last_mut() {
            truncate_line_with_ellipsis(last, available);
        }
    }

    wrapped
        .into_iter()
        .map(|segment| {
            Line::from(vec![
                Span::styled(prefix.to_string(), prefix_style),
                Span::styled(segment, content_style),
            ])
        })
        .collect()
}

fn truncate_line_with_ellipsis(line: &mut String, max_width: usize) {
    if max_width == 0 {
        line.clear();
        line.push('…');
        return;
    }

    let current_width = UnicodeWidthStr::width(line.as_str());
    if current_width >= max_width {
        let mut truncated = String::new();
        let limit = max_width.saturating_sub(1);
        let mut width = 0usize;
        for ch in line.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + ch_width > limit {
                break;
            }
            truncated.push(ch);
            width += ch_width;
        }
        *line = truncated;
    }

    if !line.ends_with('…') {
        line.push('…');
    }
}

fn truncate_line(line: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(line) <= max_width {
        return line.to_string();
    }

    let mut owned = line.to_string();
    truncate_line_with_ellipsis(&mut owned, max_width);
    owned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::{FieldKind, FieldSchema};
    use crate::tui::state::FieldState;
    use serde_json::Value;

    fn field_with_value(value: &str) -> FieldState {
        FieldState::from_schema(FieldSchema {
            name: "unicode".into(),
            path: vec!["unicode".into()],
            pointer: "/unicode".into(),
            title: "Unicode".into(),
            description: None,
            kind: FieldKind::String,
            required: false,
            default: Some(Value::String(value.to_string())),
            metadata: Default::default(),
        })
    }

    #[test]
    fn wrap_preserving_spaces_keeps_wide_characters() {
        let wrapped = wrap_preserving_spaces("宽度🙂", 4);
        assert_eq!(wrapped, vec!["宽度".to_string(), "🙂".to_string()]);
    }

    #[test]
    fn cursor_hint_includes_border_and_highlight_width() {
        let field = field_with_value("汉字");
        let render = build_field_render(&field, true, 10, 12);
        let hint = render
            .cursor_hint
            .expect("cursor hint present for selected field");
        let highlight = highlight_symbol_width();
        let prefix = UnicodeWidthStr::width(VALUE_BORDER_PREFIX) as u16 + GUTTER_PADDING;
        let value_width = UnicodeWidthStr::width("汉字") as u16;
        let expected = LIST_BORDER_OFFSET + highlight + prefix + value_width;
        assert_eq!(hint.column_offset, expected);
        assert!(
            hint.line_in_field > 0,
            "cursor line should point to value content"
        );
    }
}
