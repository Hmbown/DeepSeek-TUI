use std::sync::Arc;

use serde_json::Value;

use crate::tui::model::{FieldKind, FieldSchema};
use crate::tui::state::error::FieldCoercionError;
use crate::tui::state::field::convert::{array_to_string, array_value};

use super::helpers::handle_text_edit;
use super::{ComponentKind, FieldComponent, palette::ComponentPalette};

#[derive(Debug, Clone)]
pub struct ArrayBufferComponent {
    buffer: String,
    palette: Arc<ComponentPalette>,
}

impl ArrayBufferComponent {
    pub fn new(schema: &FieldSchema, palette: Arc<ComponentPalette>) -> Self {
        let buffer = schema
            .default
            .as_ref()
            .and_then(|value| value.as_array())
            .map(|items| array_to_string(items))
            .unwrap_or_default();
        Self { buffer, palette }
    }
}

impl FieldComponent for ArrayBufferComponent {
    fn kind(&self) -> ComponentKind {
        ComponentKind::ArrayBuffer
    }

    fn display_value(&self, _schema: &FieldSchema) -> String {
        format!("[{}]", self.buffer.trim())
    }

    fn handle_key(&mut self, schema: &FieldSchema, key: &crossterm::event::KeyEvent) -> bool {
        handle_text_edit(&mut self.buffer, schema, key, &self.palette)
    }

    fn seed_value(&mut self, _schema: &FieldSchema, value: &Value) {
        if let Value::Array(items) = value {
            self.buffer = array_to_string(items);
        }
    }

    fn current_value(&self, schema: &FieldSchema) -> Result<Option<Value>, FieldCoercionError> {
        if let FieldKind::Array(inner) = &schema.kind {
            array_value(&self.buffer, inner.as_ref(), schema)
        } else {
            Ok(None)
        }
    }
}
