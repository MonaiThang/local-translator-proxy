mod config;

use axum::{Router, routing::get};
use config::Config;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    
    let config = Config::load()?;
    info!("Loaded config");
    info!("Listening on {}", config.server.listen_addr);
    
    let app = Router::new()
        .route("/", get(hello));
    
    let addr: SocketAddr = config.server.listen_addr.parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!("Server started successfully");
    
    axum::serve(listener, app).await?;
    
    Ok(())
}

async fn hello() -> &'static str {
    "local-translator-proxy"
}
