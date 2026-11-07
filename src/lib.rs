//! Game-facing Translation Proxy.
//!
//! [`build_router`] wires configuration, the Model Runtime client, and the
//! translation pipeline into an axum router. A request flows from
//! [`http::handle`] into [`pipeline::Pipeline::translate`], which splits the
//! Translation Unit into lines, calls the runtime, verifies Rendering Tokens,
//! and reassembles the text. Any unsafe failure returns the original unit
//! unchanged (the passthrough-first contract).

pub mod config;
pub mod http;
pub mod language;
pub mod lines;
pub mod pipeline;
pub mod runtime;
pub mod tokens;

use std::sync::Arc;

use axum::Router;

use config::Config;
use pipeline::{AppState, Pipeline};
use runtime::RuntimeClient;

/// Build the shared runtime client and pipeline, then expose them on any path.
pub fn build_router(config: Config) -> anyhow::Result<Router> {
    let config = Arc::new(config);
    let runtime = Arc::new(RuntimeClient::new(&config.model_runtime, &config.prompt)?);
    let pipeline = Arc::new(Pipeline::new(config.clone(), runtime));
    let state = AppState { config, pipeline };

    Ok(Router::new().fallback(http::handle).with_state(state))
}
