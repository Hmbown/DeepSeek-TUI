use crate::tui::view::components::fields::{error_lines, meta_lines};
use crate::{
    tui::model::{FieldKind, FieldSchema},
    tui::state::FieldState,
};
use ratatui::style::{Color, Modifier};

fn make_field() -> FieldState {
    FieldState::from_schema(FieldSchema {
        name: "test".into(),
        path: vec!["test".into()],
        pointer: "/test".into(),
        title: "Test".into(),
        description: None,
        kind: FieldKind::String,
        required: false,
        default: None,
        metadata: Default::default(),
    })
}

#[test]
fn meta_line_selected_uses_dark_text() {
    let field = make_field();
    let lines = meta_lines(&field, true, 40);
    let span = lines
        .first()
        .and_then(|line| line.spans.first())
        .expect("type span");
    assert_eq!(span.style.fg, Some(Color::Blue));
    assert!(span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn meta_line_unselected_uses_gray() {
    let field = make_field();
    let lines = meta_lines(&field, false, 40);
    let span = lines
        .first()
        .and_then(|line| line.spans.first())
        .expect("type span");
    assert_eq!(span.style.fg, Some(Color::DarkGray));
    assert!(!span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn error_lines_wrap_and_cap_at_three_lines() {
    let mut field = make_field();
    field.set_error(
        "this validation error is intentionally long so the renderer must wrap it across multiple lines without letting the panel grow forever".to_string(),
    );

    let lines = error_lines(&field, 28).expect("error lines");
    assert_eq!(lines.len(), 4, "label + at most 3 wrapped lines");

    let last = lines
        .last()
        .expect("last wrapped line")
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(
        last.contains('…'),
        "truncated final line should end with ellipsis: {last}"
    );
}
