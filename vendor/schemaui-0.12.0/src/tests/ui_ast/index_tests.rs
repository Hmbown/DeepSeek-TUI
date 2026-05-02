use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;

use crate::io::{DocumentFormat, input::parse_document_str};
use crate::ui_ast::{
    build_ui_ast,
    index::{PointerIndex, build_pointer_index, collect_pointers},
};

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("schemas")
        .join("test-comprehensive.schema.json")
}

#[test]
fn pointer_index_covers_all_pointers() {
    let path = schema_path();

    let contents = std::fs::read_to_string(&path).expect("schema file readable");
    let schema_value: Value =
        parse_document_str(&contents, DocumentFormat::Json).expect("schema parses at runtime");
    let ast = build_ui_ast(&schema_value).expect("runtime UiAst");

    let pointers: BTreeSet<String> = collect_pointers(&ast);
    let index: PointerIndex = build_pointer_index(&ast);

    assert_eq!(
        index.len(),
        pointers.len(),
        "index size must match pointer set"
    );
    for p in &pointers {
        assert!(index.contains_key(p), "missing pointer in index: {p}");
    }
}

#[test]
fn pointer_index_assigns_unique_positions() {
    let path = schema_path();

    let contents = std::fs::read_to_string(&path).expect("schema file readable");
    let schema_value: Value =
        parse_document_str(&contents, DocumentFormat::Json).expect("schema parses at runtime");
    let ast = build_ui_ast(&schema_value).expect("runtime UiAst");

    let index: PointerIndex = build_pointer_index(&ast);
    let mut values: Vec<usize> = index.values().copied().collect();
    values.sort_unstable();

    // Ensure that all assigned positions are unique. We do not require the
    // index to be perfectly dense because multiple UiNodes may share the same
    // pointer, which would naturally create gaps when later entries override
    // earlier ones in the map.
    let unique: BTreeSet<usize> = values.iter().copied().collect();
    assert_eq!(
        values.len(),
        unique.len(),
        "pointer index positions must be unique"
    );
}
