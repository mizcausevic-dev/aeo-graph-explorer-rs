//! Crate-wide error type.

use thiserror::Error;

/// Anything that can go wrong inside the crate.
#[derive(Debug, Error)]
pub enum GraphError {
    /// Graph replacement has not been enabled by the operator.
    #[error("ingestion is disabled; configure AEO_GRAPH_INGEST_TOKEN")]
    IngestDisabled,

    /// The request did not carry the configured ingestion bearer token.
    #[error("ingestion requires a valid bearer token")]
    Unauthorized,

    /// A JSONL line was not valid JSON.
    #[error("failed to parse JSONL line {line}: {source}")]
    JsonLine {
        /// 1-based line number for operator-friendly messages.
        line: usize,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },

    /// Current aeo-crawler output is a crawl summary, not an enriched node.
    #[error("line {0} is an aeo-crawler summary row; /ingest requires enriched JSONL with id, entity, and body")]
    CrawlerSummary(usize),

    /// The upload cannot replace the graph with an empty one accidentally.
    #[error("JSONL must contain at least one enriched node")]
    EmptyGraph,

    /// Enriched rows require the source document body.
    #[error("enriched JSONL line {0} must contain a body object")]
    InvalidBody(usize),

    /// Duplicate ids make provenance ambiguous; reject the whole upload.
    #[error("duplicate node id on line {0}")]
    DuplicateNode(usize),

    /// A node's identifiers are missing or disagree.
    #[error("node id and entity.id must be matching non-empty strings on line {0}")]
    InvalidNode(usize),

    /// A node referenced by an edge was not present in the input.
    #[error("unknown node id: {0}")]
    UnknownNode(String),

    /// A request asked about a node that isn't in the loaded graph.
    #[error("node not found in graph: {0}")]
    NotFound(String),

    /// `/find-by-claim` requires at least one of `predicate` or `value`.
    #[error("at least one of `predicate` or `value` must be supplied")]
    EmptyQuery,
}
