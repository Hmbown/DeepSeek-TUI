use std::sync::Arc;

use serde_json::Value;

use crate::tui::model::{FieldSchema, KeyValueField};
use crate::tui::state::error::FieldCoercionError;
use crate::tui::state::key_value::{KeyValueEditorContext, KeyValueEditorSession, KeyValueState};

use super::helpers::{EntryPanelState, OverlayContext, format_collection_value, list_hint_for};
use super::{ComponentKind, FieldComponent, palette::ComponentPalette};

#[derive(Debug, Clone)]
pub struct KeyValueComponent {
    state: KeyValueState,
    palette: Arc<ComponentPalette>,
}

impl KeyValueComponent {
    pub fn new(
        pointer: &str,
        template: &KeyValueField,
        default: Option<&Value>,
        palette: Arc<ComponentPalette>,
    ) -> Self {
        Self {
            state: KeyValueState::new(pointer, template, default, Arc::clone(&palette)),
            palette,
        }
    }
}

impl FieldComponent for KeyValueComponent {
    fn kind(&self) -> ComponentKind {
        ComponentKind::KeyValue
    }

    fn display_value(&self, _schema: &FieldSchema) -> String {
        format_collection_value(
            "Map",
            self.state.len(),
            self.state.selected_label(),
            &list_hint_for(ComponentKind::KeyValue, &self.palette),
        )
    }

    fn display_value_with_limit(&self, _schema: &FieldSchema, max_visible: usize) -> String {
        format_collection_value(
            "Map",
            self.state.len(),
            self.state.selected_label_with_limit(max_visible),
            &list_hint_for(ComponentKind::KeyValue, &self.palette),
        )
    }

    fn seed_value(&mut self, _schema: &FieldSchema, value: &Value) {
        if let Value::Object(map) = value {
            self.state.seed_entries_from_object(map);
        }
    }

    fn current_value(&self, schema: &FieldSchema) -> Result<Option<Value>, FieldCoercionError> {
        self.state.build_value(schema.required)
    }

    fn collection_panel(&self) -> Option<(Vec<String>, usize)> {
        self.state.panel()
    }

    fn collection_panel_with_limit(&self, max_visible: usize) -> Option<(Vec<String>, usize)> {
        self.state.panel_with_limit(max_visible)
    }

    fn collection_selected_label(&self) -> Option<String> {
        self.state.selected_label()
    }

    fn collection_selected_label_with_limit(&self, max_visible: usize) -> Option<String> {
        self.state.selected_label_with_limit(max_visible)
    }

    fn collection_selected_index(&self) -> Option<usize> {
        self.state.selected_index()
    }

    fn collection_select(&mut self, delta: i32) -> bool {
        self.state.select(delta)
    }

    fn collection_set_selected(&mut self, index: usize) -> bool {
        self.state.set_selected(index)
    }

    fn collection_add(&mut self) -> bool {
        self.state.add_entry()
    }

    fn collection_remove(&mut self) -> bool {
        self.state.remove_selected()
    }

    fn collection_move(&mut self, delta: i32) -> bool {
        self.state.move_selected(delta)
    }

    fn open_key_value_editor(
        &mut self,
        pointer: &str,
    ) -> Result<KeyValueEditorContext, FieldCoercionError> {
        let _ = pointer;
        self.state.open_selected_editor()
    }

    fn apply_key_value_editor(
        &mut self,
        entry_index: usize,
        session: &KeyValueEditorSession,
    ) -> Result<bool, FieldCoercionError> {
        self.state.apply_editor_session(entry_index, session)
    }

    fn overlay_context(&self, _schema: &FieldSchema) -> Option<OverlayContext> {
        let mut context = OverlayContext::new();
        if let Some(label) = self.state.selected_label() {
            context.title = Some(label.clone());
            context.description = Some(label);
        }
        if let Some((entries, selected)) = self.collection_panel() {
            context.entry_panel = Some(EntryPanelState { entries, selected });
        }
        context.instructions = Some(self.palette.collection.overlay_instructions.to_string());
        Some(context)
    }
}
