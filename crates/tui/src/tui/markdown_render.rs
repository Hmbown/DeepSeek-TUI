//! Markdown rendering for TUI transcript lines.
//!
//! ## Width-independent parse vs width-dependent render (CX#6)
//!
//! The previous renderer was a single function `render_markdown(content, width)`
//! that scanned the source, classified each line (heading / list / code-fence /
//! paragraph / link), and word-wrapped to `Line<'static>` in one pass. That meant
//! every terminal resize forced a full re-parse of the source for every visible
//! cell — wasted work on the streaming cell whose content is changing anyway.
//!
//! The codex tui solves this by splitting parse from render. We mirror that:
//!
//! * [`parse`] turns the markdown source into a [`ParsedMarkdown`] AST: a vector
//!   of width-independent [`Block`]s. The block kind already records all the
//!   classification decisions (heading level, list bullet, code block membership)
//!   that don't depend on width.
//! * [`render_parsed`] takes a `ParsedMarkdown` plus a width and a base style and
//!   produces `Vec<Line<'static>>`. It only does word-wrap and span styling.
//!
//! [`render_markdown`] is kept as a thin convenience that does both — useful for
//! callers (Thinking body, message body) that don't want to manage the cache.
//!
//! The transcript cache layer (see `tui/transcript.rs`) caches the parsed AST per
//! cell and re-runs only the render step on width changes. That makes resize a
//! re-flow operation rather than a re-parse + re-flow operation.

#[cfg(test)]
use std::cell::Cell;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::palette;
use crate::tui::osc8;

// Thread-local counter incremented every time `parse` runs. Used by tests to
// prove that width-only changes hit the cached-AST path and skip parsing.
// Thread-local (not global atomic) so concurrent tests calling `parse()` can't
// pollute each other's counters.
#[cfg(test)]
thread_local! {
    static PARSE_INVOCATIONS: Cell<u64> = const { Cell::new(0) };
}

#[cfg(test)]
#[must_use]
pub fn parse_invocation_count() -> u64 {
    PARSE_INVOCATIONS.with(|c| c.get())
}

#[cfg(test)]
pub fn reset_parse_invocation_count() {
    PARSE_INVOCATIONS.with(|c| c.set(0));
}

/// One classified line of markdown source, width-independent.
///
/// All decisions that depend only on the source text (heading level, bullet
/// kind, whether we're inside a fenced code block, paragraph text) are made at
/// parse time. Width-dependent layout (word-wrap, prefix indent) is deferred to
/// the render step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `# heading text`. Includes the heading level (1..6).
    Heading { level: usize, text: String },
    /// A horizontal rule emitted under a level-1 heading.
    HeadingRule,
    /// A bullet (`-`/`*`) or ordered (`1.`) list item with its prefix and body.
    ListItem { bullet: String, text: String },
    /// A line inside a fenced code block. Fences themselves are dropped.
    Code { line: String },
    /// A non-empty paragraph line that may contain inline links.
    Paragraph { text: String },
    /// An empty source line, preserved so paragraph spacing survives.
    Blank,
}

/// Width-independent parsed-markdown AST for one cell's source.
///
/// Wrapped in `Arc` at the cache layer so the cache can hand the same AST to
/// many render calls without copying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMarkdown {
    blocks: Vec<Block>,
}

/// Parse markdown source into a width-independent block AST.
///
/// This is a small line-oriented parser tuned for the patterns we render:
/// fenced code blocks, ATX headings, dash/star/numbered list items, and plain
/// paragraphs with optional links. It does not attempt to handle every CommonMark
/// edge case — that's intentional. The renderer will treat anything we don't
/// classify as `Block::Paragraph`.
#[must_use]
pub fn parse(content: &str) -> ParsedMarkdown {
    #[cfg(test)]
    PARSE_INVOCATIONS.with(|c| c.set(c.get() + 1));

    let mut blocks = Vec::new();
    let mut in_code_block = false;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            blocks.push(Block::Code {
                line: raw_line.to_string(),
            });
            continue;
        }

        if let Some((level, text)) = parse_heading(trimmed) {
            blocks.push(Block::Heading {
                level,
                text: text.to_string(),
            });
            if level == 1 {
                blocks.push(Block::HeadingRule);
            }
            continue;
        }

        if let Some((bullet, text)) = parse_list_item(trimmed) {
            blocks.push(Block::ListItem {
                bullet,
                text: text.to_string(),
            });
            continue;
        }

        if raw_line.is_empty() {
            blocks.push(Block::Blank);
            continue;
        }

        blocks.push(Block::Paragraph {
            text: trimmed.to_string(),
        });
    }

    ParsedMarkdown { blocks }
}

/// Render a parsed-markdown AST at the given terminal width.
///
/// This is the width-dependent half: word-wrapping, link styling, code-block
/// formatting. The AST is owned by the caller (typically the transcript cache),
/// so width-only changes can call `render_parsed` again with the same AST and
/// skip the parse step entirely.
#[must_use]
pub fn render_parsed(parsed: &ParsedMarkdown, width: u16, base_style: Style) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut out: Vec<Line<'static>> = Vec::with_capacity(parsed.blocks.len());

    for block in &parsed.blocks {
        match block {
            Block::Heading { text, .. } => {
                let style = Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .add_modifier(Modifier::BOLD);
                out.extend(render_wrapped_line(text, width, style, false));
            }
            Block::HeadingRule => {
                out.push(Line::from(Span::styled(
                    "─".repeat(width.min(40)),
                    Style::default().fg(palette::TEXT_DIM),
                )));
            }
            Block::ListItem { bullet, text } => {
                let bullet_style = Style::default().fg(palette::DEEPSEEK_SKY);
                out.extend(render_list_line(
                    bullet,
                    text,
                    width,
                    bullet_style,
                    base_style,
                ));
            }
            Block::Code { line } => {
                let code_style = Style::default()
                    .fg(palette::DEEPSEEK_SKY)
                    .add_modifier(Modifier::ITALIC);
                out.extend(render_wrapped_line(line, width, code_style, true));
            }
            Block::Paragraph { text } => {
                let link_style = Style::default()
                    .fg(palette::DEEPSEEK_BLUE)
                    .add_modifier(Modifier::UNDERLINED);
                out.extend(render_line_with_links(text, width, base_style, link_style));
            }
            Block::Blank => {
                // Preserve paragraph spacing. The original renderer also pushed
                // a blank line for empty source lines that fell through the
                // paragraph branch; mirror that exactly.
                out.push(Line::from(""));
            }
        }
    }

    if out.is_empty() {
        out.push(Line::from(""));
    }

    out
}

/// Convenience wrapper: parse + render in one call.
///
/// Equivalent to `render_parsed(&parse(content), width, base_style)`. Callers
/// that don't manage their own cache (the Thinking body, the immediate message
/// body) use this.
#[must_use]
pub fn render_markdown(content: &str, width: u16, base_style: Style) -> Vec<Line<'static>> {
    let parsed = parse(content);
    render_parsed(&parsed, width, base_style)
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 {
        return None;
    }
    let text = trimmed[hashes..].trim();
    if text.is_empty() {
        None
    } else {
        Some((hashes, text))
    }
}

fn parse_list_item(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return Some(("-".to_string(), trimmed[2..].trim()));
    }
    let bytes = trimmed.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == 0 || idx >= bytes.len() || bytes[idx] != b'.' {
        return None;
    }
    let rest = &trimmed[idx + 1..];
    if !rest.starts_with(' ') {
        return None;
    }
    Some((format!("{}.", &trimmed[..idx]), rest.trim_start()))
}

fn render_wrapped_line(
    line: &str,
    width: usize,
    style: Style,
    indent_code: bool,
) -> Vec<Line<'static>> {
    let prefix = if indent_code { "  " } else { "" };
    let prefix_width = prefix.width();
    let available = width.saturating_sub(prefix_width).max(1);
    let wrapped = wrap_text(line, available);
    let mut out = Vec::new();

    for (idx, chunk) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            out.push(Line::from(vec![
                Span::raw(prefix),
                Span::styled(chunk, style),
            ]));
        } else {
            out.push(Line::from(vec![
                Span::raw(" ".repeat(prefix_width)),
                Span::styled(chunk, style),
            ]));
        }
    }

    out
}

fn render_list_line(
    bullet: &str,
    text: &str,
    width: usize,
    bullet_style: Style,
    text_style: Style,
) -> Vec<Line<'static>> {
    let bullet_prefix = format!("{bullet} ");
    let bullet_width = bullet_prefix.width();
    let available = width.saturating_sub(bullet_width).max(1);
    let wrapped = render_line_with_links(text, available, text_style, link_style());

    let mut out = Vec::new();
    for (idx, line) in wrapped.into_iter().enumerate() {
        if idx == 0 {
            let mut spans = vec![Span::styled(bullet_prefix.clone(), bullet_style)];
            spans.extend(line.spans);
            out.push(Line::from(spans));
        } else {
            let mut spans = vec![Span::raw(" ".repeat(bullet_width))];
            spans.extend(line.spans);
            out.push(Line::from(spans));
        }
    }
    out
}

fn render_line_with_links(
    line: &str,
    width: usize,
    base_style: Style,
    link_style: Style,
) -> Vec<Line<'static>> {
    if line.trim().is_empty() {
        return vec![Line::from("")];
    }

    let mut lines = Vec::new();
    let mut current_spans: Vec<Span> = Vec::new();
    let mut current_width = 0usize;

    for word in line.split_whitespace() {
        let is_link = looks_like_link(word);
        let style = if is_link { link_style } else { base_style };
        let word_width = word.width();
        let additional = if current_width == 0 {
            word_width
        } else {
            word_width + 1
        };

        if current_width + additional > width && !current_spans.is_empty() {
            lines.push(Line::from(current_spans));
            current_spans = Vec::new();
            current_width = 0;
        }

        if current_width > 0 {
            current_spans.push(Span::raw(" "));
            current_width += 1;
        }

        // For URLs, wrap the visible text in OSC 8 escapes when the runtime
        // flag allows it. Display width is computed from the bare URL — the
        // escapes are zero-width on supporting terminals and ignored on the
        // rest. The clipboard / selection path strips OSC 8 before yanking.
        let content = if is_link && osc8::enabled() {
            osc8::wrap_link(word, word)
        } else {
            word.to_string()
        };
        current_spans.push(Span::styled(content, style));
        current_width += word_width;
    }

    if !current_spans.is_empty() {
        lines.push(Line::from(current_spans));
    }

    lines
}

fn looks_like_link(word: &str) -> bool {
    word.starts_with("http://") || word.starts_with("https://")
}

fn link_style() -> Style {
    Style::default()
        .fg(palette::DEEPSEEK_BLUE)
        .add_modifier(Modifier::UNDERLINED)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for word in text.split_whitespace() {
        let word_width = word.width();
        let additional = if current.is_empty() {
            word_width
        } else {
            word_width + 1
        };
        if current_width + additional > width && !current.is_empty() {
            lines.push(current);
            current = word.to_string();
            current_width = word_width;
        } else {
            if !current.is_empty() {
                current.push(' ');
                current_width += 1;
            }
            current.push_str(word);
            current_width += word_width;
        }
    }

    if current.is_empty() {
        lines.push(String::new());
    } else {
        lines.push(current);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn collect_text(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn render_markdown_matches_parse_then_render() {
        // The convenience wrapper must produce byte-identical output to the
        // explicit two-step path. Without this guarantee the transcript cache
        // and the live render diverge.
        let source = "# Title\n\nA paragraph with a https://example.com link.\n\n- one\n- two\n```\ncode\n```";
        let direct = render_markdown(source, 40, Style::default());
        let parsed = parse(source);
        let two_step = render_parsed(&parsed, 40, Style::default());
        assert_eq!(collect_text(&direct), collect_text(&two_step));
    }

    #[test]
    fn parse_is_width_independent() {
        // Same source, two parses, must produce identical AST. (Sanity:
        // parse must not depend on hidden global state like terminal width.)
        let source = "Hello\n\n## Heading\n- list\n";
        let a = parse(source);
        let b = parse(source);
        assert_eq!(a, b);
    }

    #[test]
    fn render_parsed_word_wrap_changes_with_width() {
        // The same AST must produce different layouts at different widths;
        // otherwise the split is decorative, not functional.
        let parsed = parse("alpha beta gamma delta epsilon zeta");
        let wide = render_parsed(&parsed, 80, Style::default());
        let narrow = render_parsed(&parsed, 10, Style::default());
        assert!(
            narrow.len() > wide.len(),
            "narrow should produce more lines"
        );
    }

    #[test]
    fn parse_invocations_increment() {
        // Counter is thread-local, so concurrent tests calling `parse()`
        // can't pollute each other.
        reset_parse_invocation_count();
        let _ = parse("hello\n");
        let _ = parse("world\n");
        assert_eq!(parse_invocation_count(), 2);
    }

    #[test]
    fn render_parsed_does_not_call_parse() {
        // Width-only changes must hit only the render path. This is the
        // perf invariant CX#6 was filed for.
        let parsed = parse("multiline\nsource\nwith several\nlines\n");
        reset_parse_invocation_count();
        let _ = render_parsed(&parsed, 80, Style::default());
        let _ = render_parsed(&parsed, 40, Style::default());
        let _ = render_parsed(&parsed, 20, Style::default());
        assert_eq!(
            parse_invocation_count(),
            0,
            "render_parsed must not call parse"
        );
    }

    #[test]
    fn fenced_code_block_collected_in_parse() {
        let parsed = parse("text\n```\ncode line one\ncode line two\n```\nmore\n");
        let blocks = &parsed.blocks;
        // text paragraph, two code lines, more paragraph (fences are dropped)
        let code_lines: Vec<_> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Code { line } => Some(line.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(code_lines, vec!["code line one", "code line two"]);
    }

    #[test]
    fn ordered_and_unordered_list_items_parse() {
        let parsed = parse("- alpha\n* beta\n1. gamma\n");
        let items: Vec<_> = parsed
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::ListItem { bullet, text } => Some((bullet.as_str(), text.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(items, vec![("-", "alpha"), ("-", "beta"), ("1.", "gamma")]);
    }

    /// Render with the OSC 8 flag pinned to `enabled`, then restore the prior
    /// value. We serialize through a static mutex because `osc8::ENABLED` is
    /// process-wide state and other tests touching it would race otherwise.
    fn render_with_osc8(enabled: bool, source: &str) -> String {
        use std::sync::Mutex;
        static OSC8_GUARD: Mutex<()> = Mutex::new(());
        let _guard = OSC8_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let prior = osc8::enabled();
        osc8::set_enabled(enabled);
        let lines = render_markdown(source, 80, Style::default());
        let joined = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect::<String>();
        osc8::set_enabled(prior);
        joined
    }

    #[test]
    fn http_links_get_osc_8_wrapped_when_enabled() {
        let joined = render_with_osc8(true, "see https://example.com for details");
        assert!(
            joined.contains("\x1b]8;;https://example.com\x1b\\https://example.com\x1b]8;;\x1b\\"),
            "expected OSC 8 wrapper around URL; got {joined:?}"
        );
    }

    #[test]
    fn osc_8_disabled_emits_plain_url() {
        let joined = render_with_osc8(false, "see https://example.com for details");
        assert!(
            !joined.contains("\x1b]8;;"),
            "expected no OSC 8 wrapper when disabled; got {joined:?}"
        );
        assert!(joined.contains("https://example.com"));
    }
}
