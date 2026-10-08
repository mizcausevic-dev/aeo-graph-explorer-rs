//! End-to-end HTTP tests via tower's `Service` trait — no real network.

use aeo_graph_explorer::{build_router, AppState};
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

const JSONL: &str = r#"
{"id":"https://acme.example/#org","entity":{"id":"https://acme.example/#org","kind":"Organization","name":"Acme","canonical_url":"https://acme.example/"},"body":{"aeo_version":"0.1","peers":[{"id":"https://other.example/#org"}],"claims":[{"id":"c1","predicate":"industry","value":"AI tutoring"}]}}
{"id":"https://other.example/#org","entity":{"id":"https://other.example/#org","kind":"Organization","name":"Other","canonical_url":"https://other.example/"},"body":{"aeo_version":"0.1","claims":[{"id":"c2","predicate":"industry","value":"AI tutoring"}]}}
"#;

async fn ingest(app: &axum::Router) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-token")
                .body(Body::from(JSONL))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn router() -> axum::Router {
    build_router(AppState::new().with_ingest_token("test-token"))
}

#[tokio::test]
async fn root_lists_endpoints() {
    let app = router();
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["name"], "aeo-graph-explorer");
}

#[tokio::test]
async fn healthz() {
    let app = router();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn ingest_then_stats() {
    let app = router();
    ingest(&app).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["nodes"], 2);
    assert!(json["edges"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn list_nodes_after_ingest() {
    let app = router();
    ingest(&app).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/nodes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(resp).await;
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 2);
}

#[tokio::test]
async fn fetch_specific_node() {
    let app = router();
    ingest(&app).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/nodes/https%3A%2F%2Facme.example%2F%23org")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["entity"]["name"], "Acme");
    assert_eq!(json["body"]["aeo_version"], "0.1");
}

#[tokio::test]
async fn missing_node_is_404() {
    let app = router();
    ingest(&app).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/nodes/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn neighbors_view() {
    let app = router();
    ingest(&app).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/nodes/https%3A%2F%2Facme.example%2F%23org/neighbors")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["outbound"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn shortest_path_endpoint() {
    let app = router();
    ingest(&app).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/shortest-path?from=https%3A%2F%2Facme.example%2F%23org&to=https%3A%2F%2Fother.example%2F%23org")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json["found"], true);
    assert_eq!(json["length"], 1);
}

#[tokio::test]
async fn find_by_claim_endpoint() {
    let app = router();
    ingest(&app).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/find-by-claim?predicate=industry&value=AI%20tutoring")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_json(resp).await;
    assert_eq!(json.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn find_by_claim_empty_query_is_400() {
    let app = router();
    ingest(&app).await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/find-by-claim")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn ingest_malformed_jsonl_is_400() {
    let app = router();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest")
                .header("authorization", "Bearer test-token")
                .body(Body::from("not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn ingest_is_disabled_by_default() {
    let app = build_router(AppState::new());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest")
                .body(Body::from(JSONL))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn ingest_requires_the_correct_token_and_preserves_the_graph() {
    let app = router();
    for authorization in [None, Some("Bearer wrong-token")] {
        let mut request = Request::builder().method("POST").uri("/ingest");
        if let Some(value) = authorization {
            request = request.header("authorization", value);
        }
        let resp = app
            .clone()
            .oneshot(request.body(Body::from(JSONL)).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
    let stats = app
        .oneshot(
            Request::builder()
                .uri("/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body_json(stats).await["nodes"], 0);
}

#[tokio::test]
async fn oversized_ingest_is_rejected_before_replacement() {
    let app = router();
    let oversized = "x".repeat(2 * 1024 * 1024 + 1);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest")
                .header("authorization", "Bearer test-token")
                .body(Body::from(oversized))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let stats = app
        .oneshot(
            Request::builder()
                .uri("/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body_json(stats).await["nodes"], 0);
}

#[tokio::test]
async fn current_aeo_crawler_summary_is_rejected_with_contract_guidance() {
    // Source: aeo-crawler README at f577f020, its CLI emits Result rows.
    const CRAWLER_ROW: &str = r#"{"origin":"https://mizcausevic-dev.github.io","depth":0,"success":true,"entity_name":"Miz Causevic","entity_type":"Person","claims_count":6,"audit_mode":"none","fetched_at":"2026-05-12T04:00:00Z"}"#;
    let app = router();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest")
                .header("authorization", "Bearer test-token")
                .body(Body::from(CRAWLER_ROW))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(body_json(resp).await["error"]
        .as_str()
        .unwrap()
        .contains("aeo-crawler summary row"));
    let stats = app
        .oneshot(
            Request::builder()
                .uri("/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body_json(stats).await["nodes"], 0);
}
