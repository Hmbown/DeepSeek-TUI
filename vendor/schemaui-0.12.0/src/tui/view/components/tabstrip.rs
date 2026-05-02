use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Tabs},
};
use unicode_width::UnicodeWidthStr;

use crate::deepseek_palette::{ACCENT_SKY, BORDER_DIM, SURFACE_INK, TEXT_DIM, TEXT_MUTED};

pub(crate) fn render_tab_strip(
    frame: &mut Frame<'_>,
    area: Rect,
    titles: &[String],
    selected: usize,
    label: &str,
) {
    if titles.is_empty() {
        let placeholder = Block::default()
            .title(label)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_DIM))
            .style(Style::default().bg(SURFACE_INK));
        frame.render_widget(placeholder, area);
        return;
    }

    let labels: Vec<TabLabel> = titles.iter().cloned().map(TabLabel::new).collect();
    let total = labels.len();
    let clamped_selected = selected.min(total.saturating_sub(1));
    let available = area.width.saturating_sub(2) as usize; // borders consume 2 columns
    let window = compute_visible_window(&labels, clamped_selected, available);

    let indicator_style = Style::default().fg(TEXT_DIM).bg(SURFACE_INK);
    let mut visible = Vec::new();
    for (offset, label) in labels[window.start..window.end].iter().enumerate() {
        let absolute_index = window.start + offset;
        let mut spans = Vec::new();

        if window.left_overflow && absolute_index == window.start {
            spans.push(Span::styled(format!("{LEFT_CHEVRON} "), indicator_style));
        } else {
            spans.push(Span::raw(" ".to_string()));
        }

        spans.push(Span::raw(label.text.clone()));

        if window.right_overflow && absolute_index == window.end.saturating_sub(1) {
            spans.push(Span::styled(format!(" {RIGHT_CHEVRON}"), indicator_style));
        } else {
            spans.push(Span::raw(" ".to_string()));
        }

        visible.push(Line::from(spans));
    }

    let tabs = Tabs::new(visible)
        .block(
            Block::default()
                .title(label)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_DIM))
                .style(Style::default().bg(SURFACE_INK)),
        )
        .select(window.selected_offset)
        .style(Style::default().fg(TEXT_MUTED).bg(SURFACE_INK))
        .highlight_style(
            Style::default()
                .fg(ACCENT_SKY)
                .bg(SURFACE_INK)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, area);
}

#[derive(Clone)]
struct TabLabel {
    text: String,
    width: usize,
}

impl TabLabel {
    fn new(text: String) -> Self {
        // Width includes the label text plus padding and room for potential
        // overflow chevrons that are rendered at the tab edges. Using a
        // slightly conservative estimate here avoids packing more tabs into
        // the window than can actually be displayed, which previously caused
        // rightmost entries to be truncated.
        let width = UnicodeWidthStr::width(text.as_str()) + TAB_EXTRA_PADDING;
        Self { text, width }
    }
}

// Base padding for each tab label. This is intentionally a bit larger than
// the raw text padding to account for the leading/trailing spaces and
// overflow chevrons ("<<"/">>") that may be rendered for the first/last
// visible tabs.
const TAB_EXTRA_PADDING: usize = 4;
const LEFT_CHEVRON: &str = "<<";
const RIGHT_CHEVRON: &str = ">>";

#[derive(Debug, Clone, Copy)]
struct TabWindow {
    start: usize,
    end: usize,
    selected_offset: usize,
    left_overflow: bool,
    right_overflow: bool,
}

fn compute_visible_window(labels: &[TabLabel], selected: usize, available: usize) -> TabWindow {
    if labels.is_empty() {
        return TabWindow {
            start: 0,
            end: 0,
            selected_offset: 0,
            left_overflow: false,
            right_overflow: false,
        };
    }
    let selected = selected.min(labels.len() - 1);
    if available == 0 {
        return TabWindow {
            start: selected,
            end: selected + 1,
            selected_offset: 0,
            left_overflow: selected > 0,
            right_overflow: selected + 1 < labels.len(),
        };
    }
    let total_width: usize = labels.iter().map(|label| label.width).sum();
    if total_width <= available {
        return TabWindow {
            start: 0,
            end: labels.len(),
            selected_offset: selected,
            left_overflow: false,
            right_overflow: false,
        };
    }
    let selected_width = labels[selected].width;
    if selected_width > available {
        return TabWindow {
            start: selected,
            end: selected + 1,
            selected_offset: 0,
            left_overflow: selected > 0,
            right_overflow: true,
        };
    }

    let mut start = selected;
    let mut end = selected + 1;
    let mut width = selected_width;

    loop {
        let mut expanded = false;
        if start > 0 {
            let extra = labels[start - 1].width;
            if width + extra <= available {
                start -= 1;
                width += extra;
                continue;
            }
        }
        if end < labels.len() {
            let extra = labels[end].width;
            if width + extra <= available {
                width += extra;
                end += 1;
                expanded = true;
            }
        }
        if !expanded {
            break;
        }
    }

    TabWindow {
        start,
        end,
        selected_offset: selected - start,
        left_overflow: start > 0,
        right_overflow: end < labels.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_keeps_selected_visible() {
        let labels = vec![
            TabLabel::new("one".into()),
            TabLabel::new("two".into()),
            TabLabel::new("three".into()),
            TabLabel::new("four".into()),
        ];
        let window = compute_visible_window(&labels, 2, 12);
        assert!(window.start <= 2 && window.end > 2);
        assert!(
            window.right_overflow,
            "should show overflow indicator to the right"
        );
        assert!(window.end <= labels.len());
        assert_eq!(window.selected_offset, 2 - window.start);
    }

    #[test]
    fn window_handles_narrow_width() {
        let labels = vec![TabLabel::new("alpha".into()), TabLabel::new("beta".into())];
        let window = compute_visible_window(&labels, 1, 4);
        assert_eq!(window.start, 1);
        assert_eq!(window.end, 2);
        assert!(window.left_overflow);
        assert_eq!(window.selected_offset, 0);
    }

    #[test]
    fn window_spans_all_tabs_when_width_available() {
        let labels = vec![
            TabLabel::new("svc".into()),
            TabLabel::new("db".into()),
            TabLabel::new("metrics".into()),
        ];
        let total_width: usize = labels.iter().map(|label| label.width).sum();
        let window = compute_visible_window(&labels, 1, total_width);
        assert_eq!(window.start, 0);
        assert_eq!(window.end, labels.len());
        assert_eq!(window.selected_offset, 1);
        assert!(
            !window.left_overflow && !window.right_overflow,
            "indicators should be hidden when tabs fit"
        );
    }

    #[test]
    fn window_scrolls_when_selection_moves_past_visible_range() {
        let labels = vec![
            TabLabel::new("alpha".into()),
            TabLabel::new("beta".into()),
            TabLabel::new("gamma".into()),
            TabLabel::new("delta".into()),
            TabLabel::new("epsilon".into()),
        ];
        let available = labels.iter().take(3).map(|label| label.width).sum();
        let window = compute_visible_window(&labels, 4, available);
        assert!(
            window.start > 0,
            "window should scroll once the selection moves beyond the initial range"
        );
    }

    #[test]
    fn window_avoids_unneeded_scroll_when_space_is_sufficient() {
        let labels = vec![
            TabLabel::new("root section1".into()),
            TabLabel::new("root section2".into()),
            TabLabel::new("root section3".into()),
            TabLabel::new("root section4".into()),
            TabLabel::new("root section5".into()),
        ];
        let available = labels.iter().take(4).map(|label| label.width).sum();
        let window = compute_visible_window(&labels, 3, available);
        assert_eq!(window.start, 0, "first tab should stay visible");
        assert_eq!(
            window.end, 4,
            "window should fill the available width with fully rendered tabs"
        );
        assert!(
            window.right_overflow,
            "overflow indicator should remain for the hidden tab"
        );
        assert!(
            !window.left_overflow,
            "no left indicator expected when the first tab is visible"
        );
    }

    #[test]
    fn window_expands_after_resize() {
        let labels = vec![
            TabLabel::new("alpha".into()),
            TabLabel::new("beta".into()),
            TabLabel::new("gamma".into()),
            TabLabel::new("delta".into()),
        ];
        let narrow = labels.iter().take(2).map(|label| label.width).sum();
        let wide = labels.iter().take(3).map(|label| label.width).sum();
        let narrow_window = compute_visible_window(&labels, 2, narrow);
        assert_eq!(narrow_window.start, 1);
        assert_eq!(narrow_window.end, 3);
        let wide_window = compute_visible_window(&labels, 2, wide);
        assert_eq!(wide_window.start, 0, "resize should reveal the first tab");
        assert_eq!(wide_window.end, 3, "wider windows include more tabs");
    }
}
