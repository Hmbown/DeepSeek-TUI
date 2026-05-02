use std::sync::Arc;

use serde_json::Value;

use crate::tui::model::FieldSchema;
use crate::tui::state::error::FieldCoercionError;
use crate::tui::state::field::convert::value_to_string;

use super::{ComponentKind, EnumStateRef, FieldComponent, palette::ComponentPalette};

#[derive(Debug, Clone)]
pub struct EnumComponent {
    labels: Vec<String>,
    values: Vec<Value>,
    selected: usize,
    palette: Arc<ComponentPalette>,
}

impl EnumComponent {
    pub fn new(
        labels: &[String],
        values: &[Value],
        schema: &FieldSchema,
        palette: Arc<ComponentPalette>,
    ) -> Self {
        let selected = schema
            .default
            .as_ref()
            .and_then(|default| values.iter().position(|candidate| candidate == default))
            .or_else(|| {
                let default_label = schema
                    .default
                    .as_ref()
                    .map(value_to_string)
                    .and_then(|value| if value.is_empty() { None } else { Some(value) })?;
                labels.iter().position(|item| item == &default_label)
            })
            .unwrap_or(0);
        Self {
            labels: labels.to_vec(),
            values: values.to_vec(),
            selected,
            palette,
        }
    }
}

impl FieldComponent for EnumComponent {
    fn kind(&self) -> ComponentKind {
        ComponentKind::Enum
    }

    fn display_value(&self, _schema: &FieldSchema) -> String {
        self.labels
            .get(self.selected)
            .cloned()
            .unwrap_or_else(|| "<none>".to_string())
    }

    fn handle_key(&mut self, _schema: &FieldSchema, key: &crossterm::event::KeyEvent) -> bool {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Left => {
                if self.labels.is_empty() {
                    return false;
                }
                if self.selected == 0 {
                    if self.palette.enums.wrap_around {
                        self.selected = self.labels.len().saturating_sub(1);
                    }
                } else {
                    self.selected -= 1;
                }
                true
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Right => {
                if self.labels.is_empty() {
                    return false;
                }
                if self.selected + 1 >= self.labels.len() {
                    if self.palette.enums.wrap_around {
                        self.selected = 0;
                    }
                } else {
                    self.selected += 1;
                }
                true
            }
            _ => false,
        }
    }

    fn seed_value(&mut self, _schema: &FieldSchema, value: &Value) {
        if let Some(idx) = self.values.iter().position(|candidate| candidate == value) {
            self.selected = idx;
        }
    }

    fn current_value(&self, _schema: &FieldSchema) -> Result<Option<Value>, FieldCoercionError> {
        Ok(self.values.get(self.selected).cloned())
    }

    fn enum_state(&self) -> Option<EnumStateRef<'_>> {
        Some(EnumStateRef {
            options: &self.labels,
            selected: self.selected,
        })
    }

    fn set_enum_index(&mut self, index: usize) -> bool {
        if self.labels.is_empty() {
            return false;
        }
        let bounded = index.min(self.labels.len() - 1);
        if self.selected != bounded {
            self.selected = bounded;
            true
        } else {
            false
        }
    }
}
