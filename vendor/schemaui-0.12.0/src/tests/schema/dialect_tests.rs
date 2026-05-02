use serde_json::json;

use crate::schema::dialect::{RootDialectContext, SchemaDialect};

#[test]
fn detects_draft_07_from_schema_keyword() {
    let schema = json!({
        "$schema": "http://json-schema.org/draft-07/schema#"
    });

    assert_eq!(SchemaDialect::detect(&schema), SchemaDialect::Draft7);
}

#[test]
fn detects_2020_12_from_schema_keyword() {
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema"
    });

    assert_eq!(SchemaDialect::detect(&schema), SchemaDialect::Draft202012);
}

#[test]
fn root_dialect_context_applies_schema_and_definition_keywords_to_overlay() {
    let root = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": {
            "bar": { "type": "string" }
        },
        "definitions": {
            "legacy": { "type": "integer" }
        }
    });
    let context = RootDialectContext::from_root(&root);
    let mut overlay = serde_json::Map::new();
    overlay.insert("type".to_string(), json!("object"));

    context.apply_to_overlay(&mut overlay);

    assert_eq!(
        overlay.get("$schema"),
        root.as_object().and_then(|obj| obj.get("$schema"))
    );
    assert_eq!(
        overlay.get("$defs"),
        root.as_object().and_then(|obj| obj.get("$defs"))
    );
    assert_eq!(
        overlay.get("definitions"),
        root.as_object().and_then(|obj| obj.get("definitions"))
    );
}
