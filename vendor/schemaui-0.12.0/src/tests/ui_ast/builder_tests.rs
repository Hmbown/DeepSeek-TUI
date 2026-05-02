use serde_json::json;

use crate::ui_ast::{UiNodeKind, build_ui_ast};

fn root_kind<'a>(ast: &'a crate::ui_ast::UiAst, pointer: &str) -> &'a UiNodeKind {
    &ast.roots
        .iter()
        .find(|node| node.pointer == pointer)
        .unwrap_or_else(|| panic!("root {pointer} should exist"))
        .kind
}

#[test]
fn composite_variant_title_uses_kind_const() {
    let schema = json!({
        "type": "object",
        "properties": {
            "item": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "kind": { "const": "simple" },
                            "label": { "type": "string" }
                        },
                        "required": ["kind", "label"]
                    }
                ]
            }
        }
    });

    let ast = build_ui_ast(&schema).expect("ui ast should build");
    let UiNodeKind::Composite { variants, .. } = root_kind(&ast, "/item") else {
        panic!("item should render as composite");
    };

    assert_eq!(variants[0].title.as_deref(), Some("Simple"));
}

#[test]
fn composite_variant_title_for_scalar_array_uses_list_notation() {
    let schema = json!({
        "type": "object",
        "properties": {
            "item": {
                "oneOf": [
                    {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                ]
            }
        }
    });

    let ast = build_ui_ast(&schema).expect("ui ast should build");
    let UiNodeKind::Composite { variants, .. } = root_kind(&ast, "/item") else {
        panic!("item should render as composite");
    };

    assert_eq!(variants[0].title.as_deref(), Some("List<string>"));
}

#[test]
fn one_of_and_any_of_roots_render_as_composites() {
    let schema = json!({
        "type": "object",
        "properties": {
            "oneOfItem": {
                "oneOf": [
                    { "type": "string" },
                    { "type": "integer" }
                ]
            },
            "anyOfItem": {
                "anyOf": [
                    { "type": "boolean" },
                    { "type": "string" }
                ]
            }
        }
    });

    let ast = build_ui_ast(&schema).expect("ui ast should build");

    assert!(matches!(
        root_kind(&ast, "/oneOfItem"),
        UiNodeKind::Composite { .. }
    ));
    assert!(matches!(
        root_kind(&ast, "/anyOfItem"),
        UiNodeKind::Composite { .. }
    ));
}
