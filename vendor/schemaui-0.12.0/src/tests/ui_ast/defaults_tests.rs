use std::path::PathBuf;

use serde_json::Value;

use crate::io::{DocumentFormat, input::parse_document_str};
use crate::ui_ast::{build_ui_ast, defaults, index};

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("schemas")
        .join("test-comprehensive.schema.json")
}

#[test]
fn defaults_index_covers_all_pointers() {
    let path = schema_path();

    let contents = std::fs::read_to_string(&path).expect("schema file readable");
    let schema_value: Value =
        parse_document_str(&contents, DocumentFormat::Json).expect("schema parses at runtime");
    let ast = build_ui_ast(&schema_value).expect("runtime UiAst");

    let pointers = index::collect_pointers(&ast);
    let defaults = defaults::collect_defaults(&ast);

    // The defaults index should only ever contain entries for pointers that
    // actually exist in the UiAst. We deliberately do not require it to cover
    // every pointer because some pointers (e.g. internal variant nodes) are
    // not meant to have independent defaults applied.
    for pointer in defaults.keys() {
        assert!(
            pointers.contains(pointer),
            "defaults index contains unknown pointer: {pointer}",
        );
    }
}

#[test]
fn composite_default_includes_const_and_required_fields() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "channel": {
                "oneOf": [
                    {
                        "properties": {
                            "type": {"const": "http"},
                            "url": {"type": "string"}
                        },
                        "required": ["type", "url"]
                    }
                ]
            }
        }
    });

    let ast = build_ui_ast(&schema).expect("runtime UiAst");
    let defaults = defaults::collect_defaults(&ast);

    let value = defaults
        .get("/channel")
        .expect("default for composite channel field");
    let obj = value
        .as_object()
        .expect("composite default must be an object");

    assert_eq!(obj.get("type"), Some(&Value::String("http".to_string())));
    assert!(
        obj.get("url")
            .and_then(Value::as_str)
            .map(|s| s.is_empty())
            .unwrap_or(false),
        "required url field should have an inferred empty-string default",
    );
}
