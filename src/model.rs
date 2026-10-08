//! Serde-friendly view of an enriched AEO graph node.
//!
//! We don't pull in `aeo-sdk-rust` because that would force callers into a
//! specific spec version. Ingestion requires matching `id` and `entity.id`
//! plus a `body` object, but does not validate the full AEO document schema.
//! The upstream crawler's opt-in `--format graph` uses this input shape.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One ingested AEO node — the parsed JSONL body plus a denormalised
/// `entity` so the query API doesn't have to peek inside `body` every time.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AeoNode {
    /// Stable entity identifier — typically the canonical entity URL.
    pub id: String,
    /// Lightweight summary used by `/nodes` so list responses don't carry the
    /// whole body.
    pub entity: AeoEntity,
    /// Parsed body supplied in the enriched input row. Returned by
    /// `/nodes/{id}`; completeness is not independently verified.
    #[serde(default)]
    pub body: HashMap<String, Value>,
    /// Optional source metadata from the crawler's opt-in graph export.
    /// This is caller supplied, not an independent provenance check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<CrawlProvenance>,
}

/// Source metadata emitted by `aeo-crawler --format graph`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CrawlProvenance {
    /// Normalized origin whose well-known declaration was fetched.
    pub origin: String,
    /// Breadth-first depth from the seed.
    pub depth: usize,
    /// UTC fetch time, formatted as RFC 3339 by the crawler.
    pub fetched_at: String,
}

/// Denormalised view of the most-asked-about fields.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AeoEntity {
    /// The same identifier as `AeoNode.id`. Kept here for consumers who only
    /// receive the summary.
    pub id: String,
    /// `Organization`, `Person`, `Product`, ...
    #[serde(default)]
    pub kind: Option<String>,
    /// Human-readable name.
    #[serde(default)]
    pub name: Option<String>,
    /// Where the entity says its truth-source lives.
    #[serde(default)]
    pub canonical_url: Option<String>,
}

/// One assertion the entity makes about itself.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AeoClaim {
    /// Stable identifier for the claim (used for round-tripping).
    pub id: String,
    /// The predicate / type — e.g. `description`, `industry`, `headquartered_in`.
    pub predicate: String,
    /// The claim's value — a string in the common case, but kept as JSON for
    /// flexibility.
    pub value: Value,
    /// Self-reported confidence (`"high"`, `"medium"`, `"low"`).
    #[serde(default)]
    pub confidence: Option<String>,
}
