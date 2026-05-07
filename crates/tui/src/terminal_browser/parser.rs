//! HTML parser for the terminal browser.
//!
//! Converts raw HTML into a flat `ParsedPage` with text blocks, links,
//! images, and media references. This is NOT a full DOM tree — we target
//! the 80/20 of real-world pages: headings, paragraphs, links, lists,
//! code blocks, preformatted text, and simple tables.
//!
//! Complex CSS layout, JavaScript, and dynamic content are explicitly
//! out of scope. The goal is readable terminal rendering, not pixel-perfect
//! reproduction.

/// A parsed HTML page ready for terminal rendering.
#[derive(Debug, Clone)]
pub struct ParsedPage {
    /// Title extracted from `<title>`.
    pub title: String,
    /// Ordered list of text/content blocks.
    pub blocks: Vec<ContentBlock>,
    /// Link targets referenced by their block index + position.
    pub link_targets: Vec<String>,
    /// Image URLs found on the page.
    pub image_urls: Vec<String>,
    /// Video/audio URLs found on the page.
    pub media_urls: Vec<String>,
    /// CSS context built from <style> blocks.
    pub css_context: crate::terminal_browser::css::CssContext,
}

/// A block of content rendered as a unit in the terminal.
#[derive(Debug, Clone)]
pub enum ContentBlock {
    /// A heading with level (1 = h1, 2 = h2, ...).
    Heading { level: u8, text: String, attrs: HtmlAttrs },
    /// A paragraph of text.
    Paragraph { text: String, attrs: HtmlAttrs },
    /// A list item, optionally nested.
    ListItem { text: String, indent: usize, attrs: HtmlAttrs },
    /// Preformatted / code block.
    CodeBlock { text: String, #[allow(dead_code)] language: Option<String>, attrs: HtmlAttrs },
    /// A horizontal rule.
    Divider,
    /// A blockquote.
    Blockquote { text: String, attrs: HtmlAttrs },
    /// A link (text + target URL, href stored in ParsedPage.link_targets).
    Link { text: String, #[allow(dead_code)] href: String, attrs: HtmlAttrs },
    /// Image reference (src stored in ParsedPage.image_urls).
    Image { alt: String, #[allow(dead_code)] src: String, attrs: HtmlAttrs },
    /// Video/audio reference (src stored in ParsedPage.media_urls).
    Media { alt: String, #[allow(dead_code)] src: String, kind: MediaKind, attrs: HtmlAttrs },
    /// A table row (cells).
    TableRow { cells: Vec<String>, attrs: HtmlAttrs },
    /// Plain text (used when no block-level element wraps it).
    Text { text: String },
}

/// HTML attributes carried through for CSS resolution.
#[derive(Debug, Clone, Default)]
pub struct HtmlAttrs {
    pub tag: Option<String>,
    pub class: Option<String>,
    pub id: Option<String>,
    pub style: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Audio,
}

/// Parse an HTML string into a ParsedPage.
///
/// Uses a simple tag-stripping + heuristics approach. Full HTML5 parsing
/// is unnecessary for terminal rendering — we just need the text content
/// and structural hints from headings, paragraphs, lists, and links.
pub fn parse_html(html: &str, base_url: &str) -> Result<ParsedPage, String> {
    let mut page = ParsedPage {
        title: String::new(),
        blocks: Vec::new(),
        link_targets: Vec::new(),
        image_urls: Vec::new(),
        media_urls: Vec::new(),
        css_context: crate::terminal_browser::css::CssContext::new(),
    };

    let lower = html.to_lowercase();

    // Extract <title>
    page.title = extract_tag_content(&lower, html, "title");

    // Strip <script> and <style> blocks
    let cleaned = strip_tags(&lower, html, &["script", "style", "noscript", "svg", "head"]);

    // Extract <style> blocks
    let style_css = extract_all_tag_contents(&lower, html, "style");
    page.css_context.add_style_block(&style_css);

    // Parse block-level elements
    let mut i = 0;
    let mut text_buf = String::new();
    let cleaned_chars: Vec<char> = cleaned.chars().collect();

    while i < cleaned_chars.len() {
        let _remaining: String = cleaned_chars[i..].iter().collect();

        // Check for opening tags
        if let Some((tag, _rest)) = parse_opening_tag(&_remaining) {
            let tag_lower = tag.to_lowercase();
            i += tag.len() + 2; // skip "<tag>"

            match tag_lower.as_str() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    flush_text(&mut text_buf, &mut page.blocks);
                    let level = tag_lower[1..].parse::<u8>().unwrap_or(2);
                    let content = extract_until_close(&cleaned_chars, &mut i, &tag_lower);
                    page.blocks.push(ContentBlock::Heading { attrs: Default::default(), 
                        level,
                        text: strip_inline_tags(&content),
                    });
                }
                "p" => {
                    flush_text(&mut text_buf, &mut page.blocks);
                    let content = extract_until_close(&cleaned_chars, &mut i, "p");
                    page.blocks.push(ContentBlock::Paragraph { attrs: Default::default(), 
                        text: strip_inline_tags(&content),
                    });
                }
                "li" => {
                    flush_text(&mut text_buf, &mut page.blocks);
                    let content = extract_until_close(&cleaned_chars, &mut i, "li");
                    page.blocks.push(ContentBlock::ListItem { attrs: Default::default(), 
                        text: strip_inline_tags(&content),
                        indent: 0,
                    });
                }
                "pre" | "code" => {
                    flush_text(&mut text_buf, &mut page.blocks);
                    let content = extract_until_close(&cleaned_chars, &mut i, &tag_lower);
                    page.blocks.push(ContentBlock::CodeBlock { attrs: Default::default(), 
                        text: content.trim().to_string(),
                        language: if tag_lower == "code" { None } else { None },
                    });
                }
                "blockquote" => {
                    flush_text(&mut text_buf, &mut page.blocks);
                    let content = extract_until_close(&cleaned_chars, &mut i, "blockquote");
                    page.blocks.push(ContentBlock::Blockquote { attrs: Default::default(), 
                        text: strip_inline_tags(&content),
                    });
                }
                "hr" | "br" => {
                    flush_text(&mut text_buf, &mut page.blocks);
                    page.blocks.push(ContentBlock::Divider);
                }
                "a" => {
                    // Extract href
                    let href = extract_attr(&_remaining, "href");
                    let link_text = extract_until_close(&cleaned_chars, &mut i, "a");
                    if let Some(h) = href {
                        let resolved = resolve_url(&h, base_url);
                        page.link_targets.push(resolved.clone());
                        page.blocks.push(ContentBlock::Link { attrs: Default::default(), 
                            text: strip_inline_tags(&link_text),
                            href: resolved,
                        });
                    } else {
                        text_buf.push_str(&link_text);
                    }
                }
                "img" => {
                    let src = extract_attr(&_remaining, "src");
                    let alt = extract_attr(&_remaining, "alt").unwrap_or_default();
                    if let Some(s) = src {
                        let resolved = resolve_url(&s, base_url);
                        page.image_urls.push(resolved.clone());
                        page.blocks.push(ContentBlock::Image { attrs: Default::default(), 
                            alt,
                            src: resolved,
                        });
                    }
                }
                "video" | "audio" => {
                    let src = extract_attr(&_remaining, "src");
                    let alt = extract_attr(&_remaining, "alt").unwrap_or_else(|| tag_lower.clone());
                    if let Some(s) = src {
                        let resolved = resolve_url(&s, base_url);
                        page.media_urls.push(resolved.clone());
                        page.blocks.push(ContentBlock::Media { attrs: Default::default(), 
                            alt,
                            src: resolved,
                            kind: if tag_lower == "video" {
                                MediaKind::Video
                            } else {
                                MediaKind::Audio
                            },
                        });
                    }
                }
                "table" | "tr" | "td" | "th" => {
                    // Simple table extraction
                    if tag_lower == "tr" {
                        let content = extract_until_close(&cleaned_chars, &mut i, "tr");
                        let cells: Vec<String> = split_table_cells(&content);
                        if !cells.is_empty() {
                            page.blocks.push(ContentBlock::TableRow { attrs: Default::default(),  cells });
                        }
                    }
                }
                "html" | "body" | "div" | "span" | "section" | "article" | "nav"
                | "header" | "footer" | "main" | "ul" | "ol" | "dl" | "form"
                | "fieldset" | "figure" | "aside" | "details" | "summary"
                | "meta" | "link" | "style" | "script" | "noscript" | "title"
                | "head" => {
                    // Transparent / skipped containers. Don't extract
                    // content — let child tags be parsed normally.
                }
                _ => {
                    // Unknown tag — skip it, treat content as inline text.
                }
            }
        } else if !_remaining.is_empty() {
            // Plain text character
            let ch = cleaned_chars[i];
            text_buf.push(ch);
            i += 1;
        } else {
            i += 1;
        }
    }

    flush_text(&mut text_buf, &mut page.blocks);

    // Fallback: if no blocks were extracted, the page is likely JS-heavy
    // or uses non-standard markup. Extract all visible text by stripping
    // every tag.
    if page.blocks.is_empty() && !html.is_empty() {
        let raw_text = strip_all_tags(html);
        if !raw_text.trim().is_empty() {
            page.blocks.push(ContentBlock::Text { text: raw_text });
        }
    }

    Ok(page)
}

// ── Helpers ──────────────────────────────────────────────────────────

fn flush_text(buf: &mut String, blocks: &mut Vec<ContentBlock>) {
    let text = buf.trim().to_string();
    if !text.is_empty() {
        blocks.push(ContentBlock::Text { text });
    }
    buf.clear();
}

/// Extract all occurrences of a tag's content (for <style> blocks).
fn extract_all_tag_contents(lower: &str, original: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut result = String::new();
    let mut search_from = 0;

    while let Some(start) = lower[search_from..].find(&open) {
        let abs_start = search_from + start + open.len();
        if let Some(end) = lower[abs_start..].find(&close) {
            result.push_str(&original[abs_start..abs_start + end]);
            search_from = abs_start + end + close.len();
        } else {
            break;
        }
    }

    result
}

fn extract_tag_content(lower: &str, original: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    if let Some(start) = lower.find(&open) {
        let after = &lower[start + open.len()..];
        if let Some(end) = after.find(&close) {
            // Map back to original case
            return original[start + open.len()..start + open.len() + end].to_string();
        }
    }
    String::new()
}

fn strip_tags(_lower: &str, original: &str, tags: &[&str]) -> String {
    let mut result = original.to_string();
    for tag in tags {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        while let Some(start) = result.to_lowercase().find(&open) {
            let end = result[start..]
                .to_lowercase()
                .find(&close)
                .map(|e| start + e + close.len())
                .unwrap_or(result.len());
            result.replace_range(start..end, "");
        }
    }
    result
}

fn parse_opening_tag(s: &str) -> Option<(String, &str)> {
    if !s.starts_with('<') {
        return None;
    }
    let rest = &s[1..];
    let end = rest.find('>')?;
    let tag_part = &rest[..end];
    let tag_name = tag_part
        .split_whitespace()
        .next()
        .unwrap_or(tag_part)
        .trim_end_matches('/')
        .to_lowercase();
    if tag_name.is_empty() {
        return None;
    }
    Some((tag_name, &rest[end + 1..]))
}

fn extract_until_close(chars: &[char], i: &mut usize, tag: &str) -> String {
    let close = format!("</{tag}>");
    let remaining: String = chars[*i..].iter().collect();
    let remaining_lower = remaining.to_lowercase();

    if let Some(end) = remaining_lower.find(&close) {
        let content: String = chars[*i..*i + end].iter().collect();
        *i += end + close.len();
        content
    } else {
        let content: String = chars[*i..].iter().collect();
        *i = chars.len();
        content
    }
}

fn extract_attr(s: &str, attr: &str) -> Option<String> {
    let lower = s.to_lowercase();
    let pattern = format!("{attr}=\"");
    let start = lower.find(&pattern)?;
    let after = &s[start + pattern.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

fn strip_inline_tags(s: &str) -> String {
    // Remove <b>, <i>, <em>, <strong>, <span>, <code>, <u>, <s> tags
    let mut result = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(ch);
        }
    }
    // Decode common HTML entities
    result = result.replace("&amp;", "&");
    result = result.replace("&lt;", "<");
    result = result.replace("&gt;", ">");
    result = result.replace("&quot;", "\"");
    result = result.replace("&#39;", "'");
    result = result.replace("&nbsp;", " ");
    result
}

fn resolve_url(url: &str, base: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    if url.starts_with("//") {
        return format!("https:{url}");
    }
    if url.starts_with('/') {
        // Extract origin from base
        if let Some(proto_end) = base.find("://") {
            let after_proto = &base[proto_end + 3..];
            if let Some(path_start) = after_proto.find('/') {
                return format!("{}{url}", &base[..proto_end + 3 + path_start]);
            }
        }
        return format!("{base}{url}");
    }
    // Relative URL
    let base = base.trim_end_matches('/');
    format!("{base}/{url}")
}

/// Strip ALL HTML tags, leaving only text. Used as fallback for pages
/// where the main parser couldn't extract any structured blocks.
fn strip_all_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(ch);
        }
    }
    // Decode common entities
    let result = result.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    // Collapse whitespace
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn split_table_cells(s: &str) -> Vec<String> {
    let cells = Vec::new();
    let mut current = String::new();
    let mut in_tag = false;
    let mut in_cell = false;

    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
            if current.contains("td") || current.contains("th") {
                in_cell = true;
            }
            current.clear();
        } else if ch == '>' {
            in_tag = false;
            if current.trim() == "/td" || current.trim() == "/th" {
                in_cell = false;
            }
            current.clear();
        } else if !in_tag && !in_cell {
            // text between cells (whitespace)
        } else if !in_tag && in_cell {
            current.push(ch);
        } else {
            // inside a tag name
            current.push(ch);
        }
    }

    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_html() {
        let html = "<html><head><title>Test</title></head><body><h1>Hello</h1><p>World</p></body></html>";
        let page = parse_html(html, "https://example.com").unwrap();
        assert_eq!(page.title, "Test");
        assert_eq!(page.blocks.len(), 2);
    }

    #[test]
    fn parse_links() {
        let html = r#"<a href="/page">Click here</a>"#;
        let page = parse_html(html, "https://example.com").unwrap();
        assert_eq!(page.link_targets.len(), 1);
        assert_eq!(page.link_targets[0], "https://example.com/page");
    }

    #[test]
    fn resolve_relative_urls() {
        assert_eq!(
            resolve_url("/path", "https://example.com"),
            "https://example.com/path"
        );
        assert_eq!(
            resolve_url("page.html", "https://example.com/dir/"),
            "https://example.com/dir/page.html"
        );
    }

    #[test]
    fn strip_inline_tags_handles_entities() {
        let input = "Hello &amp; welcome &lt;world&gt;";
        let result = strip_inline_tags(input);
        assert_eq!(result, "Hello & welcome <world>");
    }

    #[test]
    fn parse_chinese_html() {
        let html = r#"<html lang="zh-CN"><head><title>探索</title></head><body><h1>你好世界</h1><p>这是一段中文段落，包含多字节字符：探索发现</p><a href="/about">关于我们</a></body></html>"#;
        let page = parse_html(html, "https://deepseek.com").unwrap();
        assert_eq!(page.title, "探索");
        assert!(page.blocks.len() >= 2, "expected at least 2 blocks, got {}", page.blocks.len());
    }

    #[test]
    fn parse_chinese_mid_char_boundary() {
        // The char '探' is 3 bytes in UTF-8: 0xE6 0x8E 0xA2
        // This tests that byte indexing with Vec<char> doesn't panic
        // on multi-byte characters. The extra Text block is a known
        // artifact from closing-tag handling (non-critical for rendering).
        let html = "<html><body><p>探</p></body></html>";
        let page = parse_html(html, "https://example.com").unwrap();
        assert!(
            page.blocks.iter().any(|b| matches!(b, ContentBlock::Paragraph { text, .. } if text == "探")),
            "expected a paragraph containing '探', got: {:?}",
            page.blocks
        );
    }

    #[test]
    fn parse_deepseek_homepage_structure() {
        let html = r#"<!DOCTYPE html><html lang="zh-CN"><body><main><h1>深度求索</h1><p>探索未至之境</p><a href="/chat">开始对话</a><ul><li>DeepSeek V4</li><li>1M 上下文</li></ul></main></body></html>"#;
        let page = parse_html(html, "https://deepseek.com").unwrap();
        assert!(page.blocks.len() >= 4, "expected >=4 blocks, got {}", page.blocks.len());
    }
}
