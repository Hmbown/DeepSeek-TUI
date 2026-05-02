use jsonschema::validator_for;
use std::sync::Arc;

use crate::tui::app::runtime::App;
use crate::tui::app::runtime::overlay::state::OverlaySession;
use crate::tui::state::{FormCommand, FormEngine};

impl App {
    pub(crate) fn validate_overlay_field(&mut self, pointer: String) {
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
}
