//! Game-facing Translation Proxy.
//!
//! [`build_state`] wires configuration, the Model Runtime client, and the
//! translation pipeline into the shared [`AppState`]; `main` builds it once and
//! runs the startup warm-up against the same pipeline. [`build_router_from_state`]
//! exposes that state on any path. A request flows from [`http::handle`] into
//! [`pipeline::Pipeline::translate`], which splits the Translation Unit into
//! lines, calls the runtime, verifies Rendering Tokens, and reassembles the
//! text. Any unsafe failure returns the original unit unchanged (the
//! passthrough-first contract).

pub mod config;
pub mod http;
pub mod language;
pub mod lines;
pub mod pipeline;
pub(crate) mod plan;
pub mod runtime;
pub mod tokens;

use std::sync::Arc;

use axum::Router;

use config::Config;
use pipeline::{AppState, Pipeline};
use runtime::RuntimeClient;

/// Build the shared runtime client and pipeline that the router and the
/// startup warm-up both use.
pub fn build_state(config: Config) -> anyhow::Result<AppState> {
    let config = Arc::new(config);
    let runtime = Arc::new(RuntimeClient::new(&config.model_runtime, &config.prompt)?);
    let pipeline = Arc::new(Pipeline::new(config.clone(), runtime));
    Ok(AppState { config, pipeline })
}

/// Expose the shared state on any path.
pub fn build_router_from_state(state: AppState) -> Router {
    Router::new().fallback(http::handle).with_state(state)
}

/// Build the shared state, then expose it on any path.
pub fn build_router(config: Config) -> anyhow::Result<Router> {
    let state = build_state(config)?;
    Ok(build_router_from_state(state))
}
