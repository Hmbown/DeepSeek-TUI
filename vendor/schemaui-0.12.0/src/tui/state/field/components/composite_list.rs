use std::sync::Arc;

use serde_json::Value;

use crate::tui::model::{CompositeField, FieldSchema};
use crate::tui::state::composite::{
    CompositeEditorSession, CompositeListEditorContext, CompositeListState,
};
use crate::tui::state::error::FieldCoercionError;

use super::helpers::{EntryPanelState, OverlayContext, format_collection_value, list_hint_for};
use super::{
    ComponentKind, CompositePopupData, CompositeSelectorView, FieldComponent,
    palette::ComponentPalette,
};

#[derive(Debug, Clone)]
pub struct CompositeListComponent {
    state: CompositeListState,
    palette: Arc<ComponentPalette>,
}

impl CompositeListComponent {
    pub fn new(
        pointer: &str,
        template: &CompositeField,
        defaults: Option<&Value>,
        palette: Arc<ComponentPalette>,
    ) -> Self {
        Self {
            state: CompositeListState::new(pointer, template, defaults, Arc::clone(&palette)),
            palette,
        }
    }
}

impl FieldComponent for CompositeListComponent {
    fn kind(&self) -> ComponentKind {
        ComponentKind::CompositeList
    }

    fn display_value(&self, _schema: &FieldSchema) -> String {
        format_collection_value(
            "List",
            self.state.len(),
            self.state.selected_compact_label(),
            &list_hint_for(ComponentKind::CompositeList, &self.palette),
        )
    }

    fn display_value_with_limit(&self, _schema: &FieldSchema, max_visible: usize) -> String {
        format_collection_value(
            "List",
            self.state.len(),
            self.state.selected_compact_label_with_limit(max_visible),
            &list_hint_for(ComponentKind::CompositeList, &self.palette),
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
        self.state
            .selected_index()
            .map(|idx| (self.state.summaries(), idx))
    }

    fn collection_panel_with_limit(&self, max_visible: usize) -> Option<(Vec<String>, usize)> {
        self.state
            .selected_index()
            .map(|idx| (self.state.summaries_with_limit(max_visible), idx))
    }

    fn collection_selected_label(&self) -> Option<String> {
        self.state.selected_compact_label()
    }

    fn collection_selected_label_with_limit(&self, max_visible: usize) -> Option<String> {
        self.state.selected_compact_label_with_limit(max_visible)
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
        self.state.add_entry(None);
        true
    }

    fn collection_add_with_variant(&mut self, variant_index: usize) -> bool {
        self.state.add_entry(Some(variant_index));
        true
    }

    fn composite_list_variant_selector(&self) -> Option<CompositePopupData> {
        self.state.variant_selector_popup()
    }

    fn composite_list_variant_count(&self) -> usize {
        self.state.variant_count()
    }

    fn collection_remove(&mut self) -> bool {
        self.state.remove_selected()
    }

    fn collection_move(&mut self, delta: i32) -> bool {
        self.state.move_selected(delta)
    }

    fn open_composite_list_editor(
        &mut self,
        pointer: &str,
    ) -> Result<CompositeListEditorContext, FieldCoercionError> {
        let _ = pointer;
        self.state.open_selected_editor()
    }

    fn restore_composite_list_editor(
        &mut self,
        entry_index: usize,
        session: CompositeEditorSession,
    ) {
        self.state.restore_entry_editor(entry_index, session);
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

    fn composite_popup(&self) -> Option<CompositePopupData> {
        self.state.popup()
    }

    fn apply_composite_selection(&mut self, selection: usize, flags: Option<Vec<bool>>) -> bool {
        self.state.apply_selection(selection, flags)
    }

    fn composite_entry_selector(&self) -> Option<CompositeSelectorView> {
        self.state.selected_entry_selector()
    }
}
