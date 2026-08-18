use compact_str::CompactString;
use lumen_pattern::*;

#[test]
fn test_tarjan_scc_detects_3_cycle_loop() {
    let mut g = TrajectoryGraph::new();

    let n1 = g.push_tool(ToolNode {
        turn_index: 0,
        tool_name: CompactString::new("grep_search"),
        target_symbol: Some(CompactString::new("get_balance")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    let _n2 = g.push_tool(ToolNode {
        turn_index: 1,
        tool_name: CompactString::new("view_file"),
        target_symbol: Some(CompactString::new("get_balance")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    let n3 = g.push_tool(ToolNode {
        turn_index: 2,
        tool_name: CompactString::new("grep_search"),
        target_symbol: Some(CompactString::new("get_balance")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    // Close the cycle
    g.graph.add_edge(n3, n1, ());

    let loops = g.detect_circular_loops();
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].symbol, "get_balance");
    assert_eq!(loops[0].cycle_depth, 3);
}

#[test]
fn test_monotonicity_score_drops_on_cycle() {
    let mut g = TrajectoryGraph::new();

    let n1 = g.push_tool(ToolNode {
        turn_index: 0,
        tool_name: CompactString::new("view_file"),
        target_symbol: Some(CompactString::new("sym_a")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    let _n2 = g.push_tool(ToolNode {
        turn_index: 1,
        tool_name: CompactString::new("view_file"),
        target_symbol: Some(CompactString::new("sym_a")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    let n3 = g.push_tool(ToolNode {
        turn_index: 2,
        tool_name: CompactString::new("view_file"),
        target_symbol: Some(CompactString::new("sym_a")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    g.graph.add_edge(n3, n1, ());

    let monotonicity = g.calculate_monotonicity();
    assert!(monotonicity < 1.0);
}
