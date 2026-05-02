use std::{fs, path::Path};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::io::input::{parse_document_str, schema_with_defaults};
use crate::io::{DocumentFormat, DocumentFormatProbe};
#[cfg(feature = "tui")]
use crate::tui::model::{FormSchema, form_schema_from_ui_ast};
#[cfg(feature = "tui")]
use crate::tui::state::LayoutNavModel;
use crate::ui_ast::{UiAst, UiAstBundle, build_ui_ast_bundle};

pub mod defaults;
pub mod layout;

#[cfg(feature = "tui")]
pub mod tui;

#[cfg(feature = "web")]
pub mod web;

pub const UI_ARTIFACT_BUNDLE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiArtifactFingerprint {
    pub schema_sha256: String,
    pub defaults_sha256: String,
    pub input_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg(feature = "tui")]
pub struct TuiArtifacts {
    pub form_schema: FormSchema,
    pub layout_nav: LayoutNavModel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiArtifactBundle {
    pub artifact_version: u32,
    pub fingerprint: UiArtifactFingerprint,
    pub ui: UiAstBundle,
    #[cfg(feature = "tui")]
    pub tui: TuiArtifacts,
}

pub fn build_ui_artifact_bundle(
    schema: &Value,
    defaults: Option<&Value>,
) -> Result<UiArtifactBundle> {
    let defaults = defaults
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let enriched = schema_with_defaults(schema, &defaults);
    let ui = build_ui_ast_bundle(&enriched)?;
    let fingerprint = UiArtifactFingerprint {
        schema_sha256: sha256_hex(&stable_value_bytes(schema)?),
        defaults_sha256: sha256_hex(&stable_value_bytes(&defaults)?),
        input_sha256: sha256_hex(&stable_value_bytes(&Value::Object(Map::from_iter([
            ("schema".to_string(), stable_value(schema)),
            ("defaults".to_string(), stable_value(&defaults)),
        ])))?),
    };
    #[cfg(feature = "tui")]
    let tui = TuiArtifacts {
        form_schema: form_schema_from_ui_ast(&ui.ui_ast),
        layout_nav: LayoutNavModel::from_uilayout(&ui.layout),
    };

    Ok(UiArtifactBundle {
        artifact_version: UI_ARTIFACT_BUNDLE_VERSION,
        fingerprint,
        ui,
        #[cfg(feature = "tui")]
        tui,
    })
}

pub fn build_ui_artifact_bundle_from_file(
    schema_path: &Path,
    schema_format: DocumentFormat,
    defaults_path: Option<&Path>,
) -> Result<UiArtifactBundle> {
    let schema = read_document_file(schema_path, schema_format)?;
    let defaults = if let Some(path) = defaults_path {
        let format = resolve_file_format(path, schema_format, "defaults file")?;
        Some(read_document_file(path, format)?)
    } else {
        None
    };
    build_ui_artifact_bundle(&schema, defaults.as_ref())
}

/// Read a schema file, parse it as JSON/YAML/TOML, and build a UiAst.
pub fn build_ui_ast_from_file(path: &Path, format: DocumentFormat) -> Result<UiAst> {
    Ok(build_ui_ast_bundle_from_file(path, format)?.ui_ast)
}

/// Read a schema file, parse it as JSON/YAML/TOML, and build a shared UI
/// artifact bundle.
pub fn build_ui_ast_bundle_from_file(path: &Path, format: DocumentFormat) -> Result<UiAstBundle> {
    let schema = read_document_file(path, format)?;
    // For compile-time we typically do not apply data-driven defaults.
    build_ui_ast_bundle(&schema)
}

/// Serialize a UiAst value to pretty-printed JSON.
pub fn ui_ast_to_json(ast: &UiAst) -> Result<String> {
    Ok(serde_json::to_string_pretty(ast)?)
}

/// Generate a Rust module under OUT_DIR that exposes a UiAst JSON constant.
///
/// The generated module will contain:
/// `pub const <const_name>: &str = r#"<UiAst-json>"#;`
pub fn generate_ui_ast_rust_module(
    schema_path: &Path,
    format: DocumentFormat,
    out_module_path: &Path,
    const_name: &str,
) -> Result<()> {
    let ast = build_ui_ast_from_file(schema_path, format)?;
    let json = ui_ast_to_json(&ast)?;
    let src = format!(
        "pub const {name}: &str = r#\"{json}\"#;\n",
        name = const_name,
        json = json,
    );
    fs::write(out_module_path, src)?;
    Ok(())
}

/// Decode a UiAst from a JSON string produced by `ui_ast_to_json`.
pub fn decode_ui_ast(json: &str) -> Result<UiAst> {
    Ok(serde_json::from_str(json)?)
}

pub fn ui_ast_bundle_to_json(bundle: &UiAstBundle) -> Result<String> {
    Ok(serde_json::to_string_pretty(bundle)?)
}

pub fn decode_ui_ast_bundle(json: &str) -> Result<UiAstBundle> {
    Ok(serde_json::from_str(json)?)
}

pub fn ui_artifact_bundle_to_json(bundle: &UiArtifactBundle) -> Result<String> {
    Ok(serde_json::to_string_pretty(bundle)?)
}

pub fn decode_ui_artifact_bundle(json: &str) -> Result<UiArtifactBundle> {
    Ok(serde_json::from_str(json)?)
}

/// Generate a Rust module under OUT_DIR that exposes a constructor for
/// `UiArtifactBundle` built from the given schema and optional defaults.
pub fn generate_ui_artifact_bundle_module(
    schema_path: &Path,
    format: DocumentFormat,
    defaults_path: Option<&Path>,
    out_module_path: &Path,
    fn_name: &str,
) -> Result<()> {
    let bundle = build_ui_artifact_bundle_from_file(schema_path, format, defaults_path)?;
    let json = ui_artifact_bundle_to_json(&bundle)?;
    let src = format!(
        "pub fn {fn_name}() -> schemaui::precompile::UiArtifactBundle {{\n    serde_json::from_str::<schemaui::precompile::UiArtifactBundle>(r#\"{json}\"#).expect(\"invalid UI artifact bundle JSON\")\n}}\n",
    );
    fs::write(out_module_path, src)?;
    Ok(())
}

fn read_document_file(path: &Path, format: DocumentFormat) -> Result<Value> {
    let contents = fs::read_to_string(path)?;
    let schema: Value = parse_document_str(&contents, format)?;
    Ok(schema)
}

pub(crate) fn resolve_file_format(
    path: &Path,
    fallback: DocumentFormat,
    label: &str,
) -> Result<DocumentFormat> {
    match DocumentFormat::probe_extension(path) {
        DocumentFormatProbe::Known(format) => Ok(format),
        DocumentFormatProbe::UnsupportedFeature {
            format_name,
            feature_flag,
        } => Err(anyhow!(
            "{label} {} requires {format_name} support, but this build lacks the '{feature_flag}' feature",
            path.display()
        )),
        DocumentFormatProbe::Unknown => Ok(fallback),
    }
}

fn stable_value_bytes(value: &Value) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&stable_value(value))?)
}

fn stable_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));

            let mut normalized = Map::new();
            for (key, value) in entries {
                normalized.insert(key.clone(), stable_value(value));
            }
            Value::Object(normalized)
        }
        Value::Array(values) => Value::Array(values.iter().map(stable_value).collect()),
        _ => value.clone(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}
