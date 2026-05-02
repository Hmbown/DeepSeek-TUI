use crossterm::event::KeyEvent;
use serde_json::Value;

use super::helpers::OverlayContext;
use crate::tui::model::FieldSchema;
use crate::tui::state::array::{ArrayEditorContext, ArrayEditorSession};
use crate::tui::state::composite::{
    CompositeEditorSession, CompositeListEditorContext, CompositeVariantSummary,
};
use crate::tui::state::error::FieldCoercionError;
use crate::tui::state::key_value::{KeyValueEditorContext, KeyValueEditorSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    TextInput,
    ArrayBuffer,
    Bool,
    Enum,
    MultiSelect,
    Composite,
    CompositeList,
    ScalarArray,
    KeyValue,
}

pub(crate) trait FieldComponent: FieldComponentClone + std::fmt::Debug {
    fn kind(&self) -> ComponentKind;
    fn display_value(&self, schema: &FieldSchema) -> String;
    fn display_value_with_limit(&self, schema: &FieldSchema, _max_visible: usize) -> String {
        self.display_value(schema)
    }
    fn handle_key(&mut self, schema: &FieldSchema, key: &KeyEvent) -> bool {
        let _ = (schema, key);
        false
    }
    fn seed_value(&mut self, schema: &FieldSchema, value: &Value);
    fn current_value(&self, schema: &FieldSchema) -> Result<Option<Value>, FieldCoercionError>;
    fn cursor_offset(&self, _schema: &FieldSchema) -> Option<usize> {
        None
    }

    fn bool_value(&self) -> Option<bool> {
        None
    }

    fn set_bool(&mut self, _value: bool) -> bool {
        false
    }

    fn enum_state(&self) -> Option<EnumStateRef<'_>> {
        None
    }

    fn set_enum_index(&mut self, _index: usize) -> bool {
        false
    }

    fn multi_state(&self) -> Option<MultiSelectStateRef<'_>> {
        None
    }

    fn set_multi_state(&mut self, _flags: &[bool]) -> bool {
        false
    }

    fn composite_popup(&self) -> Option<CompositePopupData> {
        None
    }

    fn composite_selector(&self) -> Option<CompositeSelectorView> {
        None
    }

    fn composite_entry_selector(&self) -> Option<CompositeSelectorView> {
        None
    }

    fn composite_summaries(&self) -> Option<Vec<CompositeVariantSummary>> {
        None
    }

    fn active_composite_variants(&self) -> Vec<usize> {
        Vec::new()
    }

    fn apply_composite_selection(&mut self, _selection: usize, _flags: Option<Vec<bool>>) -> bool {
        false
    }

    fn open_composite_editor(
        &mut self,
        pointer: &str,
        variant_index: usize,
    ) -> Result<CompositeEditorSession, FieldCoercionError> {
        let _ = variant_index;
        Err(FieldCoercionError::unsupported(
            pointer,
            "composite editing",
        ))
    }

    fn restore_composite_editor(&mut self, _session: CompositeEditorSession) {}

    fn collection_panel(&self) -> Option<(Vec<String>, usize)> {
        None
    }

    fn collection_panel_with_limit(&self, _max_visible: usize) -> Option<(Vec<String>, usize)> {
        self.collection_panel()
    }

    fn collection_selected_label(&self) -> Option<String> {
        None
    }

    fn collection_selected_label_with_limit(&self, _max_visible: usize) -> Option<String> {
        self.collection_selected_label()
    }

    fn collection_selected_index(&self) -> Option<usize> {
        None
    }

    fn collection_select(&mut self, _delta: i32) -> bool {
        false
    }

    fn collection_set_selected(&mut self, _index: usize) -> bool {
        false
    }

    fn collection_add(&mut self) -> bool {
        false
    }

    fn collection_add_with_variant(&mut self, _variant_index: usize) -> bool {
        false
    }

    fn composite_list_variant_selector(&self) -> Option<CompositePopupData> {
        None
    }

    fn composite_list_variant_count(&self) -> usize {
        0
    }

    fn collection_remove(&mut self) -> bool {
        false
    }

    fn collection_move(&mut self, _delta: i32) -> bool {
        false
    }

    fn open_composite_list_editor(
        &mut self,
        pointer: &str,
    ) -> Result<CompositeListEditorContext, FieldCoercionError> {
        Err(FieldCoercionError::unsupported(
            pointer,
            "composite list editing",
        ))
    }

    fn restore_composite_list_editor(
        &mut self,
        _entry_index: usize,
        _session: CompositeEditorSession,
    ) {
    }

    fn open_key_value_editor(
        &mut self,
        pointer: &str,
    ) -> Result<KeyValueEditorContext, FieldCoercionError> {
        Err(FieldCoercionError::unsupported(pointer, "map editing"))
    }

    fn apply_key_value_editor(
        &mut self,
        _entry_index: usize,
        _session: &KeyValueEditorSession,
    ) -> Result<bool, FieldCoercionError> {
        Err(FieldCoercionError::unsupported("", "map editing"))
    }

    fn open_scalar_array_editor(
        &mut self,
        pointer: &str,
    ) -> Result<ArrayEditorContext, FieldCoercionError> {
        Err(FieldCoercionError::unsupported(pointer, "array editing"))
    }

    fn apply_scalar_array_editor(
        &mut self,
        _entry_index: usize,
        _session: &ArrayEditorSession,
    ) -> Result<bool, FieldCoercionError> {
        Err(FieldCoercionError::unsupported("", "array editing"))
    }

    fn overlay_context(&self, _schema: &FieldSchema) -> Option<OverlayContext> {
        None
    }
}

pub(crate) trait FieldComponentClone {
    fn clone_box(&self) -> Box<dyn FieldComponent>;
}

impl<T> FieldComponentClone for T
where
    T: 'static + FieldComponent + Clone,
{
    fn clone_box(&self) -> Box<dyn FieldComponent> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn FieldComponent> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

#[derive(Debug, Clone)]
pub struct EnumStateRef<'a> {
    pub options: &'a [String],
    pub selected: usize,
}

#[derive(Debug, Clone)]
pub struct MultiSelectStateRef<'a> {
    pub options: &'a [String],
    pub selected: &'a [bool],
}

#[derive(Debug, Clone)]
pub struct CompositePopupData {
    pub options: Vec<String>,
    pub selected: usize,
    pub multi: bool,
    pub active: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct CompositeSelectorView {
    pub multi: bool,
    pub options: Vec<String>,
    pub active: Vec<bool>,
    pub descriptions: Vec<Option<String>>,
}
