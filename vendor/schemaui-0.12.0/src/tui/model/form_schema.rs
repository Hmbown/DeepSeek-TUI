use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormSchema {
    #[allow(dead_code)]
    pub title: Option<String>,
    #[allow(dead_code)]
    pub description: Option<String>,
    pub roots: Vec<RootSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootSection {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub sections: Vec<FormSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormSection {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub path: Vec<String>,
    pub fields: Vec<FieldSchema>,
    pub children: Vec<FormSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldKind {
    String,
    Integer,
    Number,
    Boolean,
    Enum {
        labels: Vec<String>,
        values: Vec<Value>,
    },
    Nullable(Box<FieldKind>),
    Array(Box<FieldKind>),
    Json,
    Composite(Box<CompositeField>),
    KeyValue(Box<KeyValueField>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositeField {
    pub mode: CompositeMode,
    pub variants: Vec<CompositeVariant>,
}

impl CompositeField {
    pub fn variant_titles(&self) -> Vec<String> {
        self.variants.iter().map(|v| v.title.clone()).collect()
    }

    pub fn variant_count(&self) -> usize {
        self.variants.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_names_use_title_only() {
        let schema = FieldSchema {
            name: "__value".to_string(),
            path: vec!["__value".to_string()],
            pointer: "/root/__value".to_string(),
            title: "String List".to_string(),
            description: None,
            kind: FieldKind::String,
            required: false,
            default: None,
            metadata: HashMap::new(),
        };
        assert_eq!(schema.display_label(), "String List");
    }

    #[test]
    fn external_names_show_title_and_name() {
        let schema = FieldSchema {
            name: "deepItems".to_string(),
            path: vec!["deepItems".to_string()],
            pointer: "/deepItems".to_string(),
            title: "Deep Items".to_string(),
            description: None,
            kind: FieldKind::String,
            required: false,
            default: None,
            metadata: HashMap::new(),
        };
        assert_eq!(schema.display_label(), "Deep Items (deepItems)");
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyValueField {
    pub key_title: String,
    pub key_description: Option<String>,
    pub key_default: Option<Value>,
    pub key_schema: Value,
    pub value_title: String,
    pub value_description: Option<String>,
    pub value_default: Option<Value>,
    pub value_schema: Value,
    pub value_kind: Box<FieldKind>,
    pub entry_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompositeMode {
    OneOf,
    AnyOf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositeVariant {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub schema: Value,
    pub is_object: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    pub path: Vec<String>,
    pub pointer: String,
    pub title: String,
    pub description: Option<String>,
    pub kind: FieldKind,
    pub required: bool,
    pub default: Option<Value>,
    pub metadata: HashMap<String, Value>,
}

impl FieldSchema {
    pub fn display_label(&self) -> String {
        // Field names starting with "__" are reserved for internal plumbing
        // (e.g. composite wrappers) and should not surface their raw name.
        if self.name.starts_with("__") {
            return self.title.clone();
        }
        if self.title.eq_ignore_ascii_case(&self.name) {
            self.title.clone()
        } else {
            format!("{} ({})", self.title, self.name)
        }
    }
}
