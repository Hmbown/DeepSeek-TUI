use crate::schema::{loader::load_root_schema, resolver::SchemaResolver};
use serde_json::json;

#[test]
fn resolves_definition_reference() {
    let raw = json!({
        "definitions": {
            "duration": {
                "type": "object",
                "properties": {
                    "value": {"type": "integer"}
                }
            }
        },
        "type": "object",
        "properties": {
            "timeout": {"$ref": "#/definitions/duration"}
        }
    });
    let root = load_root_schema(&raw).expect("valid root schema");
    let resolver = SchemaResolver::new(&raw);
    let timeout_schema = root
        .object
        .as_ref()
        .unwrap()
        .properties
        .get("timeout")
        .unwrap();
    let resolved = resolver
        .resolve_schema(timeout_schema)
        .expect("resolution succeeds");
    assert!(resolved.object.is_some());
    assert!(
        resolved
            .object
            .as_ref()
            .unwrap()
            .properties
            .contains_key("value")
    );
}

#[test]
fn resolves_pointer_reference() {
    let raw = json!({
        "type": "object",
        "properties": {
            "base": {
                "type": "object",
                "properties": {
                    "url": {"type": "string"}
                }
            },
            "clone": {"$ref": "#/properties/base"}
        }
    });
    let root = load_root_schema(&raw).expect("valid root schema");
    let resolver = SchemaResolver::new(&raw);
    let clone_schema = root
        .object
        .as_ref()
        .unwrap()
        .properties
        .get("clone")
        .unwrap();
    let resolved = resolver
        .resolve_schema(clone_schema)
        .expect("resolution succeeds");
    let object = resolved.object.expect("object schema");
    assert!(object.properties.contains_key("url"));
}

#[test]
fn preserves_instance_metadata_and_extensions_when_resolving_reference() {
    let raw = json!({
        "definitions": {
            "duration": {
                "title": "Definition Title",
                "description": "Definition description",
                "type": "integer"
            }
        },
        "type": "object",
        "properties": {
            "timeout": {
                "$ref": "#/definitions/duration",
                "title": "Request Timeout",
                "description": "Per-request timeout",
                "default": 5,
                "x-group": "advanced",
                "x-group-title": "Advanced Settings"
            }
        }
    });
    let root = load_root_schema(&raw).expect("valid root schema");
    let resolver = SchemaResolver::new(&raw);
    let timeout_schema = root
        .object
        .as_ref()
        .unwrap()
        .properties
        .get("timeout")
        .unwrap();
    let resolved = resolver
        .resolve_schema(timeout_schema)
        .expect("resolution succeeds");

    let metadata = resolved.metadata.expect("metadata preserved");
    assert_eq!(metadata.title.as_deref(), Some("Request Timeout"));
    assert_eq!(metadata.description.as_deref(), Some("Per-request timeout"));
    assert_eq!(metadata.default, Some(json!(5)));
    assert_eq!(
        resolved
            .extensions
            .get("x-group")
            .and_then(serde_json::Value::as_str),
        Some("advanced")
    );
    assert_eq!(
        resolved.instance_type,
        load_root_schema(&raw["definitions"]["duration"])
            .expect("definition schema")
            .instance_type
    );
}

#[test]
fn preserves_instance_metadata_for_pointer_references() {
    let raw = json!({
        "type": "object",
        "properties": {
            "base": {
                "title": "Base URL",
                "type": "string"
            },
            "clone": {
                "$ref": "#/properties/base",
                "title": "Service URL",
                "description": "Instance-specific description"
            }
        }
    });
    let root = load_root_schema(&raw).expect("valid root schema");
    let resolver = SchemaResolver::new(&raw);
    let clone_schema = root
        .object
        .as_ref()
        .unwrap()
        .properties
        .get("clone")
        .unwrap();
    let resolved = resolver
        .resolve_schema(clone_schema)
        .expect("resolution succeeds");

    let metadata = resolved.metadata.expect("metadata preserved");
    assert_eq!(metadata.title.as_deref(), Some("Service URL"));
    assert_eq!(
        metadata.description.as_deref(),
        Some("Instance-specific description")
    );
}
