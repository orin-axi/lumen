use chrono::Utc;
use compact_str::CompactString;
use lumen_analysis::detect_trajectory_anomalies;
use lumen_model::*;
use smallvec::smallvec;

fn spawn_turn(turn_index: usize, agent_type: &str) -> CanonicalTurn {
    CanonicalTurn {
        attribution: None,
        turn_index,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 100,
        text: None,
        tool_calls: smallvec![CanonicalToolCall {
            call_id: CompactString::new(format!("call_{turn_index}")),
            tool_name: CompactString::new("invoke_subagent"),
            intent: ToolIntent::SubagentSpawn {
                agent_type: CompactString::new(agent_type),
                description: CompactString::new("review this"),
            },
            raw_arguments: serde_json::json!({}),
        }],
        tool_results: smallvec![],
        usage: None,
    }
}

fn code_search_turn(turn_index: usize, query: &str) -> CanonicalTurn {
    CanonicalTurn {
        attribution: None,
        turn_index,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 100,
        text: None,
        tool_calls: smallvec![CanonicalToolCall {
            call_id: CompactString::new(format!("call_{turn_index}")),
            tool_name: CompactString::new("grep_search"),
            intent: ToolIntent::CodeSearch {
                tool: CompactString::new("grep_search"),
                query: CompactString::new(query),
                is_ast: false,
            },
            raw_arguments: serde_json::json!({}),
        }],
        tool_results: smallvec![],
        usage: None,
    }
}

fn transcript_with_turns(turns: Vec<CanonicalTurn>) -> CanonicalTranscript {
    let now = Utc::now();
    CanonicalTranscript {
        session_id: CompactString::new("sess"),
        parent_session_id: None,
        subagent_role: None,
        orchestrator: OrchestratorKind::ClaudeCode,
        model_family: CompactString::new("claude-3-5-sonnet-20241022"),
        timing: ExecutionTiming {
            started_at: now,
            ended_at: now,
            wall_duration_ms: 0,
            active_duration_ms: 0,
            idle_duration_ms: 0,
            idle_gap_count: 0,
        },
        economics: TokenEconomics::default(),
        turns,
        subagents: vec![],
        extracted_schemas: smallvec![],
        detected_anomalies: smallvec![],
        otel_conversation_id: None,
        service_tier: None,
        parse_failures: smallvec![],
    }
}

#[test]
fn test_detect_trajectory_anomalies_finds_circular_loop_from_repeated_search_target() {
    // CRIT-LUMEN-179: same real detector lumen-pattern's own tests exercise directly
    // (tarjan_scc over 3+ same-target reads), reached this time through the full
    // CanonicalTranscript-level glue function rather than TrajectoryGraph directly.
    let transcript = transcript_with_turns(vec![
        code_search_turn(0, "get_balance"),
        code_search_turn(1, "get_balance"),
        code_search_turn(2, "get_balance"),
    ]);

    let anomalies = detect_trajectory_anomalies(&transcript);

    assert_eq!(anomalies.len(), 1);
    assert!(matches!(
        &anomalies[0],
        TrajectoryAnomaly::CircularLoop { symbol, cycle_depth } if symbol == "get_balance" && *cycle_depth == 3
    ));
}

#[test]
fn test_detect_trajectory_anomalies_finds_gate_stall_from_repeated_subagent_spawns() {
    let transcript =
        transcript_with_turns(vec![spawn_turn(0, "auditor"), spawn_turn(1, "auditor"), spawn_turn(2, "auditor")]);

    let anomalies = detect_trajectory_anomalies(&transcript);

    assert_eq!(anomalies.len(), 1);
    assert!(matches!(
        &anomalies[0],
        TrajectoryAnomaly::GateStall { agent_pair, observed_rounds }
            if agent_pair == "parent->auditor" && *observed_rounds == 3
    ));
}

#[test]
fn test_detect_trajectory_anomalies_finds_both_kinds_together() {
    let transcript = transcript_with_turns(vec![
        code_search_turn(0, "get_balance"),
        code_search_turn(1, "get_balance"),
        code_search_turn(2, "get_balance"),
        spawn_turn(3, "auditor"),
        spawn_turn(4, "auditor"),
        spawn_turn(5, "auditor"),
    ]);

    let anomalies = detect_trajectory_anomalies(&transcript);

    assert_eq!(anomalies.len(), 2);
    assert!(anomalies.iter().any(|a| matches!(a, TrajectoryAnomaly::CircularLoop { .. })));
    assert!(anomalies.iter().any(|a| matches!(a, TrajectoryAnomaly::GateStall { .. })));
}

#[test]
fn test_detect_trajectory_anomalies_empty_for_normal_session() {
    let transcript = transcript_with_turns(vec![
        code_search_turn(0, "get_balance"),
        spawn_turn(1, "auditor"),
        spawn_turn(2, "reviewer"),
    ]);

    let anomalies = detect_trajectory_anomalies(&transcript);

    assert!(anomalies.is_empty());
}
