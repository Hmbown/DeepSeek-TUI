#![allow(clippy::doc_overindented_list_items)]
#![doc = include_str!("../README.md")]

mod core;
#[cfg(feature = "tui")]
pub(crate) mod deepseek_palette;
pub mod io;
// #[cfg(feature = "precompile")]
pub mod precompile;
mod schema;
#[cfg(feature = "tui")]
pub mod tui;
pub mod ui_ast;
#[cfg(feature = "web")]
pub mod web;

#[cfg(test)]
pub(crate) mod tests;

pub use core::frontend::FrontendOptions;
pub use core::schema_ui::{DocumentInput, SchemaUI};
pub use io::{
    DocumentFormat, DocumentFormatProbe,
    input::{
        looks_like_json_schema, parse_document_auto, parse_document_str, schema_from_data_str,
        schema_from_data_value, schema_with_defaults,
    },
    output::{OutputDestination, OutputOptions},
};
#[cfg(feature = "tui")]
pub use precompile::TuiArtifacts;
pub use precompile::UiArtifactBundle;
#[cfg(feature = "tui")]
pub use tui::{
    app::options::UiOptions,
    model::FormSchema,
    session::TuiFrontend,
    state::LayoutNavModel,
    view::{CompositeOverlay, PopupRender, UiContext, draw},
};
#[cfg(feature = "web")]
pub use web::{frontend::WebFrontend, session::ServeOptions};
pub mod prelude {
    pub use super::DocumentInput;
    pub use super::FrontendOptions;
    pub use super::SchemaUI;
    #[cfg(feature = "tui")]
    pub use super::TuiFrontend;
    #[cfg(feature = "tui")]
    pub use super::UiOptions;
    #[cfg(feature = "tui")]
    pub use super::draw;
    #[cfg(feature = "tui")]
    pub use super::tui::view::{CompositeOverlay, PopupRender, UiContext};

    #[cfg(feature = "web")]
    pub use super::ServeOptions;
    #[cfg(feature = "web")]
    pub use super::WebFrontend;
}
