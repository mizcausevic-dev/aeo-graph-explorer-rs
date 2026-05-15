//! `aeo-graph-explorer` binary entry point.
//!
//! Reads `PORT` and `HOST` from the environment (defaults to `0.0.0.0:8092`),
//! starts the axum server, and listens until ctrl-C.

use std::net::SocketAddr;

use aeo_graph_explorer::{build_router, AppState};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8092);
    let host: String = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let addr: SocketAddr = format!("{host}:{port}").parse()?;

    let app = build_router(AppState::new());
    let listener = TcpListener::bind(addr).await?;
    eprintln!("aeo-graph-explorer listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
