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
    build_router(AppState::new())
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
                .body(Body::from("not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
