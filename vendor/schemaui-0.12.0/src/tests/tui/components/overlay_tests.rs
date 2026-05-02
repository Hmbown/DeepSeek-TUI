use serde_json::json;

use crate::tui::model::form_schema_from_ui_ast;
use crate::tui::state::LayoutNavModel;
use crate::tui::view::components::overlay::layout_section_description;
use crate::ui_ast::{build_ui_ast, layout::build_ui_layout};

#[test]
fn layout_section_description_matches_nav_model() {
    let schema = json!({
        "type": "object",
        "properties": {
            "service": {
                "title": "Service",
                "type": "object",
                "properties": {
                    "routes": {
                        "title": "Routes",
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            }
        }
    });

    let ast = build_ui_ast(&schema).expect("ast");
    let form_schema = form_schema_from_ui_ast(&ast);
    let mut form_state = crate::tui::state::FormState::from_schema(&form_schema);
    let layout = build_ui_layout(&ast);
    let layout_nav = LayoutNavModel::from_uilayout(&layout);
    form_state.set_layout_nav(layout_nav.clone());

    assert!(form_state.focus_first_field_with_layout());

    let expected = {
        let root = layout_nav.roots.first().expect("root");
        let section = root.sections.first().expect("section");
        format!("Section: {} - {}", root.title, section.title)
    };

    let desc = layout_section_description(&form_state).expect("description");
    assert_eq!(desc, expected);
}
