use compact_str::CompactString;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lumen_pattern::{ToolNode, TrajectoryGraph};

fn graph_with_cycle(node_count: usize) -> TrajectoryGraph {
    let mut graph = TrajectoryGraph::new();
    for i in 0..node_count {
        // Every 5th run of 4 nodes shares a target_symbol with zero mutation, forming a
        // detectable circular-exploration loop (depth >= 3, per AGENTS.md rule 4).
        let in_loop = i % 5 < 4;
        graph.push_tool(ToolNode {
            turn_index: i,
            tool_name: CompactString::new("Grep"),
            target_symbol: if in_loop {
                Some(CompactString::new("shared_symbol"))
            } else {
                Some(CompactString::new(format!("symbol_{i}")))
            },
            target_file: Some(CompactString::new("src/lib.rs")),
            is_mutation: false,
            had_error: false,
        });
    }
    graph
}

fn tarjan_benchmark(c: &mut Criterion) {
    let graph_500 = graph_with_cycle(500);

    c.bench_function("detect_circular_loops_500_nodes", |b| b.iter(|| black_box(graph_500.detect_circular_loops())));
}

criterion_group!(benches, tarjan_benchmark);
criterion_main!(benches);
