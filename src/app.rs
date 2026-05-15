//! Axum HTTP layer.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::error::GraphError;
use crate::graph::AeoGraph;
use crate::model::{AeoEntity, AeoNode};
use crate::query::{find_by_claim, neighbors, shortest_path, ClaimMatch, NeighborView, PathResult};

/// Shared app state — a single graph protected by a `RwLock` so `/ingest`
/// can replace it atomically without blocking concurrent reads.
#[derive(Clone)]
pub struct AppState {
    /// The graph itself.
    pub graph: Arc<RwLock<AeoGraph>>,
}

impl AppState {
    /// Construct fresh empty state.
    pub fn new() -> Self {
        Self {
            graph: Arc::new(RwLock::new(AeoGraph::default())),
        }
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
        .route("/ingest", post(post_ingest))
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
        "description": "HTTP graph-query service over AEO Protocol crawls. Layer 5 of the AEO Reference Stack.",
        "endpoints": {
            "GET  /healthz": "liveness probe",
            "GET  /nodes": "list every entity in the graph (summary)",
            "GET  /nodes/{id}": "fetch one entity's full AEO body",
            "GET  /nodes/{id}/neighbors": "outbound + inbound neighbours",
            "GET  /shortest-path?from=&to=": "A* over the graph",
            "GET  /find-by-claim?predicate=&value=": "linear claim search",
            "POST /ingest": "load JSONL (one AEO doc per line) and rebuild",
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
    body: String,
) -> Result<Json<serde_json::Value>, GraphError> {
    let new_graph = AeoGraph::from_jsonl(&body)?;
    let nodes = new_graph.node_count();
    let edges = new_graph.edge_count();
    *state.graph.write().await = new_graph;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "nodes": nodes,
        "edges": edges,
    })))
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
            GraphError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            GraphError::JsonLine { .. } | GraphError::UnknownNode(_) | GraphError::EmptyQuery => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
        };
        (status, Json(serde_json::json!({"error": message}))).into_response()
    }
}
