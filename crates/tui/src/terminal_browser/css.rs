//! CSS parser for the terminal browser.
//!
//! Parses `<style>` blocks and inline `style="..."` attributes into a
//! cascade of computed styles. Maps CSS properties to terminal capabilities:
//! - `color` / `background-color` → ratatui fg/bg (true color via RGB)
//! - `font-weight` → bold
//! - `text-decoration` → underline
//! - `font-style` → italic (terminal-dependent)
//! - `text-align` → left/center/right
//! - `display: none` → skip rendering
//!
//! Selector support: tag (h1, p, ...), .class, #id, and compound selectors
//! with basic specificity calculation (ID > class > tag).

use std::collections::HashMap;

/// Default browser user-agent stylesheet, applied before any page CSS.
/// Mirrors the default styling that real browsers apply to HTML elements.
static DEFAULT_USER_AGENT_CSS: &str = r#"
html, body { color: #e0e0e0; background-color: #121218; }
h1 { color: #ffffff; font-weight: bold; }
h2 { color: #e8e8e8; font-weight: bold; }
h3 { color: #d0d0d0; font-weight: bold; }
h4, h5, h6 { color: #c0c0c0; }
p { color: #d0d0d0; }
a { color: #4da6ff; text-decoration: underline; }
code, pre { color: #a0ffa0; background-color: #1a1a2e; }
blockquote { color: #a0a0a0; }
li { color: #d0d0d0; }
strong, b { font-weight: bold; }
em, i { font-style: italic; }
table { color: #d0d0d0; }
th { font-weight: bold; color: #e0e0e0; }
hr { color: #404050; }
"#;

// ── CSS Data Structures ────────────────────────────────────────────

/// A computed style for a single element.
#[derive(Debug, Clone, Default)]
pub struct ComputedStyle {
    pub fg: Option<RgbColor>,
    pub bg: Option<RgbColor>,
    pub bold: Option<bool>,
    pub underline: Option<bool>,
    pub italic: Option<bool>,
    pub align: Option<TextAlign>,
    pub display: Option<Display>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    None,
}

/// A parsed CSS rule with selector and declarations.
#[derive(Debug, Clone)]
struct CssRule {
    selector: Selector,
    declarations: HashMap<String, String>,
}

/// A CSS selector with specificity.
#[derive(Debug, Clone)]
struct Selector {
    parts: Vec<SelectorPart>,
    specificity: u32,
}

#[derive(Debug, Clone)]
struct SelectorPart {
    tag: Option<String>,
    class: Option<String>,
    id: Option<String>,
}

// ── CSS Engine ──────────────────────────────────────────────────────

/// Context for CSS resolution. Builds from `<style>` blocks and
/// provides computed styles for elements by matching selectors.
#[derive(Debug, Clone)]
pub struct CssContext {
    rules: Vec<CssRule>,
}

impl CssContext {
    pub fn new() -> Self {
        let mut ctx = Self { rules: Vec::new() };
        // Inject default browser stylesheet (user-agent style)
        ctx.add_style_block(DEFAULT_USER_AGENT_CSS);
        ctx
    }

    /// Feed a `<style>` block into the context.
    pub fn add_style_block(&mut self, css: &str) {
        self.rules.extend(parse_css_rules(css));
    }

    /// Resolve the computed style for an element.
    pub fn resolve(
        &self,
        tag: Option<&str>,
        class: Option<&str>,
        id: Option<&str>,
        inline_style: Option<&str>,
    ) -> ComputedStyle {
        let mut style = ComputedStyle::default();

        // 1. Apply matching rules by specificity (low to high).
        let mut matching: Vec<&CssRule> = self
            .rules
            .iter()
            .filter(|r| r.selector.matches(tag, class, id))
            .collect();
        matching.sort_by_key(|r| r.selector.specificity);

        for rule in &matching {
            apply_declarations(&mut style, &rule.declarations);
        }

        // 2. Inline style overrides everything.
        if let Some(inline) = inline_style {
            let inline_decls = parse_inline_style(inline);
            apply_declarations(&mut style, &inline_decls);
        }

        style
    }
}

impl Default for CssContext {
    fn default() -> Self {
        Self::new()
    }
}

// ── CSS Parser ──────────────────────────────────────────────────────

/// Parse a raw CSS string (from `<style>` block) into `CssRule`s.
fn parse_css_rules(css: &str) -> Vec<CssRule> {
    let mut rules = Vec::new();

    // Remove comments
    let css = strip_css_comments(css);

    for block in css.split('}') {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        let mut parts = block.splitn(2, '{');
        let selector_text = parts.next().unwrap_or("").trim();
        let decl_text = parts.next().unwrap_or("").trim();

        if selector_text.is_empty() || decl_text.is_empty() {
            continue;
        }

        // Handle comma-separated selectors
        for sel in selector_text.split(',') {
            let sel = sel.trim();
            if sel.is_empty() {
                continue;
            }

            let selector = parse_selector(sel);
            let declarations = parse_declarations(decl_text);

            if !declarations.is_empty() {
                rules.push(CssRule {
                    selector,
                    declarations,
                });
            }
        }
    }

    rules
}

/// Parse inline `style="..."` into a declarations map.
pub fn parse_inline_style(s: &str) -> HashMap<String, String> {
    parse_declarations(s)
}

// ── Selector Parsing ────────────────────────────────────────────────

fn parse_selector(text: &str) -> Selector {
    let text = text.trim();
    let mut parts = Vec::new();
    let mut specificity: u32 = 0;

    // Split compound selector like "h1.title#main" into parts
    let mut current = String::new();
    let mut i = 0;
    let chars: Vec<char> = text.chars().collect();

    while i < chars.len() {
        match chars[i] {
            '.' => {
                if !current.is_empty() {
                    flush_selector_part(&mut parts, &current, &mut specificity);
                    current.clear();
                }
                i += 1;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '_') {
                    current.push(chars[i]);
                    i += 1;
                }
                if !current.is_empty() {
                    parts.push(SelectorPart {
                        tag: None,
                        class: Some(current.clone()),
                        id: None,
                    });
                    specificity += 10; // class specificity
                    current.clear();
                }
            }
            '#' => {
                if !current.is_empty() {
                    flush_selector_part(&mut parts, &current, &mut specificity);
                    current.clear();
                }
                i += 1;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '-' || chars[i] == '_') {
                    current.push(chars[i]);
                    i += 1;
                }
                if !current.is_empty() {
                    parts.push(SelectorPart {
                        tag: None,
                        class: None,
                        id: Some(current.clone()),
                    });
                    specificity += 100; // id specificity
                    current.clear();
                }
            }
            ' ' => {
                if !current.is_empty() {
                    flush_selector_part(&mut parts, &current, &mut specificity);
                    current.clear();
                }
                i += 1;
                // Descendant combinator: add all previous parts' specificity to new parts
                // Simplified: just start fresh specificity for descendant
            }
            _ => {
                current.push(chars[i]);
                i += 1;
            }
        }
    }

    if !current.is_empty() {
        flush_selector_part(&mut parts, &current, &mut specificity);
    }

    if parts.is_empty() {
        // Empty selector — shouldn't happen but create a wildcard
        parts.push(SelectorPart {
            tag: Some("*".to_string()),
            class: None,
            id: None,
        });
        specificity = 0;
    }

    Selector { parts, specificity }
}

fn flush_selector_part(parts: &mut Vec<SelectorPart>, raw: &str, specificity: &mut u32) {
    let tag = raw.trim().to_lowercase();
    if tag.is_empty() || tag == "*" {
        // Wildcard: treated as a universal part with 0 specificity
        return;
    }
    parts.push(SelectorPart {
        tag: if is_html_tag(&tag) { Some(tag) } else { None },
        class: None,
        id: None,
    });
    *specificity += 1; // tag specificity
}

fn is_html_tag(s: &str) -> bool {
    matches!(
        s,
        "html" | "body" | "head" | "title" | "div" | "span" | "p"
            | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
            | "a" | "img" | "ul" | "ol" | "li" | "table" | "tr" | "td" | "th"
            | "pre" | "code" | "blockquote" | "hr" | "br"
            | "section" | "article" | "nav" | "header" | "footer" | "main"
            | "aside" | "figure" | "figcaption" | "details" | "summary"
            | "form" | "input" | "button" | "label" | "textarea" | "select"
            | "video" | "audio" | "canvas" | "svg"
            | "em" | "strong" | "b" | "i" | "u" | "s" | "small" | "mark"
            | "sub" | "sup" | "del" | "ins"
            | "dl" | "dt" | "dd" | "fieldset" | "legend" | "iframe"
            | "script" | "style" | "noscript" | "meta" | "link"
    )
}

// ── Selector Matching ───────────────────────────────────────────────

impl Selector {
    fn matches(&self, tag: Option<&str>, class: Option<&str>, id: Option<&str>) -> bool {
        let tag = tag.map(|t| t.to_lowercase());
        let class = class.map(|c| c.to_lowercase());
        let id = id.map(|i| i.to_lowercase());

        for part in &self.parts {
            if let Some(ref pt) = part.tag {
                if let Some(ref t) = tag {
                    if pt != t {
                        return false;
                    }
                }
            }
            if let Some(ref pc) = part.class {
                if let Some(ref c) = class {
                    // Simple match: class contains pc (not full word matching)
                    if !c.split_whitespace().any(|wc| wc == pc.as_str()) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            if let Some(ref pi) = part.id {
                if let Some(ref i) = id {
                    if pi != i {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        }

        !self.parts.is_empty()
    }
}

// ── Declaration Parsing ─────────────────────────────────────────────

fn parse_declarations(text: &str) -> HashMap<String, String> {
    let mut decls = HashMap::new();

    for decl in text.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }

        let mut parts = decl.splitn(2, ':');
        let prop = parts.next().unwrap_or("").trim().to_lowercase();
        let value = parts.next().unwrap_or("").trim().to_string();

        if prop.is_empty() || value.is_empty() {
            continue;
        }

        decls.insert(prop, value);
    }

    decls
}

fn apply_declarations(style: &mut ComputedStyle, decls: &HashMap<String, String>) {
    for (prop, value) in decls {
        match prop.as_str() {
            "color" => {
                if let Some(c) = parse_color(value) {
                    style.fg = Some(c);
                }
            }
            "background-color" | "background" => {
                if let Some(c) = parse_color(value) {
                    style.bg = Some(c);
                }
            }
            "font-weight" => {
                style.bold = Some(parse_font_weight(value));
            }
            "text-decoration" => {
                style.underline = Some(value.contains("underline"));
            }
            "font-style" => {
                style.italic = Some(value == "italic" || value == "oblique");
            }
            "text-align" => {
                style.align = Some(match value.as_str() {
                    "center" => TextAlign::Center,
                    "right" => TextAlign::Right,
                    _ => TextAlign::Left,
                });
            }
            "display" => {
                style.display = Some(match value.as_str() {
                    "none" => Display::None,
                    "inline" | "inline-block" => Display::Inline,
                    _ => Display::Block,
                });
            }
            _ => {}
        }
    }
}

// ── Color Parsing ───────────────────────────────────────────────────

fn parse_color(s: &str) -> Option<RgbColor> {
    let s = s.trim().to_lowercase();

    // Named colors
    if let Some(c) = NAMED_COLORS.get(s.as_str()) {
        return Some(*c);
    }

    // #rgb / #rrggbb
    if s.starts_with('#') {
        let hex = &s[1..];
        return match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                Some(RgbColor { r, g, b })
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(RgbColor { r, g, b })
            }
            _ => None,
        };
    }

    // rgb(r, g, b)
    if s.starts_with("rgb(") && s.ends_with(')') {
        let inner = &s[4..s.len() - 1];
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 3 {
            let r = parts[0].trim().parse::<u8>().ok()?;
            let g = parts[1].trim().parse::<u8>().ok()?;
            let b = parts[2].trim().parse::<u8>().ok()?;
            return Some(RgbColor { r, g, b });
        }
    }

    // rgba(r, g, b, a) — ignore alpha
    if s.starts_with("rgba(") && s.ends_with(')') {
        let inner = &s[5..s.len() - 1];
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() >= 3 {
            let r = parts[0].trim().parse::<u8>().ok()?;
            let g = parts[1].trim().parse::<u8>().ok()?;
            let b = parts[2].trim().parse::<u8>().ok()?;
            return Some(RgbColor { r, g, b });
        }
    }

    None
}

fn parse_font_weight(s: &str) -> bool {
    match s.trim().to_lowercase().as_str() {
        "bold" | "bolder" => true,
        "normal" | "lighter" => false,
        n => {
            if let Ok(num) = n.parse::<u32>() {
                num >= 600
            } else {
                false
            }
        }
    }
}

fn strip_css_comments(css: &str) -> String {
    let mut result = String::with_capacity(css.len());
    let chars: Vec<char> = css.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2; // skip */
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

// ── Named Colors ─────────────────────────────────────────────────────

static NAMED_COLORS: std::sync::LazyLock<HashMap<&'static str, RgbColor>> =
    std::sync::LazyLock::new(|| {
        let mut m = HashMap::new();
        // CSS 1/2 basic colors
        m.insert("black", RgbColor { r: 0, g: 0, b: 0 });
        m.insert("white", RgbColor { r: 255, g: 255, b: 255 });
        m.insert("red", RgbColor { r: 255, g: 0, b: 0 });
        m.insert("green", RgbColor { r: 0, g: 128, b: 0 });
        m.insert("blue", RgbColor { r: 0, g: 0, b: 255 });
        m.insert("yellow", RgbColor { r: 255, g: 255, b: 0 });
        m.insert("cyan", RgbColor { r: 0, g: 255, b: 255 });
        m.insert("magenta", RgbColor { r: 255, g: 0, b: 255 });
        m.insert("gray", RgbColor { r: 128, g: 128, b: 128 });
        m.insert("grey", RgbColor { r: 128, g: 128, b: 128 });
        m.insert("silver", RgbColor { r: 192, g: 192, b: 192 });
        m.insert("maroon", RgbColor { r: 128, g: 0, b: 0 });
        m.insert("purple", RgbColor { r: 128, g: 0, b: 128 });
        m.insert("fuchsia", RgbColor { r: 255, g: 0, b: 255 });
        m.insert("lime", RgbColor { r: 0, g: 255, b: 0 });
        m.insert("olive", RgbColor { r: 128, g: 128, b: 0 });
        m.insert("navy", RgbColor { r: 0, g: 0, b: 128 });
        m.insert("teal", RgbColor { r: 0, g: 128, b: 128 });
        m.insert("aqua", RgbColor { r: 0, g: 255, b: 255 });
        m.insert("orange", RgbColor { r: 255, g: 165, b: 0 });
        m.insert("transparent", RgbColor { r: 0, g: 0, b: 0 }); // handled specially
        // CSS 3 extended colors (common subset)
        m.insert("darkgray", RgbColor { r: 169, g: 169, b: 169 });
        m.insert("darkgrey", RgbColor { r: 169, g: 169, b: 169 });
        m.insert("dimgray", RgbColor { r: 105, g: 105, b: 105 });
        m.insert("dimgrey", RgbColor { r: 105, g: 105, b: 105 });
        m.insert("lightgray", RgbColor { r: 211, g: 211, b: 211 });
        m.insert("lightgrey", RgbColor { r: 211, g: 211, b: 211 });
        m.insert("lightblue", RgbColor { r: 173, g: 216, b: 230 });
        m.insert("lightgreen", RgbColor { r: 144, g: 238, b: 144 });
        m.insert("lightyellow", RgbColor { r: 255, g: 255, b: 224 });
        m.insert("lightcyan", RgbColor { r: 224, g: 255, b: 255 });
        m.insert("darkblue", RgbColor { r: 0, g: 0, b: 139 });
        m.insert("darkgreen", RgbColor { r: 0, g: 100, b: 0 });
        m.insert("darkred", RgbColor { r: 139, g: 0, b: 0 });
        m.insert("darkcyan", RgbColor { r: 0, g: 139, b: 139 });
        m.insert("darkmagenta", RgbColor { r: 139, g: 0, b: 139 });
        m.insert("darkorange", RgbColor { r: 255, g: 140, b: 0 });
        m.insert("darkviolet", RgbColor { r: 148, g: 0, b: 211 });
        m.insert("indigo", RgbColor { r: 75, g: 0, b: 130 });
        m.insert("crimson", RgbColor { r: 220, g: 20, b: 60 });
        m.insert("tomato", RgbColor { r: 255, g: 99, b: 71 });
        m.insert("coral", RgbColor { r: 255, g: 127, b: 80 });
        m.insert("salmon", RgbColor { r: 250, g: 128, b: 114 });
        m.insert("gold", RgbColor { r: 255, g: 215, b: 0 });
        m.insert("khaki", RgbColor { r: 240, g: 230, b: 140 });
        m.insert("plum", RgbColor { r: 221, g: 160, b: 221 });
        m.insert("violet", RgbColor { r: 238, g: 130, b: 238 });
        m.insert("orchid", RgbColor { r: 218, g: 112, b: 214 });
        m.insert("beige", RgbColor { r: 245, g: 245, b: 220 });
        m.insert("bisque", RgbColor { r: 255, g: 228, b: 196 });
        m.insert("wheat", RgbColor { r: 245, g: 222, b: 179 });
        m.insert("tan", RgbColor { r: 210, g: 180, b: 140 });
        m.insert("brown", RgbColor { r: 165, g: 42, b: 42 });
        m.insert("sienna", RgbColor { r: 160, g: 82, b: 45 });
        m.insert("chocolate", RgbColor { r: 210, g: 105, b: 30 });
        m.insert("peru", RgbColor { r: 205, g: 133, b: 63 });
        m
    });

// ── Helpers for ratatui conversion ──────────────────────────────────

impl ComputedStyle {
    /// Convert to a ratatui `Style`, applying overrides.
    pub fn to_ratatui(&self, base: ratatui::style::Style) -> ratatui::style::Style {
        let mut s = base;

        if let Some(c) = self.fg {
            s = s.fg(ratatui::style::Color::Rgb(c.r, c.g, c.b));
        }
        if let Some(c) = self.bg {
            s = s.bg(ratatui::style::Color::Rgb(c.r, c.g, c.b));
        }
        if self.bold == Some(true) {
            s = s.add_modifier(ratatui::style::Modifier::BOLD);
        }
        if self.underline == Some(true) {
            s = s.add_modifier(ratatui::style::Modifier::UNDERLINED);
        }
        if self.italic == Some(true) {
            s = s.add_modifier(ratatui::style::Modifier::ITALIC);
        }

        s
    }

    /// Convert RgbColor to ratatui Color.
    #[allow(dead_code)]
    pub fn fg_color(&self) -> Option<ratatui::style::Color> {
        self.fg.map(|c| ratatui::style::Color::Rgb(c.r, c.g, c.b))
    }

    #[allow(dead_code)]
    pub fn bg_color(&self) -> Option<ratatui::style::Color> {
        self.bg.map(|c| ratatui::style::Color::Rgb(c.r, c.g, c.b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rgb_color() {
        let c = parse_color("#ff0044").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 68);
    }

    #[test]
    fn parse_named_color() {
        let c = parse_color("red").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn parse_rgba() {
        let c = parse_color("rgba(10, 20, 30, 0.5)").unwrap();
        assert_eq!(c.r, 10);
        assert_eq!(c.g, 20);
        assert_eq!(c.b, 30);
    }

    #[test]
    fn parse_short_hex() {
        let c = parse_color("#f04").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 68);
    }

    #[test]
    fn parse_simple_rule() {
        let css = "h1 { color: red; font-weight: bold; }";
        let rules = parse_css_rules(css);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector.parts[0].tag.as_deref(), Some("h1"));
        assert_eq!(rules[0].declarations.get("color").unwrap(), "red");
    }

    #[test]
    fn match_tag_selector() {
        let css = "p { color: blue; }";
        let ctx = { let mut c = CssContext::new(); c.add_style_block(css); c };
        let style = ctx.resolve(Some("p"), None, None, None);
        assert!(style.fg.is_some());
    }

    #[test]
    fn match_class_selector() {
        let css = ".highlight { color: yellow; }";
        let ctx = { let mut c = CssContext::new(); c.add_style_block(css); c };
        let style = ctx.resolve(Some("span"), Some("highlight"), None, None);
        assert!(style.fg.is_some());
    }

    #[test]
    fn inline_style_overrides() {
        let css = "p { color: blue; }";
        let ctx = { let mut c = CssContext::new(); c.add_style_block(css); c };
        let style = ctx.resolve(Some("p"), None, None, Some("color: red"));
        let c = style.fg.unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn specificity_ordering() {
        let css = "p { color: blue; } p.highlight { color: green; } #main { color: red; }";
        let ctx = { let mut c = CssContext::new(); c.add_style_block(css); c };
        // #main has highest specificity
        let style = ctx.resolve(Some("p"), Some("highlight"), Some("main"), None);
        let c = style.fg.unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
    }
}
