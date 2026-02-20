use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect};
use std::cell::Cell;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::palette;
use crate::settings::Settings;
use crate::tools::UserInputResponse;
use crate::tools::spec::ApprovalRequirement;
use crate::tools::spec::ToolCapability;
use crate::tools::subagent::{SubAgentResult, SubAgentStatus, SubAgentType};
use crate::tools::{ToolContext, ToolRegistryBuilder};
use crate::tui::app::App;
use crate::tui::approval::{ElevationOption, ReviewDecision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    Approval,
    Elevation,
    UserInput,
    PlanPrompt,
    CommandPalette,
    Help,
    SubAgents,
    Pager,
    SessionPicker,
    Config,
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
    ConfigUpdated {
        key: String,
        value: String,
        persist: bool,
    },
    PlanPromptSelected {
        option: usize,
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

const HELP_COMMAND_SECTION_ORDER: [&str; 7] = [
    "Core",
    "Modes",
    "Session",
    "Configuration",
    "Workflows",
    "Planning",
    "Debug",
];

fn help_command_section(name: &str) -> &'static str {
    match name {
        "help" | "clear" | "exit" | "model" | "models" | "home" | "links" => "Core",
        "normal" | "agent" | "yolo" | "plan" | "trust" | "logout" => "Modes",
        "save" | "sessions" | "load" | "export" | "compact" | "queue" => "Session",
        "config" | "settings" => "Configuration",
        "task" | "skills" | "skill" | "subagents" | "review" => "Workflows",
        "note" | "cost" | "context" | "system" | "undo" | "retry" => "Planning",
        "init" => "Debug",
        _ => "Debug",
    }
}

fn grouped_commands(
    commands: &'static [crate::commands::CommandInfo],
) -> Vec<(&'static str, Vec<&'static crate::commands::CommandInfo>)> {
    let mut grouped: Vec<(&'static str, Vec<&crate::commands::CommandInfo>)> =
        HELP_COMMAND_SECTION_ORDER
            .iter()
            .map(|section| (*section, Vec::new()))
            .collect();

    for command in commands {
        let section = help_command_section(command.name);
        let Some((_, entries)) = grouped
            .iter_mut()
            .find(|(entry_section, _)| *entry_section == section)
        else {
            continue;
        };
        entries.push(command);
    }

    grouped
        .into_iter()
        .filter(|(_, entries)| !entries.is_empty())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigScope {
    Session,
    Saved,
}

impl ConfigScope {
    fn label(self) -> &'static str {
        match self {
            ConfigScope::Session => "SESSION",
            ConfigScope::Saved => "SAVED",
        }
    }

    fn persist(self) -> bool {
        matches!(self, ConfigScope::Saved)
    }
}

#[derive(Debug, Clone)]
struct ConfigRow {
    key: String,
    value: String,
    editable: bool,
    scope: ConfigScope,
}

#[derive(Debug, Clone)]
struct ConfigEdit {
    key: String,
    original_value: String,
    buffer: Vec<char>,
    cursor: usize,
    select_all: bool,
    scope: ConfigScope,
}

pub struct ConfigView {
    rows: Vec<ConfigRow>,
    selected: usize,
    scroll: usize,
    editing: Option<ConfigEdit>,
    status: Option<String>,
    last_visible_rows: Cell<usize>,
}

impl ConfigView {
    pub fn new_for_app(app: &App) -> Self {
        let settings = Settings::load().unwrap_or_else(|_| Settings::default());
        let rows = vec![
            ConfigRow {
                key: "model".to_string(),
                value: app.model.clone(),
                editable: true,
                scope: ConfigScope::Session,
            },
            ConfigRow {
                key: "approval_mode".to_string(),
                value: app.approval_mode.label().to_string(),
                editable: true,
                scope: ConfigScope::Session,
            },
            ConfigRow {
                key: "auto_compact".to_string(),
                value: settings.auto_compact.to_string(),
                editable: true,
                scope: ConfigScope::Saved,
            },
            ConfigRow {
                key: "show_thinking".to_string(),
                value: settings.show_thinking.to_string(),
                editable: true,
                scope: ConfigScope::Saved,
            },
            ConfigRow {
                key: "show_tool_details".to_string(),
                value: settings.show_tool_details.to_string(),
                editable: true,
                scope: ConfigScope::Saved,
            },
            ConfigRow {
                key: "default_mode".to_string(),
                value: settings.default_mode.clone(),
                editable: true,
                scope: ConfigScope::Saved,
            },
            ConfigRow {
                key: "theme".to_string(),
                value: settings.theme.clone(),
                editable: true,
                scope: ConfigScope::Saved,
            },
            ConfigRow {
                key: "sidebar_width".to_string(),
                value: settings.sidebar_width_percent.to_string(),
                editable: true,
                scope: ConfigScope::Saved,
            },
            ConfigRow {
                key: "sidebar_focus".to_string(),
                value: settings.sidebar_focus.clone(),
                editable: true,
                scope: ConfigScope::Saved,
            },
            ConfigRow {
                key: "max_history".to_string(),
                value: settings.max_input_history.to_string(),
                editable: true,
                scope: ConfigScope::Saved,
            },
            ConfigRow {
                key: "default_model".to_string(),
                value: settings
                    .default_model
                    .as_deref()
                    .unwrap_or("(default)")
                    .to_string(),
                editable: true,
                scope: ConfigScope::Saved,
            },
        ];

        Self {
            rows,
            selected: 0,
            scroll: 0,
            editing: None,
            status: None,
            last_visible_rows: Cell::new(0),
        }
    }

    fn visible_rows_cached(&self) -> usize {
        let cached = self.last_visible_rows.get();
        if cached == 0 { 8 } else { cached }
    }

    fn adjust_scroll(&mut self, visible_rows: usize) {
        if self.rows.is_empty() {
            self.selected = 0;
            self.scroll = 0;
            return;
        }

        let max = self.rows.len().saturating_sub(1);
        self.selected = self.selected.min(max);

        if self.selected < self.scroll {
            self.scroll = self.selected;
        }

        let visible_rows = visible_rows.max(1);
        if self.selected >= self.scroll + visible_rows {
            self.scroll = self.selected.saturating_sub(visible_rows.saturating_sub(1));
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }

        let max = self.rows.len().saturating_sub(1);
        let next = if delta.is_negative() {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            (self.selected + delta as usize).min(max)
        };

        self.selected = next;
        let visible_rows = self.visible_rows_cached();
        self.adjust_scroll(visible_rows);
    }

    fn handle_editing_key(&mut self, key: KeyEvent) -> ViewAction {
        match key.code {
            KeyCode::Esc => {
                self.editing = None;
                self.status = Some("Edit cancelled".to_string());
                ViewAction::None
            }
            KeyCode::Enter => {
                let Some(edit) = self.editing.take() else {
                    return ViewAction::None;
                };
                let submitted = edit.buffer.iter().collect::<String>();
                let value = submitted.trim().to_string();
                ViewAction::Emit(ViewEvent::ConfigUpdated {
                    key: edit.key,
                    value,
                    persist: edit.scope.persist(),
                })
            }
            KeyCode::Backspace => {
                if let Some(edit) = self.editing.as_mut() {
                    if edit.select_all {
                        edit.buffer.clear();
                        edit.cursor = 0;
                        edit.select_all = false;
                    } else if edit.cursor > 0 {
                        edit.cursor = edit.cursor.saturating_sub(1);
                        edit.buffer.remove(edit.cursor);
                    }
                }
                ViewAction::None
            }
            KeyCode::Delete => {
                if let Some(edit) = self.editing.as_mut() {
                    if edit.select_all {
                        edit.buffer.clear();
                        edit.cursor = 0;
                        edit.select_all = false;
                    } else if edit.cursor < edit.buffer.len() {
                        edit.buffer.remove(edit.cursor);
                    }
                }
                ViewAction::None
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(edit) = self.editing.as_mut() {
                    edit.buffer.clear();
                    edit.cursor = 0;
                    edit.select_all = false;
                }
                ViewAction::None
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(edit) = self.editing.as_mut() {
                    edit.cursor = edit.buffer.len();
                    edit.select_all = true;
                }
                ViewAction::None
            }
            KeyCode::Left => {
                if let Some(edit) = self.editing.as_mut() {
                    if edit.select_all {
                        edit.cursor = 0;
                        edit.select_all = false;
                    } else {
                        edit.cursor = edit.cursor.saturating_sub(1);
                    }
                }
                ViewAction::None
            }
            KeyCode::Right => {
                if let Some(edit) = self.editing.as_mut() {
                    if edit.select_all {
                        edit.cursor = edit.buffer.len();
                        edit.select_all = false;
                    } else {
                        edit.cursor = (edit.cursor + 1).min(edit.buffer.len());
                    }
                }
                ViewAction::None
            }
            KeyCode::Home => {
                if let Some(edit) = self.editing.as_mut() {
                    edit.cursor = 0;
                    edit.select_all = false;
                }
                ViewAction::None
            }
            KeyCode::End => {
                if let Some(edit) = self.editing.as_mut() {
                    edit.cursor = edit.buffer.len();
                    edit.select_all = false;
                }
                ViewAction::None
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL) && !ch.is_control() =>
            {
                if let Some(edit) = self.editing.as_mut() {
                    if edit.select_all {
                        edit.buffer.clear();
                        edit.cursor = 0;
                        edit.select_all = false;
                    }
                    edit.buffer.insert(edit.cursor, ch);
                    edit.cursor += 1;
                }
                ViewAction::None
            }
            _ => ViewAction::None,
        }
    }

    fn start_edit(&mut self) {
        let Some(row) = self.rows.get(self.selected) else {
            return;
        };
        let key = row.key.clone();
        let original_value = row.value.clone();
        let initial_value = if key == "default_model" && original_value == "(default)" {
            String::new()
        } else {
            original_value.clone()
        };

        let buffer: Vec<char> = initial_value.chars().collect();
        self.editing = Some(ConfigEdit {
            key,
            original_value,
            cursor: buffer.len(),
            buffer,
            select_all: true,
            scope: row.scope,
        });
        self.status = None;
    }
}

fn config_hint_for_key(key: &str) -> &'static str {
    match key {
        "model" => {
            "wagmii-chat | wagmii-reasoner (aliases: wagmii-v3, wagmii-v3.2, wagmii-r1)"
        }
        "approval_mode" => "auto | suggest | never",
        "auto_compact" | "show_thinking" | "show_tool_details" => "on/off, true/false, yes/no, 1/0",
        "default_mode" => "agent | plan | yolo",
        "theme" => "default | dark | light | whale",
        "sidebar_width" => "10..=50",
        "sidebar_focus" => "auto | plan | todos | tasks | agents",
        "max_history" => "integer (0 allowed)",
        "default_model" => "wagmii-chat | wagmii-reasoner | none/default",
        _ => "",
    }
}

fn render_config_editor_value_line(edit: &ConfigEdit) -> ratatui::text::Line<'static> {
    use ratatui::{
        prelude::Stylize,
        style::Style,
        text::{Line, Span},
    };

    let mut spans = Vec::new();
    spans.push(Span::styled(
        "New: ",
        Style::default().fg(palette::TEXT_MUTED),
    ));

    let cursor_style = Style::default()
        .fg(palette::WAGMII_INK)
        .bg(palette::WAGMII_SKY)
        .bold();
    let selected_style = Style::default()
        .fg(palette::SELECTION_TEXT)
        .bg(palette::SELECTION_BG);

    if edit.select_all && !edit.buffer.is_empty() {
        let text = edit.buffer.iter().collect::<String>();
        spans.push(Span::styled(text, selected_style));
        spans.push(Span::styled(" ", cursor_style));
        return Line::from(spans);
    }

    let before = edit.buffer.iter().take(edit.cursor).collect::<String>();
    spans.push(Span::raw(before));
    if edit.cursor < edit.buffer.len() {
        let ch = edit.buffer[edit.cursor];
        spans.push(Span::styled(ch.to_string(), cursor_style));
        let after = edit
            .buffer
            .iter()
            .skip(edit.cursor.saturating_add(1))
            .collect::<String>();
        spans.push(Span::raw(after));
    } else {
        spans.push(Span::styled(" ", cursor_style));
    }

    Line::from(spans)
}

impl ModalView for ConfigView {
    fn kind(&self) -> ModalKind {
        ModalKind::Config
    }

    fn handle_key(&mut self, key: KeyEvent) -> ViewAction {
        if self.editing.is_some() {
            return self.handle_editing_key(key);
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => ViewAction::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                ViewAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                ViewAction::None
            }
            KeyCode::PageUp => {
                self.move_selection(-5);
                ViewAction::None
            }
            KeyCode::PageDown => {
                self.move_selection(5);
                ViewAction::None
            }
            KeyCode::Char('e') | KeyCode::Char('E') | KeyCode::Enter => {
                if self.rows.get(self.selected).is_some_and(|row| row.editable) {
                    self.start_edit();
                }
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
            widgets::{Block, Borders, Clear, Padding, Paragraph, Widget},
        };

        let popup_width = 84.min(area.width.saturating_sub(4));
        let popup_height = 22.min(area.height.saturating_sub(4));

        let popup_area = Rect {
            x: (area.width - popup_width) / 2,
            y: (area.height - popup_height) / 2,
            width: popup_width,
            height: popup_height,
        };

        Clear.render(popup_area, buf);

        let base_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::WAGMII_INK))
            .padding(Padding::uniform(1));

        let inner = base_block.inner(popup_area);
        let (lines, footer) = if let Some(edit) = self.editing.as_ref() {
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(vec![Span::styled(
                format!("Edit {}", edit.key),
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Scope: ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw(edit.scope.label()),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Current: ", Style::default().fg(palette::TEXT_MUTED)),
                Span::raw(truncate_view_text(&edit.original_value, 60)),
            ]));
            lines.push(Line::from(""));
            lines.push(render_config_editor_value_line(edit));
            lines.push(Line::from(""));
            let hint = config_hint_for_key(&edit.key);
            if !hint.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("Hint: ", Style::default().fg(palette::TEXT_MUTED)),
                    Span::raw(hint),
                ]));
            }
            (
                lines,
                " Enter=apply, Esc=cancel, Ctrl+U=clear, Ctrl+A=all, \u{2190}/\u{2192}=move "
                    .to_string(),
            )
        } else {
            let content_height = usize::from(inner.height);
            let header_lines = 4usize;
            let bottom_lines = 1usize;
            let visible_rows = content_height
                .saturating_sub(header_lines + bottom_lines)
                .max(1);
            self.last_visible_rows.set(visible_rows);

            let start = self.scroll.min(self.rows.len());
            let end = (start + visible_rows).min(self.rows.len());
            let scrollable = self.rows.len() > visible_rows;

            let mut lines: Vec<Line> = vec![
                Line::from(vec![Span::styled(
                    "Session Configuration",
                    Style::default().fg(palette::WAGMII_BLUE).bold(),
                )]),
                Line::from(""),
                Line::from("  Key               Value                                    Scope"),
                Line::from("  ─────────────────────────────────────────────────────────────────"),
            ];

            for (idx, row) in self.rows.iter().enumerate().skip(start).take(visible_rows) {
                let selected = idx == self.selected;
                let style = if selected {
                    Style::default()
                        .fg(palette::SELECTION_TEXT)
                        .bg(palette::SELECTION_BG)
                } else {
                    Style::default().fg(palette::TEXT_PRIMARY)
                };
                let value = truncate_view_text(&row.value, 44);
                let mut line = Line::from(format!(
                    "  {:<17} {:<44} {}",
                    row.key,
                    value,
                    row.scope.label()
                ));
                line.style = style;
                lines.push(line);
            }

            if self.rows.is_empty() {
                lines.push(Line::from("  No settings available."));
            }

            let bottom_text = if let Some(status) = self.status.as_ref() {
                status.clone()
            } else if scrollable && !self.rows.is_empty() {
                format!(
                    "  Showing {}-{} / {}",
                    self.scroll.saturating_add(1),
                    end,
                    self.rows.len()
                )
            } else {
                String::new()
            };
            lines.push(Line::from(Span::styled(
                bottom_text,
                Style::default().fg(palette::TEXT_MUTED),
            )));

            let footer = if scrollable {
                " ↑/↓=select, Enter=edit, PgUp/PgDn=scroll, Esc=close "
            } else {
                " ↑/↓=select, Enter=edit, Esc=close "
            };
            (lines, footer.to_string())
        };

        let block = Block::default()
            .title(Line::from(vec![Span::styled(
                " Config ",
                Style::default().fg(palette::WAGMII_BLUE).bold(),
            )]))
            .title_bottom(Line::from(Span::styled(
                footer,
                Style::default().fg(palette::TEXT_MUTED),
            )))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::BORDER_COLOR))
            .style(Style::default().bg(palette::WAGMII_INK))
            .padding(Padding::uniform(1));

        let inner = block.inner(popup_area);
        block.render(popup_area, buf);
        Paragraph::new(lines)
            .style(Style::default().fg(palette::TEXT_PRIMARY))
            .scroll((0, 0))
            .render(inner, buf);
    }
}

pub struct HelpView {
    scroll: usize,
    tool_sections: Vec<(String, Vec<String>)>,
}

impl HelpView {
    pub fn new() -> Self {
        Self::new_for_workspace(PathBuf::from("."))
    }

    pub fn new_for_workspace(workspace: PathBuf) -> Self {
        Self {
            scroll: 0,
            tool_sections: build_help_tool_sections(&workspace),
        }
    }
}

fn build_help_tool_sections(workspace: &Path) -> Vec<(String, Vec<String>)> {
    let context = ToolContext::new(workspace.to_path_buf());
    let registry = ToolRegistryBuilder::new()
        .with_file_tools()
        .with_search_tools()
        .with_shell_tools()
        .with_web_tools()
        .with_git_tools()
        .with_structured_data_tools()
        .with_user_input_tool()
        .with_parallel_tool()
        .with_patch_tools()
        .with_note_tool()
        .with_diagnostics_tool()
        .with_project_tools()
        .with_test_runner_tool()
        .build(context);

    let mut auto = Vec::new();
    let mut suggest = Vec::new();
    let mut required = Vec::new();

    for tool in registry.all() {
        let mut tags = Vec::new();
        let capabilities = tool.capabilities();
        if capabilities.contains(&ToolCapability::ReadOnly) {
            tags.push("read-only");
        }
        if capabilities.contains(&ToolCapability::WritesFiles) {
            tags.push("writes");
        }
        if capabilities.contains(&ToolCapability::ExecutesCode) {
            tags.push("shell");
        }
        if capabilities.contains(&ToolCapability::Network) {
            tags.push("network");
        }
        if tool.supports_parallel() {
            tags.push("parallel");
        }

        let mut description = tool.description().to_string();
        if !tags.is_empty() {
            description.push_str(" [");
            description.push_str(&tags.join(", "));
            description.push(']');
        }

        let name = tool.name();
        if name.trim().is_empty() {
            continue;
        }

        let line = if description.is_empty() {
            format!("  {}", name)
        } else {
            format!("  {:<15} - {}", name, description)
        };

        match tool.approval_requirement() {
            ApprovalRequirement::Required => required.push(line),
            ApprovalRequirement::Suggest => suggest.push(line),
            ApprovalRequirement::Auto => auto.push(line),
        }
    }

    auto.sort_unstable();
    suggest.sort_unstable();
    required.sort_unstable();

    let mut sections = Vec::new();
    if !auto.is_empty() {
        sections.push(("Tools (auto/low risk)".to_string(), auto));
    }
    if !suggest.is_empty() {
        sections.push(("Tools (suggest approval)".to_string(), suggest));
    }
    if !required.is_empty() {
        sections.push(("Tools (requires approval)".to_string(), required));
    }

    sections
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
            widgets::{Block, Borders, Clear, Padding, Paragraph, Widget},
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
                "Wagmii CLI Help",
                Style::default().fg(palette::WAGMII_BLUE).bold(),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Navigation ===",
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]),
            Line::from("  Up / Down         - Scroll transcript (or navigate history)"),
            Line::from("  Ctrl+Up / Ctrl+Down - Navigate input history"),
            Line::from("  Alt+Up / Alt+Down - Scroll transcript"),
            Line::from("  PageUp / PageDown - Scroll transcript by page"),
            Line::from("  Home / End        - Jump to top / bottom of transcript"),
            Line::from("  g / G             - Jump to top / bottom (when input empty)"),
            Line::from("  [ / ]             - Jump between tool output blocks"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Input Editing ===",
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]),
            Line::from("  Left / Right      - Move cursor"),
            Line::from("  Ctrl+A / Ctrl+E   - Jump to start / end of line"),
            Line::from("  Backspace / Delete - Delete character before / after cursor"),
            Line::from("  Ctrl+U            - Clear entire input line"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Multi-line Input ===",
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]),
            Line::from("  Ctrl+J / Alt+Enter - Insert newline (without submitting)"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Actions ===",
                Style::default().fg(palette::WAGMII_SKY).bold(),
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
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]),
            Line::from("  Tab / Shift+Tab  - Complete /command or cycle modes"),
            Line::from("  Alt+1/2/3/4       - Directly jump to Normal/Agent/YOLO/Plan"),
            Line::from("  Alt+N/A/Y/P       - Alternative jump to Normal/Agent/YOLO/Plan"),
            Line::from("  Alt+!/@/#/$/+)     - Focus Plan/Todos/Tasks/Agents/Auto sidebar"),
            Line::from("  /normal /agent /yolo /plan - Set mode directly"),
            Line::from("  Ctrl+X            - Toggle between Agent and Normal modes"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Sessions ===",
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]),
            Line::from("  Ctrl+R            - Open session picker"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Clipboard ===",
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]),
            Line::from("  Ctrl+V            - Paste from clipboard (Cmd+V on macOS)"),
            Line::from("  Ctrl+Shift+C      - Copy selection (Cmd+C on macOS)"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Help ===",
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]),
            Line::from("  F1 / Ctrl+/       - Toggle this help view"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "=== Mouse ===",
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]),
            Line::from("  Scroll wheel      - Scroll transcript"),
            Line::from("  Drag to select    - Select text (auto-copies on release)"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Mode Cycle:",
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]),
            Line::from("  Normal → Agent → YOLO → Plan (Tab), reverse with Shift+Tab"),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Commands:",
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )]),
        ];

        let grouped = grouped_commands(crate::commands::COMMANDS);
        for (section, commands) in grouped {
            if !commands.is_empty() {
                help_lines.push(Line::from(Span::styled(
                    format!("  [{}]", section),
                    Style::default().fg(palette::WAGMII_BLUE).bold(),
                )));
                for cmd in commands {
                    help_lines.push(Line::from(format!(
                        "    /{:<11} - {}",
                        cmd.name, cmd.description
                    )));
                }
                help_lines.push(Line::from(""));
            }
        }

        help_lines.push(Line::from(""));
        help_lines.push(Line::from(Span::styled(
            "Tools:",
            Style::default().fg(palette::WAGMII_SKY).bold(),
        )));
        if self.tool_sections.is_empty() {
            help_lines.push(Line::from("  Tool registry unavailable"));
        } else {
            for (section, tools) in &self.tool_sections {
                help_lines.push(Line::from(Span::styled(
                    format!("  {}", section),
                    Style::default().fg(palette::WAGMII_BLUE).bold(),
                )));
                for tool in tools {
                    help_lines.push(Line::from(tool.as_str()));
                }
                help_lines.push(Line::from(""));
            }
        }
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
                        Style::default().fg(palette::WAGMII_BLUE).bold(),
                    )]))
                    .title_bottom(Line::from(vec![
                        Span::styled(" Esc to close ", Style::default().fg(palette::TEXT_MUTED)),
                        Span::styled(scroll_indicator, Style::default().fg(palette::WAGMII_SKY)),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(palette::BORDER_COLOR))
                    .style(Style::default().bg(palette::WAGMII_INK))
                    .padding(Padding::uniform(1)),
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
            widgets::{Block, Borders, Clear, Padding, Paragraph, Widget},
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
        let content_width = popup_width.saturating_sub(4) as usize;

        if self.agents.is_empty() {
            lines.push(Line::from(Span::styled(
                "No sub-agents running.",
                Style::default().fg(palette::TEXT_MUTED),
            )));
        } else {
            let mut running = Vec::new();
            let mut completed = Vec::new();
            let mut failed = Vec::new();
            let mut cancelled = Vec::new();

            for agent in &self.agents {
                match agent.status {
                    SubAgentStatus::Running => running.push(agent),
                    SubAgentStatus::Completed => completed.push(agent),
                    SubAgentStatus::Failed(_) => failed.push(agent),
                    SubAgentStatus::Cancelled => cancelled.push(agent),
                }
            }

            let status_summary = [
                ("Running", running.len(), palette::STATUS_WARNING),
                ("Completed", completed.len(), palette::STATUS_SUCCESS),
                ("Failed", failed.len(), palette::WAGMII_RED),
                ("Cancelled", cancelled.len(), palette::TEXT_MUTED),
            ];

            lines.push(Line::from(Span::styled(
                "Sub-agent swarm",
                Style::default().fg(palette::WAGMII_SKY).bold(),
            )));

            let mut summary_parts = Vec::new();
            for (label, count, color) in status_summary {
                summary_parts.push(Line::from(Span::styled(
                    format!("{}: {}", label, count),
                    Style::default().fg(color),
                )));
            }

            let mut summary = vec![Span::styled("  ", Style::default().fg(palette::TEXT_DIM))];
            for (idx, part) in summary_parts.into_iter().enumerate() {
                if idx > 0 {
                    summary.push(Span::raw("  ·  "));
                }
                summary.extend(part.into_iter());
            }
            lines.push(Line::from(summary));
            lines.push(Line::from(Span::styled(
                "",
                Style::default().fg(palette::TEXT_DIM),
            )));

            running.sort_by(|a, b| {
                let order = agent_type_order(&a.agent_type).cmp(&agent_type_order(&b.agent_type));
                order.then_with(|| a.agent_id.cmp(&b.agent_id))
            });
            completed.sort_by(|a, b| {
                let order = agent_type_order(&a.agent_type).cmp(&agent_type_order(&b.agent_type));
                order.then_with(|| a.agent_id.cmp(&b.agent_id))
            });
            failed.sort_by(|a, b| {
                let order = agent_type_order(&a.agent_type).cmp(&agent_type_order(&b.agent_type));
                order.then_with(|| a.agent_id.cmp(&b.agent_id))
            });
            cancelled.sort_by(|a, b| {
                let order = agent_type_order(&a.agent_type).cmp(&agent_type_order(&b.agent_type));
                order.then_with(|| a.agent_id.cmp(&b.agent_id))
            });

            append_subagent_group(
                &mut lines,
                "Running",
                palette::STATUS_WARNING.into(),
                &running,
                content_width,
            );
            append_subagent_group(
                &mut lines,
                "Completed",
                palette::STATUS_SUCCESS.into(),
                &completed,
                content_width,
            );
            append_subagent_group(
                &mut lines,
                "Failed",
                palette::WAGMII_RED.into(),
                &failed,
                content_width,
            );
            append_subagent_group(
                &mut lines,
                "Cancelled",
                palette::TEXT_MUTED.into(),
                &cancelled,
                content_width,
            );
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
                        Style::default().fg(palette::WAGMII_BLUE).bold(),
                    )]))
                    .title_bottom(Line::from(vec![
                        Span::styled(" Esc to close ", Style::default().fg(palette::TEXT_MUTED)),
                        Span::styled(" R to refresh ", Style::default().fg(palette::TEXT_MUTED)),
                        Span::styled(scroll_indicator, Style::default().fg(palette::WAGMII_SKY)),
                    ]))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(palette::BORDER_COLOR))
                    .style(Style::default().bg(palette::WAGMII_INK))
                    .padding(Padding::uniform(1)),
            )
            .scroll((scroll as u16, 0));

        view.render(popup_area, buf);
    }
}

fn append_subagent_group(
    lines: &mut Vec<ratatui::text::Line<'static>>,
    title: &str,
    section_style: ratatui::style::Style,
    agents: &[&SubAgentResult],
    content_width: usize,
) {
    use ratatui::{
        prelude::Stylize,
        style::Style,
        text::{Line, Span},
    };
    if agents.is_empty() {
        return;
    }

    lines.push(Line::from(Span::styled(
        format!("{title} ({})", agents.len()),
        section_style.bold(),
    )));

    for agent in agents {
        let id = truncate_view_text(&agent.agent_id, 11);
        let kind = format_agent_type(&agent.agent_type);
        let (status, status_style, status_detail) = format_agent_status(&agent.status);

        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{id:<12}"),
                Style::default().fg(palette::TEXT_PRIMARY),
            ),
            Span::styled(
                format!("{kind:<9}"),
                Style::default().fg(palette::TEXT_MUTED),
            ),
            Span::raw("  "),
            Span::styled(format!("{status:<10}"), status_style),
            Span::raw("  "),
            Span::styled(
                format!("{:>4}✦", agent.steps_taken),
                Style::default().fg(palette::TEXT_DIM),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{:>6}ms", agent.duration_ms),
                Style::default().fg(palette::TEXT_DIM),
            ),
        ]));

        if let Some(detail) = status_detail {
            let max_len = content_width.saturating_sub(10);
            let detail = truncate_view_text(detail, max_len);
            lines.push(Line::from(vec![
                Span::styled("    reason: ", Style::default().fg(palette::TEXT_MUTED)),
                Span::styled(detail, Style::default().fg(palette::WAGMII_RED)),
            ]));
        }

        if let Some(result) = agent.result.as_ref() {
            let max_len = content_width.saturating_sub(16);
            let preview = truncate_view_text(result, max_len);
            lines.push(Line::from(vec![
                Span::styled("    result: ", Style::default().fg(palette::TEXT_MUTED)),
                Span::styled(preview, Style::default().fg(palette::TEXT_DIM)),
            ]));
        }
    }

    lines.push(Line::from(""));
}

fn agent_type_order(agent_type: &SubAgentType) -> u8 {
    match agent_type {
        SubAgentType::General => 0,
        SubAgentType::Explore => 1,
        SubAgentType::Plan => 2,
        SubAgentType::Review => 3,
        SubAgentType::Custom => 4,
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

fn format_agent_status(
    status: &SubAgentStatus,
) -> (&'static str, ratatui::style::Style, Option<&str>) {
    use ratatui::style::Style;

    match status {
        SubAgentStatus::Running => ("running", Style::default().fg(palette::WAGMII_SKY), None),
        SubAgentStatus::Completed => (
            "completed",
            Style::default().fg(palette::WAGMII_BLUE),
            None,
        ),
        SubAgentStatus::Cancelled => ("cancelled", Style::default().fg(palette::TEXT_MUTED), None),
        SubAgentStatus::Failed(reason) => (
            "failed",
            Style::default().fg(palette::WAGMII_RED),
            Some(reason.as_str()),
        ),
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
    use super::{ConfigView, ModalView, ViewAction, ViewEvent, truncate_view_text};
    use crate::config::Config;
    use crate::tui::app::{App, TuiOptions};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn create_test_app() -> App {
        let options = TuiOptions {
            model: "wagmii-reasoner".to_string(),
            workspace: PathBuf::from("."),
            allow_shell: false,
            use_alt_screen: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: false,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
        };
        App::new(options, &Config::default())
    }

    #[test]
    fn truncate_view_text_handles_unicode() {
        let text = "abc😀é";
        assert_eq!(truncate_view_text(text, 0), "");
        assert_eq!(truncate_view_text(text, 1), "a");
        assert_eq!(truncate_view_text(text, 3), "abc");
        assert_eq!(truncate_view_text(text, 4), "abc😀");
        assert_eq!(truncate_view_text(text, 5), "abc😀é");
    }

    #[test]
    fn config_view_includes_expected_editable_rows() {
        let app = create_test_app();
        let view = ConfigView::new_for_app(&app);
        let keys = view
            .rows
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>();
        assert!(keys.contains(&"model"));
        assert!(keys.contains(&"approval_mode"));
        assert!(keys.contains(&"auto_compact"));
        assert!(view.rows.iter().all(|row| row.editable));
    }

    #[test]
    fn config_view_enter_and_ctrl_u_emit_config_updated() {
        let app = create_test_app();
        let mut view = ConfigView::new_for_app(&app);

        let start = view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(start, ViewAction::None));
        assert!(view.editing.is_some());

        let clear = view.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(matches!(clear, ViewAction::None));
        let cleared = view
            .editing
            .as_ref()
            .expect("editing should remain active after Ctrl+U");
        assert!(cleared.buffer.is_empty());

        for ch in "wagmii-chat".chars() {
            let action = view.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
            assert!(matches!(action, ViewAction::None));
        }

        let submit = view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        match submit {
            ViewAction::Emit(ViewEvent::ConfigUpdated {
                key,
                value,
                persist,
            }) => {
                assert_eq!(key, "model");
                assert_eq!(value, "wagmii-chat");
                assert!(!persist);
            }
            other => panic!("expected config update emit, got {other:?}"),
        }
        assert!(view.editing.is_none());
    }

    #[test]
    fn config_view_typing_replaces_on_first_char() {
        let app = create_test_app();
        let mut view = ConfigView::new_for_app(&app);

        let _ = view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let edit = view.editing.as_ref().expect("editing should be active");
        assert!(edit.select_all, "editor should start with select-all");

        let _ = view.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let edit = view.editing.as_ref().expect("editing should remain active");
        assert_eq!(edit.buffer.iter().collect::<String>(), "x");
    }

    #[test]
    fn config_view_escape_cancels_editing() {
        let app = create_test_app();
        let mut view = ConfigView::new_for_app(&app);
        let _ = view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(view.editing.is_some());

        let cancel = view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(cancel, ViewAction::None));
        assert!(view.editing.is_none());
        assert_eq!(view.status.as_deref(), Some("Edit cancelled"));
    }
}
