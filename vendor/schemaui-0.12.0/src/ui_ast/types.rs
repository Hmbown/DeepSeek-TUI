use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "web-types")]
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[cfg_attr(feature = "web-types", ts(export, export_to = "web/types/ui-ast.ts"))]
pub struct UiAst {
    pub roots: Vec<UiNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
pub struct UiNode {
    pub pointer: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub required: bool,
    #[cfg_attr(feature = "web-types", ts(type = "Record<string, unknown> | null"))]
    pub default_value: Option<Value>,
    pub kind: UiNodeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiNodeKind {
    Field {
        scalar: ScalarKind,
        enum_options: Option<Vec<String>>,
        #[cfg_attr(feature = "web-types", ts(type = "unknown[] | null"))]
        enum_values: Option<Vec<Value>>,
        nullable: bool,
    },
    Array {
        item: Box<UiNodeKind>,
        min_items: Option<u64>,
        max_items: Option<u64>,
    },
    KeyValue {
        template: Box<UiKeyValueNode>,
    },
    Composite {
        mode: CompositeMode,
        allow_multiple: bool,
        variants: Vec<UiVariant>,
    },
    Object {
        children: Vec<UiNode>,
        required: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
pub struct UiKeyValueNode {
    pub key_title: String,
    pub key_description: Option<String>,
    #[cfg_attr(feature = "web-types", ts(type = "unknown | null"))]
    pub key_default: Option<Value>,
    #[cfg_attr(feature = "web-types", ts(type = "Record<string, unknown>"))]
    pub key_schema: Value,
    pub value_title: String,
    pub value_description: Option<String>,
    #[cfg_attr(feature = "web-types", ts(type = "unknown | null"))]
    pub value_default: Option<Value>,
    #[cfg_attr(feature = "web-types", ts(type = "Record<string, unknown>"))]
    pub value_schema: Value,
    pub value_kind: Box<UiNodeKind>,
    #[cfg_attr(feature = "web-types", ts(type = "Record<string, unknown>"))]
    pub entry_schema: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum ScalarKind {
    String,
    Integer,
    Number,
    Boolean,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum CompositeMode {
    OneOf,
    AnyOf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
pub struct UiVariant {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub is_object: bool,
    pub node: UiNodeKind,
    #[cfg_attr(feature = "web-types", ts(type = "Record<string, unknown>"))]
    pub schema: Value,
}
