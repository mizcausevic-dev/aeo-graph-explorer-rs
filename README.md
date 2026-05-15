# aeo-graph-explorer

[![CI](https://github.com/mizcausevic-dev/aeo-graph-explorer-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/mizcausevic-dev/aeo-graph-explorer-rs/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.86%2B-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**HTTP graph-query service over AEO Protocol crawls.** Ingests `aeo-crawler` JSON-Lines, builds an in-memory typed graph, exposes neighbours / shortest-path / find-by-claim. **Layer 5 of the AEO Reference Stack** — the piece that turns a crawl into a queryable view.

```text
1. SDKs       aeo-sdk-python / -typescript / -rust / -go / -swift
2. CLI        aeo-cli
3. Crawler    aeo-crawler                 produces JSONL
4. Validator  aeo-validator-service       always-on validation + drift
5. Explorer   aeo-graph-explorer-rs       <- this repo
```

---

## Why

`aeo-crawler` BFS-walks the AEO graph from a seed URL and dumps one node per JSON line. That's a perfect pipeline output and a frustrating query interface. This service:

1. Ingests the JSONL once and indexes it.
2. Exposes the things crawl consumers actually want — list every entity, fetch one entity's full doc, expand a node's neighbourhood, find the shortest citation chain between two entities, scan claims by predicate / value.
3. Rebuilds atomically — a new `POST /ingest` doesn't block ongoing reads.

No database. A typical AEO crawl is a few thousand nodes — the right tool for "give me a queryable view of a recent crawl" is an in-memory graph, not Postgres.

---

## Endpoints

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/` | Service info + endpoint list. |
| GET | `/healthz` | Liveness probe. |
| GET | `/nodes` | List every entity in the graph (summary view). |
| GET | `/nodes/{id}` | Fetch one entity's full AEO body. |
| GET | `/nodes/{id}/neighbors` | Outbound + inbound neighbours, with edge kinds. |
| GET | `/shortest-path?from=&to=` | A* search; returns `{ found, length, hops[] }`. |
| GET | `/find-by-claim?predicate=&value=` | Linear claim scan; at least one of the two parameters is required. |
| POST | `/ingest` | Load JSONL and rebuild the graph atomically. Body is the raw JSONL — `Content-Type: application/json` is fine. |
| GET | `/stats` | `{ nodes, edges }`. |

URL-encoded entity IDs are supported (`https%3A%2F%2Facme.example%2F%23org`).

---

## Run it

```bash
cargo install aeo-graph-explorer       # or build from source
aeo-graph-explorer                     # binds 0.0.0.0:8092 by default
```

Set `PORT` / `HOST` env vars to override.

---

## Quick start

```bash
# Stand the server up.
cargo run

# Ingest a crawl (the bundled example has three entities).
curl -X POST http://localhost:8092/ingest --data-binary @examples/sample.jsonl

# What's in the graph?
curl http://localhost:8092/stats
# -> {"nodes": 3, "edges": 2}

# Walk the neighbourhood of AcmeTutor.
curl 'http://localhost:8092/nodes/https%3A%2F%2Facmetutor.example%2F%23org/neighbors' | jq

# Anybody else in the AI-tutoring industry?
curl 'http://localhost:8092/find-by-claim?predicate=industry&value=AI%20tutoring' | jq
```

---

## Edge inference

When `POST /ingest` runs, the service walks each node's body and wires edges based on two well-known fields:

| Body field | Edge kind | Direction |
| --- | --- | --- |
| `peers[].id` | `DeclaresPeer` | from → to |
| `authority.primary_sources[]` (URL matches another node id) | `CitesAuthority` | from → to |

Edges to nodes that don't appear in the loaded crawl are dropped silently. This keeps the graph self-contained — you can always re-ingest later with a bigger crawl to fill in the gaps.

---

## Composes with

- **[aeo-crawler](https://github.com/mizcausevic-dev/aeo-crawler)** — produces the JSONL this service ingests.
- **[aeo-validator-service](https://github.com/mizcausevic-dev/aeo-validator-service)** — call `POST /watches` for every entity returned by `/nodes` to set up drift tracking across the whole graph.
- **[incident-correlation-rs](https://github.com/mizcausevic-dev/incident-correlation-rs)** — when an incident lands, ask `/find-by-claim` for entities declaring the affected predicate, then seed the correlator with the result.

---

## Bench

```bash
cargo bench
```

Bundled bench ingests a synthetic 2000-node chain so you can spot regressions in the parse + wire-edges pass.

---

## Tests

```bash
cargo test --all-targets
cargo test --doc
cargo clippy --all-targets -- -Dwarnings
cargo fmt --all -- --check
```

CI matrix: `stable`, `beta`, `1.86.0` (MSRV). End-to-end HTTP tests run via `tower::ServiceExt` — no real network.

---

## License

MIT. See [LICENSE](LICENSE).
