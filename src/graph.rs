//! The in-memory typed graph.

use std::collections::{HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

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
    /// Build a graph from enriched JSONL — one AEO node per line. The crawler's
    /// default summary rows remain invalid; use its opt-in graph export.
    /// Edges are inferred from `peers` and `authority.primary_sources` arrays.
    pub fn from_jsonl(raw: &str) -> Result<Self, GraphError> {
        let mut graph = Self::default();
        let mut origins = HashSet::new();
        for (line_idx, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line).map_err(|err| GraphError::JsonLine {
                line: line_idx + 1,
                source: err,
            })?;
            if value.get("origin").is_some()
                && value.get("success").is_some()
                && value.get("id").is_none()
            {
                return Err(GraphError::CrawlerSummary(line_idx + 1));
            }
            let has_body = value.get("body").is_some_and(Value::is_object);
            let node: AeoNode =
                serde_json::from_value(value).map_err(|err| GraphError::JsonLine {
                    line: line_idx + 1,
                    source: err,
                })?;
            if node.id.is_empty() || node.id != node.entity.id {
                return Err(GraphError::InvalidNode(line_idx + 1));
            }
            if !has_body {
                return Err(GraphError::InvalidBody(line_idx + 1));
            }
            if graph.index.contains_key(&node.id) {
                return Err(GraphError::DuplicateNode(line_idx + 1));
            }
            if let Some(provenance) = &node.provenance {
                let Some(origin) = http_origin(&provenance.origin) else {
                    return Err(GraphError::InvalidProvenance(line_idx + 1));
                };
                let url = Url::parse(&provenance.origin)
                    .map_err(|_| GraphError::InvalidProvenance(line_idx + 1))?;
                if url.path() != "/"
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || !origins.insert(origin)
                {
                    return Err(GraphError::InvalidProvenance(line_idx + 1));
                }
            }
            graph.upsert(node);
        }
        if graph.node_count() == 0 {
            return Err(GraphError::EmptyGraph);
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
        self.graph.clear_edges();
        // Snapshot ids -> indices so the mutable borrow doesn't fight us.
        let snapshot: Vec<(NodeIndex, AeoNode)> = self
            .graph
            .node_indices()
            .map(|i| (i, self.graph[i].clone()))
            .collect();

        // AEO primary sources are URLs, while crawler graph nodes are keyed by
        // entity IDs. Match the same normalized origin rule the crawler uses.
        // If callers upsert conflicting origins directly, do not guess a target.
        let mut by_origin: HashMap<String, Option<NodeIndex>> = HashMap::new();
        for (idx, node) in &snapshot {
            if let Some(origin) = node
                .provenance
                .as_ref()
                .and_then(|p| http_origin(&p.origin))
            {
                by_origin
                    .entry(origin)
                    .and_modify(|target| *target = None)
                    .or_insert(Some(*idx));
            }
        }

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
            // Prefer an exact entity ID when present. Crawler graph exports
            // also resolve arbitrary source URLs to fetched origins.
            if let Some(sources) = node
                .body
                .get("authority")
                .and_then(|v| v.get("primary_sources"))
                .and_then(|v| v.as_array())
            {
                let mut seen_targets = HashSet::new();
                for src in sources {
                    if let Some(url) = src.as_str() {
                        let target = self.index.get(url).copied().or_else(|| {
                            http_origin(url)
                                .and_then(|origin| by_origin.get(&origin).copied().flatten())
                        });
                        if let Some(src_idx) = target {
                            if src_idx != *from_idx && seen_targets.insert(src_idx) {
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

fn http_origin(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url.origin().ascii_serialization())
}
