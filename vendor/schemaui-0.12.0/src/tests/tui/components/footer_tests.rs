use crate::tui::view::{
    UiContext,
    components::footer::{
        FooterActionItem, FooterStatusTone, footer_action_items, footer_status_model,
    },
};

#[test]
fn footer_action_items_split_help_snippets_into_pairs() {
    let items = footer_action_items(Some(
        "Ctrl+S -> validate and save • Ctrl+Q -> quit • Tab -> next field",
    ));

    assert_eq!(
        items,
        vec![
            FooterActionItem {
                keys: "Ctrl+S".to_string(),
                action: "validate and save".to_string(),
            },
            FooterActionItem {
                keys: "Ctrl+Q".to_string(),
                action: "quit".to_string(),
            },
            FooterActionItem {
                keys: "Tab".to_string(),
                action: "next field".to_string(),
            },
        ]
    );
}

#[test]
fn footer_status_model_prioritizes_error_tone_and_focus_context() {
    let ctx = UiContext {
        status_message: "3 issue(s) remaining",
        dirty: true,
        error_count: 3,
        help: None,
        global_errors: &[String::from(
            "matrix[0][2] invalid: expected boolean or null but got string that keeps growing",
        )],
        focus_label: Some("Level4 › Matrix".to_string()),
        session_title: Some("Service Config"),
        popup: None,
        composite_overlay: None,
        help_overlay: None,
    };

    let status = footer_status_model(&ctx);

    assert_eq!(status.tone, FooterStatusTone::Error);
    assert_eq!(status.badge, "ERR 3");
    assert_eq!(status.message, "3 issue(s) remaining");
    assert!(
        status.meta.iter().any(|entry| entry == "Unsaved changes"),
        "dirty state should still be surfaced when errors exist"
    );
    assert!(
        status
            .meta
            .iter()
            .any(|entry| entry == "Focus Level4 › Matrix"),
        "focus label should remain visible in the footer meta line"
    );
    assert!(
        status
            .alert
            .as_deref()
            .is_some_and(|alert| alert.ends_with('…')),
        "global alert text should be truncated for readability"
    );
    assert_eq!(status.session_title.as_deref(), Some("Service Config"));
}

#[test]
fn footer_status_model_compacts_ready_message() {
    let ctx = UiContext {
        status_message: crate::tui::app::status::READY_STATUS,
        dirty: false,
        error_count: 0,
        help: Some("Ctrl+S -> validate and save"),
        global_errors: &[],
        focus_label: None,
        session_title: Some("SchemaUI Demo"),
        popup: None,
        composite_overlay: None,
        help_overlay: None,
    };

    let status = footer_status_model(&ctx);

    assert_eq!(status.tone, FooterStatusTone::Ready);
    assert_eq!(status.badge, "READY");
    assert_eq!(status.message, "Ready for input");
    assert!(status.meta.is_empty());
    assert!(status.alert.is_none());
    assert_eq!(status.session_title.as_deref(), Some("SchemaUI Demo"));
}
