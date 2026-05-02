use std::sync::Arc;

use serde_json::Value;

use crate::tui::model::FieldSchema;
use crate::tui::state::error::FieldCoercionError;

use super::{ComponentKind, FieldComponent, MultiSelectStateRef, palette::ComponentPalette};

#[derive(Debug, Clone)]
pub struct MultiSelectComponent {
    labels: Vec<String>,
    values: Vec<Value>,
    selected: Vec<bool>,
    palette: Arc<ComponentPalette>,
}

impl MultiSelectComponent {
    pub fn new(
        labels: &[String],
        values: &[Value],
        default: Option<&Value>,
        palette: Arc<ComponentPalette>,
    ) -> Self {
        let mut selected = vec![false; labels.len()];
        if let Some(Value::Array(items)) = default {
            for item in items {
                if let Some(idx) = values.iter().position(|candidate| candidate == item) {
                    selected[idx] = true;
                }
            }
        }
        Self {
            labels: labels.to_vec(),
            values: values.to_vec(),
            selected,
            palette,
        }
    }
}

impl FieldComponent for MultiSelectComponent {
    fn kind(&self) -> ComponentKind {
        ComponentKind::MultiSelect
    }

    fn display_value(&self, _schema: &FieldSchema) -> String {
        let values = self
            .labels
            .iter()
            .zip(self.selected.iter())
            .filter_map(|(option, flag)| if *flag { Some(option.clone()) } else { None })
            .collect::<Vec<_>>();
        if values.is_empty() {
            format!("[] {}", self.palette.collection.list_hint)
        } else {
            format!("[{}]", values.join(", "))
        }
    }

    fn seed_value(&mut self, _schema: &FieldSchema, value: &Value) {
        if let Value::Array(items) = value {
            let mut flags = vec![false; self.labels.len()];
            for item in items {
                if let Some(idx) = self.values.iter().position(|candidate| candidate == item) {
                    flags[idx] = true;
                }
            }
            if flags.len() == self.selected.len() {
                self.selected = flags;
            }
        }
    }

    fn current_value(&self, _schema: &FieldSchema) -> Result<Option<Value>, FieldCoercionError> {
        let values = self
            .values
            .iter()
            .zip(self.selected.iter())
            .filter_map(
                |(option, flag)| {
                    if *flag { Some(option.clone()) } else { None }
                },
            )
            .collect();
        Ok(Some(Value::Array(values)))
    }

    fn multi_state(&self) -> Option<MultiSelectStateRef<'_>> {
        Some(MultiSelectStateRef {
            options: &self.labels,
            selected: &self.selected,
        })
    }

    fn set_multi_state(&mut self, flags: &[bool]) -> bool {
        if flags.len() != self.selected.len() {
            return false;
        }
        if self.selected != flags {
            self.selected.clone_from_slice(flags);
            true
        } else {
            false
        }
    }
}
