//! Terminal-based web browser engine.
//!
//! Renders HTML content in a ratatui terminal view. Supports:
//! - Text content (headings, paragraphs, lists, code blocks)
//! - Links (keyboard-navigable, opens in system browser)
//! - Tables
//! - Forms (basic text input)
//! - Images (iTerm2/Kitty inline graphics protocol)
//! - Audio/Video (delegates to mpv/ffplay subprocess)
//!
//! Architecture:
//!   URL → tokio::spawn(fetch_html) → parse_dom → RenderTree → ratatui widgets
//!
//! Navigation: Tab/Shift+Tab to cycle links, Enter to follow, Esc to close.

pub mod parser;
pub mod renderer;
pub mod css;

use std::sync::{Arc, Mutex};

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

use self::parser::ParsedPage;
use self::renderer::RenderTree;

/// Shared mutable browser state behind an Arc<Mutex>.
struct BrowserState {
    url: String,
    page: Option<ParsedPage>,
    render_tree: RenderTree,
    scroll: usize,
    focused_link: Option<usize>,
    link_targets: Vec<String>,
    image_urls: Vec<String>,
    media_urls: Vec<String>,
    viewport_height: u16,
    error: Option<String>,
    loading: bool,
    cached_lines: Vec<Line<'static>>,
    /// Address bar input (when editing).
    address_input: String,
    /// Whether the address bar is actively being edited.
    editing_address: bool,
    /// Whether keyboard focus is on the address bar (vs page links).
    focus_address: bool,
}

/// A terminal-based web browser view.
pub struct TerminalBrowser {
    state: Arc<Mutex<BrowserState>>,
    graphics_protocol: GraphicsProtocol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsProtocol {
    None,
    ITerm2,
    Kitty,
    Sixel,
}

impl TerminalBrowser {
    pub fn new() -> Self {
        let protocol = detect_graphics_protocol();
        Self {
            state: Arc::new(Mutex::new(BrowserState {
                url: String::new(),
                page: None,
                render_tree: RenderTree::new(),
                scroll: 0,
                focused_link: None,
                link_targets: Vec::new(),
                image_urls: Vec::new(),
                media_urls: Vec::new(),
                viewport_height: 24,
                error: None,
                loading: false,
                cached_lines: Vec::new(),
                address_input: String::new(),
                editing_address: false,
                focus_address: false,
            })),
            graphics_protocol: protocol,
        }
    }

    /// Start loading a URL asynchronously. Does NOT block.
    pub fn start_load(&self, url: &str) {
        let url_owned = url.to_string();
        let state = self.state.clone();

        // Reset state
        let mut s = state.lock().unwrap();
        s.url = url_owned.clone();
        s.address_input = url_owned.clone();
        s.loading = true;
        s.error = None;
        s.scroll = 0;
        s.focused_link = None;
        s.link_targets.clear();
        s.image_urls.clear();
        s.media_urls.clear();
        s.cached_lines.clear();
        drop(s);

        // Spawn async task
        tokio::spawn(async move {
            match fetch_html(&url_owned).await {
                Ok(html) => {
                    let mut s = state.lock().unwrap();
                    match parser::parse_html(&html, &url_owned) {
                        Ok(page) => {
                            s.link_targets = page.link_targets.clone();
                            s.image_urls = page.image_urls.clone();
                            s.media_urls = page.media_urls.clone();
                            s.render_tree = renderer::build_render_tree(
                                &page,
                                s.viewport_height as usize,
                            );
                            s.page = Some(page);
                        }
                        Err(e) => {
                            s.error = Some(format!("Parse error: {e}"));
                        }
                    }
                    s.loading = false;
                    let proto = detect_graphics_protocol();
                    let focused = s.focused_link;
                    if let Some(ref page) = s.page {
                        s.cached_lines =
                            renderer::render_to_lines(page, focused, proto, s.viewport_height as usize);
                    }
                }
                Err(e) => {
                    let mut s = state.lock().unwrap();
                    s.error = Some(format!("Failed to fetch: {e}"));
                    s.loading = false;
                }
            }
        });
    }

    /// Handle a key event. Returns `Some(url)` if a link was activated.
    /// The bool indicates whether the key was consumed by the browser.
    #[must_use]
    pub fn handle_key(&self, key: KeyEvent) -> (Option<String>, bool) {
        let mut s = self.state.lock().unwrap();

        // Address bar editing mode
        if s.editing_address || s.focus_address {
            match key {
                KeyEvent::Char(c) => {
                    s.editing_address = true;
                    s.address_input.push(c);
                    return (None, true);
                }
                KeyEvent::Enter => {
                    let url = s.address_input.trim().to_string();
                    if !url.is_empty() {
                        s.editing_address = false;
                        s.focus_address = false;
                        s.address_input.clone_from(&url);
                        drop(s);
                        self.start_load(&url);
                        return (None, true);
                    }
                }
                KeyEvent::Esc => {
                    s.editing_address = false;
                    s.focus_address = false;
                    let url = s.url.clone();
                    s.address_input = url;
                }
                KeyEvent::Tab => {
                    s.editing_address = false;
                    s.focus_address = false;
                }
                _ => {}
            }
            return (None, true);
        }

        match key {
            KeyEvent::Tab => {
                if s.focus_address {
                    s.focus_address = false;
                    s.focused_link = None;
                } else if s.link_targets.is_empty() {
                    s.focus_address = true;
                    s.focused_link = None;
                } else {
                    s.next_link();
                }
            }
            KeyEvent::ShiftTab => {
                // Backward: if on links, prev_link; else focus address
                if s.focus_address {
                    s.focus_address = false;
                    s.focused_link = None;
                    if !s.link_targets.is_empty() {
                        s.prev_link();
                    }
                } else if s.focused_link.is_some() {
                    s.prev_link();
                } else {
                    s.focus_address = true;
                    s.focused_link = None;
                }
            }
            KeyEvent::Enter => {
                if s.focus_address { s.editing_address = true; return (None, true); }
                let url = s.activate_link();
                if let Some(u) = url {
                    return (Some(u), true);
                }
            }
            KeyEvent::Up => s.scroll_up(3),
            KeyEvent::Down => s.scroll_down(3),
            KeyEvent::PageUp => {
                let h = s.viewport_height;
                s.scroll_up(h.saturating_sub(2) as usize);
            }
            KeyEvent::PageDown => {
                let h = s.viewport_height;
                s.scroll_down(h.saturating_sub(2) as usize);
            }
            KeyEvent::Home => s.scroll = 0,
            KeyEvent::End => {
                let max = s.max_scroll();
                s.scroll = max;
            }
            KeyEvent::Esc => return (None, false),
            _ => { return (None, false); }
        }
        // Rebuild cached lines after navigation
        if let Some(ref page) = s.page {
            s.cached_lines =
                renderer::render_to_lines(page, s.focused_link, self.graphics_protocol, s.viewport_height as usize);
        }
        (None, true)
    }

    /// Render the browser view into a ratatui Frame.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let s = self.state.lock().unwrap();
        let content_bg = Color::Rgb(18, 18, 24);
        let bar_bg = Color::Rgb(30, 30, 40);

        let chunks = Layout::default().direction(Direction::Vertical).constraints([
            Constraint::Length(1), Constraint::Min(1), Constraint::Length(1),
        ]).split(area);

        // Address bar
        let addr_disp = if s.editing_address {
            format!(" ✎ {}", s.address_input)
        } else if s.focus_address {
            format!(" ▶ {}", s.url)
        } else {
            format!(" 🔒 {}", s.url)
        };
        let addr_text = if s.url.is_empty() { "about:blank".to_string() } else { addr_disp };
        f.render_widget(
            Paragraph::new(addr_text).style(Style::default().bg(bar_bg).fg(Color::White)),
            chunks[0],
        );

        // Content
        let cs = Style::default().bg(content_bg);
        if let Some(ref _page) = s.page {
            let vis = s.viewport_height.saturating_sub(3) as usize;
            let vl: Vec<Line> = s.cached_lines.iter().skip(s.scroll).take(vis).cloned().collect();
            f.render_widget(
                Paragraph::new(Text::from(vl)).block(Block::default().style(cs)).wrap(Wrap { trim: false }),
                chunks[1],
            );
        } else if let Some(ref err) = s.error {
            f.render_widget(
                Paragraph::new(format!("\n  ✗ Error: {err}\n\n  Press Esc to close")).block(Block::default().style(cs)).style(Style::default().fg(Color::Red)),
                chunks[1],
            );
        } else if s.loading {
            f.render_widget(
                Paragraph::new(format!("\n  ⏳ Loading {}...\n", s.url)).block(Block::default().style(cs)).style(Style::default().fg(Color::Gray)),
                chunks[1],
            );
        } else {
            f.render_widget(
                Paragraph::new("\n  Type /browse <url> to open a web page\n").block(Block::default().style(cs)).style(Style::default().fg(Color::DarkGray)),
                chunks[1],
            );
        }

        // Status bar
        let total = s.cached_lines.len();
        let vis = s.viewport_height.saturating_sub(2) as usize;
        let pct = if total > vis { ((s.scroll as f64 / (total - vis) as f64) * 100.0) as u32 } else { 100 };
        f.render_widget(
            Paragraph::new(format!(" {} links │ {pct}% │ Tab/↩ navigate  Esc close ", s.link_targets.len()))
                .style(Style::default().bg(bar_bg).fg(Color::Gray)),
            chunks[2],
        );
    }

    /// Open an image using the terminal graphics protocol.
    #[allow(dead_code)]
    pub fn display_image(&self, _url: &str) {}

    /// Play audio/video via mpv subprocess.
    #[allow(dead_code)]
    pub fn play_media(&self, url: &str) {
        let _ = std::process::Command::new("mpv")
            .arg(url)
            .arg("--really-quiet")
            .spawn();
    }
}

// ── Key event ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    Tab,
    ShiftTab,
    Enter,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Esc,
    Char(char),
}

// ── BrowserState helpers ─────────────────────────────────────────────

impl BrowserState {
    fn next_link(&mut self) {
        if self.link_targets.is_empty() { return; }
        let next = match self.focused_link {
            Some(i) if i + 1 < self.link_targets.len() => i + 1,
            _ => 0,
        };
        self.focused_link = Some(next);
    }

    fn prev_link(&mut self) {
        if self.link_targets.is_empty() { return; }
        let prev = match self.focused_link {
            Some(0) | None => self.link_targets.len().saturating_sub(1),
            Some(i) => i - 1,
        };
        self.focused_link = Some(prev);
    }

    fn activate_link(&self) -> Option<String> {
        let idx = self.focused_link?;
        self.link_targets.get(idx).cloned()
    }

    fn scroll_up(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_sub(lines);
    }

    fn scroll_down(&mut self, lines: usize) {
        let max = self.max_scroll();
        self.scroll = (self.scroll + lines).min(max);
    }

    fn max_scroll(&self) -> usize {
        let total = self.cached_lines.len();
        let visible = self.viewport_height.saturating_sub(2) as usize;
        total.saturating_sub(visible)
    }
}

// ── Graphics protocol detection ──────────────────────────────────────

fn detect_graphics_protocol() -> GraphicsProtocol {
    let term = std::env::var("TERM").unwrap_or_default();
    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();

    if term_program.contains("iTerm") || term_program.contains("WezTerm") {
        return GraphicsProtocol::ITerm2;
    }
    if term.contains("kitty") {
        return GraphicsProtocol::Kitty;
    }
    if term.contains("xterm") || term.contains("mlterm") {
        return GraphicsProtocol::Sixel;
    }
    GraphicsProtocol::None
}

// ── HTML fetching ────────────────────────────────────────────────────

async fn fetch_html(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("DeepSeek-TUI/0.8 TerminalBrowser")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("client build: {e}"))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HTTP: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    response
        .text()
        .await
        .map_err(|e| format!("body: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_new_is_empty() {
        let b = TerminalBrowser::new();
        let s = b.state.lock().unwrap();
        assert!(s.page.is_none());
        assert!(s.link_targets.is_empty());
        assert_eq!(s.scroll, 0);
    }

    #[test]
    fn navigation_wraps() {
        let b = TerminalBrowser::new();
        let mut s = b.state.lock().unwrap();
        s.link_targets = vec!["a".into(), "b".into(), "c".into()];
        s.next_link();
        assert_eq!(s.focused_link, Some(0));
        s.next_link();
        assert_eq!(s.focused_link, Some(1));
        s.next_link();
        assert_eq!(s.focused_link, Some(2));
        s.next_link();
        assert_eq!(s.focused_link, Some(0));
    }
}