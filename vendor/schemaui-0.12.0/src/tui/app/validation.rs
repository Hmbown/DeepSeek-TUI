use jsonschema::Validator;
use serde_json::Value;

use crate::tui::state::FormState;

#[derive(Debug)]
pub enum ValidationOutcome {
    Valid(Value),
    Invalid {
        issues: usize,
        global_errors: Vec<String>,
    },
    BuildError {
        message: String,
    },
}

pub fn validate_form(form_state: &mut FormState, validator: &Validator) -> ValidationOutcome {
    match form_state.try_build_value() {
        Ok(value) => {
            if validator.is_valid(&value) {
                form_state.clear_errors();
                ValidationOutcome::Valid(value)
            } else {
                form_state.clear_errors();
                let mut issues = 0usize;
                let mut global = Vec::new();
                for error in validator.iter_errors(&value) {
                    issues += 1;
                    let pointer = error.instance_path().to_string();
                    let message = error.to_string();
                    if !form_state.set_error(&pointer, message.clone()) {
                        let prefix = if pointer.is_empty() {
                            "<root>".to_string()
                        } else {
                            pointer.clone()
                        };
                        global.push(format!("{prefix}: {message}"));
                    }
                }
                ValidationOutcome::Invalid {
                    issues,
                    global_errors: global,
                }
            }
        }
        Err(err) => {
            form_state.set_error(&err.pointer, err.message.clone());
            ValidationOutcome::BuildError {
                message: err.message,
            }
        }
    }
}
