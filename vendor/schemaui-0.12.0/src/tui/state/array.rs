use std::sync::Arc;

use serde_json::{Number, Value, json};
use unicode_width::UnicodeWidthStr;

use crate::tui::model::{FieldKind, FieldSchema};

use super::{
    error::FieldCoercionError,
    field::FieldState,
    field::components::ComponentPalette,
    form_state::FormState,
    section::SectionState,
    value_summary::{summarize_value, summarize_value_with_limit},
};

#[derive(Debug, Clone)]
pub struct ScalarArrayState {
    pointer: String,
    template: ScalarArrayTemplate,
    entries: Vec<Value>,
    selected: usize,
    palette: Arc<ComponentPalette>,
}

#[derive(Debug, Clone)]
struct ScalarArrayTemplate {
    label: String,
    description: Option<String>,
    entry_schema: Value,
    item_kind: FieldKind,
}

#[derive(Debug, Clone)]
pub struct ArrayEditorSession {
    pub form_state: FormState,
    pub schema: Value,
    pub title: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArrayEditorContext {
    pub entry_index: usize,
    #[allow(dead_code)]
    pub entry_label: String,
    pub session: ArrayEditorSession,
}

impl ScalarArrayState {
    pub fn new(
        pointer: &str,
        label: String,
        description: Option<String>,
        kind: &FieldKind,
        default: Option<&Value>,
        palette: Arc<ComponentPalette>,
    ) -> Self {
        let entry_schema = json!({
            "type": "object",
            "required": ["value"],
            "properties": {
                "value": kind_to_schema_fragment(kind),
            }
        });
        let mut state = Self {
            pointer: pointer.to_string(),
            template: ScalarArrayTemplate {
                label,
                description,
                entry_schema,
                item_kind: kind.clone(),
            },
            entries: Vec::new(),
            selected: 0,
            palette,
        };
        if let Some(Value::Array(items)) = default {
            state.seed_entries_from_array(items);
        }
        state
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn selected_index(&self) -> Option<usize> {
        if self.entries.is_empty() {
            None
        } else {
            Some(self.selected.min(self.entries.len() - 1))
        }
    }

    pub fn select(&mut self, delta: i32) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let len = self.entries.len() as i32;
        let next = (self.selected as i32 + delta).clamp(0, len - 1);
        let changed = next as usize != self.selected;
        self.selected = next as usize;
        changed
    }

    pub fn set_selected(&mut self, index: usize) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let len = self.entries.len();
        let bounded = index.min(len.saturating_sub(1));
        let changed = bounded != self.selected;
        self.selected = bounded;
        changed
    }

    pub fn add_entry(&mut self) -> bool {
        let value = default_value(&self.template.item_kind);
        self.entries.push(value);
        if !self.entries.is_empty() {
            self.selected = self.entries.len() - 1;
        }
        true
    }

    pub fn remove_selected(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let idx = self.selected.min(self.entries.len() - 1);
        self.entries.remove(idx);
        if idx >= self.entries.len() {
            self.selected = self.entries.len().saturating_sub(1);
        }
        true
    }

    pub fn move_selected(&mut self, delta: i32) -> bool {
        if self.entries.len() < 2 {
            return false;
        }
        let len = self.entries.len() as i32;
        let next = self.selected as i32 + delta;
        if next < 0 || next >= len {
            return false;
        }
        self.entries.swap(self.selected, next as usize);
        self.selected = next as usize;
        true
    }

    pub fn selected_label(&self) -> Option<String> {
        let idx = self.selected_index()?;
        let value = self.entries.get(idx)?;
        Some(format!("#{} {}", idx + 1, summarize_value(value)))
    }

    pub fn selected_label_with_limit(&self, max_visible: usize) -> Option<String> {
        let idx = self.selected_index()?;
        let value = self.entries.get(idx)?;
        let prefix = format!("#{} ", idx + 1);
        Some(format!(
            "{prefix}{}",
            summarize_value_with_limit(
                value,
                max_visible
                    .saturating_sub(UnicodeWidthStr::width(prefix.as_str()))
                    .max(1),
            )
        ))
    }

    pub fn summaries(&self) -> Vec<String> {
        self.entries
            .iter()
            .enumerate()
            .map(|(idx, value)| format!("#{} {}", idx + 1, summarize_value(value)))
            .collect()
    }

    pub fn summaries_with_limit(&self, max_visible: usize) -> Vec<String> {
        self.entries
            .iter()
            .enumerate()
            .map(|(idx, value)| {
                let prefix = format!("#{} ", idx + 1);
                format!(
                    "{prefix}{}",
                    summarize_value_with_limit(
                        value,
                        max_visible
                            .saturating_sub(UnicodeWidthStr::width(prefix.as_str()))
                            .max(1),
                    )
                )
            })
            .collect()
    }

    pub fn panel(&self) -> Option<(Vec<String>, usize)> {
        let idx = self.selected_index()?;
        Some((self.summaries(), idx))
    }

    pub fn panel_with_limit(&self, max_visible: usize) -> Option<(Vec<String>, usize)> {
        let idx = self.selected_index()?;
        Some((self.summaries_with_limit(max_visible), idx))
    }

    pub fn seed_entries_from_array(&mut self, items: &[Value]) {
        self.entries = items.to_vec();
        if self.entries.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.entries.len() - 1);
        }
    }

    pub fn build_value(&self, required: bool) -> Result<Option<Value>, FieldCoercionError> {
        if self.entries.is_empty() {
            if required {
                return Ok(Some(Value::Array(Vec::new())));
            }
            return Ok(None);
        }
        Ok(Some(Value::Array(self.entries.clone())))
    }

    pub fn open_selected_editor(&mut self) -> Result<ArrayEditorContext, FieldCoercionError> {
        let idx = self.selected_index().ok_or_else(|| FieldCoercionError {
            pointer: self.pointer.clone(),
            message: "no entry selected".to_string(),
        })?;
        let value = self
            .entries
            .get(idx)
            .cloned()
            .unwrap_or_else(|| default_value(&self.template.item_kind));
        let form_state = build_entry_form_state(&self.template, &value, &self.palette);
        Ok(ArrayEditorContext {
            entry_index: idx,
            entry_label: format!("#{} {}", idx + 1, summarize_value(&value)),
            session: ArrayEditorSession {
                form_state,
                schema: self.template.entry_schema.clone(),
                title: format!("{} · entry #{}", self.template.label, idx + 1),
                description: self.template.description.clone(),
            },
        })
    }

    pub fn apply_editor_session(
        &mut self,
        entry_index: usize,
        session: &ArrayEditorSession,
    ) -> Result<bool, FieldCoercionError> {
        let form_value = session.form_state.try_build_value()?;
        let object = form_value.as_object().ok_or_else(|| FieldCoercionError {
            pointer: self.pointer.clone(),
            message: "overlay payload missing value".to_string(),
        })?;
        let new_value = object.get("value").cloned().unwrap_or(Value::Null);
        let entry = self
            .entries
            .get_mut(entry_index)
            .ok_or_else(|| FieldCoercionError {
                pointer: self.pointer.clone(),
                message: "invalid entry selection".to_string(),
            })?;
        if *entry != new_value {
            *entry = new_value;
            self.selected = entry_index;
            return Ok(true);
        }
        Ok(false)
    }
}

fn build_entry_form_state(
    template: &ScalarArrayTemplate,
    value: &Value,
    palette: &Arc<ComponentPalette>,
) -> FormState {
    let schema = FieldSchemaStub::new(template, value.clone());
    let mut field_state = FieldState::from_schema_with_palette(schema.into(), Arc::clone(palette));
    field_state.seed_value(value);
    let section = SectionState {
        id: "array_entry".to_string(),
        title: template.label.clone(),
        description: template.description.clone(),
        path: vec!["value".to_string()],
        depth: 0,
        fields: vec![field_state],
        scroll_offset: 0,
    };
    FormState::from_sections(
        "array_entry",
        template.label.clone(),
        template.description.clone(),
        vec![section],
    )
}

struct FieldSchemaStub {
    template: ScalarArrayTemplate,
    value: Value,
}

impl FieldSchemaStub {
    fn new(template: &ScalarArrayTemplate, value: Value) -> Self {
        Self {
            template: template.clone(),
            value,
        }
    }
}

impl From<FieldSchemaStub> for FieldSchema {
    fn from(stub: FieldSchemaStub) -> Self {
        FieldSchema {
            name: "value".to_string(),
            path: vec!["value".to_string()],
            pointer: "/value".to_string(),
            title: format!("{} item", stub.template.label),
            description: stub.template.description,
            kind: stub.template.item_kind,
            required: true,
            default: Some(stub.value),
            metadata: Default::default(),
        }
    }
}

fn kind_to_schema_fragment(kind: &FieldKind) -> Value {
    match kind {
        FieldKind::String => json!({"type": "string"}),
        FieldKind::Integer => json!({"type": "integer"}),
        FieldKind::Number => json!({"type": "number"}),
        FieldKind::Boolean => json!({"type": "boolean"}),
        FieldKind::Enum { values, .. } => json!({"enum": values}),
        FieldKind::Nullable(inner) => {
            let mut schema = kind_to_schema_fragment(inner);
            if let Some(obj) = schema.as_object_mut() {
                obj.insert(
                    "type".to_string(),
                    json!([obj.get("type").cloned().unwrap_or(json!("string")), "null"]),
                );
            }
            schema
        }
        FieldKind::Json => json!({"type": "object"}),
        _ => json!({"type": "string"}),
    }
}

fn default_value(kind: &FieldKind) -> Value {
    match kind {
        FieldKind::String | FieldKind::Json => Value::String(String::new()),
        FieldKind::Integer => Value::Number(0.into()),
        FieldKind::Number => Value::Number(Number::from_f64(0.0).unwrap()),
        FieldKind::Boolean => Value::Bool(false),
        FieldKind::Enum { values, .. } => values
            .first()
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())),
        FieldKind::Nullable(_) => Value::Null,
        _ => Value::String(String::new()),
    }
}
