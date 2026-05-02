use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jsonschema::validator_for;
use ratatui::layout::Rect;
use serde_json::{Value, json};

use crate::{
    tui::{
        app::{App, UiOptions},
        model::form_schema_from_ui_ast,
        state::FormState,
    },
    ui_ast::build_ui_ast,
};

fn runtime_form_schema(schema: &Value) -> crate::tui::model::FormSchema {
    let ast = build_ui_ast(schema).expect("ui ast");
    form_schema_from_ui_ast(&ast)
}

fn build_app(schema: Value) -> App {
    let form_schema = runtime_form_schema(&schema);
    let form_state = FormState::from_schema(&form_schema);
    let validator = validator_for(&schema).expect("validator");
    App::new(form_state, validator, UiOptions::default())
}

fn required_string_object(field_count: usize) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for idx in 0..field_count {
        let name = format!("field_{idx}");
        properties.insert(
            name.clone(),
            json!({
                "type": "string",
                "minLength": 1
            }),
        );
        required.push(Value::String(name));
    }

    json!({
        "type": "object",
        "properties": properties,
        "required": required
    })
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn help_overlay_keeps_shortcuts_visible_when_no_errors_exist() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        }
    });
    let mut app = build_app(schema);

    let viewport = Rect::new(0, 0, 140, 18);
    app.toggle_help_overlay_for_test(viewport);
    let snapshot = app
        .help_overlay_snapshot_for_test(viewport)
        .expect("help overlay snapshot");

    assert!(
        snapshot.total_shortcuts >= 10,
        "shortcut table should stay dense and visible"
    );
    assert_eq!(
        snapshot.visible_errors, 0,
        "error page should be empty when form is valid"
    );
    assert_eq!(snapshot.total_errors, 0, "total errors should be zero");
    assert_eq!(snapshot.current_page, 1);
    assert_eq!(snapshot.total_pages, 1);
    assert!(
        snapshot.summary.contains("only the error list paginates"),
        "summary should explain that shortcuts remain visible while only errors page"
    );
}

#[test]
fn help_overlay_paginates_errors_without_hiding_shortcuts() {
    let schema = required_string_object(12);
    let mut app = build_app(schema);
    let viewport = Rect::new(0, 0, 140, 18);

    app.handle_key_for_test(key(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .expect("trigger validation");
    app.toggle_help_overlay_for_test(viewport);

    let first_page = app
        .help_overlay_snapshot_for_test(viewport)
        .expect("first help page");
    assert!(
        first_page.total_shortcuts >= 10,
        "shortcuts should stay visible on every page"
    );
    assert!(
        first_page.visible_errors > 0 && first_page.visible_errors < 12,
        "first page should show a non-empty error slice rather than the whole list"
    );
    assert_eq!(
        first_page.total_errors, 12,
        "all errors should still be counted"
    );
    assert_eq!(first_page.current_page, 1);
    assert_eq!(first_page.total_pages, 2);

    app.handle_key_for_test(key(KeyCode::Tab, KeyModifiers::NONE))
        .expect("next help page");
    let second_page = app
        .help_overlay_snapshot_for_test(viewport)
        .expect("second help page");
    assert_eq!(
        second_page.total_shortcuts, first_page.total_shortcuts,
        "shortcut count should not change"
    );
    assert_eq!(
        second_page.visible_errors,
        12 - first_page.visible_errors,
        "second page should show the remaining errors"
    );
    assert_eq!(second_page.total_errors, 12);
    assert_eq!(second_page.current_page, 2);
    assert_eq!(second_page.total_pages, 2);
}

#[test]
fn help_overlay_error_page_size_tracks_terminal_height() {
    let schema = required_string_object(24);
    let short_viewport = Rect::new(0, 0, 140, 16);
    let tall_viewport = Rect::new(0, 0, 140, 28);

    let mut short_app = build_app(schema.clone());
    short_app
        .handle_key_for_test(key(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .expect("trigger validation");
    short_app.toggle_help_overlay_for_test(short_viewport);
    let short_page = short_app
        .help_overlay_snapshot_for_test(short_viewport)
        .expect("short viewport page");

    let mut tall_app = build_app(schema);
    tall_app
        .handle_key_for_test(key(KeyCode::Char('s'), KeyModifiers::CONTROL))
        .expect("trigger validation");
    tall_app.toggle_help_overlay_for_test(tall_viewport);
    let tall_page = tall_app
        .help_overlay_snapshot_for_test(tall_viewport)
        .expect("tall viewport page");

    assert_eq!(short_page.total_errors, 24);
    assert_eq!(tall_page.total_errors, 24);
    assert!(
        short_page.visible_errors < tall_page.visible_errors,
        "taller terminal should fit more errors per page"
    );
    assert!(
        short_page.total_pages > tall_page.total_pages,
        "taller terminal should require fewer pages"
    );
}

#[test]
fn help_overlay_shortcuts_can_scroll_on_short_terminals() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        }
    });
    let mut app = build_app(schema);
    let viewport = Rect::new(0, 0, 140, 14);

    app.toggle_help_overlay_for_test(viewport);
    let before = app
        .help_overlay_snapshot_for_test(viewport)
        .expect("initial help snapshot");
    assert!(
        before.visible_shortcuts < before.total_shortcuts,
        "short terminal should clip the shortcut table and require scrolling"
    );
    assert_eq!(before.shortcut_offset, 0);

    app.handle_key_for_test(key(KeyCode::Down, KeyModifiers::NONE))
        .expect("scroll help shortcuts down");
    let after = app
        .help_overlay_snapshot_for_test(viewport)
        .expect("scrolled help snapshot");
    assert_eq!(after.shortcut_offset, 1);
    assert_eq!(after.visible_shortcuts, before.visible_shortcuts);
    assert_eq!(after.total_shortcuts, before.total_shortcuts);
}

#[test]
fn footer_help_includes_string_edit_shortcuts_for_string_fields() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        }
    });
    let app = build_app(schema);

    let help = app.current_help_text_for_test().expect("string help");
    assert!(help.contains("Tab/Down -> Next field"));
    assert!(help.contains("Left -> Move cursor left"));
    assert!(help.contains("Right -> Move cursor right"));
    assert!(help.contains("Ctrl+W -> Delete previous word"));
}

#[test]
fn footer_help_includes_numeric_shortcuts_for_number_fields() {
    let schema = json!({
        "type": "object",
        "properties": {
            "threshold": {"type": "number"}
        }
    });
    let app = build_app(schema);

    let help = app.current_help_text_for_test().expect("numeric help");
    assert!(help.contains("Tab/Down -> Next field"));
    assert!(help.contains("Left -> Step value down"));
    assert!(help.contains("Right -> Step value up"));
    assert!(help.contains("Shift+Left -> Fast step value down"));
    assert!(help.contains("Shift+Right -> Fast step value up"));
}

#[test]
fn help_overlay_error_column_supports_horizontal_scroll_with_h_and_l() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        }
    });
    let mut app = build_app(schema);
    let long_message = "this validation error is intentionally long so horizontal scrolling can reveal the hidden tail of the message in the help overlay";
    assert!(
        app.form_state_mut_for_test()
            .set_error("/name", long_message.to_string()),
        "expected test field to exist"
    );

    let viewport = Rect::new(0, 0, 72, 14);
    app.toggle_help_overlay_for_test(viewport);
    let before = app
        .help_overlay_snapshot_for_test(viewport)
        .expect("initial help snapshot");
    assert_eq!(before.error_offset, 0);

    app.handle_key_for_test(key(KeyCode::Char('l'), KeyModifiers::NONE))
        .expect("scroll error text right");
    let after = app
        .help_overlay_snapshot_for_test(viewport)
        .expect("scrolled help snapshot");
    assert!(after.error_offset > 0, "expected horizontal error scroll");

    app.handle_key_for_test(key(KeyCode::Char('h'), KeyModifiers::NONE))
        .expect("scroll error text left");
    let restored = app
        .help_overlay_snapshot_for_test(viewport)
        .expect("restored help snapshot");
    assert_eq!(restored.error_offset, 0);
}
