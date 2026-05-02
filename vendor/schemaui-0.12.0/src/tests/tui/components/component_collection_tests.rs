use std::collections::HashMap;

use crate::tui::{
    model::{CompositeField, CompositeMode, CompositeVariant, FieldKind, FieldSchema},
    state::{FieldState, field::components::helpers::format_collection_value},
};
use serde_json::json;

#[test]
fn formats_collection_status_with_selection() {
    let text = format_collection_value("Map", 2, Some("key=value".to_string()), "(Ctrl+E edit)");
    assert_eq!(text, "Map[2] • key=value (Ctrl+E edit)");
}

#[test]
fn formats_collection_status_when_empty() {
    let text = format_collection_value("List", 0, None, "(Ctrl+N add)");
    assert_eq!(text, "List: empty (Ctrl+N add)");
}

#[test]
fn composite_list_display_value_uses_available_width_for_selected_entry() {
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

    let text = field.display_value_with_limit(64);
    assert!(
        text.contains("Variants: #1 Bar Item"),
        "display text: {text}"
    );
    assert!(
        !text.contains("Bar:"),
        "display text should stay compact: {text}"
    );
    assert!(
        !text.contains("Id: 0"),
        "display text should stay compact: {text}"
    );
}
