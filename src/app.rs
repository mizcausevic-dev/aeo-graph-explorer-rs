//! Axum HTTP layer.

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;

use crate::error::GraphError;
use crate::graph::AeoGraph;
use crate::model::{AeoEntity, AeoNode};
use crate::query::{find_by_claim, neighbors, shortest_path, ClaimMatch, NeighborView, PathResult};

/// Cap JSONL uploads before they are buffered or parsed.
pub const MAX_INGEST_BYTES: usize = 2 * 1024 * 1024;

/// Shared app state — a single graph protected by a `RwLock` so `/ingest`
/// can replace it atomically. Reads can pause briefly during replacement.
///
/// When built with the `audit-stream` feature (default), the state also
/// holds a `reqwest::Client` reused for governance-event emission. The
/// client itself is cheap; what matters is reusing connections so a
/// burst of ingests doesn't open one TCP socket per emit.
#[derive(Clone)]
pub struct AppState {
    /// The graph itself.
    pub graph: Arc<RwLock<AeoGraph>>,
    /// An unset token disables graph replacement, including in library routers.
    ingest_token: Option<String>,
    /// Shared HTTP client for `audit_stream::emit`. Always present when
    /// the feature is on, even if `AUDIT_STREAM_URL` is unset (in which
    /// case `emit` no-ops without using it).
    #[cfg(feature = "audit-stream")]
    pub audit_client: reqwest::Client,
}

impl AppState {
    /// Construct fresh empty state with a default audit-stream client.
    pub fn new() -> Self {
        Self {
            graph: Arc::new(RwLock::new(AeoGraph::default())),
            ingest_token: None,
            #[cfg(feature = "audit-stream")]
            audit_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("default audit client configuration must be valid"),
        }
    }

    /// Build state with a caller-provided audit-stream client. Tests use
    /// this to swap in a wiremock-backed client.
    #[cfg(feature = "audit-stream")]
    #[must_use]
    pub fn with_audit_client(audit_client: reqwest::Client) -> Self {
        Self {
            graph: Arc::new(RwLock::new(AeoGraph::default())),
            ingest_token: None,
            audit_client,
        }
    }

    /// Enable ingestion with a non-empty bearer token supplied by the operator.
    #[must_use]
    pub fn with_ingest_token(mut self, token: impl Into<String>) -> Self {
        let token = token.into();
        self.ingest_token = (!token.is_empty()).then_some(token);
        self
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the `axum` router. Tests use this directly with `tower::ServiceExt`.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/healthz", get(healthz))
        .route("/nodes", get(list_nodes))
        .route("/nodes/:id", get(get_node))
        .route("/nodes/:id/neighbors", get(get_neighbors))
        .route("/shortest-path", get(get_shortest_path))
        .route("/find-by-claim", get(get_find_by_claim))
        .route(
            "/ingest",
            post(post_ingest).layer(DefaultBodyLimit::max(MAX_INGEST_BYTES)),
        )
        .route("/stats", get(get_stats))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn root() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": "aeo-graph-explorer",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "HTTP graph-query service for enriched AEO JSONL. Designed as Layer 5 of the AEO Reference Stack.",
        "endpoints": {
            "GET  /healthz": "liveness probe",
            "GET  /nodes": "list every entity in the graph (summary)",
            "GET  /nodes/{id}": "fetch one entity's full AEO body",
            "GET  /nodes/{id}/neighbors": "outbound + inbound neighbours",
            "GET  /shortest-path?from=&to=": "A* over the graph",
            "GET  /find-by-claim?predicate=&value=": "linear claim search",
            "POST /ingest": "load enriched JSONL (one node per line) and rebuild; bearer token required",
            "GET  /stats": "node_count + edge_count"
        }
    }))
}

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

async fn list_nodes(State(state): State<AppState>) -> Json<Vec<AeoEntity>> {
    let graph = state.graph.read().await;
    Json(graph.nodes().map(|n| n.entity.clone()).collect())
}

async fn get_node(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<AeoNode>, GraphError> {
    let graph = state.graph.read().await;
    let node = graph
        .node(&id)
        .cloned()
        .ok_or_else(|| GraphError::NotFound(id.clone()))?;
    Ok(Json(node))
}

async fn get_neighbors(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NeighborView>, GraphError> {
    let graph = state.graph.read().await;
    Ok(Json(neighbors(&graph, &id)?))
}

#[derive(Deserialize)]
struct PathParams {
    from: String,
    to: String,
}

async fn get_shortest_path(
    State(state): State<AppState>,
    Query(params): Query<PathParams>,
) -> Result<Json<PathResult>, GraphError> {
    let graph = state.graph.read().await;
    Ok(Json(shortest_path(&graph, &params.from, &params.to)?))
}

#[derive(Deserialize)]
struct ClaimParams {
    predicate: Option<String>,
    value: Option<String>,
}

async fn get_find_by_claim(
    State(state): State<AppState>,
    Query(params): Query<ClaimParams>,
) -> Result<Json<Vec<ClaimMatch>>, GraphError> {
    let graph = state.graph.read().await;
    Ok(Json(find_by_claim(
        &graph,
        params.predicate.as_deref(),
        params.value.as_deref(),
    )?))
}

async fn post_ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<serde_json::Value>, GraphError> {
    let Some(expected) = state.ingest_token.as_deref() else {
        return Err(GraphError::IngestDisabled);
    };
    let supplied = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    if !token_matches(expected, supplied) {
        return Err(GraphError::Unauthorized);
    }
    let new_graph = match AeoGraph::from_jsonl(&body) {
        Ok(g) => g,
        Err(err) => {
            #[cfg(feature = "audit-stream")]
            crate::audit_stream::emit(
                &state.audit_client,
                "graph_ingest_failed",
                serde_json::json!({
                    "reason": "invalid_jsonl",
                    "input_bytes": body.len(),
                }),
            )
            .await;
            return Err(err);
        }
    };
    let nodes = new_graph.node_count();
    let edges = new_graph.edge_count();
    *state.graph.write().await = new_graph;
    #[cfg(feature = "audit-stream")]
    crate::audit_stream::emit(
        &state.audit_client,
        "graph_ingested",
        serde_json::json!({
            "nodes": nodes,
            "edges": edges,
            "input_bytes": body.len(),
        }),
    )
    .await;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "nodes": nodes,
        "edges": edges,
    })))
}

// The token length is configuration, not a secret; compare equal-length
// contents without exiting early on a matching prefix.
fn token_matches(expected: &str, supplied: &str) -> bool {
    if expected.len() != supplied.len() {
        return false;
    }
    bool::from(expected.as_bytes().ct_eq(supplied.as_bytes()))
}

async fn get_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    let graph = state.graph.read().await;
    Json(serde_json::json!({
        "nodes": graph.node_count(),
        "edges": graph.edge_count(),
    }))
}

// ---------------------------------------------------------------------------
// GraphError -> HTTP response
// ---------------------------------------------------------------------------

impl IntoResponse for GraphError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            GraphError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            GraphError::IngestDisabled => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            GraphError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            GraphError::JsonLine { .. }
            | GraphError::CrawlerSummary(_)
            | GraphError::EmptyGraph
            | GraphError::InvalidBody(_)
            | GraphError::DuplicateNode(_)
            | GraphError::InvalidNode(_)
            | GraphError::UnknownNode(_)
            | GraphError::EmptyQuery => (StatusCode::BAD_REQUEST, self.to_string()),
        };
        (status, Json(serde_json::json!({"error": message}))).into_response()
    }
}
