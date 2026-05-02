use anyhow::Result;
use jsonschema::Validator;
use serde_json::Value;

use crate::core::ui_ast::{UiAst, UiLayout};
#[cfg(feature = "tui")]
use crate::tui::app::UiOptions;
#[cfg(feature = "web")]
use crate::web::session::ServeOptions;

/// Shared context prepared by the core pipeline and consumed by frontends
/// (TUI, Web, or others).
#[derive(Debug)]
pub struct FrontendContext {
    pub title: Option<String>,
    pub description: Option<String>,
    pub ui_ast: UiAst,
    pub layout: UiLayout,
    pub initial_data: Value,
    pub schema: Value,
    pub validator: Validator,
}

/// Built-in runtime targets exposed by the high-level `SchemaUI` API.
#[derive(Debug, Clone)]
pub enum FrontendOptions {
    #[cfg(feature = "tui")]
    Tui(UiOptions),
    #[cfg(feature = "web")]
    Web(ServeOptions),
}

/// Pluggable frontend interface. A frontend receives a `FrontendContext`,
/// renders an interactive UI, and returns the final edited value.
pub trait Frontend {
    fn run(self, ctx: FrontendContext) -> Result<Value>;
}
