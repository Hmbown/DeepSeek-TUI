use std::{fs, path::PathBuf};

use jsonschema::validator_for;
use serde_json::{Value, json};

use crate::{
    tui::{
        model::{FieldKind, form_schema_from_ui_ast},
        state::FormState,
    },
    ui_ast::build_ui_ast,
};

const ULTRA_DEEP_SCHEMA_PTR: &str = "/properties/level1/properties/level2/properties/config/oneOf/1/properties/settings/properties/level3/properties/level4/properties/level5/properties/ultraDeep";
const MATRIX_SCHEMA_PTR: &str = "/properties/level1/properties/level2/properties/config/oneOf/1/properties/settings/properties/level3/properties/level4/properties/matrix";
const ULTRA_DEEP_PTR: &str = "/ultraDeep";
const MATRIX_PTR: &str = "/matrix";

fn ultra_complex_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("ultra-complex.schema.json")
}

fn load_ultra_complex_schema() -> Value {
    let contents = fs::read_to_string(ultra_complex_schema_path()).expect("ultra schema readable");
    serde_json::from_str(&contents).expect("ultra schema json")
}

fn wrap_property_schema(root_schema: &Value, field_name: &str, property_schema: Value) -> Value {
    let mut wrapped = json!({
        "type": "object",
        "properties": {
            field_name: property_schema
        }
    });
    if let Some(definitions) = root_schema
        .get("definitions")
        .cloned()
        .or_else(|| root_schema.get("$defs").cloned())
        && let Some(object) = wrapped.as_object_mut()
    {
        object.insert("definitions".to_string(), definitions.clone());
    }
    wrapped
}

fn wrapped_example_fragment(pointer: &str, field_name: &str) -> Value {
    let root_schema = load_ultra_complex_schema();
    let fragment = root_schema
        .pointer(pointer)
        .cloned()
        .unwrap_or_else(|| panic!("schema fragment {pointer} not found"));
    wrap_property_schema(&root_schema, field_name, fragment)
}

fn runtime_form_state(schema: &Value) -> FormState {
    let ast = build_ui_ast(schema).expect("ui ast");
    let form_schema = form_schema_from_ui_ast(&ast);
    FormState::from_schema(&form_schema)
}

#[test]
fn ultra_complex_const_discriminants_autofill_without_manual_kind_input() {
    let schema = wrapped_example_fragment(ULTRA_DEEP_SCHEMA_PTR, "ultraDeep");
    let mut state = runtime_form_state(&schema);

    let mut entry_ctx = {
        let field = state
            .field_mut_by_pointer(ULTRA_DEEP_PTR)
            .expect("ultraDeep field");
        assert!(
            field.composite_list_add_entry(),
            "should create ultraDeep entry"
        );
        field
            .open_composite_list_editor()
            .expect("open ultraDeep entry editor")
    };

    {
        let variant_field = entry_ctx
            .session
            .form_state
            .field_mut_by_pointer("/variant")
            .expect("variant field");
        variant_field.apply_composite_selection(1, None);

        let beta_ctx = variant_field
            .open_composite_editor(1)
            .expect("open beta variant editor");
        let built_beta = beta_ctx
            .form_state
            .try_build_value()
            .expect("beta variant value");
        let expected_beta = json!({
            "kind": "beta",
            "betaValue": 0.0
        });
        assert_eq!(built_beta, expected_beta);

        let validator = validator_for(&beta_ctx.schema).expect("beta validator");
        assert!(
            validator.validate(&built_beta).is_ok(),
            "const discriminant should make the variant valid without manual kind input"
        );

        variant_field.close_composite_editor(beta_ctx, true);
    }

    let built_entry = entry_ctx
        .session
        .form_state
        .try_build_value()
        .expect("ultraDeep entry value");
    let expected_entry = json!({
        "id": 0,
        "variant": {
            "kind": "beta",
            "betaValue": 0.0
        }
    });
    assert_eq!(built_entry, expected_entry);

    {
        let field = state
            .field_mut_by_pointer(ULTRA_DEEP_PTR)
            .expect("ultraDeep field");
        field.close_composite_list_editor(entry_ctx.entry_index, entry_ctx.session, true);
    }

    let current = state
        .field_by_pointer(ULTRA_DEEP_PTR)
        .expect("ultraDeep field")
        .current_value()
        .expect("current ultraDeep value");
    assert_eq!(current, Some(json!([expected_entry])));
}

#[test]
fn ultra_complex_matrix_rows_and_cells_roundtrip_through_nested_composite_lists() {
    let schema = wrapped_example_fragment(MATRIX_SCHEMA_PTR, "matrix");
    let mut state = runtime_form_state(&schema);

    let mut row_ctx = {
        let field = state
            .field_mut_by_pointer(MATRIX_PTR)
            .expect("matrix field");
        assert!(matches!(field.schema.kind, FieldKind::Array(_)));
        assert!(
            field.composite_list_add_entry(),
            "should create a matrix row"
        );
        field
            .open_composite_list_editor()
            .expect("open matrix row editor")
    };

    {
        let row_field = row_ctx
            .session
            .form_state
            .field_mut_by_pointer("/__value")
            .expect("wrapped row array field");
        match &row_field.schema.kind {
            FieldKind::Array(inner) => {
                assert!(
                    matches!(inner.as_ref(), FieldKind::Composite(_)),
                    "matrix rows should edit cells through composite overlays, got {inner:?}"
                );
            }
            other => panic!("expected wrapped row array field, got {other:?}"),
        }

        assert!(
            row_field.composite_list_add_entry_with_variant(4),
            "should create an object cell entry"
        );
        let mut cell_ctx = row_field
            .open_composite_list_editor()
            .expect("open matrix cell editor");

        {
            let cell_form = &mut cell_ctx.session.form_state;
            cell_form
                .field_mut_by_pointer("/x")
                .expect("x field")
                .seed_value(&json!(1.5));
            cell_form
                .field_mut_by_pointer("/y")
                .expect("y field")
                .seed_value(&json!(2.5));

            let data_field = cell_form.field_mut_by_pointer("/data").expect("data field");
            data_field.apply_composite_selection(1, None);
            let mut data_ctx = data_field
                .open_composite_editor(1)
                .expect("open vector data editor");
            data_ctx
                .form_state
                .field_mut_by_pointer("/magnitude")
                .expect("magnitude field")
                .seed_value(&json!(3.0));
            data_ctx
                .form_state
                .field_mut_by_pointer("/direction")
                .expect("direction field")
                .seed_value(&json!(90.0));

            let built_data = data_ctx
                .form_state
                .try_build_value()
                .expect("vector data value");
            let expected_data = json!({
                "type": "vector",
                "magnitude": 3.0,
                "direction": 90.0
            });
            assert_eq!(built_data, expected_data);

            let validator = validator_for(&data_ctx.schema).expect("data validator");
            assert!(validator.validate(&built_data).is_ok());

            data_field.close_composite_editor(data_ctx, true);
        }

        let expected_cell = json!({
            "x": 1.5,
            "y": 2.5,
            "data": {
                "type": "vector",
                "magnitude": 3.0,
                "direction": 90.0
            }
        });
        let built_cell = cell_ctx
            .session
            .form_state
            .try_build_value()
            .expect("matrix cell value");
        assert_eq!(built_cell, expected_cell);

        row_field.close_composite_list_editor(cell_ctx.entry_index, cell_ctx.session, true);
    }

    let expected_row = json!({
        "__value": [{
            "x": 1.5,
            "y": 2.5,
            "data": {
                "type": "vector",
                "magnitude": 3.0,
                "direction": 90.0
            }
        }]
    });
    let built_row = row_ctx
        .session
        .form_state
        .try_build_value()
        .expect("matrix row value");
    assert_eq!(built_row, expected_row);

    {
        let field = state
            .field_mut_by_pointer(MATRIX_PTR)
            .expect("matrix field");
        field.close_composite_list_editor(row_ctx.entry_index, row_ctx.session, true);
    }

    let current = state
        .field_by_pointer(MATRIX_PTR)
        .expect("matrix field")
        .current_value()
        .expect("matrix current value");
    assert_eq!(
        current,
        Some(json!([[{
            "x": 1.5,
            "y": 2.5,
            "data": {
                "type": "vector",
                "magnitude": 3.0,
                "direction": 90.0
            }
        }]]))
    );
}
