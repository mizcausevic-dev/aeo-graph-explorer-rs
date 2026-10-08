//! # aeo-graph-explorer
//!
//! HTTP graph-query service over AEO Protocol crawls.
//!
//! ## The fifth layer of the AEO Reference Stack
//!
//! ```text
//! 1. SDKs       aeo-sdk-python / -typescript / -rust / -go / -swift
//! 2. CLI        aeo-cli
//! 3. Crawler    aeo-crawler --format graph   produces enriched JSONL
//! 4. Validator  aeo-validator-service       HTTP validation + caller-triggered drift rechecks
//! 5. Explorer   aeo-graph-explorer-rs       <- this repo
//! ```
//!
//! ## What it does
//!
//! This crate ingests enriched AEO JSONL into a typed petgraph + an
//! `axum` HTTP layer, so callers can ask:
//!
//! - `GET /nodes` — list every entity in the graph.
//! - `GET /nodes/{id}` — fetch one stored node and its supplied body.
//! - `GET /nodes/{id}/neighbors` — declared peers + reverse references.
//! - `GET /shortest-path?from=X&to=Y` — is there a directed path through declared edges?
//! - `GET /find-by-claim?predicate=...&value=...` — pull entities whose
//!   claims match a predicate/value pair.
//! - `POST /ingest` — load a JSONL document and rebuild the graph atomically.
//!
//! ## Design
//!
//! - Graph is `petgraph::Graph<AeoNode, EdgeKind>`; cheap to walk, cheap to
//!   serialize. Edge kinds (`DeclaresPeer`, `CitesAuthority`) are typed so
//!   future endpoints can answer "what authorities does X chain through?"
//!   without re-walking.
//! - The whole graph lives behind a `tokio::sync::RwLock` so an `/ingest`
//!   atomically replaces it. Read paths can pause briefly during replacement.
//! - No database. Crawls are small (thousands of nodes, not millions) and
//!   the right tool for "give me a queryable view of a recent crawl" is an
//!   in-memory graph.
//!
//! ## Potential integrations
//!
//! - **[aeo-crawler](https://github.com/mizcausevic-dev/aeo-crawler)** —
//!   `--format graph` emits directly ingestible parsed v0.1 declarations.
//! - **[aeo-validator-service](https://github.com/mizcausevic-dev/aeo-validator-service)**
//!   — a separate adapter could select watch URLs from `/nodes`.
//! - **[incident-correlation-rs](https://github.com/mizcausevic-dev/incident-correlation-rs)**
//!   — a separate adapter could find candidate affected entities via `/find-by-claim`.

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::doc_markdown)]

pub mod app;
pub mod error;
pub mod graph;
pub mod model;
pub mod query;

/// Optional audit-stream-py producer. Gated behind the `audit-stream`
/// Cargo feature (on by default for the binary service).
#[cfg(feature = "audit-stream")]
pub mod audit_stream;

pub use app::{build_router, AppState};
pub use error::GraphError;
pub use graph::{AeoGraph, EdgeKind};
pub use model::{AeoClaim, AeoEntity, AeoNode, CrawlProvenance};
pub use query::{
    find_by_claim, neighbors, shortest_path, ClaimMatch, NeighborView, PathHop, PathResult,
};
