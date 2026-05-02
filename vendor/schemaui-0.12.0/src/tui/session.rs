use anyhow::Result;
use serde_json::Value;

use crate::core::frontend::{Frontend, FrontendContext};
use crate::precompile::TuiArtifacts;
use crate::tui::app::{App, UiOptions};
use crate::tui::model::form_schema_from_ui_ast;
use crate::tui::state::{FormState, LayoutNavModel};
use crate::ui_ast::{UiAst, UiLayout};

/// TUI frontend implementation that consumes a prepared `FrontendContext`
/// and runs the interactive terminal UI.
#[derive(Debug)]
pub struct TuiFrontend {
    pub options: UiOptions,
    pub tui_artifacts: Option<TuiArtifacts>,
}

pub(crate) fn resolve_tui_artifacts(
    ui_ast: &UiAst,
    layout: &UiLayout,
    tui_artifacts: Option<TuiArtifacts>,
) -> TuiArtifacts {
    tui_artifacts.unwrap_or_else(|| TuiArtifacts {
        form_schema: form_schema_from_ui_ast(ui_ast),
        layout_nav: LayoutNavModel::from_uilayout(layout),
    })
}

impl Frontend for TuiFrontend {
    fn run(self, ctx: FrontendContext) -> Result<Value> {
        let TuiFrontend {
            options,
            tui_artifacts,
        } = self;

        let FrontendContext {
            title,
            description: _,
            ui_ast,
            layout,
            initial_data: _,
            schema: _,
            validator,
        } = ctx;

        let resolved = resolve_tui_artifacts(&ui_ast, &layout, tui_artifacts);

        let palette = options.component_palette();
        let mut form_state = FormState::from_schema_with_palette(&resolved.form_schema, palette);
        form_state.set_layout_nav(resolved.layout_nav);

        let mut app = App::new(form_state, validator, options);
        app.set_session_title(title);
        let result = app.run()?;
        Ok(result)
    }
}
