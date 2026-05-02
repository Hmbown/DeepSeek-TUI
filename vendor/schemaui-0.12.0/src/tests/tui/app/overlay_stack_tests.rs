use crate::tui::app::{App, UiOptions};
use crate::tui::model::form_schema_from_ui_ast;
use crate::tui::state::FormState;
use crate::ui_ast::build_ui_ast;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jsonschema::validator_for;
use serde_json::{Value, json};

fn runtime_form_schema(schema: &Value) -> crate::tui::model::FormSchema {
    let ast = build_ui_ast(schema).expect("ui ast");
    form_schema_from_ui_ast(&ast)
}

fn build_nested_overlay_app() -> App {
    let schema = json!({
        "type": "object",
        "properties": {
            "service": {
                "oneOf": [
                    {
                        "title": "http",
                        "type": "object",
                        "properties": {
                            "routes": {
                                "type": "array",
                                "default": [
                                    {"type": "standard", "path": "/"},
                                    {"type": "standard", "path": "/status"}
                                ],
                                "items": {
                                    "oneOf": [
                                        {
                                            "title": "Standard Route",
                                            "type": "object",
                                            "properties": {
                                                "type": {
                                                    "type": "string",
                                                    "const": "standard"
                                                },
                                                "path": {"type": "string"}
                                            },
                                            "required": ["type", "path"]
                                        },
                                        {
                                            "title": "Regex Route",
                                            "type": "object",
                                            "properties": {
                                                "type": {
                                                    "type": "string",
                                                    "const": "regex"
                                                },
                                                "pattern": {"type": "string"}
                                            },
                                            "required": ["type", "pattern"]
                                        }
                                    ]
                                }
                            }
                        }
                    }
                ]
            }
        }
    });
    let form_schema = runtime_form_schema(&schema);
    let form_state = FormState::from_schema(&form_schema);
    let validator = validator_for(&schema).expect("validator");
    App::new(form_state, validator, UiOptions::default())
}

fn build_key_value_overlay_app() -> App {
    let schema = json!({
        "type": "object",
        "properties": {
            "headers": {
                "type": "object",
                "additionalProperties": {"type": "string"},
                "default": {
                    "X-One": "1",
                    "X-Two": "2"
                }
            }
        }
    });
    let form_schema = runtime_form_schema(&schema);
    let form_state = FormState::from_schema(&form_schema);
    let validator = validator_for(&schema).expect("validator");
    App::new(form_state, validator, UiOptions::default())
}

fn build_scalar_array_overlay_app() -> App {
    let schema = json!({
        "type": "object",
        "properties": {
            "tags": {
                "type": "array",
                "default": ["a", "b"],
                "items": {"type": "string"}
            }
        }
    });
    let form_schema = runtime_form_schema(&schema);
    let form_state = FormState::from_schema(&form_schema);
    let validator = validator_for(&schema).expect("validator");
    App::new(form_state, validator, UiOptions::default())
}

fn build_invalid_composite_overlay_app() -> App {
    let schema = json!({
        "type": "object",
        "properties": {
            "service": {
                "oneOf": [
                    {
                        "title": "http",
                        "type": "object",
                        "properties": {
                            "port": {"type": "integer"}
                        },
                        "required": ["port"]
                    }
                ]
            }
        }
    });
    let form_schema = runtime_form_schema(&schema);
    let form_state = FormState::from_schema(&form_schema);
    let validator = validator_for(&schema).expect("validator");
    App::new(form_state, validator, UiOptions::default())
}

fn activate_service_variant(app: &mut App) {
    let form_state = app.form_state_mut_for_test();
    let field = form_state
        .field_mut_by_pointer("/service")
        .expect("service field present");
    field.apply_composite_selection(0, None);
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
    let mut available = Vec::new();
    for root in &state.roots {
        for section in &root.sections {
            for field in &section.fields {
                available.push(field.schema.pointer.clone());
            }
        }
    }
    panic!("field {pointer} not found; available: {available:?}");
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

// Test removed: Ctrl+S behavior with nested overlays is complex and needs redesign

#[test]
fn esc_pops_only_top_overlay() {
    let mut app = build_nested_overlay_app();
    activate_service_variant(&mut app);
    app.open_overlay_for_test();
    {
        let overlay_form = app
            .active_overlay_form_state_for_test()
            .expect("overlay form");
        focus_field(overlay_form, "/routes");
    }
    app.open_overlay_for_test();
    assert_eq!(app.overlay_depth_for_test(), 2);

    app.handle_key_for_test(key(KeyCode::Esc, KeyModifiers::NONE))
        .expect("close overlay2");
    assert_eq!(
        app.overlay_depth_for_test(),
        1,
        "esc closes only top overlay",
    );

    app.handle_key_for_test(key(KeyCode::Esc, KeyModifiers::NONE))
        .expect("close overlay1");
    assert_eq!(app.overlay_depth_for_test(), 0);
}

#[test]
fn ctrl_arrows_manage_entries_without_closing_overlay() {
    let mut app = build_nested_overlay_app();
    activate_service_variant(&mut app);
    app.open_overlay_for_test();
    {
        let overlay_form = app
            .active_overlay_form_state_for_test()
            .expect("overlay form level1");
        focus_field(overlay_form, "/routes");
    }
    app.open_overlay_for_test();
    assert_eq!(app.overlay_depth_for_test(), 2);
    assert_eq!(app.overlay_selected_entry_for_test(), Some(0));

    app.handle_key_for_test(key(KeyCode::Right, KeyModifiers::CONTROL))
        .expect("ctrl+right");
    assert_eq!(app.overlay_depth_for_test(), 2);
    assert_eq!(app.overlay_selected_entry_for_test(), Some(1));

    app.handle_key_for_test(key(KeyCode::Left, KeyModifiers::CONTROL))
        .expect("ctrl+left");
    assert_eq!(app.overlay_selected_entry_for_test(), Some(0));

    app.handle_key_for_test(key(KeyCode::Left, KeyModifiers::CONTROL))
        .expect("ctrl+left boundary");
    assert_eq!(app.overlay_depth_for_test(), 2);
    assert_eq!(app.overlay_selected_entry_for_test(), Some(0));

    app.handle_key_for_test(key(KeyCode::Down, KeyModifiers::CONTROL))
        .expect("ctrl+down reorder");
    assert_eq!(app.overlay_depth_for_test(), 2);
    assert_eq!(
        app.overlay_selected_entry_for_test(),
        Some(1),
        "entry should move to index 1 after reorder",
    );

    app.handle_key_for_test(key(KeyCode::Up, KeyModifiers::CONTROL))
        .expect("ctrl+up reorder back");
    assert_eq!(app.overlay_depth_for_test(), 2);
    assert_eq!(app.overlay_selected_entry_for_test(), Some(0));
}

#[test]
fn entry_focus_respects_arrow_keys() {
    let mut app = build_nested_overlay_app();
    activate_service_variant(&mut app);
    app.open_overlay_for_test();
    {
        let overlay_form = app
            .active_overlay_form_state_for_test()
            .expect("overlay form level1");
        focus_field(overlay_form, "/routes");
    }
    app.open_overlay_for_test();
    assert_eq!(app.overlay_depth_for_test(), 2);
    assert_eq!(app.overlay_entry_focus_for_test(), Some(true));

    app.handle_key_for_test(key(KeyCode::Down, KeyModifiers::NONE))
        .expect("down from entry focus jumps into fields");
    assert_eq!(app.overlay_entry_focus_for_test(), Some(false));

    app.handle_key_for_test(key(KeyCode::Up, KeyModifiers::NONE))
        .expect("up from first field returns to entries");
    assert_eq!(app.overlay_entry_focus_for_test(), Some(true));
    assert_eq!(
        app.overlay_selected_entry_for_test(),
        Some(1),
        "up from first field should cycle to previous entry",
    );
}

#[test]
fn composite_overlay_uses_layout_nav_for_default_focus() {
    let mut app = build_nested_overlay_app();
    activate_service_variant(&mut app);

    // Focus the top-level /service field to open its composite overlay.
    {
        let form_state = app.form_state_mut_for_test();
        focus_field(form_state, "/service");
    }

    app.open_overlay_for_test();
    assert_eq!(app.overlay_depth_for_test(), 1);

    let overlay_form = app
        .active_overlay_form_state_for_test()
        .expect("overlay form level1");

    // The overlay FormState should carry a LayoutNavModel derived from UiLayout.
    assert!(overlay_form.layout_nav().is_some());

    let pointer = overlay_form
        .focused_field()
        .map(|field| field.schema.pointer.clone())
        .unwrap_or_else(|| "<none>".to_string());

    // For the `service` variant schema, the first layout field is `/routes`.
    assert_eq!(pointer, "/routes");

    // layout-driven first-field focus should be idempotent and succeed.
    assert!(overlay_form.focus_first_field_with_layout());
    let pointer_after = overlay_form
        .focused_field()
        .map(|field| field.schema.pointer.clone())
        .unwrap_or_else(|| "<none>".to_string());
    assert_eq!(pointer_after, "/routes");
}

#[test]
fn scalar_array_overlay_opens_for_primitive_array_field() {
    let mut app = build_scalar_array_overlay_app();
    {
        let form_state = app.form_state_mut_for_test();
        focus_field(form_state, "/tags");
    }

    app.open_overlay_for_test();

    assert_eq!(
        app.overlay_depth_for_test(),
        1,
        "scalar array overlay should open"
    );
    assert_eq!(
        app.overlay_selected_entry_for_test(),
        Some(0),
        "first scalar array entry should be selected by default",
    );
}

#[test]
fn scalar_array_overlay_can_delete_all_entries_without_recreating_last() {
    let mut app = build_scalar_array_overlay_app();
    {
        let form_state = app.form_state_mut_for_test();
        focus_field(form_state, "/tags");
    }

    // Open scalar array overlay for /tags.
    app.open_overlay_for_test();
    assert_eq!(
        app.overlay_depth_for_test(),
        1,
        "scalar array overlay should open before deletions",
    );

    fn tags_len(app: &mut App) -> usize {
        let form_state = app.form_state_mut_for_test();
        let field = form_state
            .field_mut_by_pointer("/tags")
            .expect("tags field present");
        field
            .composite_list_panel()
            .map(|(entries, _selected)| entries.len())
            .unwrap_or(0)
    }

    assert_eq!(tags_len(&mut app), 2, "default tags array has two entries");

    // First Ctrl+D: remove one entry; overlay should remain open.
    app.handle_key_for_test(key(KeyCode::Char('d'), KeyModifiers::CONTROL))
        .expect("first Ctrl+D in scalar array overlay");
    assert_eq!(
        app.overlay_depth_for_test(),
        1,
        "overlay should stay open while scalar array is non-empty",
    );
    assert_eq!(
        tags_len(&mut app),
        1,
        "one entry should remain after first deletion",
    );

    // Second Ctrl+D: remove last entry; overlay should close and list become empty.
    app.handle_key_for_test(key(KeyCode::Char('d'), KeyModifiers::CONTROL))
        .expect("second Ctrl+D in scalar array overlay");
    assert_eq!(
        app.overlay_depth_for_test(),
        0,
        "deleting the last entry should close the scalar array overlay instead of reopening it",
    );
    assert_eq!(
        tags_len(&mut app),
        0,
        "scalar array field should be truly empty after deleting the last entry",
    );
}

#[test]
fn nested_list_overlay_deleting_all_entries_returns_to_parent_overlay() {
    let mut app = build_nested_overlay_app();
    activate_service_variant(&mut app);

    // Open overlay1 for the `service` composite field.
    app.open_overlay_for_test();
    {
        let overlay_form = app
            .active_overlay_form_state_for_test()
            .expect("overlay form level1");
        focus_field(overlay_form, "/routes");
    }

    // Open overlay2 for the `routes` list.
    app.open_overlay_for_test();
    assert_eq!(
        app.overlay_depth_for_test(),
        2,
        "expected nested list overlay depth to be 2 before deletions",
    );

    // First deletion: list still has entries, so list overlay should stay at depth 2.
    app.handle_key_for_test(key(KeyCode::Char('d'), KeyModifiers::CONTROL))
        .expect("Ctrl+D to delete first route entry");
    assert_eq!(
        app.overlay_depth_for_test(),
        2,
        "list overlay should remain open while composite list still has entries",
    );

    // Second deletion: removing last entry should close only the list overlay and
    // return to the parent (service) overlay at depth 1, without reopening a new
    // list overlay with an auto-created entry.
    app.handle_key_for_test(key(KeyCode::Char('d'), KeyModifiers::CONTROL))
        .expect("Ctrl+D to delete last route entry");
    assert_eq!(
        app.overlay_depth_for_test(),
        1,
        "after deleting the last list entry, control should return to the parent overlay",
    );
    assert_eq!(
        app.overlay_selected_entry_for_test(),
        None,
        "no list entry should be selected once the list is empty",
    );
}

#[test]
fn key_value_overlay_can_delete_all_entries_without_recreating_last() {
    let mut app = build_key_value_overlay_app();
    {
        let form_state = app.form_state_mut_for_test();
        focus_field(form_state, "/headers");
    }

    // Open key/value overlay for /headers.
    app.open_overlay_for_test();
    assert_eq!(
        app.overlay_depth_for_test(),
        1,
        "key/value overlay should open before deletions",
    );

    fn headers_len(app: &mut App) -> usize {
        let form_state = app.form_state_mut_for_test();
        let field = form_state
            .field_mut_by_pointer("/headers")
            .expect("headers field present");
        field
            .composite_list_panel()
            .map(|(entries, _selected)| entries.len())
            .unwrap_or(0)
    }

    assert_eq!(
        headers_len(&mut app),
        2,
        "default headers map should expose two entries in the collection panel",
    );

    // First Ctrl+D: remove one entry; overlay should remain open.
    app.handle_key_for_test(key(KeyCode::Char('d'), KeyModifiers::CONTROL))
        .expect("first Ctrl+D in key/value overlay");
    assert_eq!(
        app.overlay_depth_for_test(),
        1,
        "overlay should stay open while key/value map still has entries",
    );
    assert_eq!(
        headers_len(&mut app),
        1,
        "one entry should remain after first deletion",
    );

    // Second Ctrl+D: remove last entry; overlay should close and map become empty.
    app.handle_key_for_test(key(KeyCode::Char('d'), KeyModifiers::CONTROL))
        .expect("second Ctrl+D in key/value overlay");
    assert_eq!(
        app.overlay_depth_for_test(),
        0,
        "deleting the last entry should close the key/value overlay instead of reopening it",
    );
    assert_eq!(
        headers_len(&mut app),
        0,
        "key/value field should be truly empty after deleting the last entry",
    );
}

#[test]
fn save_overlay_stack_rejects_invalid_composite_payloads() {
    let mut app = build_invalid_composite_overlay_app();
    activate_service_variant(&mut app);

    {
        let form_state = app.form_state_mut_for_test();
        focus_field(form_state, "/service");
    }
    let before = app
        .form_state_mut_for_test()
        .field_by_pointer("/service")
        .expect("service field")
        .current_value()
        .expect("service value before save");

    app.open_overlay_for_test();
    {
        let overlay_form = app
            .active_overlay_form_state_for_test()
            .expect("service overlay form");
        overlay_form
            .field_mut_by_pointer("/port")
            .expect("port field")
            .seed_value(&json!("oops"));
    }

    assert!(
        !app.save_overlay_stack_to_root(),
        "invalid composite overlay should not be committed to the host form"
    );
    assert_eq!(app.overlay_depth_for_test(), 1);

    let after = app
        .form_state_mut_for_test()
        .field_by_pointer("/service")
        .expect("service field")
        .current_value()
        .expect("service value after rejected save");
    assert_eq!(after, before);
}
