use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::Arc;

use crate::core::frontend::{Frontend, FrontendContext};

use super::assets::EmbeddedAssets;
use super::session::{ServeOptions, WebSessionConfig, bind_session};

/// Web frontend implementation that consumes a prepared `FrontendContext`
/// and runs the browser-based UI via a temporary HTTP server.
#[derive(Debug, Clone, Copy)]
pub struct WebFrontend {
    pub serve: ServeOptions,
}

impl WebFrontend {
    pub async fn run_async(self, ctx: FrontendContext) -> Result<Value> {
        let config = web_session_config(ctx);
        let title = &config.title.clone().unwrap_or_default();
        let bound = bind_session(config, self.serve)
            .await
            .context("failed to bind web session")?;
        let addr = bound.local_addr();
        eprintln!("{title} schemaui UI available at http://{addr}/");
        eprintln!("Press Ctrl+C to abort the session.");
        bound.run().await.context("web UI session failed")
    }
}

impl Frontend for WebFrontend {
    fn run(self, ctx: FrontendContext) -> Result<Value> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to initialize tokio runtime")?;

        runtime.block_on(self.run_async(ctx))
    }
}

fn web_session_config(ctx: FrontendContext) -> WebSessionConfig {
    let FrontendContext {
        title,
        description,
        ui_ast,
        layout,
        initial_data,
        schema,
        validator: _,
    } = ctx;

    WebSessionConfig {
        title,
        description,
        ui_ast,
        layout,
        data: initial_data,
        schema,
        asset_provider: Arc::new(EmbeddedAssets),
    }
}
