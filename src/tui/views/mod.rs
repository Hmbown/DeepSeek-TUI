use crossterm::event::KeyEvent;
use ratatui::{buffer::Buffer, layout::Rect};
use std::fmt;

use crate::palette;
use crate::tools::UserInputResponse;
use crate::tools::subagent::{SubAgentResult, SubAgentStatus, SubAgentType};
use crate::tui::approval::{ElevationOption, ReviewDecision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    Approval,
    Elevation,
    UserInput,
    CommandPalette,
    Help,
    SubAgents,
    Pager,
    SessionPicker,
}

#[derive(Debug, Clone)]
pub enum ViewEvent {
    CommandPaletteSelected {
        command: String,
    },
    OpenTextPager {
        title: String,
        content: String,
    },
    ApprovalDecision {
        tool_id: String,
        tool_name: String,
        decision: ReviewDecision,
        timed_out: bool,
    },
    ElevationDecision {
        tool_id: String,
        tool_name: String,
        option: ElevationOption,
    },
    UserInputSubmitted {
        tool_id: String,
        response: UserInputResponse,
    },
    UserInputCancelled {
        tool_id: String,
    },
    SubAgentsRefresh,
    SessionSelected {
        session_id: String,
    },
    SessionDeleted {
        session_id: String,
        title: String,
    },
}

#[derive(Debug, Clone)]
pub enum ViewAction {
    None,
    Close,
    Emit(ViewEvent),
    EmitAndClose(ViewEvent),
}

pub trait ModalView {
    fn kind(&self) -> ModalKind;
    fn handle_key(&mut self, key: KeyEvent) -> ViewAction;
    fn render(&self, area: Rect, buf: &mut Buffer);
    fn update_subagents(&mut self, _agents: &[SubAgentResult]) -> bool {
        false
    }
    fn tick(&mut self) -> ViewAction {
        ViewAction::None
    }
}

#[derive(Default)]
pub struct ViewStack {
    views: Vec<Box<dyn ModalView>>,
}

impl ViewStack {
    pub fn new() -> Self {
        Self { views: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    pub fn top_kind(&self) -> Option<ModalKind> {
        self.views.last().map(|view| view.kind())
    }

    pub fn push<V: ModalView + 'static>(&mut self, view: V) {
        self.views.push(Box::new(view));
    }

    pub fn pop(&mut self) -> Option<Box<dyn ModalView>> {
        self.views.pop()
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        for view in &self.views {
            view.render(area, buf);
        }
    }

    pub fn update_subagents(&mut self, agents: &[SubAgentResult]) -> bool {
        self.views
            .last_mut()
            .map(|view| view.update_subagents(agents))
            .unwrap_or(false)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<ViewEvent> {
        let action = self
            .views
            .last_mut()
            .map(|view| view.handle_key(key))
            .unwrap_or(ViewAction::None);
        self.apply_action(action)
    }

    pub fn tick(&mut self) -> Vec<ViewEvent> {
        let action = self
            .views
            .last_mut()
            .map(|view| view.tick())
            .unwrap_or(ViewAction::None);
        self.apply_action(action)
    }

    fn apply_action(&mut self, action: ViewAction) -> Vec<ViewEvent> {
        let mut events = Vec::new();
        match action {
            ViewAction::None => {}
            ViewAction::Close => {
                self.views.pop();
            }
            ViewAction::Emit(event) => {
                events.push(event);
            }
            ViewAction::EmitAndClose(event) => {
                events.push(event);
                self.views.pop();
            }
        }
        events
    }
}

impl fmt::Debug for ViewStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewStack")
            .field("len", &self.views.len())
            .field("top", &self.top_kind())
            .finish()
    }
}

pub struct HelpView {
    scroll: usize,
}

impl HelpView {
    pub fn new() -> Self {
        Self { scroll: 0 }
    }
}

impl ModalView for HelpView {
    fn kind(&self) -> ModalKind {
        ModalKind::Help
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => ViewAction::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = self.scroll.saturating_add(1);
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        use ratatui::{
            prelude::Stylize,
            style::Style,
            text::{Line, Span},
            widgets::{Block, Borders, Clear, Paragraph, Widget},
        };

        let popup_width = 70.min(area.width.saturating_sub(4));
        let popup_height = 28.min(area.height.saturating_sub(4));

        let popup_area = Rect {
            x: (area.width - popup_width) / 2,
            y: (area.height - popup_height) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let mut help_lines: Vec<Line> = vec![
            Line::from(vec![Span::styled(
                "DeepSeek CLI Help",
                Style::default().fg(palette::DEEPSEEK_BLUE).bold(),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Navigation ===",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
            Line::from("  Up / Down         - Scroll transcript (or navigate history)"),
            Line::from("  Ctrl+Up / Ctrl+Down - Navigate input history"),
            Line::from("  Alt+Up / Alt+Down - Scroll transcript"),
            Line::from("  PageUp / PageDown - Scroll transcript by page"),
            Line::from("  Home / End        - Jump to top / bottom of transcript"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Input Editing ===",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
            Line::from("  Left / Right      - Move cursor"),
            Line::from("  Ctrl+A / Ctrl+E   - Jump to start / end of line"),
            Line::from("  Backspace / Delete - Delete character before / after cursor"),
            Line::from("  Ctrl+U            - Clear entire input line"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Multi-line Input ===",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
            Line::from("  Ctrl+J / Alt+Enter - Insert newline (without submitting)"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Actions ===",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
            Line::from("  Enter             - Submit message"),
            Line::from("  Esc               - Cancel request / clear input"),
            Line::from("  Ctrl+C            - Cancel request or exit application"),
            Line::from("  Ctrl+D            - Exit when input is empty"),
            Line::from("  Ctrl+K            - Open command palette"),
            Line::from("  l                 - Open pager for last message (when input empty)"),
            Line::from("  v                 - Open tool details (when input empty)"),
            Line::from("  Enter (selection) - Open pager for selected text"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Modes ===",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
            Line::from("  Tab               - Complete /command or cycle modes"),
            Line::from("  Ctrl+X            - Toggle between Agent and Normal modes"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Sessions ===",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
            Line::from("  Ctrl+R            - Open session picker"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Clipboard ===",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
            Line::from("  Ctrl+V            - Paste from clipboard (Cmd+V on macOS)"),
            Line::from("  Ctrl+Shift+C      - Copy selection (Cmd+C on macOS)"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Help ===",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
            Line::from("  F1 / Ctrl+/       - Toggle this help view"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Mouse ===",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
            Line::from("  Scroll wheel      - Scroll transcript"),
            Line::from("  Drag to select    - Select text (auto-copies on release)"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Modes:",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
            Line::from("  Tab cycles modes: Plan → Agent → YOLO"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Commands:",
                Style::default().fg(palette::DEEPSEEK_SKY).bold(),
            )]),
        ];

        for cmd in crate::commands::COMMANDS.iter() {
            help_lines.push(Line::from(format!(
                "  /{:<12} - {}",
                cmd.name, cmd.description
            )));
        }

        help_lines.push(Line::from(""));
        help_lines.push(Line::from(vec![Span::styled(
            "Tools:",
            Style::default().fg(palette::DEEPSEEK_SKY).bold(),
        )]));
        help_lines.push(Line::from(
            "  web.run      - Browse the web (search/open/click/find/screenshot)",
        ));
        help_lines.push(Line::from(
            "  web_search   - Quick web search (DuckDuckGo; MCP optional)",
        ));
        help_lines.push(Line::from(
            "  request_user_input - Ask the user to choose from short prompts",
        ));
        help_lines.push(Line::from(
            "  multi_tool_use.parallel - Execute multiple tools in parallel",
        ));
        help_lines.push(Line::from("  weather     - Daily forecast for a location"));
        help_lines.push(Line::from("  finance     - Stock/crypto price lookup"));
        help_lines.push(Line::from("  sports      - League schedules/standings"));
        help_lines.push(Line::from("  time        - Current time for UTC offsets"));
        help_lines.push(Line::from(
            "  calculator  - Evaluate arithmetic expressions",
        ));
        help_lines.push(Line::from(
            "  list_mcp_resources - List MCP resources (optionally by server)",
        ));
        help_lines.push(Line::from(
            "  list_mcp_resource_templates - List MCP resource templates",
        ));
        help_lines.push(Line::from("  mcp_*        - Tools exposed by MCP servers"));
        help_lines.push(Line::from(""));

        let total_lines = help_lines.len();
        let visible_lines = (popup_height as usize).saturating_sub(3);
        let max_scroll = total_lines.saturating_sub(visible_lines);
        let scroll = self.scroll.min(max_scroll);

        let scroll_indicator = if total_lines > visible_lines {
            format!(" [{}/{} ↑↓] ", scroll + 1, max_scroll + 1)
        } else {
            String::new()
        };

        let help = Paragraph::new(help_lines)
            .block(
                Block::default()
                    .title(Line::from(vec![Span::styled(
                        " Help ",
                        Style::default().fg(palette::DEEPSEEK_BLUE).bold(),
                    )]))
                    .title_bottom(Line::from(vec![
                        Span::styled(" Esc to close ", Style::default().fg(palette::TEXT_MUTED)),
                        Span::styled(scroll_indicator, Style::default().fg(palette::DEEPSEEK_SKY)),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(palette::DEEPSEEK_SKY)),
            )
            .scroll((scroll as u16, 0));

        help.render(popup_area, buf);
    }
}

pub struct SubAgentsView {
    agents: Vec<SubAgentResult>,
    scroll: usize,
}

impl SubAgentsView {
    pub fn new(agents: Vec<SubAgentResult>) -> Self {
        Self { agents, scroll: 0 }
    }
}

impl ModalView for SubAgentsView {
    fn kind(&self) -> ModalKind {
        ModalKind::SubAgents
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => ViewAction::Close,
            KeyCode::Enter | KeyCode::Char('r') | KeyCode::Char('R') => {
                ViewAction::Emit(ViewEvent::SubAgentsRefresh)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = self.scroll.saturating_add(1);
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn update_subagents(&mut self, agents: &[SubAgentResult]) -> bool {
        self.agents = agents.to_vec();
        self.scroll = self.scroll.min(self.agents.len().saturating_sub(1));
        true
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        use ratatui::{
            prelude::Stylize,
            style::Style,
            text::{Line, Span},
            widgets::{Block, Borders, Clear, Paragraph, Widget},
        };

        let popup_width = 78.min(area.width.saturating_sub(4));
        let popup_height = 20.min(area.height.saturating_sub(4));

        let popup_area = Rect {
            x: (area.width - popup_width) / 2,
            y: (area.height - popup_height) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(vec![
            Span::styled("ID", Style::default().fg(palette::DEEPSEEK_SKY).bold()),
            Span::raw("  "),
            Span::styled("TYPE", Style::default().fg(palette::DEEPSEEK_SKY).bold()),
            Span::raw("  "),
            Span::styled("STATUS", Style::default().fg(palette::DEEPSEEK_SKY).bold()),
            Span::raw("  "),
            Span::styled("STEPS", Style::default().fg(palette::DEEPSEEK_SKY).bold()),
            Span::raw("  "),
            Span::styled("TIME", Style::default().fg(palette::DEEPSEEK_SKY).bold()),
        ]));
        lines.push(Line::from(Span::styled(
            "----------------------------------------",
            Style::default().fg(palette::TEXT_MUTED),
        )));

        if self.agents.is_empty() {
            lines.push(Line::from(Span::styled(
                "No sub-agents running.",
                Style::default().fg(palette::TEXT_MUTED),
            )));
        } else {
            let content_width = popup_width.saturating_sub(4) as usize;
            for agent in &self.agents {
                let id = truncate_view_text(&agent.agent_id, 8);
                let kind = format_agent_type(&agent.agent_type);
                let (status, status_style) = format_agent_status(&agent.status);
                let line = Line::from(vec![
                    Span::styled(
                        format!("{id:<8}"),
                        Style::default().fg(palette::TEXT_PRIMARY).bold(),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("{kind:<6}"),
                        Style::default().fg(palette::TEXT_MUTED),
                    ),
                    Span::raw("  "),
                    Span::styled(format!("{status:<10}"), status_style),
                    Span::raw("  "),
                    Span::styled(
                        format!("{:>5}", agent.steps_taken),
                        Style::default().fg(palette::TEXT_DIM),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        format!("{:>5}ms", agent.duration_ms),
                        Style::default().fg(palette::TEXT_DIM),
                    ),
                ]);
                lines.push(line);

                if let Some(result) = agent.result.as_ref() {
                    let max_len = content_width.saturating_sub(10);
                    let preview = truncate_view_text(result, max_len);
                    lines.push(Line::from(vec![
                        Span::styled("  Result: ", Style::default().fg(palette::TEXT_MUTED)),
                        Span::styled(preview, Style::default().fg(palette::TEXT_DIM)),
                    ]));
                }
            }
        }

        let total_lines = lines.len();
        let visible_lines = (popup_height as usize).saturating_sub(3);
        let max_scroll = total_lines.saturating_sub(visible_lines);
        let scroll = self.scroll.min(max_scroll);

        let scroll_indicator = if total_lines > visible_lines {
            format!(" [{}/{} ↑↓] ", scroll + 1, max_scroll + 1)
        } else {
            String::new()
        };

        let view = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(Line::from(vec![Span::styled(
                        " Sub-agents ",
                        Style::default().fg(palette::DEEPSEEK_BLUE).bold(),
                    )]))
                    .title_bottom(Line::from(vec![
                        Span::styled(" Esc to close ", Style::default().fg(palette::TEXT_MUTED)),
                        Span::styled(" R to refresh ", Style::default().fg(palette::TEXT_MUTED)),
                        Span::styled(scroll_indicator, Style::default().fg(palette::DEEPSEEK_SKY)),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(palette::DEEPSEEK_SKY)),
            )
            .scroll((scroll as u16, 0));

        view.render(popup_area, buf);
    }
}

fn format_agent_type(agent_type: &SubAgentType) -> &'static str {
    match agent_type {
        SubAgentType::General => "general",
        SubAgentType::Explore => "explore",
        SubAgentType::Plan => "plan",
        SubAgentType::Review => "review",
        SubAgentType::Custom => "custom",
    }
}

fn format_agent_status(status: &SubAgentStatus) -> (&'static str, ratatui::style::Style) {
    use ratatui::style::Style;

    match status {
        SubAgentStatus::Running => ("running", Style::default().fg(palette::DEEPSEEK_SKY)),
        SubAgentStatus::Completed => ("completed", Style::default().fg(palette::DEEPSEEK_BLUE)),
        SubAgentStatus::Cancelled => ("cancelled", Style::default().fg(palette::TEXT_MUTED)),
        SubAgentStatus::Failed(_) => ("failed", Style::default().fg(palette::DEEPSEEK_RED)),
    }
}

fn truncate_view_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => text[..idx].to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_view_text;

    #[test]
    fn truncate_view_text_handles_unicode() {
        let text = "abc😀é";
        assert_eq!(truncate_view_text(text, 0), "");
        assert_eq!(truncate_view_text(text, 1), "a");
        assert_eq!(truncate_view_text(text, 3), "abc");
        assert_eq!(truncate_view_text(text, 4), "abc😀");
        assert_eq!(truncate_view_text(text, 5), "abc😀é");
    }
}
