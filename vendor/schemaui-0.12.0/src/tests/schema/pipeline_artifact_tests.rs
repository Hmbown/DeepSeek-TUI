use std::{fs, path::PathBuf};

use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};

use crate::core::frontend::{Frontend, FrontendContext};
use crate::core::pipeline::SchemaPipeline;
use crate::io::{DocumentFormat, input::parse_document_str};
use crate::precompile::build_ui_artifact_bundle;
use crate::ui_ast::{UiAst, UiAstBundle, UiLayout};

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

fn defaults_value() -> Value {
    json!({
        "simpleTypes": {
            "text": "hello from defaults",
            "number": 7,
            "toggle": true,
            "dropdown": "option2"
        }
    })
}

fn recursive_schema_value() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tree": {"$ref": "#/definitions/treeNode"}
        },
        "definitions": {
            "treeNode": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "children": {
                        "type": "array",
                        "items": {"$ref": "#/definitions/treeNode"}
                    }
                }
            }
        }
    })
}

#[derive(Serialize)]
struct CapturedFrontendContext {
    ui_ast: UiAst,
    layout: UiLayout,
    schema: Value,
    initial_data: Value,
}

#[derive(Debug)]
struct CaptureFrontend;

impl Frontend for CaptureFrontend {
    fn run(self, ctx: FrontendContext) -> Result<Value> {
        Ok(serde_json::to_value(CapturedFrontendContext {
            ui_ast: ctx.ui_ast,
            layout: ctx.layout,
            schema: ctx.schema,
            initial_data: ctx.initial_data,
        })?)
    }
}

fn capture_pipeline_output(schema: Value, defaults: Value, bundle: Option<UiAstBundle>) -> Value {
    let pipeline = SchemaPipeline::new(schema)
        .with_defaults(Some(defaults))
        .with_prepared_ui_bundle(bundle);
    pipeline
        .run_with_frontend(CaptureFrontend)
        .expect("pipeline capture succeeds")
}

#[test]
fn schema_pipeline_with_prepared_ui_bundle_matches_runtime_context() {
    let schema = schema_value();
    let defaults = defaults_value();
    let bundle = build_ui_artifact_bundle(&schema, Some(&defaults))
        .expect("build UI artifact bundle")
        .ui;

    let runtime = capture_pipeline_output(schema.clone(), defaults.clone(), None);
    let prepared = capture_pipeline_output(schema, defaults, Some(bundle));

    assert_eq!(runtime, prepared);
}

#[test]
fn schema_pipeline_handles_recursive_schema_without_stack_overflow() {
    let runtime = capture_pipeline_output(
        recursive_schema_value(),
        Value::Object(serde_json::Map::new()),
        None,
    );

    let ui_roots = runtime["ui_ast"]["roots"]
        .as_array()
        .expect("ui_ast roots array");
    assert_eq!(ui_roots.len(), 1);
    assert_eq!(ui_roots[0]["pointer"], json!("/tree"));

    let tree_children = runtime["ui_ast"]["roots"][0]["kind"]["children"]
        .as_array()
        .expect("tree child nodes");
    let child_pointers: Vec<_> = tree_children
        .iter()
        .filter_map(|child| child["pointer"].as_str())
        .collect();
    assert!(child_pointers.contains(&"/tree/name"));
    assert!(child_pointers.contains(&"/tree/children"));

    let layout_roots = runtime["layout"]["roots"].as_array().expect("layout roots");
    assert_eq!(layout_roots.len(), 1);
    let field_pointers = layout_roots[0]["sections"][0]["field_pointers"]
        .as_array()
        .expect("layout field pointers");
    let field_pointers: Vec<_> = field_pointers
        .iter()
        .filter_map(|value| value.as_str())
        .collect();
    assert!(field_pointers.contains(&"/tree/name"));
    assert!(field_pointers.contains(&"/tree/children"));
}
