//! Crate-wide error type.

use thiserror::Error;

/// Anything that can go wrong inside the crate.
#[derive(Debug, Error)]
pub enum GraphError {
    /// A JSONL line was not valid JSON.
    #[error("failed to parse JSONL line {line}: {source}")]
    JsonLine {
        /// 1-based line number for operator-friendly messages.
        line: usize,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },

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
