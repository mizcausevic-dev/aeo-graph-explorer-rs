//! `aeo-graph-explorer` binary entry point.
//!
//! Reads `PORT` and `HOST` from the environment (defaults to `127.0.0.1:8092`),
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
    let host: String = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    if !binding_allowed(
        addr,
        std::env::var("AEO_GRAPH_ALLOW_NON_LOOPBACK").as_deref() == Ok("1"),
    ) {
        return Err("non-loopback bind requires AEO_GRAPH_ALLOW_NON_LOOPBACK=1 and a trusted access gateway".into());
    }

    let mut state = AppState::new();
    if let Ok(token) = std::env::var("AEO_GRAPH_INGEST_TOKEN") {
        state = state.with_ingest_token(token);
    }
    let app = build_router(state);
    let listener = TcpListener::bind(addr).await?;
    eprintln!("aeo-graph-explorer listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

fn binding_allowed(addr: SocketAddr, explicit_opt_in: bool) -> bool {
    addr.ip().is_loopback() || explicit_opt_in
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_is_allowed_without_opt_in() {
        assert!(binding_allowed("127.0.0.1:8092".parse().unwrap(), false));
        assert!(binding_allowed("[::1]:8092".parse().unwrap(), false));
    }

    #[test]
    fn non_loopback_requires_explicit_opt_in() {
        assert!(!binding_allowed("0.0.0.0:8092".parse().unwrap(), false));
        assert!(binding_allowed("0.0.0.0:8092".parse().unwrap(), true));
    }
}
