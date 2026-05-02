use std::collections::HashMap;

use serde_json::json;

use crate::{
    tui::model::{CompositeField, CompositeMode, CompositeVariant, FieldKind, FieldSchema},
    tui::state::FieldState,
};

fn composite_list_field() -> FieldState {
    let variants = vec![
        CompositeVariant {
            id: "target".to_string(),
            title: "Target object".to_string(),
            description: None,
            schema: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"}
                }
            }),
            is_object: true,
        },
        CompositeVariant {
            id: "string".to_string(),
            title: "String entry".to_string(),
            description: None,
            schema: json!({"type": "string"}),
            is_object: false,
        },
        CompositeVariant {
            id: "integer".to_string(),
            title: "Integer entry".to_string(),
            description: None,
            schema: json!({"type": "integer"}),
            is_object: false,
        },
    ];
    let template = CompositeField {
        mode: CompositeMode::AnyOf,
        variants,
    };
    FieldState::from_schema(FieldSchema {
        name: "deepItems".to_string(),
        path: vec!["deepItems".to_string()],
        pointer: "/deepItems".to_string(),
        title: "Deep Items".to_string(),
        description: None,
        kind: FieldKind::Array(Box::new(FieldKind::Composite(Box::new(template)))),
        required: false,
        default: None,
        metadata: HashMap::new(),
    })
}

#[test]
fn composite_list_popup_exposes_entry_variants() {
    let mut field = composite_list_field();
    assert!(field.composite_popup().is_none(), "no entry yet");
    assert!(
        field.ensure_composite_list_popup_entry(),
        "first popup should seed an entry"
    );
    let popup = field.composite_popup().expect("popup available");
    assert_eq!(popup.options.len(), 3);
    assert!(popup.multi, "anyOf should expose multi-select popup");
    assert_eq!(popup.options[0], "Target object");
}

#[test]
fn composite_list_selection_updates_summary() {
    let mut field = composite_list_field();
    field.ensure_composite_list_popup_entry();
    let popup = field.composite_popup().expect("popup");
    let mut toggles = vec![false; popup.options.len()];
    toggles[0] = true;
    toggles[2] = true;
    field.apply_composite_selection(0, Some(toggles));
    let (entries, selected) = field
        .composite_list_panel()
        .expect("panel state must exist");
    assert_eq!(selected, 0, "first entry should be selected");
    assert_eq!(entries.len(), 2, "one entry per selected variant");
    assert!(
        entries[0].contains("Target object"),
        "first entry should summarize the 'Target object' variant: {:?}",
        entries[0]
    );
    assert!(
        entries[1].contains("Integer entry"),
        "second entry should summarize the 'Integer entry' variant: {:?}",
        entries[1]
    );
}

#[test]
fn composite_list_adds_entries_with_selected_variants() {
    let mut field = composite_list_field();

    // Add an entry explicitly as the target object variant
    assert!(
        field.composite_list_add_entry_with_variant(0),
        "should add entry for 'Target object' variant"
    );

    // Add another entry explicitly as the integer variant
    assert!(
        field.composite_list_add_entry_with_variant(2),
        "should add entry for 'Integer entry' variant"
    );

    let (entries, selected) = field
        .composite_list_panel()
        .expect("panel state must exist after adding entries");

    assert_eq!(entries.len(), 2, "expected two entries after explicit adds");
    assert_eq!(selected, 1, "last added entry should be selected");

    assert!(
        entries[0].contains("Target object"),
        "first entry should summarize the 'Target object' variant: {:?}",
        entries[0]
    );
    assert!(
        entries[1].contains("Integer entry"),
        "second entry should summarize the 'Integer entry' variant: {:?}",
        entries[1]
    );
}

#[test]
fn composite_list_summary_includes_object_preview() {
    let variants = vec![CompositeVariant {
        id: "bar".to_string(),
        title: "Bar Item".to_string(),
        description: None,
        schema: json!({
            "type": "object",
            "properties": {
                "Bar": {"type": "string"},
                "Id": {"type": "integer"}
            }
        }),
        is_object: true,
    }];
    let template = CompositeField {
        mode: CompositeMode::AnyOf,
        variants,
    };

    let mut field = FieldState::from_schema(FieldSchema {
        name: "blahList".to_string(),
        path: vec!["blahList".to_string()],
        pointer: "/blahList".to_string(),
        title: "Blah List".to_string(),
        description: None,
        kind: FieldKind::Array(Box::new(FieldKind::Composite(Box::new(template)))),
        required: false,
        default: None,
        metadata: HashMap::new(),
    });
    field.seed_value(&json!([{
        "Bar": "on tui",
        "Id": 0
    }]));

    let (entries, selected) = field.composite_list_panel().expect("panel state");
    assert_eq!(selected, 0);
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].contains("Bar Item { Bar: on tui, Id: 0 }"),
        "entry summary should include object preview: {:?}",
        entries[0]
    );
}

#[test]
fn composite_list_summary_with_limit_keeps_bar_and_id_visible() {
    let variants = vec![CompositeVariant {
        id: "bar".to_string(),
        title: "Bar Item".to_string(),
        description: None,
        schema: json!({
            "type": "object",
            "properties": {
                "Bar": {"type": "string"},
                "Id": {"type": "integer"}
            }
        }),
        is_object: true,
    }];
    let template = CompositeField {
        mode: CompositeMode::AnyOf,
        variants,
    };

    let mut field = FieldState::from_schema(FieldSchema {
        name: "blahList".to_string(),
        path: vec!["blahList".to_string()],
        pointer: "/blahList".to_string(),
        title: "Blah List".to_string(),
        description: None,
        kind: FieldKind::Array(Box::new(FieldKind::Composite(Box::new(template)))),
        required: false,
        default: None,
        metadata: HashMap::new(),
    });
    field.seed_value(&json!([{
        "Bar": "客服热线 400-820-8820 转人工服务",
        "Id": 0
    }]));

    let (entries, selected) = field
        .composite_list_panel_with_limit(44)
        .expect("panel state");
    assert_eq!(selected, 0);
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].contains("Bar:"),
        "entry summary: {:?}",
        entries[0]
    );
    assert!(
        entries[0].contains("Id: 0"),
        "entry summary: {:?}",
        entries[0]
    );
    let compact = field
        .composite_list_selected_label_with_limit(44)
        .expect("compact selected label");
    assert!(!compact.contains("Bar:"), "compact label: {compact}");
    assert!(!compact.contains("Id: 0"), "compact label: {compact}");
}
