use std::net::SocketAddr;

use anyhow::Context;
use local_translator_proxy::build_router;
use local_translator_proxy::config::Config;
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

    let app = build_router(config)?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve HTTP")?;

    tracing::info!("shutdown complete");
    Ok(())
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
