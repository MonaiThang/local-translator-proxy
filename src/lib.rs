pub mod config;
pub mod http;
pub mod language;
pub mod lines;
pub mod pipeline;
pub mod tokens;

use std::sync::Arc;

use axum::Router;

use config::Config;
use pipeline::{AppState, Pipeline};

/// Build the HTTP application.
pub fn build_router(config: Config) -> anyhow::Result<Router> {
    let config = Arc::new(config);
    let pipeline = Arc::new(Pipeline::new(config.clone()));
    let state = AppState { config, pipeline };

    Ok(Router::new().fallback(http::handle).with_state(state))
}
