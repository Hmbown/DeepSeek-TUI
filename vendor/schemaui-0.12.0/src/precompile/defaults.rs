use std::{fs, path::Path};

use anyhow::Result;
use serde_json::Value;

use crate::io::{DocumentFormat, input::parse_document_str};
use crate::ui_ast::build_ui_ast;
use crate::ui_ast::defaults::{DefaultIndex, collect_defaults};

/// Build a DefaultIndex mapping pointers to default values from a schema file.
///
/// This mirrors the backend UiAst default semantics and is suitable for
/// offline analysis or code generation in build scripts and CI jobs.
pub fn collect_defaults_from_file(path: &Path, format: DocumentFormat) -> Result<DefaultIndex> {
    let contents = fs::read_to_string(path)?;
    let schema: Value = parse_document_str(&contents, format)?;
    let ast = build_ui_ast(&schema)?;
    Ok(collect_defaults(&ast))
}
