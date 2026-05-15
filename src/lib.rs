//! # aeo-graph-explorer
//!
//! HTTP graph-query service over AEO Protocol crawls.
//!
//! ## The fifth layer of the AEO Reference Stack
//!
//! ```text
//! 1. SDKs       aeo-sdk-python / -typescript / -rust / -go / -swift
//! 2. CLI        aeo-cli
//! 3. Crawler    aeo-crawler                 produces JSONL
//! 4. Validator  aeo-validator-service       always-on validation + drift
//! 5. Explorer   aeo-graph-explorer-rs       <- this repo
//! ```
//!
//! ## What it does
//!
//! `aeo-crawler` BFS-walks an AEO graph from a seed URL and dumps one node
//! per JSON line. That's a great pipeline output and a terrible query
//! interface. This crate ingests the JSONL into a typed petgraph + an
//! `axum` HTTP layer, so callers can ask:
//!
//! - `GET /nodes` — list every entity in the graph.
//! - `GET /nodes/{id}` — fetch one entity's full AEO doc.
//! - `GET /nodes/{id}/neighbors` — declared peers + reverse references.
//! - `GET /shortest-path?from=X&to=Y` — does a citation chain connect them?
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
//!   atomically replaces it. Read paths take a snapshot and never block
//!   each other.
//! - No database. Crawls are small (thousands of nodes, not millions) and
//!   the right tool for "give me a queryable view of a recent crawl" is an
//!   in-memory graph.
//!
//! ## Composes with
//!
//! - **[aeo-crawler](https://github.com/mizcausevic-dev/aeo-crawler)** —
//!   produces the JSONL this service ingests.
//! - **[aeo-validator-service](https://github.com/mizcausevic-dev/aeo-validator-service)**
//!   — call `POST /watches` for every node returned by `/nodes` to set up
//!   drift tracking across the whole graph.
//! - **[incident-correlation-rs](https://github.com/mizcausevic-dev/incident-correlation-rs)**
//!   — when an incident lands, ask `/find-by-claim` for everyone declaring
//!   the affected entity, then seed the correlator with the result.

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

pub use app::{build_router, AppState};
pub use error::GraphError;
pub use graph::{AeoGraph, EdgeKind};
pub use model::{AeoClaim, AeoEntity, AeoNode};
pub use query::{
    find_by_claim, neighbors, shortest_path, ClaimMatch, NeighborView, PathHop, PathResult,
};
