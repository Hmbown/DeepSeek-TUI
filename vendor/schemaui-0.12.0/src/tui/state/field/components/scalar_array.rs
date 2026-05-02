use std::sync::Arc;

use serde_json::Value;

use crate::tui::model::{FieldKind, FieldSchema};
use crate::tui::state::array::{ArrayEditorContext, ArrayEditorSession, ScalarArrayState};
use crate::tui::state::error::FieldCoercionError;

use super::helpers::{EntryPanelState, OverlayContext, format_collection_value, list_hint_for};
use super::{ComponentKind, FieldComponent, palette::ComponentPalette};

#[derive(Debug, Clone)]
pub struct ScalarArrayComponent {
    state: ScalarArrayState,
    palette: Arc<ComponentPalette>,
}

impl ScalarArrayComponent {
    pub fn new(schema: &FieldSchema, inner: &FieldKind, palette: Arc<ComponentPalette>) -> Self {
        Self {
            state: ScalarArrayState::new(
                &schema.pointer,
                schema.display_label(),
                schema.description.clone(),
                inner,
                schema.default.as_ref(),
                Arc::clone(&palette),
            ),
            palette,
        }
    }
}

impl FieldComponent for ScalarArrayComponent {
    fn kind(&self) -> ComponentKind {
        ComponentKind::ScalarArray
    }

    fn display_value(&self, _schema: &FieldSchema) -> String {
        format_collection_value(
            "Array",
            self.state.len(),
            self.state.selected_label(),
            &list_hint_for(ComponentKind::ScalarArray, &self.palette),
        )
    }

    fn display_value_with_limit(&self, _schema: &FieldSchema, max_visible: usize) -> String {
        format_collection_value(
            "Array",
            self.state.len(),
            self.state.selected_label_with_limit(max_visible),
            &list_hint_for(ComponentKind::ScalarArray, &self.palette),
        )
    }

    fn seed_value(&mut self, _schema: &FieldSchema, value: &Value) {
        if let Value::Array(items) = value {
            self.state.seed_entries_from_array(items);
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

    fn open_scalar_array_editor(
        &mut self,
        pointer: &str,
    ) -> Result<ArrayEditorContext, FieldCoercionError> {
        let _ = pointer;
        self.state.open_selected_editor()
    }

    fn apply_scalar_array_editor(
        &mut self,
        entry_index: usize,
        session: &ArrayEditorSession,
    ) -> Result<bool, FieldCoercionError> {
        self.state.apply_editor_session(entry_index, session)
    }

    fn overlay_context(&self, _schema: &FieldSchema) -> Option<OverlayContext> {
        let mut context = OverlayContext::new();
        if let Some(label) = self.state.selected_label() {
            context.title = Some(label.clone());
            context.description = Some(label);
        }
        if let Some((entries, selected)) = self.state.panel() {
            context.entry_panel = Some(EntryPanelState { entries, selected });
        }
        context.instructions = Some(self.palette.collection.overlay_instructions.to_string());
        Some(context)
    }
}
