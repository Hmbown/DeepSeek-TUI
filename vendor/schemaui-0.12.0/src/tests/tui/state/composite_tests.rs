use std::sync::Arc;

use serde_json::json;

fn defs_schema_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("schemas")
        .join("test.defs.schema.json")
}

fn runtime_form_state(schema: &serde_json::Value) -> FormState {
    let ast = build_ui_ast(schema).expect("ui ast");
    let form_schema = form_schema_from_ui_ast(&ast);
    FormState::from_schema(&form_schema)
}

fn defs_2020_12_schema_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("schemas")
        .join("test.defs.2020-12.schema.json")
}

use crate::{
    tui::state::field::components::ComponentPalette,
    tui::{
        model::{CompositeField, CompositeMode, CompositeVariant, form_schema_from_ui_ast},
        state::{CompositeState, FormState},
    },
    ui_ast::build_ui_ast,
};

#[test]
fn seed_from_value_uses_enum_discriminator() {
    let variant_one = CompositeVariant {
        id: "variant_one".to_string(),
        title: "Variant 1".to_string(),
        description: None,
        schema: json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["alpha"]
                }
            }
        }),
        is_object: true,
    };
    let variant_two = CompositeVariant {
        id: "variant_two".to_string(),
        title: "Variant 2".to_string(),
        description: None,
        schema: json!({
            "type": "object",
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["beta"]
                }
            }
        }),
        is_object: true,
    };

    let field = CompositeField {
        mode: CompositeMode::OneOf,
        variants: vec![variant_one, variant_two],
    };

    let mut state = CompositeState::new(
        "/preferences",
        &field,
        Arc::new(ComponentPalette::default()),
    );
    state
        .seed_from_value(&json!({
            "kind": "beta"
        }))
        .expect("seed from value");

    assert_eq!(state.active_indices(), vec![1]);
    assert_eq!(state.summary(), "Variant: #2 Variant 2");
}

#[test]
fn non_object_variant_round_trips_through_wrapper() {
    let variant = CompositeVariant {
        id: "list".to_string(),
        title: "String List".to_string(),
        description: None,
        schema: json!({
            "type": "array",
            "items": {"type": "string"}
        }),
        is_object: false,
    };
    let field = CompositeField {
        mode: CompositeMode::OneOf,
        variants: vec![variant],
    };
    let palette = Arc::new(ComponentPalette::default());
    let mut state = CompositeState::new("/options", &field, Arc::clone(&palette));
    state
        .seed_from_value(&json!(["a", "b"]))
        .expect("seed array value");
    assert_eq!(
        state.build_value(true).expect("build value"),
        Some(json!(["a", "b"]))
    );

    let mut session = state
        .take_editor_session("/options", 0)
        .expect("editor session");
    assert!(
        session
            .schema
            .get("properties")
            .and_then(|props| props.get("__value"))
            .is_some(),
        "overlay schema should expose wrapped value field"
    );
    session
        .form_state
        .seed_from_value(&json!({"__value": ["z"]}));
    state.restore_editor_session(session);

    assert_eq!(
        state.build_value(true).expect("build updated value"),
        Some(json!(["z"]))
    );
}

#[test]
fn object_variant_overlay_keeps_root_dollar_defs_for_valid_2020_12_refs() {
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(defs_2020_12_schema_path()).expect("2020-12 defs schema readable"),
    )
    .expect("2020-12 defs schema json");

    let mut state = runtime_form_state(&schema);
    let field = state.field_mut_by_pointer("/blah").expect("blah field");
    let session = field
        .open_composite_editor(0)
        .expect("valid 2020-12 $defs schema should build variant session");

    assert_eq!(session.schema.get("$schema"), schema.get("$schema"));
    assert_eq!(session.schema.get("$defs"), schema.get("$defs"));
    assert!(
        session.form_state.field_by_pointer("/Bar").is_some(),
        "variant form should resolve $defs-backed referenced Bar field"
    );
    assert!(session.form_state.field_by_pointer("/Id").is_some());
}

#[test]
fn draft_07_schema_with_dollar_defs_builds_overlay_session() {
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(defs_schema_path()).expect("defs schema readable"),
    )
    .expect("defs schema json");

    let mut state = runtime_form_state(&schema);
    let field = state.field_mut_by_pointer("/blah").expect("blah field");
    let session = field
        .open_composite_editor(0)
        .expect("draft-07 + $defs schema should build variant session");

    assert_eq!(session.schema.get("$schema"), schema.get("$schema"));
    assert_eq!(session.schema.get("$defs"), schema.get("$defs"));
    assert!(
        session.form_state.field_by_pointer("/Bar").is_some(),
        "variant form should resolve $defs-backed referenced Bar field"
    );
    assert!(session.form_state.field_by_pointer("/Id").is_some());
}
