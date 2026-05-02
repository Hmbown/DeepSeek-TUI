use jsonschema::Validator;

use super::{actions::FormCommand, form_state::FormState};

pub fn apply_command(state: &mut FormState, command: FormCommand) {
    match command {
        FormCommand::FocusNextField => state.focus_next_field(),
        FormCommand::FocusPrevField => state.focus_prev_field(),
        FormCommand::FocusNextSection(delta) => state.focus_next_section(delta),
        FormCommand::FocusNextRoot(delta) => state.focus_next_root(delta),
        FormCommand::FieldEdited { .. } => {}
    }
}

pub struct FormEngine<'a> {
    state: &'a mut FormState,
    validator: &'a Validator,
}

impl<'a> FormEngine<'a> {
    pub fn new(state: &'a mut FormState, validator: &'a Validator) -> Self {
        Self { state, validator }
    }

    pub fn dispatch(&mut self, command: FormCommand) -> Result<(), String> {
        match command {
            FormCommand::FieldEdited { pointer } => self.validate_field(&pointer),
            other => {
                apply_command(self.state, other);
                Ok(())
            }
        }
    }

    fn validate_field(&mut self, pointer: &str) -> Result<(), String> {
        match self.state.try_build_value() {
            Ok(value) => {
                self.state.clear_error(pointer);
                let mut matched = false;
                for error in self.validator.iter_errors(&value) {
                    let err_pointer = error.instance_path().to_string();
                    if err_pointer == pointer {
                        matched = true;
                        self.state.set_error(&err_pointer, error.to_string());
                    }
                }
                if !matched {
                    self.state.clear_error(pointer);
                }
                Ok(())
            }
            Err(err) => {
                self.state.set_error(&err.pointer, err.message.clone());
                Err(err.message)
            }
        }
    }
}
