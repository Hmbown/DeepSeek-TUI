use anyhow::Result;
use serde_json::{Value, json};

use crate::SchemaUI;
use crate::core::frontend::{Frontend, FrontendContext};
#[cfg(feature = "tui")]
use crate::precompile::build_ui_artifact_bundle;
#[cfg(feature = "tui")]
use crate::tui::app::UiOptions;

struct CaptureFrontend;

impl Frontend for CaptureFrontend {
    fn run(self, ctx: FrontendContext) -> Result<Value> {
        Ok(json!({
            "title": ctx.title,
            "description": ctx.description,
            "data": ctx.initial_data,
            "schema": ctx.schema,
        }))
    }
}

#[test]
fn schema_ui_accepts_inline_data_and_validation_schema() {
    let schema = r#"
        {
            "title": "Service Config",
            "description": "Runtime-validated configuration",
            "type": "object",
            "properties": {
                "host": { "type": "string" },
                "port": { "type": "integer" }
            }
        }
    "#;

    let result = SchemaUI::new(r#"{ "host": "127.0.0.1", "port": 8080 }"#)
        .with_schema(schema)
        .run_with_frontend(CaptureFrontend)
        .expect("inline document sources should parse at runtime");

    assert_eq!(result["title"], "Service Config");
    assert_eq!(result["description"], "Runtime-validated configuration");
    assert_eq!(result["data"], json!({ "host": "127.0.0.1", "port": 8080 }));
    assert_eq!(
        result["schema"]["properties"]["host"]["default"],
        "127.0.0.1"
    );
    assert_eq!(result["schema"]["properties"]["port"]["default"], 8080);
}

#[test]
fn schema_ui_infers_schema_from_data_when_no_validation_schema_is_provided() {
    let result = SchemaUI::new(json!({
        "enabled": true,
        "tags": ["blue", "green"]
    }))
    .run_with_frontend(CaptureFrontend)
    .expect("schema inference from config data should succeed");

    assert_eq!(
        result["data"],
        json!({
            "enabled": true,
            "tags": ["blue", "green"]
        })
    );
    assert_eq!(result["schema"]["type"], "object");
    assert_eq!(result["schema"]["properties"]["enabled"]["type"], "boolean");
    assert_eq!(result["schema"]["properties"]["tags"]["type"], "array");
}

#[test]
fn schema_ui_treats_schema_like_input_as_schema_when_no_explicit_validation_schema_exists() {
    let schema = json!({
        "title": "Schema-like input",
        "description": "Keep schema-first usage ergonomic",
        "type": "object",
        "properties": {
            "enabled": { "type": "boolean" }
        }
    });

    let result = SchemaUI::new(schema)
        .run_with_frontend(CaptureFrontend)
        .expect("schema-like documents should be treated as validation schemas");

    assert_eq!(result["title"], "Schema-like input");
    assert_eq!(result["description"], "Keep schema-first usage ergonomic");
    assert_eq!(result["data"], json!({}));
    assert_eq!(result["schema"]["properties"]["enabled"]["type"], "boolean");
}

#[test]
fn schema_ui_description_override_wins_over_schema_description() {
    let schema = json!({
        "title": "Service Config",
        "description": "Schema description",
        "type": "object",
        "properties": {
            "enabled": { "type": "boolean" }
        }
    });

    let result = SchemaUI::from_schema(schema)
        .with_title("CLI override")
        .with_description("Manual description")
        .run_with_frontend(CaptureFrontend)
        .expect("description overrides should flow into frontend context");

    assert_eq!(result["title"], "CLI override");
    assert_eq!(result["description"], "Manual description");
}

#[cfg(feature = "tui")]
#[test]
fn schema_ui_builds_tui_frontend_from_frontend_options_payload() {
    let schema = json!({
        "title": "Service Config",
        "type": "object",
        "properties": {
            "enabled": { "type": "boolean" }
        }
    });
    let defaults = json!({ "enabled": true });
    let bundle = build_ui_artifact_bundle(&schema, Some(&defaults))
        .expect("artifact bundle should build for TUI frontend preparation");
    let options = UiOptions::default()
        .with_auto_validate(false)
        .with_confirm_exit(false);

    let frontend = SchemaUI::new(defaults)
        .with_schema(schema)
        .with_ui_artifact_bundle(bundle.clone())
        .build_tui_frontend(options.clone());

    assert!(!frontend.options.auto_validate);
    assert!(!frontend.options.confirm_exit);
    assert_eq!(frontend.tui_artifacts, Some(bundle.tui));
}

#[test]
fn nullable_scalar_schema_marks_field_nullable_and_preserves_null_default() {
    let schema = json!({
        "type": "object",
        "properties": {
            "data-size": {
                "type": ["integer", "null"],
                "default": null
            },
            "bind-address-v6": {
                "type": ["string", "null"],
                "format": "ipv6",
                "default": null
            }
        }
    });

    let result = SchemaUI::from_schema(schema)
        .run_with_frontend(CaptureFrontend)
        .expect("nullable schema should build frontend context");

    let data = &result["schema"]["properties"]["data-size"];
    assert_eq!(data["default"], Value::Null);
    assert_eq!(data["type"], json!(["integer", "null"]));
}
