use crate::tui::model::FieldKind;
use crate::tui::state::{FormCommand, FormEngine, FormState};
use crate::tui::view::{
    self, CompositeOverlay, HelpErrorRender, HelpOverlayPage, HelpOverlayRender,
    HelpShortcutRender, UiContext,
};
use anyhow::{Result, anyhow};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use jsonschema::Validator;
use ratatui::layout::Rect;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

use super::{
    input::{AppCommand, CommandDispatch, InputRouter},
    keymap::{KeymapContext, KeymapStore},
    options::UiOptions,
    popup::PopupState,
    status::StatusLine,
    terminal::TerminalGuard,
    validation::{ValidationOutcome, validate_form},
};

mod list_ops;
mod overlay;

use overlay::CompositeEditorOverlay;

#[derive(Clone)]
pub(crate) enum PopupOwner {
    Root,
    Composite,
    /// Variant selector for composite list add_entry
    VariantSelector {
        field_pointer: String,
        overlay_host: Option<overlay::OverlayHost>,
    },
}

struct AppPopup {
    owner: PopupOwner,
    state: PopupState,
}

pub(crate) struct App {
    form_state: FormState,
    validator: Validator,
    options: UiOptions,
    session_title: Option<String>,
    status: StatusLine,
    global_errors: Vec<String>,
    validation_errors: usize,
    exit_armed: bool,
    should_quit: bool,
    result: Option<Value>,
    popup: Option<AppPopup>,
    overlay_stack: Vec<CompositeEditorOverlay>,
    overlay_validator_cache: HashMap<String, Arc<Validator>>,
    input_router: InputRouter,
    keymap_store: Arc<KeymapStore>,
    help_overlay: Option<HelpOverlayState>,
}

struct HelpOverlayState {
    pages: Vec<HelpOverlayPage>,
    page: usize,
    viewport: Rect,
    shortcut_offset: usize,
    error_offset: usize,
}

#[cfg(all(test, feature = "json"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HelpOverlaySnapshot {
    pub total_shortcuts: usize,
    pub visible_shortcuts: usize,
    pub shortcut_offset: usize,
    pub error_offset: usize,
    pub visible_errors: usize,
    pub total_errors: usize,
    pub current_page: usize,
    pub total_pages: usize,
    pub summary: String,
}

impl App {
    fn current_help_text(&self) -> Option<String> {
        if !self.options.show_help {
            return None;
        }
        let contexts = self.current_help_contexts();
        match contexts.as_slice() {
            [context] => self.keymap_store.help_text(*context),
            _ => self.keymap_store.help_text_for_contexts(&contexts),
        }
    }

    fn current_help_contexts(&self) -> Vec<KeymapContext> {
        if self.overlay_depth() > 0 {
            let mut contexts = vec![KeymapContext::Overlay];
            if let Some(field) = self
                .active_overlay()
                .and_then(|editor| editor.form_state().focused_field())
                && let Some(field_context) = field_help_context(&field.schema.kind)
            {
                contexts.push(field_context);
            }
            return contexts;
        }

        let Some(field) = self.form_state.focused_field() else {
            return vec![KeymapContext::Default];
        };

        if field.is_composite_list() {
            return vec![KeymapContext::Collection];
        }

        let mut contexts = vec![KeymapContext::Default];
        if let Some(field_context) = field_help_context(&field.schema.kind) {
            contexts.push(field_context);
        }
        contexts
    }

    fn help_overlay_sections(&self) -> Vec<(String, Vec<KeymapContext>)> {
        let mut sections = vec![
            ("Form".to_string(), vec![KeymapContext::Default]),
            ("List".to_string(), vec![KeymapContext::Collection]),
            ("Overlay".to_string(), vec![KeymapContext::Overlay]),
            ("Help".to_string(), vec![KeymapContext::Help]),
        ];

        if let Some(context) = self.current_help_contexts().into_iter().find(|context| {
            matches!(
                context,
                KeymapContext::TextInput | KeymapContext::NumericInput
            )
        }) {
            sections.push(("Field".to_string(), vec![context]));
        }

        sections
    }

    fn handle_popup_key(&mut self, key: KeyEvent) -> Result<bool> {
        if let Some(app_popup) = &mut self.popup {
            let popup = &mut app_popup.state;
            match key.code {
                KeyCode::Esc => {
                    self.popup = None;
                    self.status.ready();
                }
                KeyCode::Up => popup.select_previous(),
                KeyCode::Down => popup.select_next(),
                KeyCode::Char(' ') if popup.is_multi() => {
                    popup.toggle_current();
                    return Ok(true);
                }
                KeyCode::Enter => {
                    let (pointer, selection, multi_flags) = {
                        let pointer = popup.pointer().to_string();
                        let selection = popup.selection();
                        let multi_flags = popup.active().map(|flags| flags.to_vec());
                        (pointer, selection, multi_flags)
                    };
                    let owner = app_popup.owner.clone();
                    self.popup = None;
                    self.apply_popup_selection_data(owner, &pointer, selection, multi_flags);
                    if self.options.auto_validate {
                        self.run_validation(false);
                    }
                    self.status.value_updated();
                }
                _ => {}
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn dispatch_form_command(&mut self, command: FormCommand) {
        let mut engine = FormEngine::new(&mut self.form_state, &self.validator);
        if let Err(message) = engine.dispatch(command) {
            self.status.set_raw(&message);
        }
        self.validation_errors = self.form_state.error_count();
    }

    fn handle_app_command(&mut self, command: AppCommand) -> bool {
        match command {
            AppCommand::Save => {
                self.exit_armed = false;
                if self.overlay_depth() > 0 {
                    let _ = self.save_active_overlay();
                } else {
                    self.on_save();
                }
            }
            AppCommand::Quit => {
                if self.overlay_depth() > 0 {
                    self.request_overlay_exit();
                    return true;
                }
                self.on_exit();
            }
            AppCommand::ResetStatus => {
                self.exit_armed = false;
                self.status.ready();
            }
            AppCommand::ShowHelp => {
                self.toggle_help_overlay();
                return true;
            }
            AppCommand::TogglePopup => {
                if self.try_open_popup(PopupOwner::Root) {
                    return true;
                }
            }
            AppCommand::EditComposite => {
                self.try_open_composite_editor();
            }
            AppCommand::ListAddEntry => {
                if self.handle_list_add_entry() {
                    return true;
                }
            }
            AppCommand::ListRemoveEntry => {
                if self.handle_list_remove_entry() {
                    return true;
                }
            }
            AppCommand::ListMove(delta) => {
                if self.handle_list_move_entry(delta) {
                    return true;
                }
            }
            AppCommand::ListSelect(delta) => {
                if self.handle_list_select_entry(delta) {
                    return true;
                }
            }
        }
        false
    }

    fn toggle_help_overlay(&mut self) {
        if self.help_overlay.is_some() {
            self.help_overlay = None;
            return;
        }

        let viewport = Rect::new(0, 0, 0, 0);
        let pages = self.build_help_overlay_pages(viewport);
        if pages.is_empty() {
            return;
        }

        // Help overlay is global and should not compete with other popups.
        self.popup = None;
        self.help_overlay = Some(HelpOverlayState {
            pages,
            page: 0,
            viewport,
            shortcut_offset: 0,
            error_offset: 0,
        });
    }

    fn handle_help_overlay_key(&mut self, key: &KeyEvent) -> bool {
        let action = self
            .input_router
            .classify_for_contexts(key, &[KeymapContext::Help]);
        if matches!(action, super::input::KeyAction::HelpClose) {
            if self.help_overlay.is_none() {
                return false;
            }
            self.help_overlay = None;
            return true;
        }
        let Some(state) = self.help_overlay.as_mut() else {
            return false;
        };
        let shortcut_capacity = view::help_overlay_panel_capacities(state.viewport).shortcuts;
        let shortcut_max_offset = state
            .pages
            .first()
            .map(|page| page.shortcuts.len().saturating_sub(shortcut_capacity))
            .unwrap_or(0);
        let error_message_width = view::help_overlay_error_message_capacity(state.viewport);
        let current_page = state.page.min(state.pages.len().saturating_sub(1));
        let error_max_offset = state
            .pages
            .get(current_page)
            .map(|page| help_page_error_max_offset(page, error_message_width))
            .unwrap_or(0);

        match action {
            super::input::KeyAction::HelpPageStep(delta) => {
                let total = state.pages.len();
                if total > 0 {
                    let next = (state.page as i32 + delta).rem_euclid(total as i32) as usize;
                    state.page = next;
                }
                let page = state.page.min(state.pages.len().saturating_sub(1));
                state.error_offset = state
                    .pages
                    .get(page)
                    .map(|entry| {
                        state
                            .error_offset
                            .min(help_page_error_max_offset(entry, error_message_width))
                    })
                    .unwrap_or(0);
            }
            super::input::KeyAction::HelpShortcutScroll(delta) => {
                if delta < 0 {
                    state.shortcut_offset = state
                        .shortcut_offset
                        .saturating_sub(delta.unsigned_abs() as usize);
                } else {
                    state.shortcut_offset =
                        (state.shortcut_offset + delta as usize).min(shortcut_max_offset);
                }
            }
            super::input::KeyAction::HelpShortcutPage(delta) => {
                let step = shortcut_capacity.saturating_mul(delta.unsigned_abs() as usize);
                if delta < 0 {
                    state.shortcut_offset = state.shortcut_offset.saturating_sub(step);
                } else {
                    state.shortcut_offset = (state.shortcut_offset + step).min(shortcut_max_offset);
                }
            }
            super::input::KeyAction::HelpShortcutHome => {
                state.shortcut_offset = 0;
            }
            super::input::KeyAction::HelpShortcutEnd => {
                state.shortcut_offset = shortcut_max_offset;
            }
            super::input::KeyAction::HelpErrorScroll(delta) => {
                if delta < 0 {
                    state.error_offset = state
                        .error_offset
                        .saturating_sub(delta.unsigned_abs() as usize);
                } else {
                    state.error_offset =
                        (state.error_offset + delta as usize).min(error_max_offset);
                }
            }
            _ => {}
        }

        true
    }

    fn build_help_overlay_pages(&self, viewport: Rect) -> Vec<HelpOverlayPage> {
        let errors_per_page = view::help_overlay_error_page_capacity(viewport);

        let mut shortcuts: Vec<HelpShortcutRender> = Vec::new();
        let sections = self.help_overlay_sections();

        for (scope, contexts) in sections {
            let entries = match contexts.as_slice() {
                [context] => self.keymap_store.help_entries(*context),
                _ => self.keymap_store.help_entries_for_contexts(&contexts),
            };
            for entry in entries {
                shortcuts.push(HelpShortcutRender {
                    scope: scope.clone(),
                    keys: entry.keys,
                    action: entry.action,
                });
            }
        }

        let mut errors: Vec<HelpErrorRender> = Vec::new();
        for (pointer, message) in self.form_state.error_entries() {
            errors.push(HelpErrorRender {
                index: errors.len() + 1,
                pointer,
                message,
            });
        }
        for msg in &self.global_errors {
            errors.push(HelpErrorRender {
                index: errors.len() + 1,
                pointer: "<global>".to_string(),
                message: msg.clone(),
            });
        }

        let total_errors = errors.len();
        if total_errors == 0 && shortcuts.is_empty() {
            return Vec::new();
        }

        let total_pages = if total_errors == 0 {
            1
        } else {
            total_errors.div_ceil(errors_per_page)
        };

        let mut pages: Vec<HelpOverlayPage> = Vec::with_capacity(total_pages.max(1));

        for page_idx in 0..total_pages {
            let start = page_idx * errors_per_page;
            let end = (start + errors_per_page).min(total_errors);
            let page_errors = if total_errors == 0 {
                Vec::new()
            } else {
                errors[start..end].to_vec()
            };

            pages.push(HelpOverlayPage {
                summary:
                    "All shortcuts stay visible here; only the error list paginates to fit terminal height."
                        .to_string(),
                current_page: page_idx + 1,
                total_pages,
                shortcuts: shortcuts.clone(),
                errors: page_errors,
                total_errors,
            });
        }

        pages
    }

    fn refresh_help_overlay_pages(&mut self, viewport: Rect) {
        let Some(current_page) = self.help_overlay.as_ref().map(|overlay| overlay.page) else {
            return;
        };
        let pages = self.build_help_overlay_pages(viewport);
        let shortcut_capacity = view::help_overlay_panel_capacities(viewport).shortcuts;
        let error_message_width = view::help_overlay_error_message_capacity(viewport);
        let shortcut_offset = self
            .help_overlay
            .as_ref()
            .map(|overlay| overlay.shortcut_offset)
            .unwrap_or(0);
        let error_offset = self
            .help_overlay
            .as_ref()
            .map(|overlay| overlay.error_offset)
            .unwrap_or(0);
        let next_page = if pages.is_empty() {
            0
        } else {
            current_page.min(pages.len().saturating_sub(1))
        };
        let next_shortcut_offset = pages
            .first()
            .map(|page| shortcut_offset.min(page.shortcuts.len().saturating_sub(shortcut_capacity)))
            .unwrap_or(0);
        let next_error_offset = pages
            .get(next_page)
            .map(|page| error_offset.min(help_page_error_max_offset(page, error_message_width)))
            .unwrap_or(0);
        if let Some(overlay) = self.help_overlay.as_mut() {
            overlay.viewport = viewport;
            overlay.pages = pages;
            overlay.page = next_page;
            overlay.shortcut_offset = next_shortcut_offset;
            overlay.error_offset = next_error_offset;
        }
    }

    fn handle_field_input(&mut self, event: &KeyEvent) {
        if let Some(field) = self.form_state.focused_field_mut()
            && field.handle_key(event)
        {
            let pointer = field.schema.pointer.clone();
            self.exit_armed = false;
            self.status.editing(&field.schema.display_label());
            if self.options.auto_validate {
                self.dispatch_form_command(FormCommand::FieldEdited { pointer });
            }
        }
    }

    pub fn new(form_state: FormState, validator: Validator, options: UiOptions) -> Self {
        let keymap_store = options.keymap_store.clone();
        Self {
            form_state,
            validator,
            options,
            session_title: None,
            status: StatusLine::new(),
            global_errors: Vec::new(),
            validation_errors: 0,
            exit_armed: false,
            should_quit: false,
            result: None,
            popup: None,
            overlay_stack: Vec::new(),
            overlay_validator_cache: HashMap::new(),
            input_router: InputRouter::new(keymap_store.clone()),
            keymap_store,
            help_overlay: None,
        }
    }

    pub fn set_session_title(&mut self, title: Option<String>) {
        self.session_title = title;
    }

    pub fn run(&mut self) -> Result<Value> {
        let mut terminal = TerminalGuard::new()?;
        while !self.should_quit {
            terminal.autoresize()?;
            terminal.draw(|frame| self.draw(frame))?;
            if !event::poll(self.options.tick_rate)? {
                continue;
            }
            match event::read()? {
                Event::Key(key) => self.handle_key(key)?,
                Event::Resize(width, height) => {
                    terminal.resize(Rect::new(0, 0, width, height))?;
                    continue;
                }
                Event::Mouse(_) => {}
                Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
            }
        }

        if let Some(value) = self.result.take() {
            Ok(value)
        } else {
            Err(anyhow!("user exited without saving"))
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame<'_>) {
        let help = self.current_help_text();
        let form_dirty = self.form_state.is_dirty();
        self.refresh_help_overlay_pages(frame.area());

        let help_overlay_render = self.help_overlay.as_ref().and_then(|state| {
            if state.pages.is_empty() {
                return None;
            }
            let total = state.pages.len();
            let idx = state.page.min(total.saturating_sub(1));
            let page = &state.pages[idx];
            Some(HelpOverlayRender {
                page,
                shortcut_offset: state.shortcut_offset,
                error_offset: state.error_offset,
            })
        });

        if let Some(editor) = self.overlay_stack.last_mut() {
            let child = editor
                .form_state()
                .focused_field()
                .map(|field| field.schema.display_label())
                .unwrap_or_else(|| "<none>".to_string());
            let focus_label = Some(format!("{} › {}", editor.field_label(), child));
            let overlay_meta = CompositeOverlay {
                title: editor.title().to_string(),
                description: editor.description().cloned(),
                dirty: editor.form_state().is_dirty(),
                instructions: editor.instructions().to_string(),
                list_entries: editor.entry_tabs_entries().map(|entries| entries.to_vec()),
                list_selected: editor.entry_tabs_selected(),
                entry_label: editor.entry_tabs_label().map(|label| label.to_string()),
                level: editor.level(),
            };
            let overlay_form_state = editor.form_state_mut();
            view::draw(
                frame,
                &mut self.form_state,
                Some(overlay_form_state),
                UiContext {
                    status_message: self.status.message(),
                    dirty: form_dirty,
                    error_count: self.validation_errors,
                    help: help.as_deref(),
                    global_errors: &self.global_errors,
                    focus_label,
                    session_title: self.session_title.as_deref(),
                    popup: self.popup.as_ref().map(|popup| popup.state.as_render()),
                    composite_overlay: Some(overlay_meta),
                    help_overlay: help_overlay_render,
                },
            );
            return;
        }

        let focus_label = self
            .form_state
            .focused_field()
            .map(|field| field.schema.display_label());

        view::draw(
            frame,
            &mut self.form_state,
            None,
            UiContext {
                status_message: self.status.message(),
                dirty: form_dirty,
                error_count: self.validation_errors,
                help: help.as_deref(),
                global_errors: &self.global_errors,
                focus_label,
                session_title: self.session_title.as_deref(),
                popup: self.popup.as_ref().map(|popup| popup.state.as_render()),
                composite_overlay: None,
                help_overlay: help_overlay_render,
            },
        );
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }

        if self.handle_help_overlay_key(&key) {
            return Ok(());
        }

        if self.handle_popup_key(key)? {
            return Ok(());
        }

        if self.overlay_depth() > 0 {
            self.handle_composite_editor_key(key)?;
            return Ok(());
        }

        let dispatch = self
            .options
            .keymap
            .resolve(self.input_router.classify(&key));
        match dispatch {
            CommandDispatch::Form(command) => {
                self.dispatch_form_command(command);
                self.exit_armed = false;
            }
            CommandDispatch::App(command) => {
                if self.handle_app_command(command) {
                    return Ok(());
                }
            }
            CommandDispatch::Input(event) => {
                self.handle_field_input(&event);
            }
            CommandDispatch::None => {}
        }

        Ok(())
    }

    fn try_open_popup(&mut self, owner: PopupOwner) -> bool {
        if self.popup.is_some() {
            return true;
        }

        match &owner {
            PopupOwner::Root => {
                if let Some(field) = self.form_state.focused_field_mut() {
                    field.ensure_composite_list_popup_entry();
                }
            }
            PopupOwner::Composite => {
                if let Some(editor) = self.active_overlay_mut()
                    && let Some(field) = editor.form_state_mut().focused_field_mut()
                {
                    field.ensure_composite_list_popup_entry();
                }
            }
            PopupOwner::VariantSelector { .. } => {
                // Variant selector popup is created directly, no field preparation needed
            }
        }

        let field_opt = match &owner {
            PopupOwner::Root => self.form_state.focused_field(),
            PopupOwner::Composite => self
                .active_overlay()
                .and_then(|editor| editor.form_state().focused_field()),
            PopupOwner::VariantSelector { .. } => {
                // Variant selector popup doesn't use focused field
                return false;
            }
        };
        let Some(field) = field_opt else {
            return false;
        };
        if let Some(popup) = PopupState::from_field(field) {
            let message = if popup.is_multi() {
                "Use ↑/↓ to move, Space to toggle, Enter to apply"
            } else {
                "Use ↑/↓ and Enter to choose"
            };
            self.status.set_raw(message);
            self.popup = Some(AppPopup {
                owner,
                state: popup,
            });
            return true;
        }
        false
    }

    fn on_save(&mut self) {
        if let Some(value) = self.run_validation(true) {
            self.status
                .set_raw("Configuration saved. Press Ctrl+Q to exit.");
            self.result = Some(value);
            self.form_state.mark_clean();
            self.exit_armed = false;
        }
    }

    fn on_exit(&mut self) {
        // First attempt: validate the full form. If it passes, treat Ctrl+Q
        // as a final save-and-exit, even if the user never pressed Ctrl+S.
        if !self.exit_armed {
            if let Some(value) = self.run_validation(true) {
                self.result = Some(value);
                self.form_state.mark_clean();
                self.should_quit = true;
                self.exit_armed = false;
                return;
            }

            // Validation failed. When confirm_exit is enabled, require a
            // second Ctrl+Q to force exit without saving the current
            // (invalid) state. The last successfully validated result, if
            // any, is preserved.
            if self.options.confirm_exit {
                self.exit_armed = true;
                self.status.pending_exit();
                return;
            }

            // confirm_exit disabled: exit immediately without updating
            // result; callers will see "user exited without saving" unless a
            // previous successful save populated result.
            self.should_quit = true;
            return;
        }

        // Second Ctrl+Q after a failed validation: force exit without
        // changing result. This mirrors the behavior of quitting without a
        // successful save.
        self.should_quit = true;
    }

    fn run_validation(&mut self, announce: bool) -> Option<Value> {
        match validate_form(&mut self.form_state, &self.validator) {
            ValidationOutcome::Valid(value) => {
                self.global_errors.clear();
                self.validation_errors = 0;
                if announce {
                    self.status.validation_passed();
                }
                Some(value)
            }
            ValidationOutcome::Invalid {
                issues,
                global_errors,
            } => {
                self.global_errors = global_errors;
                self.validation_errors = issues;
                if announce {
                    self.status.issues_remaining(issues);
                }
                None
            }
            ValidationOutcome::BuildError { message } => {
                self.global_errors = vec![message.clone()];
                self.validation_errors = 1;
                self.status.set_raw(message);
                None
            }
        }
    }
}

#[cfg(all(test, feature = "json"))]
impl App {
    pub(crate) fn form_state_mut_for_test(&mut self) -> &mut FormState {
        &mut self.form_state
    }

    pub(crate) fn handle_key_for_test(&mut self, key: KeyEvent) -> Result<()> {
        self.handle_key(key)
    }

    pub(crate) fn toggle_help_overlay_for_test(&mut self, viewport: Rect) {
        self.toggle_help_overlay();
        self.refresh_help_overlay_pages(viewport);
    }

    pub(crate) fn current_help_text_for_test(&self) -> Option<String> {
        self.current_help_text()
    }

    pub(crate) fn help_overlay_snapshot_for_test(
        &mut self,
        viewport: Rect,
    ) -> Option<HelpOverlaySnapshot> {
        self.refresh_help_overlay_pages(viewport);
        let capacities = view::help_overlay_panel_capacities(viewport);
        let overlay = self.help_overlay.as_ref()?;
        let idx = overlay.page.min(overlay.pages.len().saturating_sub(1));
        let page = overlay.pages.get(idx)?;
        let visible_shortcuts = page
            .shortcuts
            .len()
            .saturating_sub(overlay.shortcut_offset)
            .min(capacities.shortcuts);
        Some(HelpOverlaySnapshot {
            total_shortcuts: page.shortcuts.len(),
            visible_shortcuts,
            shortcut_offset: overlay.shortcut_offset,
            error_offset: overlay.error_offset,
            visible_errors: page.errors.len(),
            total_errors: page.total_errors,
            current_page: page.current_page,
            total_pages: page.total_pages,
            summary: page.summary.clone(),
        })
    }
}

fn field_help_context(kind: &FieldKind) -> Option<KeymapContext> {
    match kind {
        FieldKind::String | FieldKind::Json | FieldKind::Nullable(_) => {
            Some(KeymapContext::TextInput)
        }
        FieldKind::Integer | FieldKind::Number => Some(KeymapContext::NumericInput),
        _ => None,
    }
}

fn help_page_error_max_offset(page: &HelpOverlayPage, message_width: usize) -> usize {
    if message_width == 0 {
        return 0;
    }
    page.errors
        .iter()
        .map(|entry| entry.message.chars().count().saturating_sub(message_width))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        tui::{app::options::UiOptions, model::form_schema_from_ui_ast},
        ui_ast::build_ui_ast,
    };
    use jsonschema::validator_for;
    use serde_json::json;

    fn app_with_single_field() -> App {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            }
        });
        let ast = build_ui_ast(&schema).expect("ui ast");
        let form_schema = form_schema_from_ui_ast(&ast);
        let form_state = FormState::from_schema(&form_schema);
        let validator = validator_for(&schema).expect("validator");
        App::new(form_state, validator, UiOptions::default())
    }

    #[test]
    fn exit_after_save_keeps_last_saved_result() {
        let mut app = app_with_single_field();
        app.on_save();
        assert!(app.result.is_some(), "save should populate result");
        assert_eq!(
            app.status.message(),
            "Configuration saved. Press Ctrl+Q to exit."
        );
        assert!(
            !app.form_state.is_dirty(),
            "successful save should clear dirty flags"
        );
        assert!(!app.exit_armed, "save should reset exit confirmation");
        app.on_exit();
        assert!(app.result.is_some(), "exiting should keep last saved value");
        assert!(app.should_quit, "exit flag should be set");
    }

    #[test]
    fn exit_without_explicit_save_validates_and_sets_result() {
        let mut app = app_with_single_field();
        app.on_exit();
        assert!(
            app.result.is_some(),
            "Ctrl+Q should validate and produce a result for a valid form"
        );
    }
}
