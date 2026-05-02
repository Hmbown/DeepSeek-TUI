use std::{
    fs,
    future::IntoFuture,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result, anyhow};
use axum::{
    Json, Router,
    body::Body,
    extract::{OriginalUri, State},
    http::{Response, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use jsonschema::{Validator, validator_for};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot},
};
#[cfg(feature = "web-types")]
use ts_rs::TS;

use crate::io::{DocumentFormat, input::schema_with_defaults};
use crate::precompile::UiArtifactBundle;
use crate::schema::metadata::root_schema_header;

use super::assets::{EmbeddedAssets, FilesystemAssets, WebAssetProvider};
use crate::ui_ast::{UiAst, UiAstBundle, UiLayout, build_ui_ast_bundle};

pub struct WebSessionBuilder {
    schema: Value,
    defaults: Option<Value>,
    title: Option<String>,
    description: Option<String>,
    asset_provider: Arc<dyn WebAssetProvider>,
    ui_bundle: Option<UiAstBundle>,
    ui_artifact_bundle: Option<UiArtifactBundle>,
}

impl WebSessionBuilder {
    pub fn new(schema: Value) -> Self {
        Self {
            schema,
            defaults: None,
            title: None,
            description: None,
            #[allow(clippy::default_constructed_unit_structs)]
            asset_provider: Arc::new(EmbeddedAssets::default()),
            ui_bundle: None,
            ui_artifact_bundle: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_initial_data(mut self, data: Value) -> Self {
        self.defaults = Some(data);
        self
    }

    pub fn with_asset_provider(mut self, provider: Arc<dyn WebAssetProvider>) -> Self {
        self.asset_provider = provider;
        self
    }

    pub fn with_filesystem_assets<P: Into<PathBuf>>(mut self, root: P) -> Self {
        self.asset_provider = Arc::new(FilesystemAssets::new(root));
        self
    }

    /// Provide a prepared UiAst for this schema.
    pub fn with_ui_ast(mut self, ast: UiAst) -> Self {
        self.ui_bundle = Some(UiAstBundle::from_ui_ast(ast));
        self
    }

    /// Provide a prepared bundle of shared UI artifacts for this schema.
    pub fn with_ui_bundle(mut self, bundle: UiAstBundle) -> Self {
        self.ui_bundle = Some(bundle);
        self
    }

    /// Provide a prepared UI artifact bundle.
    pub fn with_ui_artifact_bundle(mut self, bundle: UiArtifactBundle) -> Self {
        self.ui_artifact_bundle = Some(bundle);
        self
    }

    pub fn build(mut self) -> Result<WebSessionConfig> {
        let data = self
            .defaults
            .take()
            .unwrap_or_else(|| Value::Object(Map::new()));
        let schema = schema_with_defaults(&self.schema, &data);
        let (schema_title, description) = root_schema_header(&schema);
        let bundle = if let Some(bundle) = self.ui_artifact_bundle.take() {
            bundle.ui
        } else if let Some(bundle) = self.ui_bundle.take() {
            bundle
        } else {
            build_ui_ast_bundle(&schema)?
        };
        let (ui_ast, layout) = bundle.into_parts();
        Ok(WebSessionConfig {
            title: self.title.or(schema_title),
            description: self.description.or(description),
            ui_ast,
            layout,
            data,
            schema,
            asset_provider: self.asset_provider,
        })
    }
}

#[derive(Debug, Clone)]
pub struct WebSessionConfig {
    pub title: Option<String>,
    pub description: Option<String>,
    pub ui_ast: UiAst,
    pub layout: UiLayout,
    pub data: Value,
    pub schema: Value,
    pub asset_provider: Arc<dyn WebAssetProvider>,
}

impl WebSessionConfig {
    pub fn session_response(&self) -> SessionResponse {
        SessionResponse {
            title: self.title.clone(),
            description: self.description.clone(),
            ui_ast: self.ui_ast.clone(),
            data: self.data.clone(),
            formats: DocumentFormat::available_formats()
                .into_iter()
                .map(|format| format.to_string())
                .collect(),
            layout: Some(self.layout.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ServeOptions {
    pub host: IpAddr,
    pub port: u16,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            host: IpAddr::from([127, 0, 0, 1]),
            port: 0,
        }
    }
}

pub struct BoundSession {
    router: Router,
    handles: SessionHandles,
    listener: TcpListener,
    addr: SocketAddr,
}

impl BoundSession {
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    pub async fn run(self) -> Result<Value> {
        let (result_rx, shutdown_rx) = self.handles.into_parts();
        let server = axum::serve(self.listener, self.router.into_make_service())
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .into_future();
        let server = tokio::spawn(server);

        let value = match result_rx.await {
            Ok(value) => value,
            Err(_) => match server.await {
                Ok(Ok(())) => {
                    return Err(anyhow!("web session closed before emitting a value"));
                }
                Ok(Err(err)) => {
                    return Err(err).context("web server exited before the session finished");
                }
                Err(err) => {
                    return Err(anyhow!(err))
                        .context("web server task panicked before the session finished");
                }
            },
        };

        match server.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                return Err(err).context("web server exited before the session finished");
            }
            Err(err) => {
                return Err(anyhow!(err)).context("web server task panicked");
            }
        }
        Ok(value)
    }
}

pub async fn bind_session(config: WebSessionConfig, options: ServeOptions) -> Result<BoundSession> {
    let (router, handles) = tokio::task::spawn_blocking(move || session_router(config))
        .await
        .context("failed to build web session state")??;
    let listener = TcpListener::bind(SocketAddr::new(options.host, options.port))
        .await
        .context("failed to bind web listener")?;
    let addr = listener
        .local_addr()
        .context("failed to read bound address")?;
    Ok(BoundSession {
        router,
        handles,
        listener,
        addr,
    })
}

pub async fn serve_session(config: WebSessionConfig, options: ServeOptions) -> Result<Value> {
    bind_session(config, options).await?.run().await
}

pub fn session_router(config: WebSessionConfig) -> Result<(Router, SessionHandles)> {
    let WebSessionConfig {
        title,
        description,
        ui_ast,
        layout,
        data,
        schema,
        asset_provider,
    } = config;
    let validator = Arc::new(validator_for(&schema).context("failed to compile JSON schema")?);
    let (result_tx, result_rx) = oneshot::channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let shared = SharedState {
        title,
        description,
        ui_ast: Arc::new(ui_ast),
        layout: Arc::new(layout),
        data: Arc::new(Mutex::new(data)),
        formats: DocumentFormat::available_formats(),
        validator,
        finish_line: Arc::new(FinishLine {
            result: Mutex::new(Some(result_tx)),
            shutdown: Mutex::new(Some(shutdown_tx)),
        }),
        finished: Arc::new(AtomicBool::new(false)),
        asset_provider,
    };

    let router = Router::new()
        .route("/api/session", get(get_session))
        .route("/api/session/export", get(get_session_export))
        .route("/api/save", post(post_save))
        .route("/api/exit", post(post_exit))
        .route("/api/validate", post(post_validate))
        .route("/api/preview", post(post_preview))
        .fallback(static_assets)
        .with_state(shared);

    Ok((
        router,
        SessionHandles {
            result_rx: Some(result_rx),
            shutdown_rx: Some(shutdown_rx),
        },
    ))
}

pub struct SessionHandles {
    result_rx: Option<oneshot::Receiver<Value>>,
    shutdown_rx: Option<oneshot::Receiver<()>>,
}

impl SessionHandles {
    pub fn into_parts(mut self) -> (oneshot::Receiver<Value>, oneshot::Receiver<()>) {
        let result = self
            .result_rx
            .take()
            .expect("result receiver already consumed");
        let shutdown = self
            .shutdown_rx
            .take()
            .expect("shutdown receiver already consumed");
        (result, shutdown)
    }
}

#[derive(Clone)]
struct SharedState {
    title: Option<String>,
    description: Option<String>,
    ui_ast: Arc<UiAst>,
    layout: Arc<UiLayout>,
    data: Arc<Mutex<Value>>,
    formats: Vec<DocumentFormat>,
    validator: Arc<Validator>,
    finish_line: Arc<FinishLine>,
    finished: Arc<AtomicBool>,
    asset_provider: Arc<dyn WebAssetProvider>,
}

struct FinishLine {
    result: Mutex<Option<oneshot::Sender<Value>>>,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

impl FinishLine {
    async fn complete(&self, value: Value) {
        if let Some(tx) = self.result.lock().await.take() {
            let _ = tx.send(value);
        }
        if let Some(tx) = self.shutdown.lock().await.take() {
            let _ = tx.send(());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[cfg_attr(feature = "web-types", ts(export, export_to = "web/types/"))]
pub struct SessionResponse {
    pub title: Option<String>,
    pub description: Option<String>,
    pub ui_ast: UiAst,
    #[cfg_attr(feature = "web-types", ts(type = "Record<string, unknown>"))]
    pub data: Value,
    pub formats: Vec<String>,
    pub layout: Option<UiLayout>,
}

async fn get_session(State(state): State<SharedState>) -> impl IntoResponse {
    Json(build_and_maybe_dump_session(&state).await)
}

async fn get_session_export(State(state): State<SharedState>) -> impl IntoResponse {
    let payload = build_and_maybe_dump_session(&state).await;
    (
        [(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"session.json\"",
        )],
        Json(payload),
    )
}

async fn build_and_maybe_dump_session(state: &SharedState) -> SessionResponse {
    const DEFAULT_PATH: &str = "/tmp/schemaui-session.json";
    let ui_ast = (*state.ui_ast).clone();
    let layout = (*state.layout).clone();
    let payload = SessionResponse {
        title: state.title.clone(),
        description: state.description.clone(),
        ui_ast,
        data: state.data.lock().await.clone(),
        formats: state.formats.iter().map(|f| f.to_string()).collect(),
        layout: Some(layout),
    };
    if let Ok(serialized) = serde_json::to_vec_pretty(&payload) {
        let path = std::env::var("SCHEMAUI_SESSION_DUMP").unwrap_or_else(|_| DEFAULT_PATH.into());
        let _ = tokio::task::spawn_blocking(move || fs::write(path, serialized)).await;
    }
    payload
}

#[derive(Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[cfg_attr(feature = "web-types", ts(export, export_to = "web/types/"))]
pub(crate) struct SaveRequest {
    #[cfg_attr(feature = "web-types", ts(type = "Record<string, unknown>"))]
    pub data: Value,
}

async fn post_save(
    State(state): State<SharedState>,
    Json(req): Json<SaveRequest>,
) -> (StatusCode, Json<Value>) {
    if state.finished.load(Ordering::SeqCst) {
        return (
            StatusCode::GONE,
            Json(json!({"error": "session already closed"})),
        );
    }
    *state.data.lock().await = req.data;
    (StatusCode::OK, Json(json!({"status": "saved"})))
}

#[derive(Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[cfg_attr(feature = "web-types", ts(export, export_to = "web/types/"))]
pub(crate) struct ExitRequest {
    #[cfg_attr(feature = "web-types", ts(type = "Record<string, unknown>"))]
    pub data: Value,
    #[serde(default = "crate::web::session::default_true")]
    pub commit: bool,
}

fn default_true() -> bool {
    true
}

async fn post_exit(
    State(state): State<SharedState>,
    Json(req): Json<ExitRequest>,
) -> (StatusCode, Json<Value>) {
    if state.finished.swap(true, Ordering::SeqCst) {
        return (
            StatusCode::GONE,
            Json(json!({"error": "session already closed"})),
        );
    }

    let final_value = if req.commit {
        req.data
    } else {
        state.data.lock().await.clone()
    };
    state.finish_line.complete(final_value).await;
    (StatusCode::OK, Json(json!({"status": "closing"})))
}

#[derive(Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[cfg_attr(feature = "web-types", ts(export, export_to = "web/types/"))]
pub(crate) struct ValidateRequest {
    #[cfg_attr(feature = "web-types", ts(type = "Record<string, unknown>"))]
    pub data: Value,
}

#[derive(Serialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[cfg_attr(feature = "web-types", ts(export, export_to = "web/types/"))]
pub(crate) struct ValidationResponse {
    pub ok: bool,
    pub errors: Vec<FieldError>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[cfg_attr(feature = "web-types", ts(export, export_to = "web/types/"))]
pub(crate) struct FieldError {
    pub pointer: String,
    pub message: String,
}

async fn post_validate(
    State(state): State<SharedState>,
    Json(req): Json<ValidateRequest>,
) -> impl IntoResponse {
    let mut errors = Vec::new();
    for error in state.validator.iter_errors(&req.data) {
        errors.push(FieldError {
            pointer: error.instance_path().to_string(),
            message: error.to_string(),
        });
    }
    Json(ValidationResponse {
        ok: errors.is_empty(),
        errors,
    })
}

#[derive(Deserialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[cfg_attr(feature = "web-types", ts(export, export_to = "web/types/"))]
pub(crate) struct PreviewRequest {
    #[cfg_attr(feature = "web-types", ts(type = "Record<string, unknown>"))]
    pub data: Value,
    pub format: String,
    #[serde(default = "crate::web::session::default_true")]
    pub pretty: bool,
}

#[derive(Serialize)]
#[cfg_attr(feature = "web-types", derive(TS))]
#[cfg_attr(feature = "web-types", ts(export, export_to = "web/types/"))]
pub(crate) struct PreviewResponse {
    pub payload: String,
}

async fn post_preview(
    State(state): State<SharedState>,
    Json(req): Json<PreviewRequest>,
) -> Result<Json<PreviewResponse>, (StatusCode, Json<Value>)> {
    render_payload(&req.data, &req.format, req.pretty, &state.formats)
        .map(|payload| Json(PreviewResponse { payload }))
        .map_err(|err| (StatusCode::BAD_REQUEST, Json(json!({"error": err}))))
}

fn render_payload(
    data: &Value,
    format_keyword: &str,
    pretty: bool,
    allowed: &[DocumentFormat],
) -> Result<String, String> {
    let format = DocumentFormat::from_keyword(format_keyword)?;
    if !allowed.contains(&format) {
        return Err(format!("format '{format}' is disabled for this build"));
    }
    encode_value(data, format, pretty).map_err(|err| err.to_string())
}

fn encode_value(
    value: &Value,
    format: DocumentFormat,
    pretty: bool,
) -> Result<String, anyhow::Error> {
    #[cfg(all(not(feature = "json"), not(feature = "toml")))]
    let _ = pretty;

    match format {
        #[cfg(feature = "json")]
        DocumentFormat::Json => {
            if pretty {
                Ok(serde_json::to_string_pretty(value)?)
            } else {
                Ok(serde_json::to_string(value)?)
            }
        }
        #[cfg(feature = "yaml")]
        DocumentFormat::Yaml => Ok(serde_yaml::to_string(value)?),
        #[cfg(feature = "toml")]
        DocumentFormat::Toml => {
            if pretty {
                Ok(toml::to_string_pretty(value)?)
            } else {
                Ok(toml::to_string(value)?)
            }
        }
        #[allow(unreachable_patterns)]
        #[cfg(all(not(feature = "yaml"), not(feature = "toml")))]
        _ => Err(anyhow!("unsupported format")),
    }
}

async fn static_assets(State(state): State<SharedState>, uri: OriginalUri) -> impl IntoResponse {
    if let Some(asset) = state.asset_provider.load(uri.path()) {
        let body = Body::from(asset.contents.into_owned());
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CACHE_CONTROL, "no-store")
            .header(header::CONTENT_TYPE, asset.mime)
            .body(body)
            .unwrap_or_else(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap()
            })
    } else {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("not found"))
            .unwrap()
    }
}
