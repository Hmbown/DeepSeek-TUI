use std::path::Path;

use anyhow::Result;

use crate::io::DocumentFormat;
use crate::precompile::build_ui_ast_bundle_from_file;
use crate::ui_ast::UiLayout;

/// Build a UiLayout tree from a schema file.
///
/// This is a pure helper intended for use in build scripts or external
/// codegen tools. It relies only on the canonical UiAst visitor and the
/// internal ui_ast::layout representation.
pub fn build_ui_layout_from_file(path: &Path, format: DocumentFormat) -> Result<UiLayout> {
    Ok(build_ui_ast_bundle_from_file(path, format)?.layout)
}
