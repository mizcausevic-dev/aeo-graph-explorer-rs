//! Throughput micro-bench for graph ingestion.
//!
//! Builds a synthetic JSONL with N nodes (chain shape) and times the
//! `from_jsonl` parse + wire-edges pass. Run with `cargo bench`.

use std::time::Duration;

use aeo_graph_explorer::AeoGraph;
use criterion::{criterion_group, criterion_main, Criterion};

fn make_jsonl(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        let id = format!("https://node-{i}.example/#org");
        let peer = if i + 1 < n {
            format!(r#",[{{"id":"https://node-{}.example/#org"}}]"#, i + 1)
        } else {
            ",[]".to_string()
        };
        out.push_str(&format!(
            r#"{{"id":"{id}","entity":{{"id":"{id}","kind":"Organization","name":"N{i}"}},"body":{{"peers":{peer},"claims":[{{"id":"c{i}","predicate":"industry","value":"x"}}]}}}}"#,
            id = id,
            peer = peer.trim_start_matches(',')
        ));
        out.push('\n');
    }
    out
}

fn bench_build(c: &mut Criterion) {
    let jsonl = make_jsonl(2_000);

    c.bench_function("graph_build_2k_nodes", |b| {
        b.iter(|| {
            let g = AeoGraph::from_jsonl(&jsonl).unwrap();
            assert_eq!(g.node_count(), 2_000);
        });
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20).measurement_time(Duration::from_secs(3));
    targets = bench_build
}
criterion_main!(benches);
