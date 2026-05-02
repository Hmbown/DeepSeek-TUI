use crate::{
    tui::model::{FieldKind, FieldSchema},
    tui::state::field::components::{
        BoolComponent, ComponentPalette, EnumComponent, FieldComponent, TextComponent,
    },
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use std::sync::Arc;

fn integer_schema() -> FieldSchema {
    FieldSchema {
        name: "count".to_string(),
        path: vec!["count".to_string()],
        pointer: "/count".to_string(),
        title: "count".to_string(),
        description: None,
        kind: FieldKind::Integer,
        required: false,
        default: None,
        metadata: Default::default(),
    }
}

fn string_schema(default: &str) -> FieldSchema {
    FieldSchema {
        name: "name".to_string(),
        path: vec!["name".to_string()],
        pointer: "/name".to_string(),
        title: "name".to_string(),
        description: None,
        kind: FieldKind::String,
        required: false,
        default: Some(json!(default)),
        metadata: Default::default(),
    }
}

fn number_schema() -> FieldSchema {
    FieldSchema {
        name: "ratio".to_string(),
        path: vec!["ratio".to_string()],
        pointer: "/ratio".to_string(),
        title: "ratio".to_string(),
        description: None,
        kind: FieldKind::Number,
        required: false,
        default: None,
        metadata: Default::default(),
    }
}

fn bool_schema() -> FieldSchema {
    FieldSchema {
        name: "enabled".to_string(),
        path: vec!["enabled".to_string()],
        pointer: "/enabled".to_string(),
        title: "enabled".to_string(),
        description: None,
        kind: FieldKind::Boolean,
        required: false,
        default: Some(json!(false)),
        metadata: Default::default(),
    }
}

fn enum_schema() -> FieldSchema {
    FieldSchema {
        name: "mode".to_string(),
        path: vec!["mode".to_string()],
        pointer: "/mode".to_string(),
        title: "mode".to_string(),
        description: None,
        kind: FieldKind::Enum {
            labels: vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
            values: vec![json!("alpha"), json!("beta"), json!("gamma")],
        },
        required: false,
        default: Some(json!("alpha")),
        metadata: Default::default(),
    }
}

#[test]
fn text_component_supports_numeric_stepper() {
    let schema = integer_schema();
    let mut component = TextComponent::new(&schema, Arc::new(ComponentPalette::default()));
    let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
    assert!(component.handle_key(&schema, &key));
    assert_eq!(component.display_value(&schema), "1");
    let key = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
    assert!(component.handle_key(&schema, &key));
    assert_eq!(component.display_value(&schema), "0");
}

#[test]
fn text_component_rejects_control_characters() {
    let schema = integer_schema();
    let mut component = TextComponent::new(&schema, Arc::new(ComponentPalette::default()));
    let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
    assert!(!component.handle_key(&schema, &ctrl_a));
    assert_eq!(component.display_value(&schema), "");
}

#[test]
fn number_input_rejects_non_numeric_chars_but_accepts_decimal_and_exponent_editing() {
    let schema = number_schema();
    let mut component = TextComponent::new(&schema, Arc::new(ComponentPalette::default()));

    assert!(!component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)
    ));
    assert_eq!(component.display_value(&schema), "");

    for key in [
        KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    ] {
        assert!(
            component.handle_key(&schema, &key),
            "expected {key:?} to be accepted"
        );
    }
    assert_eq!(component.display_value(&schema), "-1.2e-3");

    assert!(!component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)
    ));
    assert_eq!(component.display_value(&schema), "-1.2e-3");
}

#[test]
fn integer_input_rejects_decimal_and_exponent_chars() {
    let schema = integer_schema();
    let mut component = TextComponent::new(&schema, Arc::new(ComponentPalette::default()));

    assert!(component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)
    ));
    assert!(!component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE)
    ));
    assert!(!component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)
    ));
    assert!(!component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)
    ));
    assert_eq!(component.display_value(&schema), "1");
}

#[test]
fn text_component_supports_cursor_editing_delete_undo_redo_and_ctrl_w() {
    let schema = string_schema("alpha beta");
    let mut component = TextComponent::new(&schema, Arc::new(ComponentPalette::default()));

    assert!(component.handle_key(&schema, &KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)));
    assert!(component.handle_key(&schema, &KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)));
    assert!(component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT)
    ));
    assert_eq!(component.display_value(&schema), "alpha beXta");

    assert!(component.handle_key(&schema, &KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE)));
    assert_eq!(component.display_value(&schema), "alpha beXa");

    assert!(component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
    ));
    assert_eq!(component.display_value(&schema), "alpha bea");

    assert!(component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL)
    ));
    assert_eq!(component.display_value(&schema), "alpha beXa");
    assert!(component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)
    ));
    assert_eq!(component.display_value(&schema), "alpha bea");

    let mut component = TextComponent::new(&schema, Arc::new(ComponentPalette::default()));
    assert!(component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)
    ));
    assert_eq!(component.display_value(&schema), "alpha ");
    assert!(component.handle_key(
        &schema,
        &KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL)
    ));
    assert_eq!(component.display_value(&schema), "alpha beta");
}

#[test]
fn enum_and_bool_components_keep_left_right_toggle_behavior() {
    let palette = Arc::new(ComponentPalette::default());

    let bool_schema = bool_schema();
    let mut bool_component = BoolComponent::new(&bool_schema, Arc::clone(&palette));
    assert!(!bool_component.bool_value().expect("bool default"));
    assert!(bool_component.handle_key(
        &bool_schema,
        &KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)
    ));
    assert!(bool_component.bool_value().expect("bool toggled"));
    assert!(bool_component.handle_key(
        &bool_schema,
        &KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)
    ));
    assert!(!bool_component.bool_value().expect("bool toggled back"));

    let enum_schema = enum_schema();
    let FieldKind::Enum { labels, values } = &enum_schema.kind else {
        panic!("expected enum field kind");
    };
    let mut enum_component = EnumComponent::new(labels, values, &enum_schema, Arc::clone(&palette));
    assert_eq!(enum_component.display_value(&enum_schema), "alpha");
    assert!(enum_component.handle_key(
        &enum_schema,
        &KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)
    ));
    assert_eq!(enum_component.display_value(&enum_schema), "beta");
    assert!(enum_component.handle_key(
        &enum_schema,
        &KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)
    ));
    assert_eq!(enum_component.display_value(&enum_schema), "alpha");
}
