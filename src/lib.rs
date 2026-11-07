pub mod config;
pub mod language;

use axum::{Router, routing::get};

use config::Config;

/// Build the HTTP application.
///
/// The inbound contract is implemented in the next ticket; this placeholder
/// keeps the scaffold runnable until then.
pub fn build_router(_config: Config) -> anyhow::Result<Router> {
    Ok(Router::new().route("/", get(|| async { "local-translator-proxy" })))
}
