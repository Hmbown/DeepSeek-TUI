use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::tui::model::{CompositeField, CompositeMode, CompositeVariant, FieldKind};
use crate::tui::state::array::ScalarArrayState;
use crate::tui::state::composite::CompositeListState;
use crate::tui::state::field::components::ComponentPalette;
use crate::tui::state::key_value::KeyValueState;

fn scalar_array(values: &[Value]) -> ScalarArrayState {
    let kind = FieldKind::String;
    let mut state = ScalarArrayState::new(
        "/tags",
        "Tags".to_string(),
        None,
        &kind,
        None,
        Arc::new(ComponentPalette::default()),
    );
    state.seed_entries_from_array(values);
    state
}

fn composite_list(defaults: &[Value]) -> CompositeListState {
    let variants = vec![CompositeVariant {
        id: "variant_0".to_string(),
        title: "Entry".to_string(),
        description: None,
        schema: json!({"type": "string"}),
        is_object: false,
    }];
    let template = CompositeField {
        mode: CompositeMode::OneOf,
        variants,
    };
    CompositeListState::new(
        "/items",
        &template,
        Some(&Value::Array(defaults.to_vec())),
        Arc::new(ComponentPalette::default()),
    )
}

fn key_value_with_entries(pairs: &[(&str, &str)]) -> KeyValueState {
    use crate::tui::model::KeyValueField;

    let value_kind = FieldKind::String;
    let kv_field = KeyValueField {
        key_title: "Key".to_string(),
        key_description: None,
        key_default: None,
        key_schema: json!({"type": "string"}),
        value_title: "Value".to_string(),
        value_description: None,
        value_default: None,
        value_schema: json!({"type": "string"}),
        value_kind: Box::new(value_kind),
        entry_schema: json!({"type": "object"}),
    };

    let mut state = KeyValueState::new(
        "/headers",
        &kv_field,
        None,
        Arc::new(ComponentPalette::default()),
    );

    // Seed entries manually using seed_entries_from_object so we exercise the
    // same code path used for defaults.
    let mut map = Map::new();
    for (k, v) in pairs {
        map.insert((*k).to_string(), Value::String((*v).to_string()));
    }
    state.seed_entries_from_object(&map);
    state
}

#[test]
fn scalar_array_remove_selected_can_delete_all_entries_and_updates_selection() {
    let mut state = scalar_array(&[Value::String("a".into()), Value::String("b".into())]);
    assert_eq!(state.len(), 2);
    assert_eq!(state.selected_index(), Some(0));

    // Remove first entry; list should shrink and selection should stay valid.
    assert!(state.remove_selected());
    assert_eq!(state.len(), 1);
    assert_eq!(state.selected_index(), Some(0));

    // Remove last remaining entry; list becomes empty and selected_index is None.
    assert!(state.remove_selected());
    assert_eq!(state.len(), 0);
    assert_eq!(state.selected_index(), None);

    // Further removes are no-ops.
    assert!(!state.remove_selected());
}

#[test]
fn composite_list_remove_selected_can_delete_all_entries_and_updates_selection() {
    let mut state = composite_list(&[Value::String("a".into()), Value::String("b".into())]);
    assert_eq!(state.len(), 2);
    assert_eq!(state.selected_index(), Some(0));

    // Remove first entry.
    assert!(state.remove_selected());
    assert_eq!(state.len(), 1);
    assert_eq!(state.selected_index(), Some(0));

    // Remove last remaining entry.
    assert!(state.remove_selected());
    assert_eq!(state.len(), 0);
    assert_eq!(state.selected_index(), None);

    // Further removes are no-ops.
    assert!(!state.remove_selected());
}

#[test]
fn key_value_remove_selected_can_delete_all_entries_and_updates_selection() {
    let mut state = key_value_with_entries(&[("a", "1"), ("b", "2")]);
    assert_eq!(state.len(), 2);
    assert_eq!(state.selected_index(), Some(0));

    // Remove first entry.
    assert!(state.remove_selected());
    assert_eq!(state.len(), 1);
    assert_eq!(state.selected_index(), Some(0));

    // Remove last remaining entry.
    assert!(state.remove_selected());
    assert_eq!(state.len(), 0);
    assert_eq!(state.selected_index(), None);

    // Further removes are no-ops.
    assert!(!state.remove_selected());
}
