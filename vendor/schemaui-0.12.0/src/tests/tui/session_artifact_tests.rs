use std::{fs, path::PathBuf};

use serde_json::{Value, json};

use crate::io::{DocumentFormat, input::parse_document_str};
use crate::precompile::build_ui_artifact_bundle;
use crate::tui::session::resolve_tui_artifacts;

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("schemas")
        .join("test-comprehensive.schema.json")
}

fn schema_value() -> Value {
    let path = schema_path();
    let contents = fs::read_to_string(path).expect("schema file readable");
    parse_document_str(&contents, DocumentFormat::Json).expect("schema parses")
}

#[test]
fn resolve_tui_artifacts_matches_runtime_when_bundle_supplies_tui_derivatives() {
    let schema = schema_value();
    let defaults = json!({
        "simpleTypes": {
            "text": "hello from defaults",
            "number": 7,
            "toggle": true,
            "dropdown": "option2"
        }
    });

    let bundle =
        build_ui_artifact_bundle(&schema, Some(&defaults)).expect("build UI artifact bundle");

    let runtime = resolve_tui_artifacts(&bundle.ui.ui_ast, &bundle.ui.layout, None);
    let provided = resolve_tui_artifacts(&bundle.ui.ui_ast, &bundle.ui.layout, Some(bundle.tui));

    assert_eq!(runtime, provided);
}
