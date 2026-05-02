use std::sync::Arc;

use jsonschema::Validator;

use crate::tui::state::field::components::helpers::OverlayContext;
use crate::tui::state::{
    ArrayEditorSession, CompositeEditorSession, FormState, KeyValueEditorSession,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntryTabsKind {
    Entries,
    Variants,
}

#[derive(Debug, Clone)]
pub(super) struct EntryTabsStore {
    entries: Vec<String>,
    ids: Vec<usize>,
    selected: usize,
    kind: EntryTabsKind,
}

impl EntryTabsStore {
    pub(super) fn new(entries: Vec<String>, selected: usize) -> Self {
        let ids = (0..entries.len()).collect();
        Self::with_kind(entries, ids, selected, EntryTabsKind::Entries)
    }

    pub(super) fn with_kind(
        entries: Vec<String>,
        ids: Vec<usize>,
        selected_id: usize,
        kind: EntryTabsKind,
    ) -> Self {
        let mut store = Self {
            entries,
            ids,
            selected: 0,
            kind,
        };
        store.select_by_id(selected_id);
        store
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn entries(&self) -> &[String] {
        &self.entries
    }

    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    pub(super) fn selected_id(&self) -> Option<usize> {
        self.ids.get(self.selected).copied()
    }

    pub(super) fn kind(&self) -> EntryTabsKind {
        self.kind
    }

    pub(super) fn select(&mut self, index: usize) {
        if self.entries.is_empty() {
            self.selected = 0;
        } else {
            self.selected = index.min(self.entries.len().saturating_sub(1));
        }
    }

    pub(super) fn set_entries(&mut self, entries: Vec<String>, selected: usize) {
        self.ids = (0..entries.len()).collect();
        self.entries = entries;
        self.kind = EntryTabsKind::Entries;
        self.select(selected);
    }

    pub(super) fn set_entries_with_ids(
        &mut self,
        entries: Vec<String>,
        ids: Vec<usize>,
        selected_id: usize,
    ) {
        self.entries = entries;
        self.ids = ids;
        self.kind = EntryTabsKind::Variants;
        self.select_by_id(selected_id);
    }

    pub(super) fn select_by_id(&mut self, id: usize) {
        if let Some(pos) = self.ids.iter().position(|candidate| *candidate == id) {
            self.select(pos);
        } else {
            self.select(0);
        }
    }

    pub(super) fn advance(&mut self, delta: i32) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let len = self.entries.len() as i32;
        let mut next = self.selected as i32 + delta;
        next = ((next % len) + len) % len;
        let next = next as usize;
        if next == self.selected {
            false
        } else {
            self.selected = next;
            true
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct OverlayStore {
    pub(super) display_title: String,
    pub(super) display_description: Option<String>,
    pub(super) instructions: String,
    pub(super) entry_tabs: Option<EntryTabsStore>,
    pub(super) entry_label: Option<String>,
    pub(super) focus: OverlayFocusMode,
}

impl OverlayStore {
    pub(super) fn new(
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

    pub(super) fn entry_tabs_len(&self) -> usize {
        self.entry_tabs.as_ref().map(|tabs| tabs.len()).unwrap_or(0)
    }

    pub(super) fn entry_tabs_selected(&self) -> Option<usize> {
        self.entry_tabs.as_ref().map(|tabs| tabs.selected())
    }

    pub(super) fn entry_tabs_entries(&self) -> Option<&[String]> {
        self.entry_tabs.as_ref().map(|tabs| tabs.entries())
    }

    pub(super) fn entry_tabs_label(&self) -> Option<&String> {
        self.entry_label.as_ref()
    }

    pub(super) fn entry_tabs_snapshot(&self) -> Option<(usize, usize)> {
        self.entry_tabs
            .as_ref()
            .map(|tabs| (tabs.len(), tabs.selected()))
    }

    pub(super) fn can_focus_entries(&self) -> bool {
        self.entry_tabs
            .as_ref()
            .map(|tabs| !tabs.is_empty())
            .unwrap_or(false)
    }

    pub(super) fn focus_entries(&mut self) {
        self.focus = OverlayFocusMode::EntryTabs;
    }

    pub(super) fn focus_form(&mut self) {
        self.focus = OverlayFocusMode::FormFields;
    }

    pub(super) fn focus(&self) -> OverlayFocusMode {
        self.focus
    }

    pub(super) fn set_entry_tabs(
        &mut self,
        label: impl Into<String>,
        entries: Vec<String>,
        selected: usize,
    ) {
        if let Some(store) = self.entry_tabs.as_mut() {
            store.set_entries(entries, selected);
        } else {
            self.entry_tabs = Some(EntryTabsStore::new(entries, selected));
        }
        self.entry_label = Some(label.into());
    }

    pub(super) fn set_variant_tabs(
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

    pub(super) fn update_title(&mut self, field_label: &str, title: &str) {
        self.display_title = format!("Edit {} – {}", field_label, title);
    }

    pub(super) fn set_description(&mut self, description: Option<String>) {
        self.display_description = description;
    }

    pub(super) fn append_instructions(&mut self, extra: String) {
        if self.instructions.trim().is_empty() {
            self.instructions = extra;
        } else {
            self.instructions = format!("{} • {}", self.instructions, extra);
        }
    }

    pub(super) fn apply_component_context(&mut self, field_label: &str, ctx: OverlayContext) {
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

    pub(super) fn title(&self) -> &str {
        &self.display_title
    }

    pub(super) fn description(&self) -> Option<&String> {
        self.display_description.as_ref()
    }

    pub(super) fn instructions(&self) -> &str {
        &self.instructions
    }
}

#[derive(Clone)]
pub(super) struct OverlayResult {
    pub(super) field_pointer: String,
    pub(super) host: OverlayHost,
    pub(super) target: CompositeOverlayTarget,
    pub(super) session: OverlaySession,
}

impl OverlayResult {
    pub(super) fn host(&self) -> OverlayHost {
        self.host
    }

    pub(super) fn apply(self, host_state: &mut FormState) -> Result<(), String> {
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

#[derive(Debug, Clone)]
pub(super) struct OverlayState {
    pub(super) field_pointer: String,
    pub(super) field_label: String,
    pub(super) host: OverlayHost,
    pub(super) level: usize,
    pub(super) target: CompositeOverlayTarget,
    pub(super) session: OverlaySession,
    pub(super) exit_armed: bool,
    pub(super) validator: Option<Arc<Validator>>,
}

impl OverlayState {
    pub(super) fn new(
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

    pub(super) fn dirty(&self) -> bool {
        self.session.is_dirty()
    }

    pub(super) fn form_state(&self) -> &FormState {
        self.session.form_state()
    }

    pub(super) fn form_state_mut(&mut self) -> &mut FormState {
        self.session.form_state_mut()
    }

    pub(super) fn session(&self) -> &OverlaySession {
        if matches!(self.session, OverlaySession::Detached) {
            panic!("overlay session detached");
        }
        &self.session
    }

    pub(super) fn take_composite_session(&mut self) -> Option<CompositeEditorSession> {
        match std::mem::replace(&mut self.session, OverlaySession::Detached) {
            OverlaySession::Composite(session) => Some(session),
            other => {
                self.session = other;
                None
            }
        }
    }

    pub(super) fn replace_composite_session(&mut self, session: CompositeEditorSession) {
        self.session = OverlaySession::Composite(session);
    }

    pub(super) fn current_variant_index(&self) -> Option<usize> {
        match &self.session {
            OverlaySession::Composite(session) => Some(session.variant_index),
            _ => None,
        }
    }

    pub(super) fn field_pointer(&self) -> &str {
        &self.field_pointer
    }

    pub(super) fn field_label(&self) -> &str {
        &self.field_label
    }

    pub(super) fn level(&self) -> usize {
        self.level
    }

    pub(super) fn host(&self) -> OverlayHost {
        self.host
    }

    pub(super) fn target(&self) -> &CompositeOverlayTarget {
        &self.target
    }

    pub(super) fn target_mut(&mut self) -> &mut CompositeOverlayTarget {
        &mut self.target
    }

    pub(super) fn set_target(&mut self, target: CompositeOverlayTarget) {
        self.target = target;
    }

    pub(super) fn exit_armed(&self) -> bool {
        self.exit_armed
    }

    pub(super) fn set_exit_armed(&mut self, armed: bool) {
        self.exit_armed = armed;
    }

    pub(super) fn set_validator(&mut self, validator: Option<Arc<Validator>>) {
        self.validator = validator;
    }

    pub(super) fn validator_clone(&self) -> Option<Arc<Validator>> {
        self.validator.clone()
    }

    pub(super) fn build_result(&self) -> OverlayResult {
        OverlayResult {
            field_pointer: self.field_pointer.clone(),
            host: self.host,
            target: self.target.clone(),
            session: self.session.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum OverlaySession {
    Composite(CompositeEditorSession),
    KeyValue(KeyValueEditorSession),
    Array(ArrayEditorSession),
    Detached,
}

impl OverlaySession {
    pub(super) fn form_state(&self) -> &FormState {
        match self {
            OverlaySession::Composite(session) => &session.form_state,
            OverlaySession::KeyValue(session) => &session.form_state,
            OverlaySession::Array(session) => &session.form_state,
            OverlaySession::Detached => panic!("overlay session detached"),
        }
    }

    pub(super) fn form_state_mut(&mut self) -> &mut FormState {
        match self {
            OverlaySession::Composite(session) => &mut session.form_state,
            OverlaySession::KeyValue(session) => &mut session.form_state,
            OverlaySession::Array(session) => &mut session.form_state,
            OverlaySession::Detached => panic!("overlay session detached"),
        }
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.form_state().is_dirty()
    }

    pub(super) fn title(&self) -> &str {
        match self {
            OverlaySession::Composite(session) => &session.title,
            OverlaySession::KeyValue(_) => "Entry",
            OverlaySession::Array(session) => &session.title,
            OverlaySession::Detached => panic!("overlay session detached"),
        }
    }

    pub(super) fn description(&self) -> Option<String> {
        match self {
            OverlaySession::Composite(session) => session.description.clone(),
            OverlaySession::KeyValue(_) => None,
            OverlaySession::Array(session) => session.description.clone(),
            OverlaySession::Detached => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum OverlayHost {
    RootForm,
    Overlay { parent_level: usize },
}

#[derive(Debug, Clone)]
pub(crate) enum CompositeOverlayTarget {
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
pub(super) enum FocusDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FocusOutcome {
    Consumed,
    RequestEntryDelta(i32),
    PassThrough,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum EntryAdvance {
    Collection { selected: usize },
    Variant { variant_index: usize },
}
