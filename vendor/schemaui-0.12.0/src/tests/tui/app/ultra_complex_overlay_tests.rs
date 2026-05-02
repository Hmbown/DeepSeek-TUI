use std::{fs, path::PathBuf};

use crate::{
    tui::{
        app::{App, UiOptions},
        model::{FieldKind, form_schema_from_ui_ast},
        state::FormState,
    },
    ui_ast::build_ui_ast,
};
use jsonschema::validator_for;
use serde_json::{Value, json};

const TREE_NODE_SCHEMA_PTR: &str = "/definitions/treeNode";
const MATRIX_SCHEMA_PTR: &str = "/properties/level1/properties/level2/properties/config/oneOf/1/properties/settings/properties/level3/properties/level4/properties/matrix";
const RECURSIVE_CHILDREN_PTR: &str = "/recursiveTree/children";
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

fn build_runtime_app_from_example_fragment(pointer: &str, field_name: &str) -> App {
    let root_schema = load_ultra_complex_schema();
    let fragment = root_schema
        .pointer(pointer)
        .cloned()
        .unwrap_or_else(|| panic!("schema fragment {pointer} not found"));
    let schema = wrap_property_schema(&root_schema, field_name, fragment);
    let ast = build_ui_ast(&schema).expect("ui ast");
    let form_schema = form_schema_from_ui_ast(&ast);
    let form_state = FormState::from_schema(&form_schema);
    let validator = validator_for(&schema).expect("validator");
    App::new(form_state, validator, UiOptions::default())
}

fn focus_field(state: &mut FormState, pointer: &str) {
    for (root_idx, root) in state.roots.iter().enumerate() {
        for (section_idx, section) in root.sections.iter().enumerate() {
            if let Some(field_idx) = section.fields.iter().position(|field| {
                field.schema.pointer == pointer || field.schema.pointer.ends_with(pointer)
            }) {
                state.set_root_index(root_idx);
                state.set_section_index(section_idx);
                state.set_field_index(field_idx);
                return;
            }
        }
    }
    panic!("field {pointer} not found");
}

#[test]
fn ultra_complex_recursive_tree_children_overlay_supports_nested_depth() {
    let mut app = build_runtime_app_from_example_fragment(TREE_NODE_SCHEMA_PTR, "recursiveTree");

    {
        let form_state = app.form_state_mut_for_test();
        focus_field(form_state, RECURSIVE_CHILDREN_PTR);
    }
    app.open_overlay_for_test();
    assert_eq!(
        app.overlay_depth_for_test(),
        1,
        "recursiveTree.children should open its first node editor"
    );

    {
        let overlay = app
            .active_overlay_form_state_for_test()
            .expect("first overlay form");
        assert!(overlay.field_by_pointer("/name").is_some());
        assert!(overlay.field_by_pointer("/children").is_some());
        overlay
            .field_mut_by_pointer("/name")
            .expect("name field")
            .seed_value(&json!("child-1"));
        focus_field(overlay, "/children");
    }

    app.open_overlay_for_test();
    assert_eq!(
        app.overlay_depth_for_test(),
        2,
        "recursive child nodes should open nested overlays instead of staying inert"
    );

    {
        let overlay = app
            .active_overlay_form_state_for_test()
            .expect("nested overlay form");
        assert!(overlay.field_by_pointer("/name").is_some());
        overlay
            .field_mut_by_pointer("/name")
            .expect("grandchild name")
            .seed_value(&json!("grandchild-1"));
    }

    assert!(
        app.save_overlay_stack_to_root(),
        "nested recursive overlays should commit back to the root form"
    );

    let current = app
        .form_state_mut_for_test()
        .field_by_pointer(RECURSIVE_CHILDREN_PTR)
        .expect("recursive children field")
        .current_value()
        .expect("recursive children value");
    assert_eq!(
        current,
        Some(json!([{
            "name": "child-1",
            "children": [{
                "name": "grandchild-1"
            }]
        }]))
    );
}

#[test]
fn ultra_complex_matrix_overlay_opens_for_rows_and_cells() {
    let mut app = build_runtime_app_from_example_fragment(MATRIX_SCHEMA_PTR, "matrix");

    {
        let form_state = app.form_state_mut_for_test();
        focus_field(form_state, MATRIX_PTR);
    }
    app.open_overlay_for_test();
    assert_eq!(
        app.overlay_depth_for_test(),
        1,
        "matrix should open a row overlay instead of ignoring Ctrl+E/Enter"
    );

    {
        let overlay = app
            .active_overlay_form_state_for_test()
            .expect("matrix row overlay");
        assert!(
            overlay.field_by_pointer("/__value").is_some(),
            "row overlay should expose the wrapped inner array field"
        );
        focus_field(overlay, "/__value");
    }

    app.open_overlay_for_test();
    assert_eq!(
        app.overlay_depth_for_test(),
        2,
        "matrix row cells should open a second overlay instead of staying inert"
    );

    let overlay = app
        .active_overlay_form_state_for_test()
        .expect("matrix cell overlay");
    let field = overlay
        .field_by_pointer("/__value")
        .expect("wrapped null cell field");
    match &field.schema.kind {
        FieldKind::Enum { values, .. } => {
            assert_eq!(values, &vec![Value::Null]);
        }
        other => panic!("null matrix variant should be a fixed null enum, got {other:?}"),
    }
    assert_eq!(
        field.current_value().expect("null cell current value"),
        Some(Value::Null)
    );
}
