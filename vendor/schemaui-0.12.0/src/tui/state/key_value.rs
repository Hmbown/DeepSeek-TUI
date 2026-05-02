use std::sync::Arc;

use serde_json::{Map, Value};
use unicode_width::UnicodeWidthStr;

use crate::tui::model::{FieldKind, FieldSchema, KeyValueField};

use super::{
    error::FieldCoercionError,
    field::{FieldState, components::ComponentPalette},
    form_state::FormState,
    section::SectionState,
    value_summary::{summarize_value, summarize_value_with_limit},
};

#[derive(Debug, Clone)]
pub struct KeyValueState {
    pointer: String,
    template: KeyValueField,
    entries: Vec<KeyValueEntry>,
    selected: usize,
    counter: usize,
    palette: Arc<ComponentPalette>,
}

#[derive(Debug, Clone)]
struct KeyValueEntry {
    key: String,
    value: Value,
}

#[derive(Debug, Clone)]
pub struct KeyValueEditorSession {
    pub form_state: FormState,
    pub schema: Value,
}

#[derive(Debug)]
pub struct KeyValueEditorContext {
    pub entry_index: usize,
    #[allow(dead_code)]
    pub entry_label: String,
    pub session: KeyValueEditorSession,
}

impl KeyValueState {
    pub fn new(
        pointer: &str,
        template: &KeyValueField,
        default: Option<&Value>,
        palette: Arc<ComponentPalette>,
    ) -> Self {
        let mut state = Self {
            pointer: pointer.to_string(),
            template: template.clone(),
            entries: Vec::new(),
            selected: 0,
            counter: 0,
            palette,
        };
        if let Some(Value::Object(map)) = default {
            state.seed_entries_from_object(map);
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
        let placeholder = self.next_placeholder_key();
        let entry = KeyValueEntry {
            key: placeholder,
            value: self.template.value_default.clone().unwrap_or(Value::Null),
        };
        self.entries.push(entry);
        self.selected = self.entries.len().saturating_sub(1);
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
        let target = self.selected as i32 + delta;
        if target < 0 || target >= len {
            return false;
        }
        self.entries.swap(self.selected, target as usize);
        self.selected = target as usize;
        true
    }

    pub fn selected_label(&self) -> Option<String> {
        let idx = self.selected_index()?;
        let entry = self.entries.get(idx)?;
        Some(format!("{} = {}", entry.key, summarize_value(&entry.value)))
    }

    pub fn selected_label_with_limit(&self, max_visible: usize) -> Option<String> {
        let idx = self.selected_index()?;
        let entry = self.entries.get(idx)?;
        let prefix = format!("{} = ", entry.key);
        let remaining = max_visible.saturating_sub(UnicodeWidthStr::width(prefix.as_str()));
        Some(format!(
            "{prefix}{}",
            summarize_value_with_limit(&entry.value, remaining.max(1))
        ))
    }

    pub fn summaries(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| format!("{} = {}", entry.key, summarize_value(&entry.value)))
            .collect()
    }

    pub fn summaries_with_limit(&self, max_visible: usize) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| {
                let prefix = format!("{} = ", entry.key);
                let remaining = max_visible.saturating_sub(UnicodeWidthStr::width(prefix.as_str()));
                format!(
                    "{prefix}{}",
                    summarize_value_with_limit(&entry.value, remaining.max(1))
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

    pub fn build_value(&self, required: bool) -> Result<Option<Value>, FieldCoercionError> {
        if self.entries.is_empty() {
            return if required {
                Ok(Some(Value::Object(Map::new())))
            } else {
                Ok(None)
            };
        }
        let mut map = Map::new();
        for entry in &self.entries {
            let key = entry.key.trim();
            if key.is_empty() {
                return Err(FieldCoercionError {
                    pointer: self.pointer.clone(),
                    message: "key cannot be empty".to_string(),
                });
            }
            if map.insert(key.to_string(), entry.value.clone()).is_some() {
                return Err(FieldCoercionError {
                    pointer: pointer_for_key(&self.pointer, key),
                    message: format!("duplicate key '{key}'"),
                });
            }
        }
        Ok(Some(Value::Object(map)))
    }

    pub fn seed_entries_from_object(&mut self, map: &Map<String, Value>) {
        self.entries.clear();
        for (key, value) in map {
            self.entries.push(KeyValueEntry {
                key: key.clone(),
                value: value.clone(),
            });
        }
        if self.entries.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.entries.len() - 1);
        }
    }

    pub fn open_selected_editor(&mut self) -> Result<KeyValueEditorContext, FieldCoercionError> {
        let idx = self.selected_index().ok_or_else(|| FieldCoercionError {
            pointer: self.pointer.clone(),
            message: "no entry selected".to_string(),
        })?;
        let entry = self.entries.get(idx).ok_or_else(|| FieldCoercionError {
            pointer: self.pointer.clone(),
            message: "invalid entry selection".to_string(),
        })?;
        let form_state = self.build_form_state(Some(entry));
        Ok(KeyValueEditorContext {
            entry_index: idx,
            entry_label: format!("{} = {}", entry.key, summarize_value(&entry.value)),
            session: KeyValueEditorSession {
                form_state,
                schema: self.template.entry_schema.clone(),
            },
        })
    }

    pub fn apply_editor_session(
        &mut self,
        entry_index: usize,
        session: &KeyValueEditorSession,
    ) -> Result<bool, FieldCoercionError> {
        let value = session.form_state.try_build_value()?;
        let object = value.as_object().ok_or_else(|| FieldCoercionError {
            pointer: self.pointer.clone(),
            message: "overlay did not produce an object".to_string(),
        })?;
        let key_value = object.get("key").ok_or_else(|| FieldCoercionError {
            pointer: self.pointer.clone(),
            message: "missing key field".to_string(),
        })?;
        let key = if let Some(raw) = key_value.as_str() {
            raw.to_string()
        } else {
            key_value.to_string()
        };
        if key.trim().is_empty() {
            return Err(FieldCoercionError {
                pointer: pointer_for_key(&self.pointer, &key),
                message: "key cannot be empty".to_string(),
            });
        }
        let value_field = object.get("value").cloned().unwrap_or(Value::Null);

        for (idx, existing) in self.entries.iter().enumerate() {
            if idx != entry_index && existing.key == key {
                return Err(FieldCoercionError {
                    pointer: pointer_for_key(&self.pointer, &key),
                    message: format!("duplicate key '{key}'"),
                });
            }
        }

        let changed = self
            .entries
            .get_mut(entry_index)
            .map(|entry| {
                let mut dirty = false;
                if entry.key != key {
                    entry.key = key;
                    dirty = true;
                }
                if entry.value != value_field {
                    entry.value = value_field;
                    dirty = true;
                }
                dirty
            })
            .unwrap_or(false);
        if changed {
            self.selected = entry_index;
        }

        Ok(changed)
    }

    fn build_form_state(&self, entry: Option<&KeyValueEntry>) -> FormState {
        let mut key_schema = self.key_field_schema();
        key_schema.default = entry
            .map(|item| Value::String(item.key.clone()))
            .or_else(|| self.template.key_default.clone());
        let mut value_schema = self.value_field_schema();
        if let Some(item) = entry {
            value_schema.default = Some(item.value.clone());
        } else if value_schema.default.is_none() {
            value_schema.default = self.template.value_default.clone();
        }

        let key_field = FieldState::from_schema_with_palette(key_schema, Arc::clone(&self.palette));
        let value_field =
            FieldState::from_schema_with_palette(value_schema, Arc::clone(&self.palette));
        let section = SectionState {
            id: "key_value".to_string(),
            title: "Key/Value Entry".to_string(),
            description: None,
            path: vec!["entry".to_string()],
            depth: 0,
            fields: vec![key_field, value_field],
            scroll_offset: 0,
        };
        FormState::from_sections("key_value", "Key/Value Entry", None, vec![section])
    }

    fn key_field_schema(&self) -> FieldSchema {
        FieldSchema {
            name: "key".to_string(),
            path: vec!["key".to_string()],
            pointer: "/key".to_string(),
            title: self.template.key_title.clone(),
            description: self.template.key_description.clone(),
            kind: FieldKind::String,
            required: true,
            default: self.template.key_default.clone(),
            metadata: Default::default(),
        }
    }

    fn value_field_schema(&self) -> FieldSchema {
        FieldSchema {
            name: "value".to_string(),
            path: vec!["value".to_string()],
            pointer: "/value".to_string(),
            title: self.template.value_title.clone(),
            description: self.template.value_description.clone(),
            kind: (*self.template.value_kind).clone(),
            required: true,
            default: self.template.value_default.clone(),
            metadata: Default::default(),
        }
    }

    fn next_placeholder_key(&mut self) -> String {
        loop {
            let candidate = format!("key-{}", self.counter + 1);
            self.counter = self.counter.saturating_add(1);
            if !self.entries.iter().any(|entry| entry.key == candidate) {
                return candidate;
            }
        }
    }
}

fn pointer_for_key(base: &str, key: &str) -> String {
    let mut encoded = String::new();
    for ch in key.chars() {
        match ch {
            '~' => encoded.push_str("~0"),
            '/' => encoded.push_str("~1"),
            other => encoded.push(other),
        }
    }
    if base.is_empty() {
        format!("/{}", encoded)
    } else if base.ends_with('/') {
        format!("{base}{encoded}")
    } else {
        format!("{base}/{}", encoded)
    }
}
