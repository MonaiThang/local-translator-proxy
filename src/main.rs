//! Process entry point: load and validate configuration, initialise logging,
//! bind the loopback listener, run a best-effort warm-up, and serve the router
//! until Ctrl+C.

use std::net::SocketAddr;

use anyhow::Context;
use local_translator_proxy::build_router_from_state;
use local_translator_proxy::build_state;
use local_translator_proxy::config::Config;
use local_translator_proxy::pipeline::{AppState, WarmUp};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = Config::load()?;
    tracing::info!(
        listen_addr = %config.server.listen_addr,
        endpoint = %config.model_runtime.endpoint,
        model = %config.model_runtime.model,
        timeout_secs = config.model_runtime.timeout_secs,
        max_in_flight = config.model_runtime.max_in_flight,
        num_ctx = config.model_runtime.num_ctx,
        keep_alive_secs = config.model_runtime.keep_alive_secs,
        warm_up = config.model_runtime.warm_up,
        "starting local-translator-proxy"
    );

    let addr: SocketAddr = config
        .server
        .listen_addr
        .parse()
        .with_context(|| format!("invalid listen address {:?}", config.server.listen_addr))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(%addr, "listening");

    let state = build_state(config)?;
    spawn_warm_up(&state);

    axum::serve(listener, build_router_from_state(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve HTTP")?;

    tracing::info!("shutdown complete");
    Ok(())
}

/// Load the model in the background so the first game request does not pay the
/// cold load. Never blocks or fails startup.
fn spawn_warm_up(state: &AppState) {
    let pipeline = state.pipeline.clone();
    tokio::spawn(async move {
        match pipeline.warm_up().await {
            WarmUp::Disabled => {}
            WarmUp::Warmed { elapsed } => {
                tracing::info!(elapsed_ms = elapsed.as_millis() as u64, "warm-up completed");
            }
            WarmUp::Failed { elapsed, error } => {
                tracing::warn!(
                    %error,
                    elapsed_ms = elapsed.as_millis() as u64,
                    "warm-up failed; the proxy will serve anyway"
                );
            }
        }
    });
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::warn!(%error, "failed to listen for Ctrl+C");
    }
}
