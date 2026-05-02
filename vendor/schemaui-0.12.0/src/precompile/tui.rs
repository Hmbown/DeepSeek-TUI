use std::{fs, path::Path};

use anyhow::Result;
use serde_json::Value;

use crate::io::DocumentFormat;
use crate::precompile::{
    TuiArtifacts, build_ui_artifact_bundle, build_ui_artifact_bundle_from_file, resolve_file_format,
};
use crate::tui::model::FormSchema;
use crate::tui::state::LayoutNavModel;
use crate::ui_ast::UiAst;

/// Build UiAst and TUI artifacts from schema/default values.
pub fn build_tui_artifacts(
    schema: &Value,
    defaults: Option<&Value>,
) -> Result<(UiAst, TuiArtifacts)> {
    let bundle = build_ui_artifact_bundle(schema, defaults)?;
    Ok((bundle.ui.ui_ast, bundle.tui))
}

/// Build UiAst and TUI artifacts from a schema file.
///
/// This is a pure helper intended for use in build scripts or external
/// codegen tools. It does not depend on any runtime state.
pub fn build_tui_artifacts_from_file(
    path: &Path,
    format: DocumentFormat,
    defaults_path: Option<&Path>,
) -> Result<(UiAst, TuiArtifacts)> {
    let bundle = build_ui_artifact_bundle_from_file(path, format, defaults_path)?;
    Ok((bundle.ui.ui_ast, bundle.tui))
}

/// Build UiAst and TUI FormSchema from schema/default values.
pub fn build_tui_form_schema(
    schema: &Value,
    defaults: Option<&Value>,
) -> Result<(UiAst, FormSchema)> {
    let (ui_ast, tui) = build_tui_artifacts(schema, defaults)?;
    Ok((ui_ast, tui.form_schema))
}

/// Build UiAst and TUI FormSchema from a schema file plus optional defaults.
pub fn build_tui_form_schema_from_file(
    path: &Path,
    format: DocumentFormat,
    defaults_path: Option<&Path>,
) -> Result<(UiAst, FormSchema)> {
    let (ui_ast, tui) = build_tui_artifacts_from_file(path, format, defaults_path)?;
    Ok((ui_ast, tui.form_schema))
}

/// Build UiAst and a TUI LayoutNavModel from schema/default values.
pub fn build_tui_layout_nav(
    schema: &Value,
    defaults: Option<&Value>,
) -> Result<(UiAst, LayoutNavModel)> {
    let (ui_ast, tui) = build_tui_artifacts(schema, defaults)?;
    Ok((ui_ast, tui.layout_nav))
}

/// Build UiAst and a TUI LayoutNavModel from a schema file plus optional
/// defaults.
pub fn build_tui_layout_nav_from_file(
    path: &Path,
    format: DocumentFormat,
    defaults_path: Option<&Path>,
) -> Result<(UiAst, LayoutNavModel)> {
    let (ui_ast, tui) = build_tui_artifacts_from_file(path, format, defaults_path)?;
    Ok((ui_ast, tui.layout_nav))
}

/// Generate a Rust module under OUT_DIR that exposes a constructor for
/// `TuiArtifacts` built from the given schema and optional defaults values.
pub fn generate_tui_artifacts_module_from_value(
    schema: &Value,
    defaults: Option<&Value>,
    out_module_path: &Path,
    fn_name: &str,
) -> Result<()> {
    let (_ast, artifacts) = build_tui_artifacts(schema, defaults)?;
    let json = serde_json::to_string_pretty(&artifacts)?;
    let src = format!(
        "pub fn {fn_name}() -> schemaui::TuiArtifacts {{\n    serde_json::from_str::<schemaui::TuiArtifacts>(r#\"{json}\"#).expect(\"invalid TUI artifacts JSON\")\n}}\n",
    );
    fs::write(out_module_path, src)?;
    Ok(())
}

/// Generate a Rust module under OUT_DIR that exposes a constructor for
/// `TuiArtifacts` built from the given schema and optional defaults.
pub fn generate_tui_artifacts_module(
    schema_path: &Path,
    format: DocumentFormat,
    defaults_path: Option<&Path>,
    out_module_path: &Path,
    fn_name: &str,
) -> Result<()> {
    let (schema, defaults) = load_schema_and_defaults(schema_path, format, defaults_path)?;
    generate_tui_artifacts_module_from_value(&schema, defaults.as_ref(), out_module_path, fn_name)
}

/// Generate a Rust module under OUT_DIR that exposes a constructor for
/// `FormSchema` built from the given schema and optional defaults values.
pub fn generate_tui_form_schema_module_from_value(
    schema: &Value,
    defaults: Option<&Value>,
    out_module_path: &Path,
    fn_name: &str,
) -> Result<()> {
    let (_ast, form_schema) = build_tui_form_schema(schema, defaults)?;
    let json = serde_json::to_string_pretty(&form_schema)?;
    let src = format!(
        "pub fn {fn_name}() -> schemaui::FormSchema {{\n    serde_json::from_str::<schemaui::FormSchema>(r#\"{json}\"#).expect(\"invalid generated FormSchema JSON\")\n}}\n",
    );
    fs::write(out_module_path, src)?;
    Ok(())
}

/// Generate a Rust module under OUT_DIR that exposes a constructor for
/// `FormSchema` built from the given schema and optional defaults.
///
/// The generated module will contain a function with the given `fn_name`:
///
/// ```ignore
/// pub fn <fn_name>() -> schemaui::FormSchema { ... }
/// ```
///
/// The implementation deserializes `FormSchema` from an embedded JSON
/// representation produced at codegen time.
pub fn generate_tui_form_schema_module(
    schema_path: &Path,
    format: DocumentFormat,
    defaults_path: Option<&Path>,
    out_module_path: &Path,
    fn_name: &str,
) -> Result<()> {
    let (schema, defaults) = load_schema_and_defaults(schema_path, format, defaults_path)?;
    generate_tui_form_schema_module_from_value(&schema, defaults.as_ref(), out_module_path, fn_name)
}

/// Generate a Rust module under OUT_DIR that exposes a constructor for
/// `LayoutNavModel` built from the given schema and optional defaults values.
pub fn generate_tui_layout_nav_module_from_value(
    schema: &Value,
    defaults: Option<&Value>,
    out_module_path: &Path,
    fn_name: &str,
) -> Result<()> {
    let (_ast, layout_nav) = build_tui_layout_nav(schema, defaults)?;
    let json = serde_json::to_string_pretty(&layout_nav)?;
    let src = format!(
        "pub fn {fn_name}() -> schemaui::LayoutNavModel {{\n    serde_json::from_str::<schemaui::LayoutNavModel>(r#\"{json}\"#).expect(\"invalid generated LayoutNavModel JSON\")\n}}\n",
    );
    fs::write(out_module_path, src)?;
    Ok(())
}

/// Generate a Rust module under OUT_DIR that exposes a constructor for
/// `LayoutNavModel` built from the given schema and optional defaults.
///
/// The generated module will contain a function with the given `fn_name`:
///
/// ```ignore
/// pub fn <fn_name>() -> schemaui::LayoutNavModel { ... }
/// ```
///
/// The implementation deserializes `LayoutNavModel` from an embedded JSON
/// representation produced at codegen time.
pub fn generate_tui_layout_nav_module(
    schema_path: &Path,
    format: DocumentFormat,
    defaults_path: Option<&Path>,
    out_module_path: &Path,
    fn_name: &str,
) -> Result<()> {
    let (schema, defaults) = load_schema_and_defaults(schema_path, format, defaults_path)?;
    generate_tui_layout_nav_module_from_value(&schema, defaults.as_ref(), out_module_path, fn_name)
}

fn load_schema_and_defaults(
    schema_path: &Path,
    format: DocumentFormat,
    defaults_path: Option<&Path>,
) -> Result<(Value, Option<Value>)> {
    let schema = std::fs::read_to_string(schema_path)?;
    let schema = crate::parse_document_str(&schema, format)?;
    let defaults = if let Some(path) = defaults_path {
        let format = resolve_file_format(path, format, "defaults file")?;
        let raw = std::fs::read_to_string(path)?;
        Some(crate::parse_document_str(&raw, format)?)
    } else {
        None
    };
    Ok((schema, defaults))
}
