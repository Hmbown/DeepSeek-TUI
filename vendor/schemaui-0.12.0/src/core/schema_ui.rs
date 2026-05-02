use anyhow::Result;
use serde_json::Value;

use crate::core::frontend::{Frontend, FrontendOptions};
use crate::core::pipeline::SchemaPipeline;
use crate::io::{
    self, DocumentFormat,
    input::{looks_like_json_schema, parse_document_auto},
};

#[cfg(feature = "tui")]
use crate::precompile::TuiArtifacts;
use crate::precompile::UiArtifactBundle;
#[cfg(feature = "tui")]
use crate::tui::app::UiOptions;
#[cfg(feature = "tui")]
use crate::tui::session::TuiFrontend;
use crate::ui_ast::{UiAst, UiAstBundle};
#[cfg(feature = "web")]
use crate::web::{frontend::WebFrontend, session::ServeOptions as WebServeOptions};

#[derive(Debug, Clone)]
pub enum DocumentInput {
    Value(Value),
    Text(String),
}

impl DocumentInput {
    pub fn into_value(self) -> Result<Value> {
        match self {
            Self::Value(value) => Ok(value),
            Self::Text(contents) => parse_document_auto(&contents),
        }
    }
}

impl From<Value> for DocumentInput {
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}

impl From<&Value> for DocumentInput {
    fn from(value: &Value) -> Self {
        Self::Value(value.clone())
    }
}

impl From<String> for DocumentInput {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for DocumentInput {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

#[derive(Debug)]
struct ResolvedRuntimeInputs {
    schema: Value,
    defaults: Option<Value>,
}

#[derive(Debug)]
pub struct SchemaUI {
    document: Option<DocumentInput>,
    validation_schema: Option<DocumentInput>,
    title: Option<String>,
    description: Option<String>,
    ui_ast: Option<UiAst>,
    ui_bundle: Option<UiAstBundle>,
    ui_artifact_bundle: Option<UiArtifactBundle>,
    #[cfg(feature = "tui")]
    tui_artifacts: Option<TuiArtifacts>,
}

impl SchemaUI {
    pub fn new(document: impl Into<DocumentInput>) -> Self {
        Self {
            document: Some(document.into()),
            validation_schema: None,
            title: None,
            description: None,
            ui_ast: None,
            ui_bundle: None,
            ui_artifact_bundle: None,
            #[cfg(feature = "tui")]
            tui_artifacts: None,
        }
    }

    pub fn from_schema(schema: impl Into<DocumentInput>) -> Self {
        let mut ui = Self::new(Value::Object(Default::default()));
        ui.document = None;
        ui.validation_schema = Some(schema.into());
        ui
    }

    pub fn from_schema_str(contents: &str, format: DocumentFormat) -> Result<Self> {
        let schema = io::input::parse_document_str(contents, format)?;
        Ok(Self::from_schema(schema))
    }

    pub fn from_data_value(value: Value) -> Self {
        Self::new(value)
    }

    pub fn from_data_str(contents: &str, format: DocumentFormat) -> Result<Self> {
        let value = io::input::parse_document_str(contents, format)?;
        Ok(Self::new(value))
    }

    pub fn from_schema_and_data(schema: Value, defaults: Value) -> Self {
        Self::new(defaults).with_schema(schema)
    }

    /// Provide a prepared UiAst so the runtime pipeline can skip rebuilding it
    /// from the schema.
    pub fn with_ui_ast(mut self, ast: UiAst) -> Self {
        self.ui_ast = Some(ast);
        self
    }

    /// Provide a prepared bundle containing shared UI artifacts.
    pub fn with_ui_bundle(mut self, bundle: UiAstBundle) -> Self {
        self.ui_bundle = Some(bundle);
        self
    }

    /// Provide a prepared UI artifact bundle, including shared UI and
    /// frontend-specific derived structures.
    pub fn with_ui_artifact_bundle(mut self, bundle: UiArtifactBundle) -> Self {
        self.ui_artifact_bundle = Some(bundle);
        self
    }

    /// Provide prepared TUI artifacts. When set, the TUI frontend will use
    /// these instead of deriving TUI-specific structures from the UiAst at
    /// runtime.
    #[cfg(feature = "tui")]
    pub fn with_tui_artifacts(mut self, artifacts: TuiArtifacts) -> Self {
        self.tui_artifacts = Some(artifacts);
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_schema(mut self, schema: impl Into<DocumentInput>) -> Self {
        self.validation_schema = Some(schema.into());
        self
    }

    pub fn with_data(mut self, document: impl Into<DocumentInput>) -> Self {
        self.document = Some(document.into());
        self
    }

    pub fn with_default_data(mut self, defaults: &Value) -> Self {
        self.document = Some(DocumentInput::Value(defaults.clone()));
        self
    }

    fn resolve_inputs(&self) -> Result<ResolvedRuntimeInputs> {
        let document = self
            .document
            .clone()
            .map(DocumentInput::into_value)
            .transpose()?;
        let validation_schema = self
            .validation_schema
            .clone()
            .map(DocumentInput::into_value)
            .transpose()?;

        if let Some(schema) = validation_schema {
            return Ok(ResolvedRuntimeInputs {
                schema,
                defaults: document,
            });
        }

        let Some(document) = document else {
            return Ok(ResolvedRuntimeInputs {
                schema: Value::Object(Default::default()),
                defaults: None,
            });
        };

        if looks_like_json_schema(&document) {
            Ok(ResolvedRuntimeInputs {
                schema: document,
                defaults: None,
            })
        } else {
            Ok(ResolvedRuntimeInputs {
                schema: io::input::schema_from_data_value(&document),
                defaults: Some(document),
            })
        }
    }

    #[cfg(feature = "tui")]
    pub(crate) fn build_tui_frontend(&self, options: UiOptions) -> TuiFrontend {
        let tui_artifacts = self.tui_artifacts.clone().or_else(|| {
            self.ui_artifact_bundle
                .as_ref()
                .map(|bundle| bundle.tui.clone())
        });
        TuiFrontend {
            options,
            tui_artifacts,
        }
    }

    pub fn run(self, frontend: FrontendOptions) -> Result<Value> {
        match frontend {
            #[cfg(feature = "tui")]
            FrontendOptions::Tui(options) => {
                let frontend = self.build_tui_frontend(options);
                self.run_with_frontend(frontend)
            }
            #[cfg(feature = "web")]
            FrontendOptions::Web(serve) => self.run_web(serve),
        }
    }

    /// Run explicitly in TUI mode using default `UiOptions`.
    #[cfg(feature = "tui")]
    pub fn run_tui(self) -> Result<Value> {
        self.run(FrontendOptions::Tui(UiOptions::default()))
    }

    /// Run in Web mode, using the given serve options to configure the
    /// temporary HTTP server.
    #[cfg(feature = "web")]
    pub fn run_web(self, serve: WebServeOptions) -> Result<Value> {
        self.run_with_frontend(WebFrontend { serve })
    }

    /// Run in Web mode from an existing async runtime.
    #[cfg(feature = "web")]
    pub async fn run_web_async(self, serve: WebServeOptions) -> Result<Value> {
        let ctx = self.into_frontend_context()?;
        WebFrontend { serve }.run_async(ctx).await
    }

    pub fn run_with_frontend<F>(self, frontend: F) -> Result<Value>
    where
        F: Frontend,
    {
        let ctx = self.into_frontend_context()?;
        frontend.run(ctx)
    }

    fn into_frontend_context(self) -> Result<crate::core::frontend::FrontendContext> {
        let inputs = self.resolve_inputs()?;
        let SchemaUI {
            document: _,
            validation_schema: _,
            title,
            description,
            ui_ast,
            ui_bundle,
            ui_artifact_bundle,
            #[cfg(feature = "tui")]
                tui_artifacts: _,
        } = self;

        let ui_bundle = ui_artifact_bundle.map(|bundle| bundle.ui).or(ui_bundle);
        let pipeline = SchemaPipeline::new(inputs.schema)
            .with_title(title)
            .with_description(description)
            .with_defaults(inputs.defaults)
            .with_prepared_ui_ast(ui_ast)
            .with_prepared_ui_bundle(ui_bundle);
        pipeline.build_frontend_context()
    }
}
