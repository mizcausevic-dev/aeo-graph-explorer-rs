//! Unit tests for the graph builder + query layer (no HTTP).

use aeo_graph_explorer::{neighbors, shortest_path, AeoGraph, EdgeKind};

const JSONL: &str = r#"
{"id":"https://acme.example/#org","entity":{"id":"https://acme.example/#org","kind":"Organization","name":"Acme","canonical_url":"https://acme.example/"},"body":{"aeo_version":"0.1","peers":[{"id":"https://other.example/#org"}],"authority":{"primary_sources":["https://acme.example/"]},"claims":[{"id":"c1","predicate":"industry","value":"AI tutoring","confidence":"high"}]}}
{"id":"https://other.example/#org","entity":{"id":"https://other.example/#org","kind":"Organization","name":"Other","canonical_url":"https://other.example/"},"body":{"aeo_version":"0.1","peers":[{"id":"https://third.example/#org"}],"claims":[{"id":"c2","predicate":"industry","value":"AI tutoring","confidence":"medium"}]}}
{"id":"https://third.example/#org","entity":{"id":"https://third.example/#org","kind":"Organization","name":"Third","canonical_url":"https://third.example/"},"body":{"aeo_version":"0.1","claims":[{"id":"c3","predicate":"industry","value":"finance","confidence":"high"}]}}
"#;

#[test]
fn jsonl_round_trips_into_a_three_node_graph() {
    let g = AeoGraph::from_jsonl(JSONL).unwrap();
    assert_eq!(g.node_count(), 3);
    // peers wires acme -> other -> third = 2 edges
    assert!(g.edge_count() >= 2);
}

#[test]
fn blank_lines_are_skipped() {
    let with_blanks = format!("\n{JSONL}\n\n");
    let g = AeoGraph::from_jsonl(&with_blanks).unwrap();
    assert_eq!(g.node_count(), 3);
}

#[test]
fn malformed_line_returns_jsonline_error() {
    let bad = "not json\n";
    let err = AeoGraph::from_jsonl(bad).unwrap_err();
    match err {
        aeo_graph_explorer::GraphError::JsonLine { line, .. } => assert_eq!(line, 1),
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn duplicate_ids_are_rejected_instead_of_silently_replaced() {
    let line = r#"{"id":"a","entity":{"id":"a"},"body":{}}"#;
    let err = AeoGraph::from_jsonl(&format!("{line}\n{line}\n")).unwrap_err();
    assert!(matches!(
        err,
        aeo_graph_explorer::GraphError::DuplicateNode(2)
    ));
}

#[test]
fn node_and_summary_ids_must_match() {
    let err = AeoGraph::from_jsonl(r#"{"id":"a","entity":{"id":"b"}}"#).unwrap_err();
    assert!(matches!(
        err,
        aeo_graph_explorer::GraphError::InvalidNode(1)
    ));
}

#[test]
fn enriched_rows_require_a_body_object() {
    let err = AeoGraph::from_jsonl(r#"{"id":"a","entity":{"id":"a"}}"#).unwrap_err();
    assert!(matches!(
        err,
        aeo_graph_explorer::GraphError::InvalidBody(1)
    ));
}

#[test]
fn wiring_edges_twice_is_idempotent() {
    let mut graph = AeoGraph::from_jsonl(JSONL).unwrap();
    let edges = graph.edge_count();
    graph.wire_edges();
    assert_eq!(graph.edge_count(), edges);
}

#[test]
fn empty_upload_cannot_replace_the_graph() {
    let err = AeoGraph::from_jsonl("\n \n").unwrap_err();
    assert!(matches!(err, aeo_graph_explorer::GraphError::EmptyGraph));
}

#[test]
fn neighbors_split_inbound_and_outbound() {
    let g = AeoGraph::from_jsonl(JSONL).unwrap();
    let view = neighbors(&g, "https://other.example/#org").unwrap();
    // Other declares Third as a peer => one outbound edge
    assert_eq!(view.outbound.len(), 1);
    assert_eq!(view.outbound[0].entity.id, "https://third.example/#org");
    // Acme declared Other as a peer => one inbound edge
    assert_eq!(view.inbound.len(), 1);
    assert_eq!(view.inbound[0].entity.id, "https://acme.example/#org");
    assert!(matches!(view.inbound[0].edge, EdgeKind::DeclaresPeer));
}

#[test]
fn neighbors_for_missing_node_is_404_equivalent() {
    let g = AeoGraph::from_jsonl(JSONL).unwrap();
    let err = neighbors(&g, "https://nope.example/").unwrap_err();
    assert!(matches!(err, aeo_graph_explorer::GraphError::NotFound(_)));
}

#[test]
fn shortest_path_finds_two_hop_route() {
    let g = AeoGraph::from_jsonl(JSONL).unwrap();
    let r = shortest_path(
        &g,
        "https://acme.example/#org",
        "https://third.example/#org",
    )
    .unwrap();
    assert!(r.found);
    assert_eq!(r.length, 2);
    assert_eq!(r.hops.len(), 3);
    assert_eq!(r.hops[0].entity.id, "https://acme.example/#org");
    assert_eq!(
        r.hops.last().unwrap().entity.id,
        "https://third.example/#org"
    );
}

#[test]
fn shortest_path_returns_not_found_for_disconnected_nodes() {
    let g = AeoGraph::from_jsonl(JSONL).unwrap();
    // Third has no outgoing edges, so going Third -> Acme has no path.
    let r = shortest_path(
        &g,
        "https://third.example/#org",
        "https://acme.example/#org",
    )
    .unwrap();
    assert!(!r.found);
    assert_eq!(r.hops.len(), 0);
}

#[test]
fn find_by_claim_predicate() {
    let g = AeoGraph::from_jsonl(JSONL).unwrap();
    let r = aeo_graph_explorer::query::find_by_claim(&g, Some("industry"), None).unwrap();
    assert_eq!(r.len(), 3);
}

#[test]
fn find_by_claim_value_string() {
    let g = AeoGraph::from_jsonl(JSONL).unwrap();
    let r = aeo_graph_explorer::query::find_by_claim(&g, None, Some("AI tutoring")).unwrap();
    assert_eq!(r.len(), 2);
}

#[test]
fn find_by_claim_predicate_and_value() {
    let g = AeoGraph::from_jsonl(JSONL).unwrap();
    let r = aeo_graph_explorer::query::find_by_claim(&g, Some("industry"), Some("AI tutoring"))
        .unwrap();
    assert_eq!(r.len(), 2);
    for m in &r {
        assert_eq!(m.predicate, "industry");
    }
}

#[test]
fn find_by_claim_empty_query_is_error() {
    let g = AeoGraph::from_jsonl(JSONL).unwrap();
    let err = aeo_graph_explorer::query::find_by_claim(&g, None, None).unwrap_err();
    assert!(matches!(err, aeo_graph_explorer::GraphError::EmptyQuery));
}

#[test]
fn crawler_graph_export_links_sources_and_preserves_parsed_claims() {
    let graph = AeoGraph::from_jsonl(include_str!("../examples/crawler-graph.jsonl")).unwrap();
    assert_eq!(graph.node_count(), 2);
    assert_eq!(graph.edge_count(), 1);
    let alpha = graph.node("https://alpha.example/#org").unwrap();
    assert_eq!(
        alpha.provenance.as_ref().unwrap().origin,
        "https://alpha.example"
    );
    assert_eq!(alpha.body["claims"][0]["predicate"], "industry");
    let path = shortest_path(
        &graph,
        "https://alpha.example/#org",
        "https://beta.example/#org",
    )
    .unwrap();
    assert!(path.found);
    assert_eq!(path.length, 1);
    assert!(matches!(path.hops[0].via, Some(EdgeKind::CitesAuthority)));
    let claims =
        aeo_graph_explorer::find_by_claim(&graph, Some("industry"), Some("education")).unwrap();
    assert_eq!(claims.len(), 2);
}

#[test]
fn duplicate_or_untrusted_crawl_origins_are_rejected() {
    let first = r#"{"id":"a","entity":{"id":"a"},"body":{},"provenance":{"origin":"https://example.com","depth":0,"fetched_at":"now"}}"#;
    let duplicate = r#"{"id":"b","entity":{"id":"b"},"body":{},"provenance":{"origin":"https://example.com/path","depth":1,"fetched_at":"now"}}"#;
    let err = AeoGraph::from_jsonl(&format!("{first}\n{duplicate}\n")).unwrap_err();
    assert!(matches!(
        err,
        aeo_graph_explorer::GraphError::InvalidProvenance(2)
    ));

    let duplicate = r#"{"id":"b","entity":{"id":"b"},"body":{},"provenance":{"origin":"https://example.com/","depth":1,"fetched_at":"now"}}"#;
    let err = AeoGraph::from_jsonl(&format!("{first}\n{duplicate}\n")).unwrap_err();
    assert!(matches!(
        err,
        aeo_graph_explorer::GraphError::InvalidProvenance(2)
    ));

    let credentialed = first.replace("https://example.com", "https://user@example.com");
    let err = AeoGraph::from_jsonl(&credentialed).unwrap_err();
    assert!(matches!(
        err,
        aeo_graph_explorer::GraphError::InvalidProvenance(1)
    ));
}
