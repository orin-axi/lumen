use chrono::Utc;
use compact_str::CompactString;
use lumen_model::{
    CanonicalToolCall, CanonicalTranscript, CanonicalTurn, ExecutionTiming, OrchestratorKind, PricingTable,
    TokenEconomics, ToolIntent, TurnPricingInput, TurnRole, TurnTokenUsage,
};
use lumen_pattern::*;
use smallvec::smallvec;

fn code_search_turn(turn_index: usize, query: &str) -> CanonicalTurn {
    CanonicalTurn {
        attribution: None,
        turn_index,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 0,
        text: None,
        tool_calls: smallvec![CanonicalToolCall {
            call_id: CompactString::new(format!("call_{turn_index}")),
            tool_name: CompactString::new("grep_search"),
            intent: ToolIntent::CodeSearch {
                tool: CompactString::new("grep"),
                query: CompactString::new(query),
                is_ast: false,
            },
            raw_arguments: serde_json::Value::Null,
        }],
        tool_results: smallvec![],
        usage: None,
    }
}

#[test]
fn test_calculate_monotonicity_free_function_detects_repeated_read_cycle() {
    // CRIT-LUMEN-155: 3 consecutive tool invocations grounded on the same target
    // symbol with no interleaving mutation must make calculate_monotonicity(transcript)
    // return M < 1.0 through the PUBLIC calculate_monotonicity(&CanonicalTranscript) entry
    // point -- not just via manually-constructed TrajectoryGraph as the other tests in this
    // file do.
    let transcript = CanonicalTranscript {
        session_id: CompactString::new("s1"),
        parent_session_id: None,
        subagent_role: None,
        orchestrator: OrchestratorKind::ClaudeCode,
        model_family: CompactString::new("claude-3-5-sonnet-20241022"),
        timing: ExecutionTiming {
            started_at: Utc::now(),
            ended_at: Utc::now(),
            wall_duration_ms: 0,
            active_duration_ms: 0,
            idle_duration_ms: 0,
            idle_gap_count: 0,
        },
        economics: TokenEconomics::calculate(
            &[TurnPricingInput { usage: TurnTokenUsage::default(), timestamp: Utc::now(), tier: None }],
            "claude-3-5-sonnet-20241022",
            &PricingTable::seed(),
            None,
        ),
        turns: vec![
            code_search_turn(0, "get_balance"),
            code_search_turn(1, "get_balance"),
            code_search_turn(2, "get_balance"),
        ],
        subagents: vec![],
        extracted_schemas: smallvec![],
        detected_anomalies: smallvec![],
        otel_conversation_id: None,
        service_tier: None,
        parse_failures: smallvec![],
    };

    let m = calculate_monotonicity(&transcript);
    assert!(m < 1.0, "expected M < 1.0 for a 3-node repeated-read cycle, got {m}");
}

#[test]
fn test_3_node_cycles_on_identical_symbols() {
    let mut g = TrajectoryGraph::new();

    let n1 = g.push_tool(ToolNode {
        turn_index: 0,
        tool_name: CompactString::new("read_config"),
        target_symbol: Some(CompactString::new("Config")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    let _n2 = g.push_tool(ToolNode {
        turn_index: 1,
        tool_name: CompactString::new("read_config"),
        target_symbol: Some(CompactString::new("Config")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    let n3 = g.push_tool(ToolNode {
        turn_index: 2,
        tool_name: CompactString::new("read_config"),
        target_symbol: Some(CompactString::new("Config")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    // Add back-edge to create cycle of length >= 3
    g.graph.add_edge(n3, n1, ());

    let loops = g.detect_circular_loops();
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].symbol, "Config");
    assert_eq!(loops[0].cycle_depth, 3);
}

#[test]
fn test_disconnected_subgraphs_with_partial_cycles() {
    let mut g = TrajectoryGraph::new();

    // Component 1: linear progress without cycles
    let _c1 = g.push_tool(ToolNode {
        turn_index: 0,
        tool_name: CompactString::new("init_repo"),
        target_symbol: None,
        target_file: Some(CompactString::new("Cargo.toml")),
        is_mutation: true,
        had_error: false,
    });

    // Component 2: 3-cycle stall
    let n1 = g.push_tool(ToolNode {
        turn_index: 1,
        tool_name: CompactString::new("edit_main"),
        target_symbol: Some(CompactString::new("App")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    let _n2 = g.push_tool(ToolNode {
        turn_index: 2,
        tool_name: CompactString::new("run_tests"),
        target_symbol: Some(CompactString::new("App")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    let n3 = g.push_tool(ToolNode {
        turn_index: 3,
        tool_name: CompactString::new("edit_main"),
        target_symbol: Some(CompactString::new("App")),
        target_file: None,
        is_mutation: false,
        had_error: false,
    });

    g.graph.add_edge(n3, n1, ());

    let loops = g.detect_circular_loops();
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].symbol, "App");

    let monotonicity = g.calculate_monotonicity();
    assert!(monotonicity < 1.0);
}

#[test]
fn test_deep_linear_acyclic_dag() {
    let mut g = TrajectoryGraph::new();

    for i in 0..50 {
        g.push_tool(ToolNode {
            turn_index: i,
            tool_name: CompactString::new(format!("step_{i}")),
            target_symbol: Some(CompactString::new(format!("sym_{i}"))),
            target_file: None,
            is_mutation: true,
            had_error: false,
        });
    }

    let loops = g.detect_circular_loops();
    assert_eq!(loops.len(), 0);

    let monotonicity = g.calculate_monotonicity();
    assert!((monotonicity - 1.0).abs() < 1e-4);
}

#[test]
fn test_3_node_cycle_grounded_on_file_alone_is_detected() {
    // CRIT-LUMEN-070 follow-up: FileRead-derived nodes ground on target_file only
    // (target_symbol is always None for them, per ground_tool_intent). push_tool
    // already closes a back-edge for file-grounded repeats (target_symbol.or(target_file)
    // key), so this SCC exists in the graph either way -- the bug is that
    // detect_circular_loops used to check target_symbol alone, so it silently dropped
    // any component whose nodes were grounded on target_file alone. This test proves
    // the file-based cycle is now detected and penalized, not just topologically present.
    let mut g = TrajectoryGraph::new();

    let n1 = g.push_tool(ToolNode {
        turn_index: 0,
        tool_name: CompactString::new("view_file"),
        target_symbol: None,
        target_file: Some(CompactString::new("src/lib.rs")),
        is_mutation: false,
        had_error: false,
    });

    let _n2 = g.push_tool(ToolNode {
        turn_index: 1,
        tool_name: CompactString::new("view_file"),
        target_symbol: None,
        target_file: Some(CompactString::new("src/lib.rs")),
        is_mutation: false,
        had_error: false,
    });

    let n3 = g.push_tool(ToolNode {
        turn_index: 2,
        tool_name: CompactString::new("view_file"),
        target_symbol: None,
        target_file: Some(CompactString::new("src/lib.rs")),
        is_mutation: false,
        had_error: false,
    });

    // push_tool already closes this back-edge itself via the file-grounded key, but we
    // add it explicitly too so this test's cycle doesn't depend on push_tool's internal
    // bookkeeping -- it isolates detect_circular_loops as the thing under test.
    g.graph.add_edge(n3, n1, ());

    let loops = g.detect_circular_loops();
    assert_eq!(loops.len(), 1, "expected a single file-grounded cycle to be reported, got {loops:?}");
    assert_eq!(loops[0].symbol, "src/lib.rs");
    assert!(loops[0].cycle_depth >= 3);

    let monotonicity = g.calculate_monotonicity();
    assert!(monotonicity < 1.0, "expected monotonicity to be penalized for the file-read loop, got {monotonicity}");
}

#[test]
fn test_empty_and_single_step_graphs() {
    let empty = TrajectoryGraph::new();
    assert_eq!(empty.detect_circular_loops().len(), 0);
    assert_eq!(empty.calculate_monotonicity(), 1.0);

    let mut single = TrajectoryGraph::new();
    single.push_tool(ToolNode {
        turn_index: 0,
        tool_name: CompactString::new("only_step"),
        target_symbol: None,
        target_file: None,
        is_mutation: true,
        had_error: false,
    });
    assert_eq!(single.detect_circular_loops().len(), 0);
    assert_eq!(single.calculate_monotonicity(), 1.0);
}
