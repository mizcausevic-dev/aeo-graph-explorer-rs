//! Query operations on top of [`AeoGraph`].

use petgraph::algo::astar;
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};

use crate::error::GraphError;
use crate::graph::{AeoGraph, EdgeKind};
use crate::model::{AeoEntity, AeoNode};

/// Neighbour result for `/nodes/{id}/neighbors`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NeighborView {
    /// The center node of the neighbourhood (just the summary).
    pub center: AeoEntity,
    /// Outbound: things `center` declared.
    pub outbound: Vec<NeighborEdge>,
    /// Inbound: things that declared `center`.
    pub inbound: Vec<NeighborEdge>,
}

/// One side of a [`NeighborView`].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NeighborEdge {
    /// Edge kind.
    pub edge: EdgeKind,
    /// The other end of the edge.
    pub entity: AeoEntity,
}

/// One step in a [`PathResult`].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PathHop {
    /// Entity at this step.
    pub entity: AeoEntity,
    /// Edge kind that connects this step to the next one. `None` for the
    /// final hop.
    pub via: Option<EdgeKind>,
}

/// Output of `/shortest-path`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PathResult {
    /// Whether the search found a path at all.
    pub found: bool,
    /// Number of edges (= hops.len() - 1 when `found` is true).
    pub length: u32,
    /// Ordered path, source → destination.
    pub hops: Vec<PathHop>,
}

/// Match for `/find-by-claim`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClaimMatch {
    /// The entity that made the claim.
    pub entity: AeoEntity,
    /// Claim id within the entity's `claims` array.
    pub claim_id: String,
    /// The matched predicate.
    pub predicate: String,
    /// The matched value (verbatim).
    pub value: serde_json::Value,
}

/// Query operations over the in-memory graph.
pub fn neighbors(graph: &AeoGraph, id: &str) -> Result<NeighborView, GraphError> {
    let idx = graph
        .idx(id)
        .ok_or_else(|| GraphError::NotFound(id.to_string()))?;
    let raw = graph.raw();
    let center = raw[idx].entity.clone();

    let outbound = raw
        .edges_directed(idx, petgraph::Outgoing)
        .map(|e| NeighborEdge {
            edge: *e.weight(),
            entity: raw[e.target()].entity.clone(),
        })
        .collect();

    let inbound = raw
        .edges_directed(idx, petgraph::Incoming)
        .map(|e| NeighborEdge {
            edge: *e.weight(),
            entity: raw[e.source()].entity.clone(),
        })
        .collect();

    Ok(NeighborView {
        center,
        outbound,
        inbound,
    })
}

/// A* over the graph (uniform edge cost). Returns `found=false` if no path
/// exists.
pub fn shortest_path(graph: &AeoGraph, from: &str, to: &str) -> Result<PathResult, GraphError> {
    let from_idx = graph
        .idx(from)
        .ok_or_else(|| GraphError::NotFound(from.to_string()))?;
    let to_idx = graph
        .idx(to)
        .ok_or_else(|| GraphError::NotFound(to.to_string()))?;
    let raw = graph.raw();

    let path = astar(raw, from_idx, |n| n == to_idx, |_| 1u32, |_| 0u32);
    let Some((cost, nodes)) = path else {
        return Ok(PathResult {
            found: false,
            length: 0,
            hops: Vec::new(),
        });
    };

    let mut hops: Vec<PathHop> = Vec::with_capacity(nodes.len());
    for (i, idx) in nodes.iter().enumerate() {
        let via = if i + 1 < nodes.len() {
            raw.edges_connecting(*idx, nodes[i + 1])
                .next()
                .map(|e| *e.weight())
        } else {
            None
        };
        hops.push(PathHop {
            entity: raw[*idx].entity.clone(),
            via,
        });
    }

    Ok(PathResult {
        found: true,
        length: cost,
        hops,
    })
}

/// Linear scan over every node's claims. Both `predicate` and `value` are
/// optional but at least one must be supplied. Match semantics:
///
/// - `predicate` (when supplied) is exact equality against the claim's
///   `predicate` string.
/// - `value` (when supplied) is exact equality on the claim's `value`. If
///   the claim's value is a string and the search value parses as a string,
///   the comparison is plain string equality; otherwise the comparison is
///   JSON equality.
pub fn find_by_claim(
    graph: &AeoGraph,
    predicate: Option<&str>,
    value: Option<&str>,
) -> Result<Vec<ClaimMatch>, GraphError> {
    if predicate.is_none() && value.is_none() {
        return Err(GraphError::EmptyQuery);
    }

    let mut out: Vec<ClaimMatch> = Vec::new();
    for node in graph.nodes() {
        match_node(node, predicate, value, &mut out);
    }
    Ok(out)
}

fn match_node(
    node: &AeoNode,
    predicate: Option<&str>,
    value: Option<&str>,
    out: &mut Vec<ClaimMatch>,
) {
    let Some(claims) = node.body.get("claims").and_then(|v| v.as_array()) else {
        return;
    };
    for raw in claims {
        let Some(pred) = raw.get("predicate").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some(want) = predicate {
            if pred != want {
                continue;
            }
        }
        let claim_value = raw.get("value").cloned().unwrap_or(serde_json::Value::Null);
        if let Some(want) = value {
            if !value_matches(&claim_value, want) {
                continue;
            }
        }
        let claim_id = raw
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push(ClaimMatch {
            entity: node.entity.clone(),
            claim_id,
            predicate: pred.to_string(),
            value: claim_value,
        });
    }
}

fn value_matches(actual: &serde_json::Value, query: &str) -> bool {
    if let Some(s) = actual.as_str() {
        return s == query;
    }
    serde_json::from_str::<serde_json::Value>(query).is_ok_and(|q| &q == actual)
}
