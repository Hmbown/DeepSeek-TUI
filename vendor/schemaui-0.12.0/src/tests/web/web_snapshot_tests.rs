use std::{fs, path::PathBuf};

use crate::io::DocumentFormat;
use crate::io::input::parse_document_str;
use crate::precompile::build_ui_artifact_bundle;
use crate::precompile::web;
use crate::web::session::SessionResponse;
use crate::web::session::WebSessionBuilder;
use serde_json::{Value, json};

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("schemas")
        .join("test-comprehensive.schema.json")
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

fn opaque_object_schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "Opaque Object Schema",
        "type": "object",
        "properties": {
            "permissions": {
                "allOf": [
                    { "$ref": "#/definitions/PermissionsToml" }
                ],
                "description": "Named permission profiles."
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
    })
}

fn issue72_schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "My title",
        "description": "My description",
        "type": "object",
        "properties": {
            "EnumInteger": {
                "title": "integer title",
                "description": "Select one of a few integers",
                "enum": [2, 5]
            },
            "comment": {
                "type": "string",
                "title": "Comment title",
                "description": "Comment description"
            }
        },
        "propertyNames": true,
        "required": [],
        "additionalProperties": false
    })
}

#[test]
fn web_session_builder_accepts_opaque_object_fields() {
    let snapshot = WebSessionBuilder::new(opaque_object_schema())
        .with_initial_data(json!({
            "features": {
                "apps": true
            }
        }))
        .build()
        .expect("web session config")
        .session_response();

    assert_eq!(snapshot.title.as_deref(), Some("Opaque Object Schema"));
    assert_eq!(snapshot.data, json!({ "features": { "apps": true } }));
    assert!(
        snapshot
            .ui_ast
            .roots
            .iter()
            .any(|node| node.pointer == "/permissions"),
        "opaque object fields should remain representable in the web session UI AST",
    );
}

#[test]
fn build_session_snapshot_for_comprehensive_schema_succeeds() {
    let path = schema_path();

    let snapshot = web::build_session_snapshot_from_files(&path, DocumentFormat::Json, None)
        .expect("session snapshot");

    assert!(!snapshot.formats.is_empty());
    assert!(!snapshot.ui_ast.roots.is_empty());
}

#[test]
fn web_session_builder_uses_root_schema_metadata_for_header() {
    let snapshot = WebSessionBuilder::new(issue72_schema())
        .build()
        .expect("web session config")
        .session_response();

    assert_eq!(snapshot.title.as_deref(), Some("My title"));
    assert_eq!(snapshot.description.as_deref(), Some("My description"));
}

#[test]
fn web_session_builder_with_title_overrides_only_header_title() {
    let snapshot = WebSessionBuilder::new(issue72_schema())
        .with_title("CLI override")
        .build()
        .expect("web session config")
        .session_response();

    assert_eq!(snapshot.title.as_deref(), Some("CLI override"));
    assert_eq!(snapshot.description.as_deref(), Some("My description"));
}

#[test]
fn web_snapshot_builder_uses_root_schema_metadata_for_header() {
    let mut schema_path = std::env::temp_dir();
    schema_path.push(format!(
        "schemaui_issue72_{}_schema.json",
        std::process::id()
    ));
    fs::write(
        &schema_path,
        serde_json::to_vec_pretty(&issue72_schema()).expect("serialize schema"),
    )
    .expect("write schema file");

    let snapshot = web::build_session_snapshot_from_files(&schema_path, DocumentFormat::Json, None)
        .expect("session snapshot");

    assert_eq!(snapshot.title.as_deref(), Some("My title"));
    assert_eq!(snapshot.description.as_deref(), Some("My description"));

    let _ = fs::remove_file(&schema_path);
}

#[test]
fn json_snapshot_roundtrip_via_file_is_deserializable() {
    let path = schema_path();

    let snapshot = web::build_session_snapshot_from_files(&path, DocumentFormat::Json, None)
        .expect("session snapshot");

    // Write JSON snapshot to a temp-ish file and ensure we can read it back
    // into a SessionResponse. This mirrors what the web frontend expects when
    // consuming the snapshot as raw JSON.
    let mut out_path = std::env::temp_dir();
    out_path.push(format!(
        "schemaui_web_snapshot_{}_json.json",
        std::process::id()
    ));

    web::write_session_snapshot_json(&snapshot, &out_path).expect("write JSON snapshot");

    let contents = std::fs::read_to_string(&out_path).expect("read JSON snapshot");
    let decoded: SessionResponse =
        serde_json::from_str(&contents).expect("deserialize JSON snapshot into SessionResponse");

    // Basic structural sanity checks
    assert_eq!(decoded.formats, snapshot.formats);
    assert!(!decoded.ui_ast.roots.is_empty());
    assert!(decoded.layout.is_some());

    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn ts_snapshot_module_has_expected_export_shape() {
    let path = schema_path();

    let snapshot = web::build_session_snapshot_from_files(&path, DocumentFormat::Json, None)
        .expect("session snapshot");

    let mut out_path = std::env::temp_dir();
    out_path.push(format!(
        "schemaui_web_snapshot_{}_module.ts",
        std::process::id()
    ));

    let export_name = "SessionSnapshot";
    web::write_session_snapshot_ts_module(&snapshot, &out_path, export_name)
        .expect("write TS snapshot module");

    let src = std::fs::read_to_string(&out_path).expect("read TS snapshot module");

    // We don't run tsc here (that belongs in the web/ui repo), but we do check
    // that the generated module has the expected import and export signature.
    assert!(
        src.contains("import type { SessionResponse } from '@schemaui/types/SessionResponse';",)
    );
    assert!(src.contains("export const SessionSnapshot: SessionResponse ="));

    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn web_session_builder_with_ui_artifact_bundle_matches_snapshot_builder() {
    let path = schema_path();
    let schema_raw = fs::read_to_string(&path).expect("schema file readable");
    let schema_value: Value =
        parse_document_str(&schema_raw, DocumentFormat::Json).expect("schema parses");
    let defaults = defaults_value();
    let bundle =
        build_ui_artifact_bundle(&schema_value, Some(&defaults)).expect("build UI artifact bundle");

    let config = WebSessionBuilder::new(schema_value)
        .with_initial_data(defaults.clone())
        .with_ui_artifact_bundle(bundle)
        .build()
        .expect("web session config");
    let runtime_snapshot = config.session_response();

    let mut defaults_path = std::env::temp_dir();
    defaults_path.push(format!(
        "schemaui_web_defaults_{}_snapshot.json",
        std::process::id()
    ));
    fs::write(
        &defaults_path,
        serde_json::to_vec_pretty(&defaults).expect("serialize defaults"),
    )
    .expect("write defaults file");

    let snapshot_from_builder =
        web::build_session_snapshot_from_files(&path, DocumentFormat::Json, Some(&defaults_path))
            .expect("build web snapshot from files");

    assert_eq!(runtime_snapshot, snapshot_from_builder);

    let _ = fs::remove_file(&defaults_path);
}
