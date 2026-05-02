use std::sync::Arc;

use serde_json::Value;

use crate::tui::model::FieldSchema;
use crate::tui::state::error::FieldCoercionError;

use super::{ComponentKind, FieldComponent, palette::ComponentPalette};

#[derive(Debug, Clone)]
pub struct BoolComponent {
    value: bool,
    palette: Arc<ComponentPalette>,
}

impl BoolComponent {
    pub fn new(schema: &FieldSchema, palette: Arc<ComponentPalette>) -> Self {
        let value = schema
            .default
            .as_ref()
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        Self { value, palette }
    }
}

impl FieldComponent for BoolComponent {
    fn kind(&self) -> ComponentKind {
        ComponentKind::Bool
    }

    fn display_value(&self, _schema: &FieldSchema) -> String {
        if self.value {
            self.palette.bools.true_label.to_string()
        } else {
            self.palette.bools.false_label.to_string()
        }
    }

    fn handle_key(&mut self, _schema: &FieldSchema, key: &crossterm::event::KeyEvent) -> bool {
        match key.code {
            crossterm::event::KeyCode::Char(' ') if self.palette.bools.toggle_with_space => {
                self.value = !self.value;
                true
            }
            crossterm::event::KeyCode::Left | crossterm::event::KeyCode::Right
                if self.palette.bools.toggle_with_arrows =>
            {
                self.value = !self.value;
                true
            }
            _ => false,
        }
    }

    fn seed_value(&mut self, _schema: &FieldSchema, value: &Value) {
        if let Some(flag) = value.as_bool() {
            self.value = flag;
        }
    }

    fn current_value(&self, _schema: &FieldSchema) -> Result<Option<Value>, FieldCoercionError> {
        Ok(Some(Value::Bool(self.value)))
    }

    fn bool_value(&self) -> Option<bool> {
        Some(self.value)
    }

    fn set_bool(&mut self, value: bool) -> bool {
        if self.value != value {
            self.value = value;
            true
        } else {
            false
        }
    }
}
