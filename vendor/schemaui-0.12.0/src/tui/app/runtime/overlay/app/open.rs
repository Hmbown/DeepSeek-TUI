use crate::tui::app::runtime::App;
use crate::tui::app::runtime::overlay::editor::CompositeEditorOverlay;
use crate::tui::app::runtime::overlay::state::{
    CompositeOverlayTarget, OverlayHost, OverlaySession,
};
use crate::tui::model::{CompositeMode, FieldKind};
use crate::tui::state::FieldState;
use crate::tui::state::field::components::CompositeSelectorView;
use crossterm::event::KeyEvent;

use super::{
    MSG_FOCUS_COMPOSITE_BEFORE_EDITING, MSG_NO_FIELD_SELECTED, MSG_SELECT_VARIANT_BEFORE_EDIT,
    MSG_UNABLE_AUTO_CREATE_ENTRY,
};

fn ensure_variant_selected_for_anyof(
    field: &mut FieldState,
    is_anyof: bool,
    variant_count: usize,
) -> std::result::Result<usize, &'static str> {
    let mut active = field.active_composite_variants();
    if active.is_empty() && is_anyof && variant_count > 0 {
        let mut flags = vec![false; variant_count];
        flags[0] = true;
        field.apply_composite_selection(0, Some(flags));
        active = field.active_composite_variants();
    }

    if let Some(&index) = active.first() {
        Ok(index)
    } else {
        Err(MSG_SELECT_VARIANT_BEFORE_EDIT)
    }
}

fn ensure_composite_list_entry(field: &mut FieldState) -> std::result::Result<(), &'static str> {
    if field.composite_list_selected_index().is_none() && !field.composite_list_add_entry() {
        Err(MSG_UNABLE_AUTO_CREATE_ENTRY)
    } else {
        Ok(())
    }
}

pub(super) fn overlay_field_input_result(
    editor: &mut CompositeEditorOverlay,
    event: &KeyEvent,
) -> Option<(String, String, String)> {
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
}

impl App {
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
            self.status.set_raw(MSG_NO_FIELD_SELECTED);
            return;
        };
        let component_context = field.overlay_context();

        match &field.schema.kind {
            FieldKind::Composite(template) => {
                let schema = template.as_ref();
                let is_anyof = matches!(schema.mode, CompositeMode::AnyOf);
                let variant_index =
                    match ensure_variant_selected_for_anyof(field, is_anyof, schema.variants.len())
                    {
                        Ok(idx) => idx,
                        Err(msg) => {
                            self.status.set_raw(msg);
                            return;
                        }
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
                if let Err(msg) = ensure_composite_list_entry(field) {
                    self.status.set_raw(msg);
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
                if let Err(msg) = ensure_composite_list_entry(field) {
                    self.status.set_raw(msg);
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
                self.status.set_raw(MSG_FOCUS_COMPOSITE_BEFORE_EDITING);
            }
        }

        if self.overlay_depth() > previous_depth
            && let Some(ctx) = component_context
            && let Some(editor) = self.active_overlay_mut()
        {
            editor.apply_component_context(ctx);
        }
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
}
