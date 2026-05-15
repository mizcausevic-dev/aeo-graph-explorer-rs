//! The in-memory typed graph.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};

use crate::error::GraphError;
use crate::model::AeoNode;

/// Relationship kinds the explorer cares about.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// `from.peers[]` declared `to` as a peer entity.
    DeclaresPeer,
    /// `from.authority.primary_sources` chained through `to`.
    CitesAuthority,
}

/// Container for a single loaded crawl.
#[derive(Debug, Default)]
pub struct AeoGraph {
    graph: DiGraph<AeoNode, EdgeKind>,
    index: HashMap<String, NodeIndex>,
}

impl AeoGraph {
    /// Build a graph from JSONL — one AEO document per line, in the same shape
    /// `aeo-crawler` emits. Edges are inferred from `peers` and
    /// `authority.primary_sources` arrays.
    pub fn from_jsonl(raw: &str) -> Result<Self, GraphError> {
        let mut graph = Self::default();
        for (line_idx, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let node: AeoNode = serde_json::from_str(line).map_err(|err| GraphError::JsonLine {
                line: line_idx + 1,
                source: err,
            })?;
            graph.upsert(node);
        }
        graph.wire_edges();
        Ok(graph)
    }

    /// Insert or replace a node. Edge inference is deferred to
    /// [`Self::wire_edges`] so bulk loads only pay for it once.
    pub fn upsert(&mut self, node: AeoNode) -> NodeIndex {
        if let Some(&idx) = self.index.get(&node.id) {
            self.graph[idx] = node;
            return idx;
        }
        let id = node.id.clone();
        let idx = self.graph.add_node(node);
        self.index.insert(id, idx);
        idx
    }

    /// After all nodes are loaded, walk the bodies and wire up edges.
    pub fn wire_edges(&mut self) {
        // Snapshot ids -> indices so the mutable borrow doesn't fight us.
        let snapshot: Vec<(NodeIndex, AeoNode)> = self
            .graph
            .node_indices()
            .map(|i| (i, self.graph[i].clone()))
            .collect();

        for (from_idx, node) in &snapshot {
            // Peers: `body.peers: [{ "id": "...", ... }, ...]`
            if let Some(peers) = node.body.get("peers").and_then(|v| v.as_array()) {
                for peer in peers {
                    if let Some(peer_id) = peer.get("id").and_then(|v| v.as_str()) {
                        if let Some(&peer_idx) = self.index.get(peer_id) {
                            self.graph
                                .add_edge(*from_idx, peer_idx, EdgeKind::DeclaresPeer);
                        }
                    }
                }
            }
            // Authority: `body.authority.primary_sources: [url, ...]`
            // Crawler-side convention is that primary_sources can be
            // arbitrary URLs; we wire an edge only if a node with that id
            // exists in the loaded graph.
            if let Some(sources) = node
                .body
                .get("authority")
                .and_then(|v| v.get("primary_sources"))
                .and_then(|v| v.as_array())
            {
                for src in sources {
                    if let Some(url) = src.as_str() {
                        if let Some(&src_idx) = self.index.get(url) {
                            if src_idx != *from_idx {
                                self.graph
                                    .add_edge(*from_idx, src_idx, EdgeKind::CitesAuthority);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Look up a node by id.
    pub fn node(&self, id: &str) -> Option<&AeoNode> {
        self.index.get(id).map(|&i| &self.graph[i])
    }

    /// All loaded nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &AeoNode> {
        self.graph.node_indices().map(|i| &self.graph[i])
    }

    /// Number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub(crate) fn idx(&self, id: &str) -> Option<NodeIndex> {
        self.index.get(id).copied()
    }

    pub(crate) fn raw(&self) -> &DiGraph<AeoNode, EdgeKind> {
        &self.graph
    }
}
