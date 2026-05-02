use crate::tui::app::{
    input::{InputRouter, KeyAction},
    keymap::{self, KeymapContext},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn router() -> InputRouter {
    InputRouter::new(keymap::default_store())
}

#[test]
fn ctrl_jl_cycle_root_sections() {
    let router = router();
    let prev = router.classify(&key(KeyCode::Char('j'), KeyModifiers::CONTROL));
    let next = router.classify(&key(KeyCode::Char('l'), KeyModifiers::CONTROL));
    assert!(matches!(prev, KeyAction::RootStep(-1)));
    assert!(matches!(next, KeyAction::RootStep(1)));
}

#[test]
fn ctrl_tab_maps_to_section_steps() {
    let router = router();
    let next = router.classify(&key(KeyCode::Tab, KeyModifiers::CONTROL));
    let prev = router.classify(&key(
        KeyCode::Tab,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert!(matches!(next, KeyAction::SectionStep(1)));
    assert!(matches!(prev, KeyAction::SectionStep(-1)));
}

#[test]
fn shift_tab_triggers_previous_field() {
    let router = router();
    let action = router.classify(&key(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert!(matches!(action, KeyAction::FieldStep(-1)));
}

#[test]
fn help_only_text_bindings_do_not_override_raw_input() {
    let router = router();

    let left = router.classify(&key(KeyCode::Left, KeyModifiers::NONE));
    let backspace = router.classify(&key(KeyCode::Backspace, KeyModifiers::NONE));
    let ctrl_w = router.classify(&key(KeyCode::Char('w'), KeyModifiers::CONTROL));

    assert!(matches!(left, KeyAction::Input(event) if event.code == KeyCode::Left));
    assert!(matches!(backspace, KeyAction::Input(event) if event.code == KeyCode::Backspace));
    assert!(matches!(ctrl_w, KeyAction::Input(event)
        if event.code == KeyCode::Char('w') && event.modifiers == KeyModifiers::CONTROL));
}

#[test]
fn default_keymap_exposes_text_and_numeric_help_contexts() {
    let store = keymap::default_store();

    let text_help = store
        .help_text(KeymapContext::TextInput)
        .expect("text input help");
    assert!(text_help.contains("Left -> Move cursor left"));
    assert!(text_help.contains("Right -> Move cursor right"));
    assert!(text_help.contains("Ctrl+W -> Delete previous word"));
    assert!(text_help.contains("Ctrl+Z -> Undo text edit"));
    assert!(text_help.contains("Ctrl+Y -> Redo text edit"));

    let numeric_help = store
        .help_text(KeymapContext::NumericInput)
        .expect("numeric input help");
    assert!(numeric_help.contains("Left -> Step value down"));
    assert!(numeric_help.contains("Right -> Step value up"));
    assert!(numeric_help.contains("Shift+Left -> Fast step value down"));
    assert!(numeric_help.contains("Shift+Right -> Fast step value up"));
    assert!(numeric_help.contains("Ctrl+Z -> Undo numeric edit"));
    assert!(numeric_help.contains("Ctrl+Y -> Redo numeric edit"));
}

#[test]
fn help_context_bindings_are_only_classified_when_requested() {
    let store = keymap::default_store();
    let esc = key(KeyCode::Esc, KeyModifiers::NONE);

    assert!(!matches!(store.classify(&esc), Some(KeyAction::HelpClose)));
    assert!(matches!(
        store.classify_for_contexts(&esc, &[KeymapContext::Help]),
        Some(KeyAction::HelpClose)
    ));
}
