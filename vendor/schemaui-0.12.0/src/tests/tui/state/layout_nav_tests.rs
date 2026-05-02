use std::collections::HashMap;

use crate::{
    tui::model::{FieldKind, FieldSchema, form_schema_from_ui_ast},
    tui::state::{FieldState, FormState, LayoutNavModel, RootSectionState, SectionState},
    ui_ast::{
        build_ui_ast,
        layout::{self, LayoutRoot, LayoutSection, UiLayout},
    },
};
use serde_json::json;

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

fn section_with_single_field(
    id: &str,
    title: &str,
    field_pointer: &str,
    field_title: &str,
) -> SectionState {
    SectionState {
        id: id.to_string(),
        title: title.to_string(),
        description: None,
        path: vec![id.to_string()],
        depth: 0,
        fields: vec![text_field(field_pointer, field_title)],
        scroll_offset: 0,
    }
}

fn build_form_and_layout() -> (FormState, UiLayout) {
    // Form layout:
    // runtime/auth -> /auth/user
    // runtime/db   -> /db/url
    // ops/cache    -> /cache/ttl
    let runtime_auth = section_with_single_field("auth", "Auth", "/auth/user", "user");
    let runtime_db = section_with_single_field("db", "DB", "/db/url", "url");
    let ops_cache = section_with_single_field("cache", "Cache", "/cache/ttl", "ttl");

    let root_runtime = RootSectionState {
        id: "runtime".to_string(),
        title: "Runtime".to_string(),
        description: None,
        sections: vec![runtime_auth, runtime_db],
    };
    let root_ops = RootSectionState {
        id: "ops".to_string(),
        title: "Ops".to_string(),
        description: None,
        sections: vec![ops_cache],
    };

    let form = FormState::from_roots_for_test(vec![root_runtime, root_ops]);

    let layout = UiLayout {
        roots: vec![
            LayoutRoot {
                id: "runtime".to_string(),
                title: Some("Runtime".to_string()),
                description: None,
                sections: vec![
                    LayoutSection {
                        id: "auth".to_string(),
                        title: "Auth".to_string(),
                        description: None,
                        pointer: "/auth".to_string(),
                        path: vec!["auth".to_string()],
                        field_pointers: vec!["/auth/user".to_string()],
                        children: Vec::new(),
                    },
                    LayoutSection {
                        id: "db".to_string(),
                        title: "DB".to_string(),
                        description: None,
                        pointer: "/db".to_string(),
                        path: vec!["db".to_string()],
                        field_pointers: vec!["/db/url".to_string()],
                        children: Vec::new(),
                    },
                ],
            },
            LayoutRoot {
                id: "ops".to_string(),
                title: Some("Ops".to_string()),
                description: None,
                sections: vec![LayoutSection {
                    id: "cache".to_string(),
                    title: "Cache".to_string(),
                    description: None,
                    pointer: "/cache".to_string(),
                    path: vec!["cache".to_string()],
                    field_pointers: vec!["/cache/ttl".to_string()],
                    children: Vec::new(),
                }],
            },
        ],
    };

    (form, layout)
}

fn focused_pointer(form: &FormState) -> String {
    form.focused_field()
        .map(|field| field.schema.pointer.clone())
        .unwrap_or_else(|| "<none>".to_string())
}

#[test]
fn layout_nav_controls_section_stepping_order() {
    let (mut form, layout) = build_form_and_layout();
    let nav = LayoutNavModel::from_uilayout(&layout);
    form.set_layout_nav(nav);

    // Start at the first section (auth).
    assert_eq!(focused_pointer(&form), "/auth/user");

    form.focus_next_section(1);
    assert_eq!(focused_pointer(&form), "/db/url");

    form.focus_next_section(1);
    assert_eq!(focused_pointer(&form), "/cache/ttl");

    // Wrap back to the first section.
    form.focus_next_section(1);
    assert_eq!(focused_pointer(&form), "/auth/user");
}

#[test]
fn layout_nav_controls_root_stepping_order() {
    let (mut form, layout) = build_form_and_layout();
    let nav = LayoutNavModel::from_uilayout(&layout);
    form.set_layout_nav(nav);

    assert_eq!(focused_pointer(&form), "/auth/user");

    form.focus_next_root(1);
    assert_eq!(focused_pointer(&form), "/cache/ttl");

    form.focus_next_root(1);
    assert_eq!(focused_pointer(&form), "/auth/user");
}

#[test]
fn layout_nav_first_field_focus_uses_layout() {
    let (mut form, layout) = build_form_and_layout();
    let nav = LayoutNavModel::from_uilayout(&layout);
    form.set_layout_nav(nav);

    // Move focus away from the first section.
    form.focus_next_section(1);
    form.focus_next_section(1);
    assert_eq!(focused_pointer(&form), "/cache/ttl");

    // layout-driven first-field focus should return to the first section.
    assert!(form.focus_first_field_with_layout());
    assert_eq!(focused_pointer(&form), "/auth/user");
}

#[test]
fn layout_nav_from_schema_preserves_declared_root_and_section_order() {
    let schema = json!({
        "type": "object",
        "properties": {
            "zeta": {
                "type": "object",
                "title": "Zeta",
                "properties": {
                    "name": {"type": "string"},
                    "network": {
                        "type": "object",
                        "title": "Network",
                        "properties": {
                            "port": {"type": "integer"}
                        }
                    },
                    "auth": {
                        "type": "object",
                        "title": "Auth",
                        "properties": {
                            "user": {"type": "string"}
                        }
                    }
                }
            },
            "alpha": {
                "type": "object",
                "title": "Alpha",
                "properties": {
                    "enabled": {"type": "boolean"}
                }
            }
        }
    });

    let ast = build_ui_ast(&schema).expect("ui ast");
    let form_schema = form_schema_from_ui_ast(&ast);
    let layout = layout::build_ui_layout(&ast);
    let nav = LayoutNavModel::from_uilayout(&layout);

    let root_titles: Vec<_> = nav.roots.iter().map(|root| root.title.as_str()).collect();
    assert_eq!(
        root_titles,
        vec!["Zeta", "Alpha"],
        "root nav tabs should follow schema declaration order",
    );

    let zeta_sections: Vec<_> = nav.roots[0]
        .sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert_eq!(
        zeta_sections,
        vec!["Zeta", "Network", "Auth"],
        "nested nav tabs should follow schema declaration order",
    );

    let mut form = FormState::from_schema(&form_schema).with_layout_nav(nav);
    assert_eq!(focused_pointer(&form), "/zeta/name");

    form.focus_next_section(1);
    assert_eq!(focused_pointer(&form), "/zeta/network/port");

    form.focus_next_section(1);
    assert_eq!(focused_pointer(&form), "/zeta/auth/user");

    form.focus_next_root(1);
    assert_eq!(focused_pointer(&form), "/alpha/enabled");
}
