use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::Value;

use crate::tui::model::{CompositeField, FieldSchema};
use crate::tui::state::composite::{
    CompositeEditorSession, CompositeState, CompositeVariantSummary,
};
use crate::tui::state::error::FieldCoercionError;

use super::{
    ComponentKind, CompositePopupData, CompositeSelectorView, FieldComponent,
    palette::ComponentPalette,
};

#[derive(Debug, Clone)]
pub struct CompositeComponent {
    state: CompositeState,
    view: CompositeViewAdapter,
}

impl CompositeComponent {
    pub fn new(pointer: &str, template: &CompositeField, palette: Arc<ComponentPalette>) -> Self {
        Self {
            state: CompositeState::new(pointer, template, Arc::clone(&palette)),
            view: CompositeViewAdapter { palette },
        }
    }
}

impl FieldComponent for CompositeComponent {
    fn kind(&self) -> ComponentKind {
        ComponentKind::Composite
    }

    fn display_value(&self, _schema: &FieldSchema) -> String {
        self.view.display_label(&self.state)
    }

    fn display_value_with_limit(&self, _schema: &FieldSchema, max_visible: usize) -> String {
        self.view.display_label_with_limit(&self.state, max_visible)
    }

    fn handle_key(&mut self, _schema: &FieldSchema, key: &crossterm::event::KeyEvent) -> bool {
        match key.code {
            KeyCode::Left => self.state.rotate_single(-1),
            KeyCode::Right => self.state.rotate_single(1),
            _ => false,
        }
    }

    fn seed_value(&mut self, _schema: &FieldSchema, value: &Value) {
        if value.is_object() {
            let _ = self.state.seed_from_value(value);
        }
    }

    fn current_value(&self, schema: &FieldSchema) -> Result<Option<Value>, FieldCoercionError> {
        self.state.build_value(schema.required)
    }

    fn composite_popup(&self) -> Option<CompositePopupData> {
        self.view.popup(&self.state)
    }

    fn composite_selector(&self) -> Option<CompositeSelectorView> {
        self.view.selector(&self.state)
    }

    fn composite_summaries(&self) -> Option<Vec<CompositeVariantSummary>> {
        self.view.summaries(&self.state)
    }

    fn active_composite_variants(&self) -> Vec<usize> {
        self.state.active_indices()
    }

    fn apply_composite_selection(&mut self, selection: usize, flags: Option<Vec<bool>>) -> bool {
        if self.state.is_multi() {
            if let Some(flags) = flags {
                self.state.apply_multi(&flags)
            } else {
                false
            }
        } else {
            self.state.apply_single(selection)
        }
    }

    fn open_composite_editor(
        &mut self,
        pointer: &str,
        variant_index: usize,
    ) -> Result<CompositeEditorSession, FieldCoercionError> {
        self.state.take_editor_session(pointer, variant_index)
    }

    fn restore_composite_editor(&mut self, session: CompositeEditorSession) {
        self.state.restore_editor_session(session);
    }
}

#[derive(Debug, Clone)]
struct CompositeViewAdapter {
    palette: Arc<ComponentPalette>,
}

impl CompositeViewAdapter {
    fn display_label(&self, state: &CompositeState) -> String {
        let mut label = state.summary_with_preview();
        if state.is_multi() {
            label.push_str(self.palette.composite.multi_variant_hint.as_ref());
        } else {
            label.push_str(self.palette.composite.single_variant_hint.as_ref());
        }
        label
    }

    fn display_label_with_limit(&self, state: &CompositeState, max_visible: usize) -> String {
        let hint = if state.is_multi() {
            self.palette.composite.multi_variant_hint.as_ref()
        } else {
            self.palette.composite.single_variant_hint.as_ref()
        };
        let hint_width = unicode_width::UnicodeWidthStr::width(hint);
        let summary_budget = max_visible.saturating_sub(hint_width).max(1);
        let mut label = state.summary_with_preview_limit(summary_budget);
        label.push_str(hint);
        label
    }

    fn popup(&self, state: &CompositeState) -> Option<CompositePopupData> {
        let options = state.option_titles();
        if options.is_empty() {
            return None;
        }
        let selected = state
            .selected_index()
            .unwrap_or(0)
            .min(options.len().saturating_sub(1));
        Some(CompositePopupData {
            options,
            selected,
            multi: state.is_multi(),
            active: state.active_flags(),
        })
    }

    fn selector(&self, state: &CompositeState) -> Option<CompositeSelectorView> {
        let options = state.option_titles();
        if options.is_empty() {
            return None;
        }
        Some(CompositeSelectorView {
            multi: state.is_multi(),
            options,
            active: state.active_flags(),
            descriptions: state.variant_descriptions(),
        })
    }

    fn summaries(&self, state: &CompositeState) -> Option<Vec<CompositeVariantSummary>> {
        let summaries = state.active_summaries();
        if summaries.is_empty() {
            None
        } else {
            Some(summaries)
        }
    }
}
