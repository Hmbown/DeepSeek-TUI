//! Terminal renderer for the browser engine.
//!
//! Converts a `ParsedPage` into ratatui `Line`s with CSS-applied styling.
//! - Resolves `<style>` blocks and inline `style="..."` via `CssContext`
//! - CSS `color` / `background-color` → ratatui fg/bg (true color)
//! - CSS `font-weight` → bold, `text-decoration` → underline
//! - Browser-style link indicators and focus highlighting

use super::parser::{ContentBlock, ParsedPage};
use super::GraphicsProtocol;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct RenderTree {}
impl RenderTree {
    pub fn new() -> Self { Self {} }
}

pub fn build_render_tree(_page: &ParsedPage, _width: usize) -> RenderTree {
    RenderTree::new()
}

/// Render a ParsedPage into ratatui `Line`s.
/// `max_width` is the available content width (usually viewport width - borders).
pub fn render_to_lines(
    page: &ParsedPage,
    focused_link: Option<usize>,
    graphics: GraphicsProtocol,
    max_width: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    let mut link_idx: usize = 0;

    let css = &page.css_context;

    // Title
    if !page.title.is_empty() {
        let title_style = css.resolve(Some("title"), None, None, None).to_ratatui(
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        );
        lines.push(Line::from(vec![
            Span::styled(format!("══ {} ══", page.title), title_style),
        ]));
        lines.push(Line::from(""));
    }

    for block in &page.blocks {
        match block {
            ContentBlock::Heading { level, text, attrs } => {
                let s = css.resolve(
                    attrs.tag.as_deref(),
                    attrs.class.as_deref(),
                    attrs.id.as_deref(),
                    attrs.style.as_deref(),
                );
                let base = match level {
                    1 => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    2 => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    3 => Style::default().fg(Color::Cyan),
                    _ => Style::default().fg(Color::White),
                };
                let style = s.to_ratatui(base);
                let prefix = match level { 1 => "# ", 2 => "## ", 3 => "### ", _ => "  " };
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled(prefix.to_string(), style),
                    Span::styled(text.clone(), style),
                ]));
            }

            ContentBlock::Paragraph { text, attrs } => {
                let s = css.resolve(
                    attrs.tag.as_deref(),
                    attrs.class.as_deref(),
                    attrs.id.as_deref(),
                    attrs.style.as_deref(),
                );
                let base = Style::default().fg(Color::White);
                let style = s.to_ratatui(base);
                for line in wrap_text(text, max_width) {
                    lines.push(Line::from(vec![Span::styled(line, style)]));
                }
            }

            ContentBlock::ListItem { text, indent, attrs } => {
                let s = css.resolve(
                    attrs.tag.as_deref(),
                    attrs.class.as_deref(),
                    attrs.id.as_deref(),
                    attrs.style.as_deref(),
                );
                let style = s.to_ratatui(Style::default());
                let indent_str = "  ".repeat(*indent);
                lines.push(Line::from(vec![
                    Span::styled(format!("{indent_str}• "), style),
                    Span::styled(text.clone(), style),
                ]));
            }

            ContentBlock::CodeBlock { text, language: _, attrs } => {
                let s = css.resolve(
                    attrs.tag.as_deref(),
                    attrs.class.as_deref(),
                    attrs.id.as_deref(),
                    attrs.style.as_deref(),
                );
                let style = s.to_ratatui(
                    Style::default().bg(Color::DarkGray).fg(Color::White),
                );
                lines.push(Line::from(""));
                for code_line in text.lines() {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  │ {code_line}"), style),
                    ]));
                }
                lines.push(Line::from(""));
            }

            ContentBlock::Divider => {
                lines.push(Line::from("─".repeat(max_width)));
            }

            ContentBlock::Blockquote { text, attrs } => {
                let s = css.resolve(
                    attrs.tag.as_deref(),
                    attrs.class.as_deref(),
                    attrs.id.as_deref(),
                    attrs.style.as_deref(),
                );
                let style = s.to_ratatui(Style::default().fg(Color::Gray));
                for line in wrap_text(text, max_width.saturating_sub(4)) {
                    lines.push(Line::from(vec![
                        Span::styled("▌ ", Style::default().fg(Color::Cyan)),
                        Span::styled(line, style),
                    ]));
                }
            }

            ContentBlock::Link { text, href: _, attrs } => {
                let s = css.resolve(
                    attrs.tag.as_deref(),
                    attrs.class.as_deref(),
                    attrs.id.as_deref(),
                    attrs.style.as_deref(),
                );
                let is_focused = focused_link == Some(link_idx);
                let base = if is_focused {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED)
                };
                let style = s.to_ratatui(base);
                let marker = if is_focused { " ▶ " } else { "   " };
                lines.push(Line::from(vec![
                    Span::raw(marker),
                    Span::styled(format!("[{link_idx}] "), Style::default().fg(Color::DarkGray)),
                    Span::styled(text.clone(), style),
                ]));
                link_idx += 1;
            }

            ContentBlock::Image { alt, src: _, attrs } => {
                let s = css.resolve(
                    attrs.tag.as_deref(),
                    attrs.class.as_deref(),
                    attrs.id.as_deref(),
                    attrs.style.as_deref(),
                );
                let style = s.to_ratatui(Style::default().fg(Color::Magenta));
                if graphics != GraphicsProtocol::None {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  [IMG] {alt}"), style),
                        Span::styled(" (press 'v')", Style::default().fg(Color::DarkGray)),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  [IMG: {alt}]"), style),
                    ]));
                }
            }

            ContentBlock::Media { alt, src: _, kind, attrs } => {
                let s = css.resolve(
                    attrs.tag.as_deref(),
                    attrs.class.as_deref(),
                    attrs.id.as_deref(),
                    attrs.style.as_deref(),
                );
                let style = s.to_ratatui(Style::default().fg(Color::Green));
                let icon = match kind {
                    super::parser::MediaKind::Video => "🎬",
                    super::parser::MediaKind::Audio => "🎵",
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {icon} {alt}"), style),
                    Span::styled(" (press 'p')", Style::default().fg(Color::DarkGray)),
                ]));
            }

            ContentBlock::TableRow { cells, attrs } => {
                let s = css.resolve(
                    attrs.tag.as_deref(),
                    attrs.class.as_deref(),
                    attrs.id.as_deref(),
                    attrs.style.as_deref(),
                );
                let style = s.to_ratatui(Style::default());
                let joined = cells.iter().map(|c| c.trim()).collect::<Vec<_>>().join(" │ ");
                lines.push(Line::from(vec![Span::styled(format!("  {joined}"), style)]));
            }

            ContentBlock::Text { text } => {
                for line in wrap_text(text, max_width) {
                    lines.push(Line::from(line));
                }
            }
        }
    }

    lines
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut result = Vec::new();
    let trimmed = text.trim();
    if trimmed.is_empty() { return result; }

    for paragraph in trimmed.split('\n') {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.len() + word.len() + 1 > max_width && !current.is_empty() {
                result.push(current.trim().to_string());
                current = String::new();
            }
            if !current.is_empty() { current.push(' '); }
            current.push_str(word);
        }
        if !current.is_empty() { result.push(current); }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_basic_page() {
        let page = ParsedPage {
            title: "Test".into(),
            blocks: vec![
                ContentBlock::Heading { level: 1, text: "Hello".into(), attrs: Default::default() },
                ContentBlock::Paragraph { text: "World".into(), attrs: Default::default() },
            ],
            link_targets: vec![],
            image_urls: vec![],
            media_urls: vec![],
            css_context: crate::terminal_browser::css::CssContext::new(),
        };
        let lines = render_to_lines(&page, None, GraphicsProtocol::None, 72);
        assert!(lines.len() >= 3);
    }

    #[test]
    fn wrap_text_preserves_words() {
        let text = "The quick brown fox jumps over the lazy dog";
        let lines = wrap_text(text, 20);
        for line in &lines { assert!(line.len() <= 20); }
    }
}
