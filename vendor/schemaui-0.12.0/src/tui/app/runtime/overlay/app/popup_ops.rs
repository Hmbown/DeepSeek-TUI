use crate::tui::app::runtime::{App, PopupOwner};
use crate::tui::model::FieldKind;
use crate::tui::state::FieldState;

fn apply_selection_to_field(field: &mut FieldState, selection: usize, multi: Option<Vec<bool>>) {
    if let Some(flags) = multi {
        match &field.schema.kind {
            FieldKind::Composite(_) => {
                field.apply_composite_selection(selection, Some(flags));
            }
            FieldKind::Array(inner) if matches!(inner.as_ref(), FieldKind::Composite(_)) => {
                // Array of composite: delegate to composite list state
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
        FieldKind::Array(inner) if matches!(inner.as_ref(), FieldKind::Composite(_)) => {
            // Single-choice selection for array-of-composite entries
            field.apply_composite_selection(selection, None);
        }
        FieldKind::Boolean => field.set_bool(selection == 0),
        FieldKind::Enum { .. } => field.set_enum_selected(selection),
        _ => {}
    }
}

impl App {
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
}
