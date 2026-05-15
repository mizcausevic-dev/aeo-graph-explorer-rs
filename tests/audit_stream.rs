//! End-to-end tests for the audit-stream-py integration.
//!
//! Strategy: build an `AppState` with a `reqwest::Client` pointed at a
//! wiremock server, exercise the relevant handlers, then assert the
//! mock saw the expected events.

#![cfg(feature = "audit-stream")]

use aeo_graph_explorer::{build_router, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use std::sync::Mutex;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// `AUDIT_STREAM_URL` is process-global; serialise the tests that
// mutate it so they can run in parallel safely.
static ENV_GUARD: Mutex<()> = Mutex::new(());

struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn lock() -> Self {
        let lock = ENV_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::env::remove_var("AUDIT_STREAM_URL");
        std::env::remove_var("AUDIT_STREAM_TIMEOUT_S");
        EnvGuard { _lock: lock }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        std::env::remove_var("AUDIT_STREAM_URL");
        std::env::remove_var("AUDIT_STREAM_TIMEOUT_S");
    }
}

const JSONL: &str = r#"{"id":"https://acme.example/#org","entity":{"id":"https://acme.example/#org","kind":"Organization","name":"Acme","canonical_url":"https://acme.example/"},"body":{"aeo_version":"0.1","claims":[]}}"#;

async fn ingest(app: &axum::Router, body: &'static str) -> StatusCode {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status()
}

#[tokio::test]
async fn ingest_emits_graph_ingested_when_enabled() {
    let _guard = EnvGuard::lock();
    let server = MockServer::start().await;
    std::env::set_var("AUDIT_STREAM_URL", server.uri());

    Mock::given(method("POST"))
        .and(path("/events"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({"event_id": 1})))
        .expect(1)
        .mount(&server)
        .await;

    let state = AppState::with_audit_client(reqwest::Client::new());
    let app = build_router(state);
    assert_eq!(ingest(&app, JSONL).await, StatusCode::OK);

    let recvd = server.received_requests().await.unwrap();
    assert_eq!(recvd.len(), 1);
    let body: Value = serde_json::from_slice(&recvd[0].body).unwrap();
    assert_eq!(body["kind"], "graph_ingested");
    assert_eq!(body["source"], "aeo-graph-explorer");
    assert_eq!(body["payload"]["nodes"], 1);
    assert!(body["payload"]["input_bytes"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn malformed_ingest_emits_graph_ingest_failed() {
    let _guard = EnvGuard::lock();
    let server = MockServer::start().await;
    std::env::set_var("AUDIT_STREAM_URL", server.uri());

    Mock::given(method("POST"))
        .and(path("/events"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let state = AppState::with_audit_client(reqwest::Client::new());
    let app = build_router(state);
    assert_eq!(
        ingest(&app, "not valid json").await,
        StatusCode::BAD_REQUEST
    );

    let recvd = server.received_requests().await.unwrap();
    assert_eq!(recvd.len(), 1);
    let body: Value = serde_json::from_slice(&recvd[0].body).unwrap();
    assert_eq!(body["kind"], "graph_ingest_failed");
    assert_eq!(body["source"], "aeo-graph-explorer");
    assert!(body["payload"]["reason"].as_str().is_some());
}

#[tokio::test]
async fn ingest_is_silent_when_env_var_unset() {
    let _guard = EnvGuard::lock();
    // No AUDIT_STREAM_URL set. emit() must short-circuit before hitting the
    // mock — we configure no expectations and assert nothing was received.
    let server = MockServer::start().await;
    let state = AppState::with_audit_client(reqwest::Client::new());
    let app = build_router(state);
    assert_eq!(ingest(&app, JSONL).await, StatusCode::OK);

    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn audit_stream_outage_does_not_break_ingest() {
    let _guard = EnvGuard::lock();
    // Point at a port nothing's listening on. The ingest must still
    // succeed; the emit failure logs to stderr and is swallowed.
    std::env::set_var("AUDIT_STREAM_URL", "http://127.0.0.1:1");

    let state = AppState::with_audit_client(
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()
            .unwrap(),
    );
    let app = build_router(state);
    assert_eq!(ingest(&app, JSONL).await, StatusCode::OK);
}
