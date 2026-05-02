use crate::{
    tui::model::{FieldKind, FieldSchema, KeyValueField},
    tui::state::FieldState,
};
use serde_json::json;
use std::collections::HashMap;

fn scalar_array_field() -> FieldState {
    FieldState::from_schema(FieldSchema {
        name: "routes".to_string(),
        path: vec!["routes".to_string()],
        pointer: "/routes".to_string(),
        title: "Routes".to_string(),
        description: Some("HTTP routes".to_string()),
        kind: FieldKind::Array(Box::new(FieldKind::String)),
        required: false,
        default: None,
        metadata: HashMap::new(),
    })
}

fn key_value_field() -> FieldState {
    let template = KeyValueField {
        key_title: "Header".to_string(),
        key_description: None,
        key_default: None,
        key_schema: json!({"type": "string"}),
        value_title: "Value".to_string(),
        value_description: None,
        value_default: None,
        value_schema: json!({"type": "string"}),
        value_kind: Box::new(FieldKind::String),
        entry_schema: json!({"type": "object"}),
    };
    FieldState::from_schema(FieldSchema {
        name: "headers".to_string(),
        path: vec!["headers".to_string()],
        pointer: "/headers".to_string(),
        title: "Headers".to_string(),
        description: None,
        kind: FieldKind::KeyValue(Box::new(template)),
        required: false,
        default: None,
        metadata: HashMap::new(),
    })
}

#[test]
fn scalar_array_overlay_context_reports_selected_entry() {
    let mut field = scalar_array_field();
    field.seed_value(&json!(["/health", "/ready"]));
    let ctx = field.overlay_context().expect("context");
    let panel = ctx.entry_panel.expect("panel state");
    assert_eq!(panel.entries[0], "#1 \"/health\"");
    assert_eq!(panel.selected, 0);
    assert!(
        ctx.instructions
            .as_ref()
            .expect("instructions")
            .contains("Ctrl+N")
    );
    assert!(ctx.title.unwrap().starts_with("#1"));
}

#[test]
fn key_value_overlay_context_includes_entry_label() {
    let mut field = key_value_field();
    field.seed_value(&json!({"env": "prod"}));
    let ctx = field.overlay_context().expect("context");
    assert!(
        ctx.instructions
            .as_ref()
            .expect("instructions")
            .contains("Ctrl+D")
    );
    let panel = ctx.entry_panel.expect("panel state");
    assert_eq!(panel.entries.len(), 1);
    assert_eq!(panel.selected, 0);
    assert!(ctx.title.unwrap().starts_with("env"));
}
