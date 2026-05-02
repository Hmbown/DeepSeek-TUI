use std::collections::HashMap;

use crate::{
    tui::model::{FieldKind, FieldSchema},
    tui::state::{FieldState, FormState, RootSectionState, SectionState},
};

fn text_field(pointer: &str, title: &str) -> FieldState {
    let path: Vec<String> = pointer
        .trim_start_matches('/')
        .split('/')
        .map(|segment| segment.to_string())
        .collect();
    let schema = FieldSchema {
        name: title.to_string(),
        path,
        pointer: pointer.to_string(),
        title: title.to_string(),
        description: None,
        kind: FieldKind::String,
        required: false,
        default: None,
        metadata: HashMap::new(),
    };
    FieldState::from_schema(schema)
}

fn section(id: &str, title: &str, depth: usize, fields: Vec<FieldState>) -> SectionState {
    SectionState {
        id: id.to_string(),
        title: title.to_string(),
        description: None,
        path: vec![id.to_string()],
        depth,
        fields,
        scroll_offset: 0,
    }
}

fn sample_form_state() -> FormState {
    let general_fields = vec![
        text_field("/general/name", "name"),
        text_field("/general/url", "url"),
    ];
    let child_fields = vec![text_field("/general/child/address", "address")];
    let root_general = RootSectionState {
        id: "general".to_string(),
        title: "General".to_string(),
        description: None,
        sections: vec![
            section("general_main", "General", 0, general_fields),
            section("general_empty", "Empty", 0, Vec::new()),
            section("general_child", "Child", 1, child_fields),
        ],
    };

    let site_fields = vec![text_field("/site/enabled", "enabled")];
    let root_site = RootSectionState {
        id: "site".to_string(),
        title: "Site".to_string(),
        description: None,
        sections: vec![section("site_main", "Site", 0, site_fields)],
    };

    FormState::from_roots_for_test(vec![root_general, root_site])
}

fn focused_pointer(form: &FormState) -> String {
    form.focused_field()
        .map(|field| field.schema.pointer.clone())
        .unwrap_or_else(|| "<none>".to_string())
}

#[test]
fn tab_cycles_sections_and_roots() {
    let mut form = sample_form_state();
    let mut seen = Vec::new();
    for _ in 0..5 {
        seen.push(focused_pointer(&form));
        form.focus_next_field();
    }
    seen.push(focused_pointer(&form));
    assert_eq!(
        seen,
        vec![
            "/general/name",
            "/general/url",
            "/general/child/address",
            "/site/enabled",
            "/general/name",
            "/general/url",
        ]
    );
}

#[test]
fn shift_tab_moves_backward_through_hierarchy() {
    let mut form = sample_form_state();
    form.set_root_index(0);
    form.set_section_index(0);
    form.set_field_index(0);

    // jump near the end
    form.focus_prev_field(); // should wrap to site/enabled
    let first = focused_pointer(&form);
    form.focus_prev_field();
    let second = focused_pointer(&form);
    form.focus_prev_field();
    let third = focused_pointer(&form);
    assert_eq!(first, "/site/enabled");
    assert_eq!(second, "/general/child/address");
    assert_eq!(third, "/general/url");
}
