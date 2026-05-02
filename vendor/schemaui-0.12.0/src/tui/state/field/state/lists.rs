use super::super::components::{
    ComponentKind, CompositePopupData, CompositeSelectorView, EnumStateRef,
};
use super::FieldState;
use crate::tui::state::array::{ArrayEditorContext, ArrayEditorSession};
use crate::tui::state::composite::{
    CompositeEditorSession, CompositeListEditorContext, CompositeVariantSummary,
};
use crate::tui::state::error::FieldCoercionError;
use crate::tui::state::key_value::{KeyValueEditorContext, KeyValueEditorSession};

impl FieldState {
    pub fn composite_selector_view(&self) -> Option<CompositeSelectorView> {
        self.component.composite_selector()
    }

    pub fn composite_entry_selector_view(&self) -> Option<CompositeSelectorView> {
        self.component.composite_entry_selector()
    }

    pub fn composite_variant_summaries(&self) -> Option<Vec<CompositeVariantSummary>> {
        self.component.composite_summaries()
    }

    pub fn bool_value(&self) -> Option<bool> {
        self.component.bool_value()
    }

    pub fn enum_state(&self) -> Option<EnumStateRef<'_>> {
        self.component.enum_state()
    }

    pub fn composite_popup(&self) -> Option<CompositePopupData> {
        self.component.composite_popup()
    }

    pub fn active_composite_variants(&self) -> Vec<usize> {
        self.component.active_composite_variants()
    }

    pub fn is_composite_list(&self) -> bool {
        matches!(
            self.component.kind(),
            ComponentKind::CompositeList | ComponentKind::KeyValue | ComponentKind::ScalarArray
        )
    }

    pub fn ensure_composite_list_popup_entry(&mut self) -> bool {
        if !matches!(self.component.kind(), ComponentKind::CompositeList) {
            return false;
        }
        if self.composite_list_selected_index().is_some() {
            return false;
        }
        self.composite_list_add_entry()
    }

    pub fn multi_states(&self) -> Option<&[bool]> {
        self.component.multi_state().map(|state| state.selected)
    }

    pub fn multi_options(&self) -> Option<&[String]> {
        self.component.multi_state().map(|state| state.options)
    }

    pub fn apply_composite_selection(&mut self, selection: usize, multi_flags: Option<Vec<bool>>) {
        if self
            .component
            .apply_composite_selection(selection, multi_flags)
        {
            self.after_edit();
        }
    }

    pub fn composite_list_select_entry(&mut self, delta: i32) -> bool {
        self.component.collection_select(delta)
    }

    pub fn composite_list_selected_label(&self) -> Option<String> {
        self.component.collection_selected_label()
    }

    pub fn composite_list_selected_label_with_limit(&self, max_visible: usize) -> Option<String> {
        self.component
            .collection_selected_label_with_limit(max_visible)
    }

    pub fn collection_selected_label(&self) -> Option<String> {
        self.component.collection_selected_label()
    }

    pub fn collection_selected_label_with_limit(&self, max_visible: usize) -> Option<String> {
        self.component
            .collection_selected_label_with_limit(max_visible)
    }

    pub fn composite_list_panel(&self) -> Option<(Vec<String>, usize)> {
        self.component.collection_panel()
    }

    pub fn composite_list_panel_with_limit(
        &self,
        max_visible: usize,
    ) -> Option<(Vec<String>, usize)> {
        self.component.collection_panel_with_limit(max_visible)
    }

    pub fn composite_list_selected_index(&self) -> Option<usize> {
        self.component.collection_selected_index()
    }

    pub fn composite_list_add_entry(&mut self) -> bool {
        if self.component.collection_add() {
            self.after_edit();
            true
        } else {
            false
        }
    }

    pub fn composite_list_variant_selector_popup(&self) -> Option<CompositePopupData> {
        self.component.composite_list_variant_selector()
    }

    pub fn composite_list_variant_count(&self) -> usize {
        self.component.composite_list_variant_count()
    }

    pub fn composite_list_add_entry_with_variant(&mut self, variant_index: usize) -> bool {
        if self.component.collection_add_with_variant(variant_index) {
            self.after_edit();
            true
        } else {
            false
        }
    }

    pub fn composite_list_remove_entry(&mut self) -> bool {
        if self.component.collection_remove() {
            self.after_edit();
            true
        } else {
            false
        }
    }

    pub fn composite_list_move_entry(&mut self, delta: i32) -> bool {
        if self.component.collection_move(delta) {
            self.after_edit();
            true
        } else {
            false
        }
    }

    pub fn collection_set_selected(&mut self, index: usize) -> bool {
        self.component.collection_set_selected(index)
    }

    pub fn open_composite_editor(
        &mut self,
        variant_index: usize,
    ) -> Result<CompositeEditorSession, FieldCoercionError> {
        self.component
            .open_composite_editor(&self.schema.pointer, variant_index)
    }

    pub fn open_composite_list_editor(
        &mut self,
    ) -> Result<CompositeListEditorContext, FieldCoercionError> {
        self.component
            .open_composite_list_editor(&self.schema.pointer)
    }

    pub fn open_key_value_editor(&mut self) -> Result<KeyValueEditorContext, FieldCoercionError> {
        self.component.open_key_value_editor(&self.schema.pointer)
    }

    pub fn open_scalar_array_editor(&mut self) -> Result<ArrayEditorContext, FieldCoercionError> {
        self.component
            .open_scalar_array_editor(&self.schema.pointer)
    }

    pub fn close_composite_editor(&mut self, session: CompositeEditorSession, mark_dirty: bool) {
        if mark_dirty {
            self.after_edit();
        }
        self.component.restore_composite_editor(session);
    }

    pub fn close_composite_list_editor(
        &mut self,
        entry_index: usize,
        session: CompositeEditorSession,
        mark_dirty: bool,
    ) {
        self.component
            .restore_composite_list_editor(entry_index, session);
        if mark_dirty {
            self.after_edit();
        }
    }

    pub fn close_key_value_editor(
        &mut self,
        entry_index: usize,
        session: &KeyValueEditorSession,
        mark_dirty: bool,
    ) -> Result<bool, FieldCoercionError> {
        let changed = self
            .component
            .apply_key_value_editor(entry_index, session)?;
        if mark_dirty && changed {
            self.after_edit();
        }
        Ok(changed)
    }

    pub fn close_scalar_array_editor(
        &mut self,
        entry_index: usize,
        session: &ArrayEditorSession,
        mark_dirty: bool,
    ) -> Result<bool, FieldCoercionError> {
        let changed = self
            .component
            .apply_scalar_array_editor(entry_index, session)?;
        if mark_dirty && changed {
            self.after_edit();
        }
        Ok(changed)
    }
}
