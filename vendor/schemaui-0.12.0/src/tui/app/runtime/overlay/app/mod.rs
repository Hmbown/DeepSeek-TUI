use anyhow::Result;

#[cfg(all(test, feature = "json"))]
use crate::tui::app::runtime::overlay::state::OverlayFocusMode;

use crate::tui::app::runtime::App;
use crate::tui::app::validation::{ValidationOutcome, validate_form};
use crate::tui::state::FormCommand;

use super::editor::CompositeEditorOverlay;
use super::state::{FocusDirection, FocusOutcome};

mod core;
mod input;
mod list_ops;
mod open;
mod popup_ops;
mod validation;

const MSG_NO_FIELD_SELECTED: &str = "No field selected";
const MSG_SELECT_VARIANT_BEFORE_EDIT: &str =
    "Select a variant via Enter before editing (oneOf/anyOf)";
const MSG_UNABLE_AUTO_CREATE_ENTRY: &str = "Unable to auto-create the first entry; use Ctrl+N";
const MSG_FOCUS_COMPOSITE_BEFORE_EDITING: &str =
    "Focus a composite or composite list field before editing";
const MSG_OVERLAY_DIRTY_CONFIRM_EXIT: &str = "Overlay dirty. Press Esc again to discard changes.";

impl App {
    fn validate_overlay_before_commit(
        overlay: &mut CompositeEditorOverlay,
    ) -> std::result::Result<(), String> {
        let Some(validator) = overlay.validator_clone() else {
            return Ok(());
        };

        match validate_form(overlay.form_state_mut(), &validator) {
            ValidationOutcome::Valid(_) => Ok(()),
            ValidationOutcome::Invalid {
                issues,
                global_errors,
            } => Err(global_errors
                .into_iter()
                .next()
                .unwrap_or_else(|| format!("{issues} issue(s) remaining"))),
            ValidationOutcome::BuildError { message } => Err(message),
        }
    }

    pub(crate) fn close_active_overlay(&mut self, commit: bool) {
        let Some(mut overlay) = self.overlay_stack.pop() else {
            return;
        };
        self.overlay_validator_cache
            .remove(&overlay.validator_cache_key());
        self.popup = None;
        if commit {
            match self.apply_overlay_commit(&overlay) {
                Ok(()) => {
                    overlay.form_state_mut().mark_clean();
                    overlay.set_exit_armed(false);
                    self.exit_armed = false;
                    self.status.value_updated();
                    if overlay.level() == 1 && self.options.auto_validate {
                        self.run_validation(false);
                    } else {
                        self.run_overlay_validation();
                    }
                }
                Err(message) => {
                    self.status.set_raw(&message);
                    self.overlay_stack.push(overlay);
                    return;
                }
            }
        } else {
            self.status.ready();
        }

        if let Some(parent) = self.active_overlay_mut() {
            parent.set_exit_armed(false);
            self.set_overlay_status_message();
            self.refresh_list_overlay_panel();
            self.run_overlay_validation();
        }
    }

    fn apply_overlay_commit(&mut self, overlay: &CompositeEditorOverlay) -> Result<(), String> {
        let payload = overlay.build_commit_payload();
        let host_state = self.host_form_state_mut(payload.host());
        payload.apply(host_state)
    }

    fn handle_overlay_focus_command(&mut self, command: &FormCommand) -> bool {
        if !matches!(
            command,
            FormCommand::FocusNextField | FormCommand::FocusPrevField
        ) {
            return false;
        }
        let direction = match command {
            FormCommand::FocusNextField => FocusDirection::Forward,
            FormCommand::FocusPrevField => FocusDirection::Backward,
            _ => return false,
        };
        let outcome = {
            let editor = match self.active_overlay_mut() {
                Some(editor) => editor,
                None => return false,
            };
            editor.set_exit_armed(false);
            editor.advance_focus(direction)
        };
        match outcome {
            FocusOutcome::Consumed => true,
            FocusOutcome::RequestEntryDelta(delta) => self.advance_overlay_entry(delta),
            FocusOutcome::PassThrough => false,
        }
    }

    pub(crate) fn request_overlay_exit(&mut self) -> bool {
        if let Some(editor) = self.active_overlay_mut()
            && editor.dirty()
            && !editor.exit_armed()
        {
            editor.set_exit_armed(true);
            self.status.set_raw(MSG_OVERLAY_DIRTY_CONFIRM_EXIT);
            return false;
        }
        self.close_active_overlay(false);
        true
    }

    pub(crate) fn save_active_overlay(&mut self) -> bool {
        let Some(mut overlay) = self.overlay_stack.pop() else {
            return false;
        };
        if let Err(message) = Self::validate_overlay_before_commit(&mut overlay) {
            self.status.set_raw(&message);
            self.overlay_stack.push(overlay);
            return false;
        }
        match self.apply_overlay_commit(&overlay) {
            Ok(()) => {
                overlay.form_state_mut().mark_clean();
                overlay.set_exit_armed(false);
                self.status
                    .set_raw(format!("Overlay {} saved.", overlay.level()));
                if overlay.level() == 1 && self.options.auto_validate {
                    self.run_validation(false);
                } else {
                    self.run_overlay_validation();
                }
                self.overlay_stack.push(overlay);
                self.set_overlay_status_message();
                self.refresh_list_overlay_panel();
                true
            }
            Err(message) => {
                self.status.set_raw(&message);
                self.overlay_stack.push(overlay);
                false
            }
        }
    }

    /// Save the entire overlay stack into the root form state.
    ///
    /// This is used for user-initiated saves (Ctrl+S) while editing inside
    /// one or more overlays. It walks from the deepest overlay up to the
    /// top-level overlay, committing each overlay's session into its host
    /// form state. This ensures that changes made in nested overlays are
    /// persisted even if intermediate overlays are later closed without an
    /// additional save.
    pub(crate) fn save_overlay_stack_to_root(&mut self) -> bool {
        let depth = self.overlay_stack.len();
        if depth == 0 {
            return true;
        }

        // Commit overlays from deepest (highest level) back towards the root.
        for idx in (0..depth).rev() {
            if let Err(message) = Self::validate_overlay_before_commit(&mut self.overlay_stack[idx])
            {
                self.status.set_raw(&message);
                return false;
            }

            // Build commit payload without borrowing the overlay mutably while
            // we also mutate the host form state.
            let payload = {
                let overlay = &self.overlay_stack[idx];
                overlay.build_commit_payload()
            };

            let host = payload.host();
            if let Err(message) = payload.apply(self.host_form_state_mut(host)) {
                self.status.set_raw(&message);
                return false;
            }

            // Mark the overlay itself as clean so that subsequent exits do
            // not discard already-committed changes.
            let overlay = &mut self.overlay_stack[idx];
            overlay.form_state_mut().mark_clean();
            overlay.set_exit_armed(false);
        }

        // After committing, keep using overlay-scoped validation so we don't
        // surprise the user with full-form validation on every overlay save.
        self.run_overlay_validation();
        self.set_overlay_status_message();
        self.refresh_list_overlay_panel();
        true
    }
}

#[cfg(all(test, feature = "json"))]
impl App {
    pub(crate) fn overlay_depth_for_test(&self) -> usize {
        self.overlay_depth()
    }

    pub(crate) fn open_overlay_for_test(&mut self) {
        self.try_open_composite_editor();
    }

    pub(crate) fn active_overlay_form_state_for_test(
        &mut self,
    ) -> Option<&mut crate::tui::state::FormState> {
        self.active_overlay_mut()
            .map(|overlay| overlay.form_state_mut())
    }

    pub(crate) fn overlay_entry_focus_for_test(&self) -> Option<bool> {
        self.active_overlay()
            .map(|overlay| overlay.focus_mode() == OverlayFocusMode::EntryTabs)
    }

    pub(crate) fn overlay_selected_entry_for_test(&self) -> Option<usize> {
        self.active_overlay()
            .and_then(|overlay| overlay.entry_tabs_selected())
    }
}

#[cfg(test)]
mod tests;
