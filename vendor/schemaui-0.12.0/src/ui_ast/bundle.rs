use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::layout::{UiLayout, build_ui_layout};
use super::{UiAst, build_ui_ast};

/// Canonical UI artifacts derived from a schema.
///
/// `UiAst` remains the source-of-truth IR, while `UiLayout` is the shared,
/// layout-oriented view consumed by both TUI and Web frontends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiAstBundle {
    pub ui_ast: UiAst,
    pub layout: UiLayout,
}

impl UiAstBundle {
    pub fn from_ui_ast(ui_ast: UiAst) -> Self {
        let layout = build_ui_layout(&ui_ast);
        Self { ui_ast, layout }
    }

    pub fn into_parts(self) -> (UiAst, UiLayout) {
        (self.ui_ast, self.layout)
    }
}

pub fn build_ui_ast_bundle(schema: &Value) -> Result<UiAstBundle> {
    let ui_ast = build_ui_ast(schema)?;
    Ok(UiAstBundle::from_ui_ast(ui_ast))
}
