use super::*;
use crate::{
    tui::app::options::UiOptions,
    tui::app::runtime::overlay::CompositeOverlayTarget,
    tui::model::{FieldKind, FieldSchema},
    tui::state::{FieldState, FormState, SectionState},
};
use jsonschema::validator_for;
use serde_json::json;
use std::collections::HashMap;

fn scalar_array_field_state() -> FieldState {
    let schema = FieldSchema {
        name: "allowed_methods".to_string(),
        path: vec!["allowed_methods".to_string()],
        pointer: "/allowed_methods".to_string(),
        title: "Allowed Methods".to_string(),
        description: None,
        kind: FieldKind::Array(Box::new(FieldKind::String)),
        required: false,
        default: Some(json!(["GET"])),
        metadata: HashMap::new(),
    };
    FieldState::from_schema(schema)
}

fn build_app_with_scalar_array() -> App {
    let section = SectionState {
        id: "section".to_string(),
        title: "Section".to_string(),
        description: None,
        path: vec!["app".to_string()],
        depth: 0,
        fields: vec![scalar_array_field_state()],
        scroll_offset: 0,
    };
    let form_state = FormState::from_sections("app", "App", None, vec![section]);
    let validator = validator_for(&json!({"type": "object"})).expect("validator");
    App::new(form_state, validator, UiOptions::default())
}

#[test]
fn ctrl_e_opens_scalar_array_overlay() {
    let mut app = build_app_with_scalar_array();
    app.try_open_composite_editor();
    assert!(
        matches!(
            app.active_overlay().map(|overlay| overlay.target()),
            Some(CompositeOverlayTarget::ArrayEntry { .. })
        ),
        "scalar arrays should open overlay via Ctrl+E"
    );
    assert_eq!(app.overlay_depth(), 1);
}
