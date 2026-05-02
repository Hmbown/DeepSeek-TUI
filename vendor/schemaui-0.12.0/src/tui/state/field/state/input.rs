use crossterm::event::KeyEvent;

use super::FieldState;

impl FieldState {
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        if self.component.handle_key(&self.schema, key) {
            self.after_edit();
            true
        } else {
            false
        }
    }

    pub fn set_bool(&mut self, value: bool) {
        if self.component.set_bool(value) {
            self.after_edit();
        }
    }

    pub fn set_enum_selected(&mut self, index: usize) {
        if self.component.set_enum_index(index) {
            self.after_edit();
        }
    }

    pub fn set_multi_selection(&mut self, selections: &[bool]) {
        if self.component.set_multi_state(selections) {
            self.after_edit();
        }
    }
}
