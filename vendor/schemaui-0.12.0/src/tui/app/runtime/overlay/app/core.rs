use crate::tui::app::runtime::App;
use crate::tui::app::runtime::overlay::editor::CompositeEditorOverlay;
use crate::tui::app::runtime::overlay::state::OverlayHost;
use crate::tui::state::FormState;

impl App {
    pub(crate) fn overlay_depth(&self) -> usize {
        self.overlay_stack.len()
    }

    pub(crate) fn active_overlay(&self) -> Option<&CompositeEditorOverlay> {
        self.overlay_stack.last()
    }

    pub(crate) fn active_overlay_mut(&mut self) -> Option<&mut CompositeEditorOverlay> {
        self.overlay_stack.last_mut()
    }

    pub(super) fn overlay_help_text(&self) -> String {
        let base = self
            .current_help_text()
            .unwrap_or_else(|| "Ctrl+S save • Esc/Ctrl+Q exit overlay".to_string());
        if let Some(editor) = self.active_overlay() {
            format!("L{} • {}", editor.level(), base)
        } else {
            base
        }
    }

    pub(super) fn set_overlay_status_message(&mut self) {
        if let Some(editor) = self.active_overlay() {
            let help = self.overlay_help_text();
            self.status
                .set_raw(format!("Overlay {}: {}", editor.level(), help));
        }
    }

    pub(crate) fn host_form_state(&self, host: OverlayHost) -> &FormState {
        match host {
            OverlayHost::RootForm => &self.form_state,
            OverlayHost::Overlay { parent_level } => {
                let idx = parent_level.saturating_sub(1);
                self.overlay_stack[idx].form_state()
            }
        }
    }

    pub(crate) fn host_form_state_mut(&mut self, host: OverlayHost) -> &mut FormState {
        match host {
            OverlayHost::RootForm => &mut self.form_state,
            OverlayHost::Overlay { parent_level } => {
                let idx = parent_level.saturating_sub(1);
                self.overlay_stack
                    .get_mut(idx)
                    .expect("overlay host should exist")
                    .form_state_mut()
            }
        }
    }

    pub(super) fn initialize_active_overlay(&mut self) {
        self.set_overlay_status_message();
        self.refresh_list_overlay_panel();
        self.setup_overlay_validator();
        self.run_overlay_validation();
        self.reset_overlay_focus_mode();
    }

    pub(super) fn reset_overlay_focus_mode(&mut self) {
        if let Some(editor) = self.active_overlay_mut()
            && !editor.focus_entries()
        {
            editor.focus_form_first();
        }
    }
}
