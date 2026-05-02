use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::tui::app::input::{AppCommand, CommandDispatch};
use crate::tui::app::runtime::overlay::app::open::overlay_field_input_result;
use crate::tui::app::runtime::{App, PopupOwner};
use crate::tui::state::apply_command;

impl App {
    pub(crate) fn handle_composite_editor_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.code == KeyCode::Esc {
            if !self.request_overlay_exit() {
                return Ok(());
            }
            return Ok(());
        }

        let dispatch = self
            .options
            .keymap
            .resolve(self.input_router.classify(&key));
        match dispatch {
            CommandDispatch::Form(command) => {
                if self.handle_overlay_focus_command(&command) {
                    return Ok(());
                }
                if let Some(editor) = self.active_overlay_mut() {
                    editor.set_exit_armed(false);
                    apply_command(editor.form_state_mut(), command.clone());
                    self.run_overlay_validation();
                }
            }
            CommandDispatch::App(command) => {
                if self.handle_overlay_app_command(command)? {
                    return Ok(());
                }
            }
            CommandDispatch::Input(event) => {
                self.handle_overlay_field_input(&event);
            }
            CommandDispatch::None => {}
        }

        Ok(())
    }

    fn handle_overlay_field_input(&mut self, event: &KeyEvent) {
        let Some(result) = ({
            let editor = match self.active_overlay_mut() {
                Some(editor) => editor,
                None => return,
            };
            editor.set_exit_armed(false);
            overlay_field_input_result(editor, event)
        }) else {
            return;
        };

        let (parent_label, child_label, pointer) = result;
        self.status
            .editing(&format!("{parent_label} › {child_label}"));
        self.validate_overlay_field(pointer);
    }

    pub(crate) fn handle_overlay_app_command(&mut self, command: AppCommand) -> Result<bool> {
        match command {
            AppCommand::Save => {
                self.save_overlay_stack_to_root();
                return Ok(true);
            }
            AppCommand::Quit => {
                self.request_overlay_exit();
                return Ok(true);
            }
            AppCommand::EditComposite => {
                self.try_open_composite_editor();
                return Ok(true);
            }
            AppCommand::ShowHelp => {
                self.toggle_help_overlay();
                return Ok(true);
            }
            AppCommand::TogglePopup => {
                if self.try_open_popup(PopupOwner::Composite) {
                    return Ok(true);
                }
            }
            AppCommand::ResetStatus => {
                self.status.ready();
                if let Some(editor) = self.active_overlay_mut() {
                    editor.set_exit_armed(false);
                }
                return Ok(true);
            }
            AppCommand::ListAddEntry => {
                if self.handle_list_add_entry() {
                    return Ok(true);
                }
            }
            AppCommand::ListRemoveEntry => {
                if self.handle_list_remove_entry() {
                    return Ok(true);
                }
            }
            AppCommand::ListMove(delta) => {
                if self.handle_list_move_entry(delta) {
                    return Ok(true);
                }
            }
            AppCommand::ListSelect(delta) => {
                if self.handle_list_select_entry(delta) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
