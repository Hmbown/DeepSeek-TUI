use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{
    tui::app::options::UiOptions,
    tui::model::{FieldKind, FieldSchema, KeyValueField},
    tui::state::FieldState,
};

fn integer_field_schema() -> FieldSchema {
    FieldSchema {
        name: "threshold".to_string(),
        path: vec!["threshold".to_string()],
        pointer: "/threshold".to_string(),
        title: "Threshold".to_string(),
        description: None,
        kind: FieldKind::Integer,
        required: false,
        default: Some(Value::Number(0.into())),
        metadata: Default::default(),
    }
}

fn bool_field_schema() -> FieldSchema {
    FieldSchema {
        name: "enabled".to_string(),
        path: vec!["enabled".to_string()],
        pointer: "/enabled".to_string(),
        title: "Enabled".to_string(),
        description: None,
        kind: FieldKind::Boolean,
        required: false,
        default: Some(Value::Bool(false)),
        metadata: Default::default(),
    }
}

fn key_value_schema() -> FieldSchema {
    let template = KeyValueField {
        key_title: "Key".to_string(),
        key_description: None,
        key_default: None,
        key_schema: json!({"type": "string"}),
        value_title: "Value".to_string(),
        value_description: None,
        value_default: None,
        value_schema: json!({"type": "string"}),
        value_kind: Box::new(FieldKind::String),
        entry_schema: json!({
            "type": "object",
            "required": ["key", "value"],
            "properties": {
                "key": {"type": "string"},
                "value": {"type": "string"}
            }
        }),
    };
    FieldSchema {
        name: "labels".to_string(),
        path: vec!["labels".to_string()],
        pointer: "/labels".to_string(),
        title: "Labels".to_string(),
        description: None,
        kind: FieldKind::KeyValue(Box::new(template)),
        required: false,
        default: None,
        metadata: Default::default(),
    }
}

#[test]
fn integer_step_uses_component_palette() {
    let options = UiOptions::default().with_integer_step(5);
    let palette = options.component_palette();
    let mut field =
        FieldState::from_schema_with_palette(integer_field_schema(), Arc::clone(&palette));

    let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
    field.handle_key(&key);

    assert_eq!(field.display_value(), "5");
}

#[test]
fn bool_labels_follow_palette_configuration() {
    let options = UiOptions::default().with_bool_labels("On", "Off");
    let palette = options.component_palette();
    let mut field = FieldState::from_schema_with_palette(bool_field_schema(), Arc::clone(&palette));

    assert_eq!(field.display_value(), "Off");

    let key = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
    field.handle_key(&key);

    assert_eq!(field.display_value(), "On");
}

#[test]
fn overlay_context_uses_custom_instructions() {
    let options =
        UiOptions::default().with_overlay_instructions("Ctrl+X remove • Ctrl+Y duplicate");
    let palette = options.component_palette();
    let field = FieldState::from_schema_with_palette(key_value_schema(), Arc::clone(&palette));

    let context = field
        .overlay_context()
        .expect("key/value overlay context should exist");

    assert_eq!(
        context.instructions.as_deref(),
        Some("Ctrl+X remove • Ctrl+Y duplicate")
    );
}
