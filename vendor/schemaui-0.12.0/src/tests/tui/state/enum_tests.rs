use crate::{
    tui::{
        model::{FieldKind, form_schema_from_ui_ast},
        state::FormState,
    },
    ui_ast::build_ui_ast,
};
use jsonschema::validator_for;
use serde_json::json;

fn enum_integer_schema() -> serde_json::Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "EnumInteger": {
                "description": "Select one of a few integers",
                "enum": [2, 5, 42]
            }
        },
        "additionalProperties": false
    })
}

#[test]
fn enum_integer_selection_preserves_numeric_value_through_runtime_pipeline() {
    let schema = enum_integer_schema();
    let ast = build_ui_ast(&schema).expect("ui ast");
    let form_schema = form_schema_from_ui_ast(&ast);
    let mut state = FormState::from_schema(&form_schema);

    let field = state
        .field_mut_by_pointer("/EnumInteger")
        .expect("enum field");
    match &field.schema.kind {
        FieldKind::Enum { labels, values } => {
            assert_eq!(
                labels,
                &vec!["2".to_string(), "5".to_string(), "42".to_string()]
            );
            assert_eq!(values, &vec![json!(2), json!(5), json!(42)]);
        }
        other => panic!("expected enum field, got {:?}", other),
    }
    field.set_enum_selected(1);

    let built = state.try_build_value().expect("form value");
    assert_eq!(built, json!({"EnumInteger": 5}));

    let validator = validator_for(&schema).expect("validator");
    assert!(
        validator.validate(&built).is_ok(),
        "numeric enum should validate"
    );
}

#[test]
fn enum_integer_seed_uses_raw_value_matching() {
    let schema = enum_integer_schema();
    let ast = build_ui_ast(&schema).expect("ui ast");
    let form_schema = form_schema_from_ui_ast(&ast);
    let mut state = FormState::from_schema(&form_schema);

    state.seed_from_value(&json!({"EnumInteger": 42}));

    let field = state.field_by_pointer("/EnumInteger").expect("enum field");
    let enum_state = field.enum_state().expect("enum state");
    assert_eq!(enum_state.selected, 2);
}
