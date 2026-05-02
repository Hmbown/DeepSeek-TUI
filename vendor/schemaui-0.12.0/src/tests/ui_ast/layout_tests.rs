use std::path::PathBuf;

use serde_json::{Value, json};

use crate::io::{DocumentFormat, input::parse_document_str};
use crate::ui_ast::{UiNodeKind, build_ui_ast, index, layout};

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("schemas")
        .join("test-comprehensive.schema.json")
}

#[test]
fn layout_field_pointers_reference_valid_nodes() {
    let path = schema_path();

    let contents = std::fs::read_to_string(&path).expect("schema file readable");
    let schema_value: Value =
        parse_document_str(&contents, DocumentFormat::Json).expect("schema parses at runtime");
    let ast = build_ui_ast(&schema_value).expect("runtime UiAst");

    let pointers = index::collect_pointers(&ast);
    let layout = layout::build_ui_layout(&ast);

    assert!(
        !layout.roots.is_empty(),
        "layout must expose at least one root for the comprehensive schema",
    );

    for root in &layout.roots {
        assert!(
            !root.sections.is_empty(),
            "layout root '{}' must contain at least one section",
            root.id,
        );
        for section in &root.sections {
            for pointer in &section.field_pointers {
                assert!(
                    pointers.contains(pointer),
                    "layout references unknown field pointer: {pointer}",
                );
            }
        }
    }
}

#[test]
fn layout_groups_scalar_roots_into_general_section() {
    let schema = json!({
        "type": "object",
        "properties": {
            "metadata": {
                "type": "object",
                "properties": {
                    "serviceName": {"type": "string"}
                }
            },
            "generalFlag": {"type": "string"}
        }
    });

    let ast = build_ui_ast(&schema).expect("runtime UiAst");
    let layout = layout::build_ui_layout(&ast);

    // Expect one object root for "metadata" and a separate "general" root
    // that collects non-object top-level fields such as "generalFlag".
    let general = layout
        .roots
        .iter()
        .find(|root| root.id == "general")
        .expect("general root");
    let general_section = general.sections.first().expect("general section");
    assert!(
        general_section
            .field_pointers
            .iter()
            .any(|p| p == "/generalFlag"),
        "general section should include the scalar top-level field",
    );

    let metadata = layout
        .roots
        .iter()
        .find(|root| root.id == "metadata")
        .expect("metadata root");
    let metadata_section = metadata.sections.first().expect("metadata section");
    assert_eq!(metadata_section.pointer, "/metadata");
    assert!(
        metadata_section
            .field_pointers
            .iter()
            .any(|p| p == "/metadata/serviceName"),
        "metadata section should include its child field pointer",
    );
}

#[test]
fn layout_preserves_declared_root_and_nested_property_order() {
    let schema = json!({
        "type": "object",
        "properties": {
            "zeta": {
                "type": "object",
                "title": "Zeta",
                "properties": {
                    "second": {"type": "string"},
                    "first": {"type": "string"},
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

    let ast = build_ui_ast(&schema).expect("runtime UiAst");
    let layout = layout::build_ui_layout(&ast);

    let root_ids: Vec<_> = layout.roots.iter().map(|root| root.id.as_str()).collect();
    assert_eq!(
        root_ids,
        vec!["zeta", "alpha"],
        "layout roots should follow schema declaration order",
    );

    let zeta = layout
        .roots
        .iter()
        .find(|root| root.id == "zeta")
        .expect("zeta root");
    let zeta_section = zeta.sections.first().expect("zeta section");

    assert_eq!(
        zeta_section.field_pointers,
        vec!["/zeta/second", "/zeta/first"],
        "field list should preserve declared property order within an object section",
    );

    let child_pointers: Vec<_> = zeta_section
        .children
        .iter()
        .map(|section| section.pointer.as_str())
        .collect();
    assert_eq!(
        child_pointers,
        vec!["/zeta/network", "/zeta/auth"],
        "nested section order should follow schema declaration order",
    );
}

#[test]
fn build_ui_ast_accepts_object_only_allof_refs_without_crashing() {
    let schema = json!({
        "type": "object",
        "properties": {
            "permissions": {
                "allOf": [
                    { "$ref": "#/definitions/PermissionsToml" }
                ]
            },
            "features": {
                "type": "object",
                "properties": {
                    "apps": { "type": "boolean" }
                }
            }
        },
        "definitions": {
            "PermissionsToml": {
                "type": "object"
            }
        }
    });

    let ast = build_ui_ast(&schema).expect("runtime UiAst");
    assert_eq!(
        ast.roots.len(),
        2,
        "all top-level roots should still be built"
    );

    let permissions = ast
        .roots
        .iter()
        .find(|node| node.pointer == "/permissions")
        .expect("permissions root");
    assert_eq!(permissions.default_value, Some(json!({})));
    assert!(
        matches!(
            permissions.kind,
            UiNodeKind::Object {
                ref children,
                ref required
            } if children.is_empty() && required.is_empty()
        ),
        "opaque object schemas should degrade to an empty object boundary instead of crashing",
    );

    let features = ast
        .roots
        .iter()
        .find(|node| node.pointer == "/features")
        .expect("features root");
    assert!(
        matches!(
            features.kind,
            UiNodeKind::Object { ref children, .. }
                if children.iter().any(|child| child.pointer == "/features/apps")
        ),
        "ordinary object schemas should remain fully traversable",
    );

    let ui_layout = layout::build_ui_layout(&ast);
    assert!(
        ui_layout.roots.iter().any(|root| root.id == "features"),
        "layout should still be buildable when an opaque object field is present",
    );
}
