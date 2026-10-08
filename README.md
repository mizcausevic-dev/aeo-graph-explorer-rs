# aeo-graph-explorer

[![CI](https://github.com/mizcausevic-dev/aeo-graph-explorer-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/mizcausevic-dev/aeo-graph-explorer-rs/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**HTTP graph-query service for enriched AEO JSONL.** It builds an in-memory typed graph and exposes neighbours, shortest paths, and claim search. It is designed as Layer 5 of the AEO Reference Stack. The current `aeo-crawler` CLI emits summary rows without document bodies or edge fields, so those rows cannot be ingested directly.

```text
1. SDKs       aeo-sdk-python / -typescript / -rust / -go / -swift
2. CLI        aeo-cli
3. Crawler    aeo-crawler                 produces summary JSONL
4. Validator  aeo-validator-service       HTTP validation + caller-triggered drift rechecks
5. Explorer   aeo-graph-explorer-rs       <- enriched JSONL required
```

---

## Why

The graph endpoints need each entity's identifier, body, claims, and relationship fields. This service:

1. Ingests enriched JSONL and indexes it.
2. Exposes the things crawl consumers actually want — list every entity, fetch one entity's full doc, expand a node's neighbourhood, find the shortest citation chain between two entities, scan claims by predicate / value.
3. Rebuilds off-lock, then atomically replaces the graph. Queries can pause briefly during replacement.

No database. This is designed for bounded, recent crawls kept in memory. Ingested state is lost when the process restarts.

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
| POST | `/ingest` | Load up to 2 MiB of JSONL and rebuild the graph atomically. Requires `Authorization: Bearer <AEO_GRAPH_INGEST_TOKEN>`. Disabled when the token is unset. |
| GET | `/stats` | `{ nodes, edges }`. |

URL-encoded entity IDs are supported (`https%3A%2F%2Facme.example%2F%23org`). Duplicate node IDs and mismatched `id` / `entity.id` values reject the entire ingest.

### Input contract

Each line must contain `id`, an `entity` object with the same `id`, and a `body` object. At least one valid node is required. `body.peers`, `body.authority.primary_sources`, and `body.claims` populate the corresponding graph queries. The bundled `examples/sample.jsonl` is an enriched example.

The current [aeo-crawler](https://github.com/mizcausevic-dev/aeo-crawler) CLI instead emits `origin`, `depth`, `success`, `entity_name`, `entity_type`, and `claims_count` summary fields. Its rows have no `body`, so they cannot produce meaningful edges or claim matches. `/ingest` rejects that shape with a contract error. An upstream enriched export or a separate fetch-and-enrich adapter is required before the crawler-to-explorer bridge can be claimed as working.

---

## Run it

```bash
cargo install aeo-graph-explorer       # or build from source
aeo-graph-explorer                     # binds 127.0.0.1:8092 by default
```

Set `PORT` / `HOST` env vars to override. Set a high-entropy `AEO_GRAPH_INGEST_TOKEN` in the process environment to enable `/ingest`. Keep it in a secret manager for a hosted service; never put it in a URL or repository. A non-loopback bind such as `HOST=0.0.0.0` also requires `AEO_GRAPH_ALLOW_NON_LOOPBACK=1` to acknowledge exposure. All read endpoints return data without built-in authentication, including full document bodies. Keep the reference service on loopback unless a trusted gateway provides read authorization, rate limits, and transport security. The crate alone does not provide tenant isolation, persistence, or a production audit trail, and has not been verified as a hosted service.

The optional `AUDIT_STREAM_URL` hook attempts to send aggregate ingest counts to `/events`. Only absolute HTTP(S) base URLs without embedded credentials, query, or fragment are accepted; the default client does not follow redirects. Delivery is best-effort and can delay an ingest response by up to `AUDIT_STREAM_TIMEOUT_S` (default 2.5 seconds, maximum 30). It is not a durable audit receipt.

---

## Quick start

```bash
# Set a secret in this shell, then start the server. Use a secret manager for hosting.
export AEO_GRAPH_INGEST_TOKEN="$(openssl rand -hex 32)"
cargo run &
until curl -fsS http://localhost:8092/healthz >/dev/null; do sleep 0.2; done

# Ingest the bundled enriched example (three entities).
curl -X POST http://localhost:8092/ingest -H "Authorization: Bearer $AEO_GRAPH_INGEST_TOKEN" --data-binary @examples/sample.jsonl

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

## Potential integrations

This crate exposes query results but does not create watches or incident plans itself.

- **[aeo-crawler](https://github.com/mizcausevic-dev/aeo-crawler)** — its current summary JSONL requires an enriched export or adapter before this service can use it for graph queries.
- **[aeo-validator-service](https://github.com/mizcausevic-dev/aeo-validator-service)** — a separate integration could use node results to select URLs for drift watches after validating the service contract.
- **[incident-correlation-rs](https://github.com/mizcausevic-dev/incident-correlation-rs)** — a separate integration could use claim search to find candidate affected entities; it must verify the claims before remediation.

---

## Bench

```bash
cargo bench
```

Bundled bench ingests a synthetic 2000-node chain so you can spot regressions in the parse + wire-edges pass.

---

## Tests

```bash
cargo test --locked --all-targets
cargo test --locked --doc
cargo clippy --locked --all-targets -- -Dwarnings
cargo fmt --all -- --check
```

CI matrix: `stable`, `beta`, `1.88.0` (MSRV). End-to-end HTTP tests run via `tower::ServiceExt` — no real network.

---

## License

MIT. See [LICENSE](LICENSE).
