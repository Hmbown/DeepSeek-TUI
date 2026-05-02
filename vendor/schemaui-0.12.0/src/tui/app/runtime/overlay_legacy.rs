use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use jsonschema::{Validator, validator_for};

use crate::tui::app::keymap::KeymapContext;
use crate::tui::model::{CompositeMode, FieldKind};
use crate::tui::state::field::components::{CompositeSelectorView, helpers::OverlayContext};
use crate::tui::state::{
    ArrayEditorSession, CompositeEditorSession, FieldState, FormCommand, FormEngine, FormState,
    KeyValueEditorSession, apply_command,
};

use crate::tui::app::input::{AppCommand, CommandDispatch};
use crate::tui::app::runtime::{App, PopupOwner};

mod state;
mod editor;

pub(super) use state::{
    EntryAdvance,
    EntryTabsKind,
    EntryTabsStore,
    OverlayFocusMode,
    OverlayHost,
    OverlayResult,
    OverlaySession,
    OverlayState,
    OverlayStore,
    CompositeOverlayTarget,
    FocusDirection,
    FocusOutcome,
};

pub(super) use editor::CompositeEditorOverlay;



#[derive(Debug, Clone)]
struct OverlayStore {
    display_title: String,
    display_description: Option<String>,
    instructions: String,
    entry_tabs: Option<EntryTabsStore>,
    entry_label: Option<String>,
    focus: OverlayFocusMode,
}

#[derive(Clone)]
struct OverlayResult {
    field_pointer: String,
    host: OverlayHost,
    target: CompositeOverlayTarget,
    session: OverlaySession,
}

impl OverlayResult {
    fn host(&self) -> OverlayHost {
        self.host
    }

    fn apply(self, host_state: &mut FormState) -> Result<(), String> {
        match self.target {
            CompositeOverlayTarget::Field => {
                let OverlaySession::Composite(session) = self.session else {
                    return Err("Invalid overlay session".to_string());
                };
                let Some(field) = host_state.field_mut_by_pointer(&self.field_pointer) else {
                    return Err("Overlay target no longer exists".to_string());
                };
                field.close_composite_editor(session, true);
                Ok(())
            }
            CompositeOverlayTarget::ListEntry { entry_index } => {
                let OverlaySession::Composite(session) = self.session else {
                    return Err("Invalid overlay session".to_string());
                };
                let Some(field) = host_state.field_mut_by_pointer(&self.field_pointer) else {
                    return Err("Overlay target no longer exists".to_string());
                };
                field.close_composite_list_editor(entry_index, session, true);
                Ok(())
            }
            CompositeOverlayTarget::KeyValueEntry { entry_index } => {
                let OverlaySession::KeyValue(session) = self.session else {
                    return Err("Invalid overlay session".to_string());
                };
                let Some(field) = host_state.field_mut_by_pointer(&self.field_pointer) else {
                    return Err("Overlay target no longer exists".to_string());
                };
                field
                    .close_key_value_editor(entry_index, &session, true)
                    .map_err(|err| err.message)
                    .map(|_| ())
            }
            CompositeOverlayTarget::ArrayEntry { entry_index } => {
                let OverlaySession::Array(session) = self.session else {
                    return Err("Invalid overlay session".to_string());
                };
                let Some(field) = host_state.field_mut_by_pointer(&self.field_pointer) else {
                    return Err("Overlay target no longer exists".to_string());
                };
                field
                    .close_scalar_array_editor(entry_index, &session, true)
                    .map_err(|err| err.message)
                    .map(|_| ())
            }
        }
    }
}

#[derive(Clone)]
struct OverlayState {
    field_pointer: String,
    field_label: String,
    host: OverlayHost,
    level: usize,
    target: CompositeOverlayTarget,
    session: OverlaySession,
    exit_armed: bool,
    validator: Option<Arc<Validator>>,
}

impl OverlayState {
    fn new(
        field_pointer: String,
        field_label: String,
        host: OverlayHost,
        level: usize,
        session: OverlaySession,
    ) -> Self {
        Self {
            field_pointer,
            field_label,
            host,
            level,
            target: CompositeOverlayTarget::Field,
            session,
            exit_armed: false,
            validator: None,
        }
    }

    fn dirty(&self) -> bool {
        self.session.is_dirty()
    }

    fn form_state(&self) -> &FormState {
        self.session.form_state()
    }

    fn form_state_mut(&mut self) -> &mut FormState {
        self.session.form_state_mut()
    }

    fn session(&self) -> &OverlaySession {
        if matches!(self.session, OverlaySession::Detached) {
            panic!("overlay session detached")
        }
        &self.session
    }

    fn take_composite_session(&mut self) -> Option<CompositeEditorSession> {
        match std::mem::replace(&mut self.session, OverlaySession::Detached) {
            OverlaySession::Composite(session) => Some(session),
            other => {
                self.session = other;
                None
            }
        }
    }

    fn replace_composite_session(&mut self, session: CompositeEditorSession) {
        self.session = OverlaySession::Composite(session);
    }

    fn current_variant_index(&self) -> Option<usize> {
        match &self.session {
            OverlaySession::Composite(session) => Some(session.variant_index),
            _ => None,
        }
    }

    fn field_pointer(&self) -> &str {
        &self.field_pointer
    }

    fn field_label(&self) -> &str {
        &self.field_label
    }

    fn level(&self) -> usize {
        self.level
    }

    fn host(&self) -> OverlayHost {
        self.host
    }

    fn target(&self) -> &CompositeOverlayTarget {
        &self.target
    }

    fn target_mut(&mut self) -> &mut CompositeOverlayTarget {
        &mut self.target
    }

    fn set_target(&mut self, target: CompositeOverlayTarget) {
        self.target = target;
    }

    fn exit_armed(&self) -> bool {
        self.exit_armed
    }

    fn set_exit_armed(&mut self, armed: bool) {
        self.exit_armed = armed;
    }

    fn set_validator(&mut self, validator: Option<Arc<Validator>>) {
        self.validator = validator;
    }

    fn validator_clone(&self) -> Option<Arc<Validator>> {
        self.validator.clone()
    }

    fn build_result(&self) -> OverlayResult {
        OverlayResult {
            field_pointer: self.field_pointer.clone(),
            host: self.host,
            target: self.target.clone(),
            session: self.session.clone(),
        }
    }
}

impl OverlayStore {
    fn new(
        display_title: String,
        display_description: Option<String>,
        instructions: String,
    ) -> Self {
        Self {
            display_title,
            display_description,
            instructions,
            entry_tabs: None,
            entry_label: None,
            focus: OverlayFocusMode::FormFields,
        }
    }

    fn entry_tabs_len(&self) -> usize {
        self.entry_tabs.as_ref().map(|tabs| tabs.len()).unwrap_or(0)
    }

    fn entry_tabs_selected(&self) -> Option<usize> {
        self.entry_tabs.as_ref().map(|tabs| tabs.selected())
    }

    fn entry_tabs_entries(&self) -> Option<&[String]> {
        self.entry_tabs.as_ref().map(|tabs| tabs.entries())
    }

    fn entry_tabs_label(&self) -> Option<&String> {
        self.entry_label.as_ref()
    }

    fn entry_tabs_snapshot(&self) -> Option<(usize, usize)> {
        self.entry_tabs
            .as_ref()
            .map(|tabs| (tabs.len(), tabs.selected()))
    }

    fn can_focus_entries(&self) -> bool {
        self.entry_tabs
            .as_ref()
            .map(|tabs| !tabs.is_empty())
            .unwrap_or(false)
    }

    fn focus_entries(&mut self) {
        self.focus = OverlayFocusMode::EntryTabs;
    }

    fn focus_form(&mut self) {
        self.focus = OverlayFocusMode::FormFields;
    }

    fn focus(&self) -> OverlayFocusMode {
        self.focus
    }

    fn set_entry_tabs(&mut self, label: impl Into<String>, entries: Vec<String>, selected: usize) {
        if let Some(store) = self.entry_tabs.as_mut() {
            store.set_entries(entries, selected);
        } else {
            self.entry_tabs = Some(EntryTabsStore::new(entries, selected));
        }
        self.entry_label = Some(label.into());
    }

    fn set_variant_tabs(
        &mut self,
        label: impl Into<String>,
        entries: Vec<String>,
        ids: Vec<usize>,
        selected_id: usize,
    ) {
        let label = label.into();
        if let Some(store) = self.entry_tabs.as_mut() {
            store.set_entries_with_ids(entries, ids, selected_id);
        } else {
            self.entry_tabs = Some(EntryTabsStore::with_kind(
                entries,
                ids,
                selected_id,
                EntryTabsKind::Variants,
            ));
        }
        self.entry_label = Some(label);
    }

    fn select_entry_by_id(&mut self, selected_id: usize) {
        if let Some(store) = self.entry_tabs.as_mut() {
            store.select_by_id(selected_id);
        }
    }

    fn update_title(&mut self, field_label: &str, title: &str) {
        self.display_title = format!("Edit {} – {}", field_label, title);
    }

    fn set_description(&mut self, description: Option<String>) {
        self.display_description = description;
    }

    fn append_instructions(&mut self, extra: String) {
        if self.instructions.trim().is_empty() {
            self.instructions = extra;
        } else {
            self.instructions = format!("{} • {}", self.instructions, extra);
        }
    }

    fn apply_component_context(&mut self, field_label: &str, ctx: OverlayContext) {
        if let Some(title) = ctx.title {
            self.update_title(field_label, &title);
        }
        if let Some(description) = ctx.description {
            self.display_description = Some(description);
        }
        if let Some(panel) = ctx.entry_panel {
            self.set_entry_tabs("Entries", panel.entries, panel.selected);
        }
        if let Some(extra) = ctx.instructions {
            self.append_instructions(extra);
        }
    }

    fn title(&self) -> &str {
        &self.display_title
    }

    fn description(&self) -> Option<&String> {
        self.display_description.as_ref()
    }

    fn instructions(&self) -> &str {
        &self.instructions
    }
}

pub(super) fn apply_selection_to_field(
    field: &mut FieldState,
    selection: usize,
    multi: Option<Vec<bool>>,
) {
    if let Some(flags) = multi {
        match &field.schema.kind {
            FieldKind::Composite(_) => {
                field.apply_composite_selection(selection, Some(flags));
            }
            FieldKind::Array(inner) if matches!(inner.as_ref(), FieldKind::Enum { .. }) => {
                field.set_multi_selection(&flags);
            }
            _ => {}
        }
        return;
    }

    match &field.schema.kind {
        FieldKind::Composite(_) => {
            field.apply_composite_selection(selection, None);
        }
        FieldKind::Boolean => field.set_bool(selection == 0),
        FieldKind::Enum { .. } => field.set_enum_selected(selection),
        _ => {}
    }
}

#[derive(Clone)]
pub(super) enum OverlaySession {
    Composite(CompositeEditorSession),
    KeyValue(KeyValueEditorSession),
    Array(ArrayEditorSession),
    Detached,
}

impl OverlaySession {
    fn form_state(&self) -> &FormState {
        match self {
            OverlaySession::Composite(session) => &session.form_state,
            OverlaySession::KeyValue(session) => &session.form_state,
            OverlaySession::Array(session) => &session.form_state,
            OverlaySession::Detached => panic!("overlay session detached"),
        }
    }

    fn form_state_mut(&mut self) -> &mut FormState {
        match self {
            OverlaySession::Composite(session) => &mut session.form_state,
            OverlaySession::KeyValue(session) => &mut session.form_state,
            OverlaySession::Array(session) => &mut session.form_state,
            OverlaySession::Detached => panic!("overlay session detached"),
        }
    }

    fn is_dirty(&self) -> bool {
        self.form_state().is_dirty()
    }

    fn title(&self) -> &str {
        match self {
            OverlaySession::Composite(session) => &session.title,
            OverlaySession::KeyValue(_) => "Entry",
            OverlaySession::Array(session) => &session.title,
            OverlaySession::Detached => panic!("overlay session detached"),
        }
    }

    fn description(&self) -> Option<String> {
        match self {
            OverlaySession::Composite(session) => session.description.clone(),
            OverlaySession::KeyValue(_) => None,
            OverlaySession::Array(session) => session.description.clone(),
            OverlaySession::Detached => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum OverlayHost {
    RootForm,
    Overlay { parent_level: usize },
}

#[derive(Debug, Clone)]
pub(super) enum CompositeOverlayTarget {
    Field,
    ListEntry { entry_index: usize },
    KeyValueEntry { entry_index: usize },
    ArrayEntry { entry_index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OverlayFocusMode {
    FormFields,
    EntryTabs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusOutcome {
    Consumed,
    RequestEntryDelta(i32),
    PassThrough,
}

enum EntryAdvance {
    Collection { selected: usize },
    Variant { variant_index: usize },
}

pub(super) struct CompositeEditorOverlay {
    state: OverlayState,
    store: OverlayStore,
}

impl CompositeEditorOverlay {
    pub(super) fn new(
        field_pointer: String,
        field_label: String,
        level: usize,
        host: OverlayHost,
        session: OverlaySession,
        instructions: String,
    ) -> Self {
        let display_title = format!("Edit {} – {}", field_label, session.title());
        let display_description = session.description();
        let store = OverlayStore::new(display_title, display_description, instructions);
        let state = OverlayState::new(field_pointer, field_label, host, level, session);
        Self { state, store }
    }

    fn build_commit_payload(&self) -> OverlayResult {
        self.state.build_result()
    }

    fn needs_list_panel(&self) -> bool {
        matches!(
            self.state.target(),
            CompositeOverlayTarget::ListEntry { .. }
                | CompositeOverlayTarget::KeyValueEntry { .. }
                | CompositeOverlayTarget::ArrayEntry { .. }
        )
    }

    pub(super) fn form_state(&self) -> &FormState {
        self.state.form_state()
    }

    pub(super) fn form_state_mut(&mut self) -> &mut FormState {
        self.state.form_state_mut()
    }

    pub(super) fn dirty(&self) -> bool {
        self.state.dirty()
    }

    pub(super) fn can_focus_entries(&self) -> bool {
        self.store.can_focus_entries()
    }

    pub(super) fn focus_entries(&mut self) -> bool {
        if self.store.can_focus_entries() {
            self.store.focus_entries();
            true
        } else {
            false
        }
    }

    pub(super) fn focus_form_default(&mut self) -> bool {
        if !self.form_state().has_focusable_fields() {
            return false;
        }
        self.store.focus_form();
        true
    }

    pub(super) fn focus_form_first(&mut self) -> bool {
        if !self.focus_form_default() {
            return false;
        }
        self.form_state_mut().focus_first_field();
        true
    }

    pub(super) fn focus_form_last(&mut self) -> bool {
        if !self.focus_form_default() {
            return false;
        }
        self.form_state_mut().focus_last_field();
        true
    }

    pub(super) fn apply_component_context(&mut self, ctx: OverlayContext) {
        let field_label = self.field_label().to_string();
        self.store.apply_component_context(&field_label, ctx);
    }

    fn entry_tabs_len(&self) -> usize {
        self.store.entry_tabs_len()
    }

    pub(super) fn entry_tabs_selected(&self) -> Option<usize> {
        self.store.entry_tabs_selected()
    }

    pub(super) fn entry_tabs_entries(&self) -> Option<&[String]> {
        self.store.entry_tabs_entries()
    }

    pub(super) fn entry_tabs_label(&self) -> Option<&str> {
        self.store.entry_tabs_label().map(|label| label.as_str())
    }

    fn entry_tabs_snapshot(&self) -> Option<(usize, usize)> {
        self.store.entry_tabs_snapshot()
    }

    fn set_entry_tabs(&mut self, entries: Vec<String>, selected: usize) {
        self.store.set_entry_tabs("Entries", entries, selected);
    }

    fn set_variant_tabs(&mut self, entries: Vec<String>, ids: Vec<usize>, selected: usize) {
        self.store
            .set_variant_tabs("Variants", entries, ids, selected);
        self.store.append_instructions(
            "Ctrl+←/→ switches variants; Tab focuses variant tabs".to_string(),
        );
    }

    pub(super) fn instructions(&self) -> &str {
        self.store.instructions()
    }

    pub(super) fn title(&self) -> &str {
        self.store.title()
    }

    pub(super) fn description(&self) -> Option<&String> {
        self.store.description()
    }

    fn focus_mode(&self) -> OverlayFocusMode {
        self.store.focus()
    }

    pub(super) fn field_label(&self) -> &str {
        self.state.field_label()
    }

    pub(super) fn level(&self) -> usize {
        self.state.level()
    }

    pub(super) fn host(&self) -> OverlayHost {
        self.state.host()
    }

    pub(super) fn field_pointer(&self) -> &str {
        self.state.field_pointer()
    }

    pub(super) fn target(&self) -> &CompositeOverlayTarget {
        self.state.target()
    }

    fn target_mut(&mut self) -> &mut CompositeOverlayTarget {
        self.state.target_mut()
    }

    pub(super) fn set_target(&mut self, target: CompositeOverlayTarget) {
        self.state.set_target(target);
    }

    fn exit_armed(&self) -> bool {
        self.state.exit_armed()
    }

    fn set_exit_armed(&mut self, armed: bool) {
        self.state.set_exit_armed(armed);
    }

    fn validator_clone(&self) -> Option<Arc<Validator>> {
        self.state.validator_clone()
    }

    fn set_validator(&mut self, validator: Option<Arc<Validator>>) {
        self.state.set_validator(validator);
    }

    fn session(&self) -> &OverlaySession {
        self.state.session()
    }

    fn current_variant_index(&self) -> Option<usize> {
        self.state.current_variant_index()
    }

    fn take_composite_session(&mut self) -> Option<CompositeEditorSession> {
        self.state.take_composite_session()
    }

    fn replace_composite_session(&mut self, session: CompositeEditorSession) {
        self.state.replace_composite_session(session);
    }

    fn sync_variant_selection(&mut self, variant_index: usize) {
        self.store.select_entry_by_id(variant_index);
    }

    fn advance_entry_tab(&mut self, delta: i32) -> Option<EntryAdvance> {
        let store = self.store.entry_tabs.as_mut()?;
        if !store.advance(delta) {
            return None;
        }
        match store.kind() {
            EntryTabsKind::Entries => Some(EntryAdvance::Collection {
                selected: store.selected(),
            }),
            EntryTabsKind::Variants => store
                .selected_id()
                .map(|variant_index| EntryAdvance::Variant { variant_index }),
        }
    }

    fn session_kind(&self) -> &'static str {
        match self.session() {
            OverlaySession::Composite(_) => "composite",
            OverlaySession::KeyValue(_) => "keyvalue",
            OverlaySession::Array(_) => "array",
            OverlaySession::Detached => "detached",
        }
    }

    fn validator_cache_key(&self) -> String {
        let target = match self.target() {
            CompositeOverlayTarget::Field => "field",
            CompositeOverlayTarget::ListEntry { .. } => "list",
            CompositeOverlayTarget::KeyValueEntry { .. } => "keyvalue-entry",
            CompositeOverlayTarget::ArrayEntry { .. } => "array-entry",
        };
        format!(
            "{}::{}::{}",
            self.field_pointer(),
            self.session_kind(),
            target
        )
    }

    fn advance_focus(&mut self, direction: FocusDirection) -> FocusOutcome {
        let has_fields = self.form_state().has_focusable_fields();
        let entries_len = self.entry_tabs_len();
        let focus_mode = self.focus_mode();
        match direction {
            FocusDirection::Forward => {
                if focus_mode == OverlayFocusMode::EntryTabs {
                    if has_fields && self.focus_form_first() {
                        return FocusOutcome::Consumed;
                    }
                    if entries_len > 0 {
                        return FocusOutcome::RequestEntryDelta(1);
                    }
                    if self.focus_entries() {
                        return FocusOutcome::Consumed;
                    }
                    return FocusOutcome::Consumed;
                }
                if !has_fields {
                    if entries_len > 0 {
                        return FocusOutcome::RequestEntryDelta(1);
                    }
                    if self.focus_entries() {
                        return FocusOutcome::Consumed;
                    }
                    return FocusOutcome::Consumed;
                }
                if self.form_state().focus_is_last() {
                    if entries_len > 0 {
                        return FocusOutcome::RequestEntryDelta(1);
                    }
                    if self.focus_entries() {
                        return FocusOutcome::Consumed;
                    }
                    return FocusOutcome::Consumed;
                }
                FocusOutcome::PassThrough
            }
            FocusDirection::Backward => {
                if focus_mode == OverlayFocusMode::EntryTabs {
                    if has_fields && self.focus_form_last() {
                        return FocusOutcome::Consumed;
                    }
                    if entries_len > 0 {
                        return FocusOutcome::RequestEntryDelta(-1);
                    }
                    if self.focus_entries() {
                        return FocusOutcome::Consumed;
                    }
                    return FocusOutcome::Consumed;
                }
                if !has_fields {
                    if entries_len > 0 {
                        return FocusOutcome::RequestEntryDelta(-1);
                    }
                    if self.focus_entries() {
                        return FocusOutcome::Consumed;
                    }
                    return FocusOutcome::Consumed;
                }
                if self.form_state().focus_is_first() {
                    if entries_len > 0 {
                        return FocusOutcome::RequestEntryDelta(-1);
                    }
                    if self.focus_entries() {
                        return FocusOutcome::Consumed;
                    }
                    return FocusOutcome::Consumed;
                }
                FocusOutcome::PassThrough
            }
        }
    }
}

impl App {
    pub(crate) fn overlay_depth(&self) -> usize {
        self.overlay_stack.len()
    }

    pub(crate) fn active_overlay(&self) -> Option<&CompositeEditorOverlay> {
        self.overlay_stack.last()
    }

    pub(crate) fn active_overlay_mut(&mut self) -> Option<&mut CompositeEditorOverlay> {
        self.overlay_stack.last_mut()
    }

    fn overlay_help_text(&self) -> String {
        let base = self
            .keymap_store
            .help_text(KeymapContext::Overlay)
            .unwrap_or_else(|| "Ctrl+S save • Esc/Ctrl+Q exit overlay".to_string());
        if let Some(editor) = self.active_overlay() {
            format!("L{} · {}", editor.level(), base)
        } else {
            base
        }
    }

    fn set_overlay_status_message(&mut self) {
        if let Some(editor) = self.active_overlay() {
            let help = self.overlay_help_text();
            self.status
                .set_raw(format!("Overlay {}: {}", editor.level(), help));
        }
    }

    pub(crate) fn host_form_state(&self, host: OverlayHost) -> &FormState {
        match host {
            OverlayHost::RootForm => &self.form_state,
            OverlayHost::Overlay { parent_level } => {
                let idx = parent_level.saturating_sub(1);
                self.overlay_stack[idx].form_state()
            }
        }
    }

    pub(crate) fn host_form_state_mut(&mut self, host: OverlayHost) -> &mut FormState {
        match host {
            OverlayHost::RootForm => &mut self.form_state,
            OverlayHost::Overlay { parent_level } => {
                let idx = parent_level.saturating_sub(1);
                self.overlay_stack
                    .get_mut(idx)
                    .expect("overlay host should exist")
                    .form_state_mut()
            }
        }
    }

    fn initialize_active_overlay(&mut self) {
        self.set_overlay_status_message();
        self.refresh_list_overlay_panel();
        self.setup_overlay_validator();
        self.run_overlay_validation();
        self.reset_overlay_focus_mode();
    }

    fn reset_overlay_focus_mode(&mut self) {
        if let Some(editor) = self.active_overlay_mut()
            && !editor.focus_entries()
        {
            editor.focus_form_first();
        }
    }

    pub(crate) fn try_open_composite_editor(&mut self) {
        let overlay_help_text = self.overlay_help_text().to_string();
        let level = self.overlay_depth() + 1;
        let host = if level == 1 {
            OverlayHost::RootForm
        } else {
            OverlayHost::Overlay {
                parent_level: level - 1,
            }
        };
        let previous_depth = self.overlay_depth();

        let field_result = if let Some(editor) = self.active_overlay_mut() {
            editor.form_state_mut().focused_field_mut()
        } else {
            self.form_state.focused_field_mut()
        };

        let Some(field) = field_result else {
            self.status.set_raw("No field selected");
            return;
        };
        let component_context = field.overlay_context();

        match &field.schema.kind {
            FieldKind::Composite(template) => {
                let schema = template.as_ref();
                let mut active = field.active_composite_variants();
                if active.is_empty()
                    && matches!(schema.mode, CompositeMode::AnyOf)
                    && !schema.variants.is_empty()
                {
                    let mut flags = vec![false; schema.variants.len()];
                    flags[0] = true;
                    field.apply_composite_selection(0, Some(flags));
                    active = field.active_composite_variants();
                }
                let Some(&variant_index) = active.first() else {
                    self.status
                        .set_raw("Select a variant via Enter before editing (oneOf/anyOf)");
                    return;
                };
                let pointer = field.schema.pointer.clone();
                let label = field.schema.display_label();
                match field.open_composite_editor(variant_index) {
                    Ok(session) => {
                        let mut overlay = CompositeEditorOverlay::new(
                            pointer,
                            label,
                            level,
                            host,
                            OverlaySession::Composite(session),
                            overlay_help_text.clone(),
                        );
                        if let Some((labels, indices)) =
                            Self::variant_tab_entries_for_field(field, overlay.target())
                        {
                            let selected = overlay.current_variant_index().unwrap_or(0);
                            overlay.set_variant_tabs(labels, indices, selected);
                        }
                        let _ = field;
                        self.popup = None;
                        self.overlay_stack.push(overlay);
                        self.initialize_active_overlay();
                    }
                    Err(err) => self.status.set_raw(&err.message),
                }
            }
            FieldKind::Array(inner) if matches!(inner.as_ref(), FieldKind::Composite(_)) => {
                let pointer = field.schema.pointer.clone();
                let label = field.schema.display_label();
                if field.composite_list_selected_index().is_none()
                    && !field.composite_list_add_entry()
                {
                    self.status
                        .set_raw("Unable to auto-create the first entry; use Ctrl+N");
                    return;
                }
                match field.open_composite_list_editor() {
                    Ok(context) => {
                        let mut overlay = CompositeEditorOverlay::new(
                            pointer,
                            label,
                            level,
                            host,
                            OverlaySession::Composite(context.session),
                            overlay_help_text.clone(),
                        );
                        overlay.set_target(CompositeOverlayTarget::ListEntry {
                            entry_index: context.entry_index,
                        });
                        if let Some((labels, indices)) =
                            Self::variant_tab_entries_for_field(field, overlay.target())
                        {
                            let selected = overlay.current_variant_index().unwrap_or(0);
                            overlay.set_variant_tabs(labels, indices, selected);
                        }
                        let _ = field;
                        self.popup = None;
                        self.overlay_stack.push(overlay);
                        self.initialize_active_overlay();
                    }
                    Err(err) => self.status.set_raw(&err.message),
                }
            }
            FieldKind::KeyValue(_) => {
                let pointer = field.schema.pointer.clone();
                let label = field.schema.display_label();
                match field.open_key_value_editor() {
                    Ok(context) => {
                        self.popup = None;
                        let mut overlay = CompositeEditorOverlay::new(
                            pointer,
                            label,
                            level,
                            host,
                            OverlaySession::KeyValue(context.session),
                            self.overlay_help_text(),
                        );
                        overlay.set_target(CompositeOverlayTarget::KeyValueEntry {
                            entry_index: context.entry_index,
                        });
                        self.overlay_stack.push(overlay);
                        self.initialize_active_overlay();
                    }
                    Err(err) => self.status.set_raw(&err.message),
                }
            }
            FieldKind::Array(inner)
                if matches!(
                    inner.as_ref(),
                    FieldKind::String
                        | FieldKind::Integer
                        | FieldKind::Number
                        | FieldKind::Boolean
                        | FieldKind::Nullable(_)
                ) =>
            {
                let pointer = field.schema.pointer.clone();
                let label = field.schema.display_label();
                if field.composite_list_selected_index().is_none()
                    && !field.composite_list_add_entry()
                {
                    self.status
                        .set_raw("Unable to auto-create the first entry; use Ctrl+N");
                    return;
                }
                match field.open_scalar_array_editor() {
                    Ok(context) => {
                        self.popup = None;
                        let mut overlay = CompositeEditorOverlay::new(
                            pointer,
                            label,
                            level,
                            host,
                            OverlaySession::Array(context.session),
                            self.overlay_help_text(),
                        );
                        overlay.set_target(CompositeOverlayTarget::ArrayEntry {
                            entry_index: context.entry_index,
                        });
                        self.overlay_stack.push(overlay);
                        self.initialize_active_overlay();
                    }
                    Err(err) => self.status.set_raw(&err.message),
                }
            }
            _ => {
                self.status
                    .set_raw("Focus a composite or composite list field before editing");
            }
        }

        if self.overlay_depth() > previous_depth
            && let Some(ctx) = component_context
            && let Some(editor) = self.active_overlay_mut()
        {
            editor.apply_component_context(ctx);
        }
    }

    pub(super) fn close_active_overlay(&mut self, commit: bool) {
        let Some(mut overlay) = self.overlay_stack.pop() else {
            return;
        };
        self.overlay_validator_cache
            .remove(&overlay.validator_cache_key());
        self.popup = None;
        if commit {
            match self.apply_overlay_commit(&overlay) {
                Ok(()) => {
                    overlay.form_state_mut().mark_clean();
                    overlay.set_exit_armed(false);
                    self.exit_armed = false;
                    self.status.value_updated();
                    if overlay.level() == 1 && self.options.auto_validate {
                        self.run_validation(false);
                    } else {
                        self.run_overlay_validation();
                    }
                }
                Err(message) => {
                    self.status.set_raw(&message);
                    self.overlay_stack.push(overlay);
                    return;
                }
            }
        } else {
            self.status.ready();
        }

        if let Some(parent) = self.active_overlay_mut() {
            parent.set_exit_armed(false);
            self.set_overlay_status_message();
            self.refresh_list_overlay_panel();
            self.run_overlay_validation();
        }
    }

    fn apply_overlay_commit(&mut self, overlay: &CompositeEditorOverlay) -> Result<(), String> {
        let payload = overlay.build_commit_payload();
        let host_state = self.host_form_state_mut(payload.host());
        payload.apply(host_state)
    }

    pub(crate) fn handle_composite_editor_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.code == KeyCode::Esc {
            if !self.request_overlay_exit() {
                return Ok(());
            }
            return Ok(());
        }

        let dispatch = self
            .options
            .keymap
            .resolve(self.input_router.classify(&key));
        match dispatch {
            CommandDispatch::Form(command) => {
                if self.handle_overlay_focus_command(&command) {
                    return Ok(());
                }
                if let Some(editor) = self.active_overlay_mut() {
                    editor.set_exit_armed(false);
                    apply_command(editor.form_state_mut(), command.clone());
                    self.run_overlay_validation();
                }
            }
            CommandDispatch::App(command) => {
                if self.handle_overlay_app_command(command)? {
                    return Ok(());
                }
            }
            CommandDispatch::Input(event) => {
                self.handle_overlay_field_input(&event);
            }
            CommandDispatch::None => {}
        }

        Ok(())
    }

    fn advance_overlay_entry(&mut self, delta: i32) -> bool {
        let (action, field_pointer, host) = {
            let editor = match self.active_overlay_mut() {
                Some(editor) => editor,
                None => return false,
            };
            if !editor.can_focus_entries() {
                return false;
            }
            if editor.entry_tabs_snapshot().is_none() {
                return false;
            }
            let pointer = editor.field_pointer().to_string();
            let host = editor.host();
            let Some(next) = editor.advance_entry_tab(delta) else {
                editor.set_exit_armed(false);
                if !editor.focus_entries() {
                    editor.focus_form_first();
                }
                return true;
            };
            (next, pointer, host)
        };

        match action {
            EntryAdvance::Variant { variant_index } => self.switch_overlay_variant(variant_index),
            EntryAdvance::Collection {
                selected: next_index,
            } => {
                let previous_depth = self.overlay_depth();
                self.close_active_overlay(true);
                if self.overlay_depth() != previous_depth.saturating_sub(1) {
                    return false;
                }

                let (changed, label) = {
                    let host_state = self.host_form_state_mut(host);
                    let Some(field) = host_state.field_mut_by_pointer(&field_pointer) else {
                        return false;
                    };
                    let changed = field.collection_set_selected(next_index);
                    let label = field.collection_selected_label();
                    (changed, label)
                };

                self.exit_armed = false;
                self.status.value_updated();
                if let Some(label) = label {
                    self.status.set_raw(format!("Selected entry {}", label));
                } else if !changed {
                    self.status.ready();
                }

                let expected_depth = previous_depth;
                self.try_open_composite_editor();
                if self.overlay_depth() != expected_depth {
                    return false;
                }

                if let Some(editor) = self.active_overlay_mut() {
                    editor.set_exit_armed(false);
                    if !editor.focus_entries() {
                        editor.focus_form_first();
                    }
                }

                true
            }
        }
    }

    fn switch_overlay_variant(&mut self, variant_index: usize) -> bool {
        let (field_pointer, host) = {
            let editor = match self.active_overlay() {
                Some(editor) => editor,
                None => return false,
            };
            match editor.session() {
                OverlaySession::Composite(_) => {}
                _ => return false,
            }
            let current = editor.current_variant_index().unwrap_or(variant_index);
            if current == variant_index {
                return true;
            }
            (editor.field_pointer().to_string(), editor.host())
        };

        let old_session = {
            let editor = match self.active_overlay_mut() {
                Some(editor) => editor,
                None => return false,
            };
            match editor.take_composite_session() {
                Some(session) => session,
                None => return false,
            }
        };

        let host_state = self.host_form_state_mut(host);
        let Some(field) = host_state.field_mut_by_pointer(&field_pointer) else {
            if let Some(editor) = self.active_overlay_mut() {
                editor.replace_composite_session(old_session);
            }
            return false;
        };

        let old_index = old_session.variant_index;
        field.close_composite_editor(old_session, false);

        let new_session = match field.open_composite_editor(variant_index) {
            Ok(session) => session,
            Err(err) => {
                if let Ok(restored) = field.open_composite_editor(old_index)
                    && let Some(editor) = self.active_overlay_mut()
                {
                    editor.replace_composite_session(restored);
                    editor.sync_variant_selection(old_index);
                }
                self.status.set_raw(&err.message);
                return false;
            }
        };

        if let Some(editor) = self.active_overlay_mut() {
            editor.replace_composite_session(new_session);
            editor.sync_variant_selection(variant_index);
            editor.set_exit_armed(false);
            if !editor.focus_entries() {
                editor.focus_form_first();
            }
        }

        self.exit_armed = false;
        self.status
            .set_raw(format!("Switched to variant #{}", variant_index + 1));
        self.setup_overlay_validator();
        self.run_overlay_validation();
        true
    }

    fn variant_tab_entries_for_field(
        field: &FieldState,
        target: &CompositeOverlayTarget,
    ) -> Option<(Vec<String>, Vec<usize>)> {
        match (target, &field.schema.kind) {
            (CompositeOverlayTarget::Field, FieldKind::Composite(meta))
                if matches!(meta.mode, CompositeMode::AnyOf) =>
            {
                let view = field.composite_selector_view()?;
                Self::variant_entries_from_view(&view)
            }
            (CompositeOverlayTarget::ListEntry { .. }, FieldKind::Array(inner)) if matches!(inner.as_ref(), FieldKind::Composite(meta) if matches!(meta.mode, CompositeMode::AnyOf)) =>
            {
                let view = field.composite_entry_selector_view()?;
                Self::variant_entries_from_view(&view)
            }
            _ => None,
        }
    }

    fn variant_entries_from_view(
        view: &CompositeSelectorView,
    ) -> Option<(Vec<String>, Vec<usize>)> {
        let mut labels = Vec::new();
        let mut indices = Vec::new();
        for (idx, option) in view.options.iter().enumerate() {
            if view.active.get(idx).copied().unwrap_or(false) {
                labels.push(format!("#{} {}", idx + 1, option));
                indices.push(idx);
            }
        }
        if labels.is_empty() {
            None
        } else {
            Some((labels, indices))
        }
    }

    fn handle_overlay_focus_command(&mut self, command: &FormCommand) -> bool {
        if !matches!(
            command,
            FormCommand::FocusNextField | FormCommand::FocusPrevField
        ) {
            return false;
        }
        let direction = match command {
            FormCommand::FocusNextField => FocusDirection::Forward,
            FormCommand::FocusPrevField => FocusDirection::Backward,
            _ => return false,
        };
        let outcome = {
            let editor = match self.active_overlay_mut() {
                Some(editor) => editor,
                None => return false,
            };
            editor.set_exit_armed(false);
            editor.advance_focus(direction)
        };
        match outcome {
            FocusOutcome::Consumed => true,
            FocusOutcome::RequestEntryDelta(delta) => self.advance_overlay_entry(delta),
            FocusOutcome::PassThrough => false,
        }
    }

    pub(crate) fn request_overlay_exit(&mut self) -> bool {
        if let Some(editor) = self.active_overlay_mut()
            && editor.dirty()
            && !editor.exit_armed()
        {
            editor.set_exit_armed(true);
            self.status
                .set_raw("Overlay dirty. Press Esc again to discard changes.");
            return false;
        }
        self.close_active_overlay(false);
        true
    }

    pub(crate) fn save_active_overlay(&mut self) -> bool {
        let Some(mut overlay) = self.overlay_stack.pop() else {
            return false;
        };
        match self.apply_overlay_commit(&overlay) {
            Ok(()) => {
                overlay.form_state_mut().mark_clean();
                overlay.set_exit_armed(false);
                self.status
                    .set_raw(format!("Overlay {} saved.", overlay.level()));
                if overlay.level() == 1 && self.options.auto_validate {
                    self.run_validation(false);
                } else {
                    self.run_overlay_validation();
                }
                self.overlay_stack.push(overlay);
                self.set_overlay_status_message();
                self.refresh_list_overlay_panel();
                true
            }
            Err(message) => {
                self.status.set_raw(&message);
                self.overlay_stack.push(overlay);
                false
            }
        }
    }

    fn handle_overlay_field_input(&mut self, event: &KeyEvent) {
        let Some(result) = ({
            let editor = match self.active_overlay_mut() {
                Some(editor) => editor,
                None => return,
            };
            editor.set_exit_armed(false);
            let field_label = editor.field_label().to_string();
            if let Some(field) = editor.form_state_mut().focused_field_mut()
                && field.handle_key(event)
            {
                Some((
                    field_label,
                    field.schema.display_label(),
                    field.schema.pointer.clone(),
                ))
            } else {
                None
            }
        }) else {
            return;
        };

        let (parent_label, child_label, pointer) = result;
        self.status
            .editing(&format!("{parent_label} › {child_label}"));
        self.validate_overlay_field(pointer);
    }

    fn validate_overlay_field(&mut self, pointer: String) {
        let Some(editor) = self.active_overlay_mut() else {
            return;
        };
        let Some(validator) = editor.validator_clone() else {
            return;
        };
        let mut engine = FormEngine::new(editor.form_state_mut(), &validator);
        if let Err(message) = engine.dispatch(FormCommand::FieldEdited { pointer }) {
            self.status.set_raw(&message);
        }
    }

    pub(crate) fn apply_popup_selection_data(
        &mut self,
        owner: PopupOwner,
        pointer: &str,
        selection: usize,
        multi: Option<Vec<bool>>,
    ) {
        match owner {
            PopupOwner::Root => {
                if let Some(field) = self.form_state.field_mut_by_pointer(pointer) {
                    apply_selection_to_field(field, selection, multi);
                }
            }
            PopupOwner::Composite => {
                if let Some(editor) = self.active_overlay_mut()
                    && let Some(field) = editor.form_state_mut().field_mut_by_pointer(pointer)
                {
                    apply_selection_to_field(field, selection, multi);
                    self.run_overlay_validation();
                }
            }
            PopupOwner::VariantSelector {
                field_pointer,
                overlay_host,
            } => {
                // Handle variant selector: add entry with selected variant
                self.handle_variant_selector_result(&field_pointer, overlay_host, selection);
            }
        }
    }

    pub(crate) fn setup_overlay_validator(&mut self) {
        let Some(cache_key) = self
            .active_overlay()
            .map(|editor| editor.validator_cache_key())
        else {
            return;
        };
        if let Some(cached) = self.overlay_validator_cache.get(&cache_key).cloned() {
            if let Some(editor) = self.active_overlay_mut() {
                editor.set_validator(Some(cached));
            }
            self.run_overlay_validation();
            return;
        }
        let validator = {
            let Some(editor) = self.active_overlay() else {
                return;
            };
            match editor.session() {
                OverlaySession::Composite(session) => {
                    validator_for(&session.schema).ok().map(Arc::new)
                }
                OverlaySession::KeyValue(session) => {
                    validator_for(&session.schema).ok().map(Arc::new)
                }
                OverlaySession::Array(session) => validator_for(&session.schema).ok().map(Arc::new),
                OverlaySession::Detached => return,
            }
        };
        if let Some(valid) = &validator {
            self.overlay_validator_cache
                .insert(cache_key, valid.clone());
        }
        if let Some(editor) = self.active_overlay_mut() {
            editor.set_validator(validator);
        }
        self.run_overlay_validation();
    }

    pub(crate) fn run_overlay_validation(&mut self) {
        let pointer = {
            let Some(editor) = self.active_overlay() else {
                return;
            };
            editor
                .form_state()
                .focused_field()
                .map(|field| field.schema.pointer.clone())
        };
        if let Some(pointer) = pointer {
            self.validate_overlay_field(pointer);
        }
    }

    pub(crate) fn refresh_list_overlay_panel(&mut self) {
        let Some(mut overlay) = self.overlay_stack.pop() else {
            return;
        };
        if !overlay.needs_list_panel() {
            self.overlay_stack.push(overlay);
            return;
        }
        let data = {
            let host_state = self.host_form_state(overlay.host());
            host_state
                .field_by_pointer(overlay.field_pointer())
                .map(|field| {
                    (
                        field.composite_list_panel(),
                        field.composite_list_selected_label(),
                        field.composite_list_selected_index(),
                    )
                })
        };
        if let Some((panel, label, idx)) = data {
            if let Some((entries, selected)) = panel {
                overlay.set_entry_tabs(entries, selected);
            }
            if let Some(label) = label {
                let field_label = overlay.field_label().to_string();
                overlay.store.update_title(&field_label, &label);
                overlay.store.set_description(Some(label));
            }
            if let Some(index) = idx {
                match overlay.target_mut() {
                    CompositeOverlayTarget::ListEntry { entry_index }
                    | CompositeOverlayTarget::KeyValueEntry { entry_index }
                    | CompositeOverlayTarget::ArrayEntry { entry_index } => {
                        *entry_index = index;
                    }
                    _ => {}
                }
            }
        }
        self.overlay_stack.push(overlay);
    }

    pub(crate) fn handle_overlay_app_command(&mut self, command: AppCommand) -> Result<bool> {
        match command {
            AppCommand::Save => {
                self.save_active_overlay();
                return Ok(true);
            }
            AppCommand::Quit => {
                self.request_overlay_exit();
                return Ok(true);
            }
            AppCommand::EditComposite => {
                self.try_open_composite_editor();
                return Ok(true);
            }
            AppCommand::TogglePopup => {
                if self.try_open_popup(PopupOwner::Composite) {
                    return Ok(true);
                }
            }
            AppCommand::ResetStatus => {
                self.status.ready();
                if let Some(editor) = self.active_overlay_mut() {
                    editor.set_exit_armed(false);
                }
                return Ok(true);
            }
            AppCommand::ListAddEntry => {
                if self.handle_list_add_entry() {
                    return Ok(true);
                }
            }
            AppCommand::ListRemoveEntry => {
                if self.handle_list_remove_entry() {
                    return Ok(true);
                }
            }
            AppCommand::ListMove(delta) => {
                if self.handle_list_move_entry(delta) {
                    return Ok(true);
                }
            }
            AppCommand::ListSelect(delta) => {
                if self.handle_list_select_entry(delta) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[cfg(all(test, feature = "json"))]
impl App {
    pub(crate) fn overlay_depth_for_test(&self) -> usize {
        self.overlay_depth()
    }

    pub(crate) fn open_overlay_for_test(&mut self) {
        self.try_open_composite_editor();
    }

    pub(crate) fn active_overlay_form_state_for_test(&mut self) -> Option<&mut FormState> {
        self.active_overlay_mut()
            .map(|overlay| overlay.form_state_mut())
    }

    pub(crate) fn overlay_entry_focus_for_test(&self) -> Option<bool> {
        self.active_overlay()
            .map(|overlay| overlay.focus_mode() == OverlayFocusMode::EntryTabs)
    }

    pub(crate) fn overlay_selected_entry_for_test(&self) -> Option<usize> {
        self.active_overlay()
            .and_then(|overlay| overlay.entry_tabs_selected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        tui::app::options::UiOptions,
        tui::model::{FieldKind, FieldSchema},
        tui::state::{FieldState, FormState, SectionState},
    };
    use serde_json::json;
    use std::collections::HashMap;

    fn scalar_array_field_state() -> FieldState {
        let schema = FieldSchema {
            name: "allowed_methods".to_string(),
            path: vec!["allowed_methods".to_string()],
            pointer: "/allowed_methods".to_string(),
            title: "Allowed Methods".to_string(),
            description: None,
            kind: FieldKind::Array(Box::new(FieldKind::String)),
            required: false,
            default: Some(json!(["GET"])),
            metadata: HashMap::new(),
        };
        FieldState::from_schema(schema)
    }

    fn build_app_with_scalar_array() -> App {
        let section = SectionState {
            id: "section".to_string(),
            title: "Section".to_string(),
            description: None,
            path: vec!["app".to_string()],
            depth: 0,
            fields: vec![scalar_array_field_state()],
            scroll_offset: 0,
        };
        let form_state = FormState::from_sections("app", "App", None, vec![section]);
        let validator = validator_for(&json!({"type": "object"})).expect("validator");
        App::new(form_state, validator, UiOptions::default())
    }

    #[test]
    fn ctrl_e_opens_scalar_array_overlay() {
        let mut app = build_app_with_scalar_array();
        app.try_open_composite_editor();
        assert!(
            matches!(
                app.active_overlay().map(|overlay| overlay.target()),
                Some(CompositeOverlayTarget::ArrayEntry { .. })
            ),
            "scalar arrays should open overlay via Ctrl+E"
        );
        assert_eq!(app.overlay_depth(), 1);
    }
}
