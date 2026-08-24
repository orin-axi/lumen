use chrono::DateTime;
use compact_str::CompactString;
use lumen_model::*;
use smallvec::smallvec;

#[test]
fn test_canonical_transcript_roundtrip() {
    let fixed_ts = DateTime::from_timestamp(1771416000, 0).unwrap();
    let pricing = PricingTable::seed();

    let transcript = CanonicalTranscript {
        session_id: CompactString::new("sess-123"),
        parent_session_id: None,
        subagent_role: None,
        orchestrator: OrchestratorKind::ClaudeCode,
        model_family: CompactString::new("claude-3-5-sonnet-20241022"),
        timing: ExecutionTiming {
            started_at: fixed_ts,
            ended_at: fixed_ts,
            wall_duration_ms: 12500,
            active_duration_ms: 12500,
            idle_duration_ms: 0,
            idle_gap_count: 0,
        },
        economics: TokenEconomics::calculate(
            &[TurnPricingInput {
                usage: TurnTokenUsage {
                    input_tokens: 1000,
                    output_tokens: 200,
                    cache_creation_tokens: 4000,
                    cache_read_tokens: 15000,
                    reasoning_tokens: 0,
                    cache_creation_1h_tokens: 0,
                },
                timestamp: fixed_ts,
                tier: None,
            }],
            "claude-3-5-sonnet-20241022",
            &pricing,
            None,
        ),
        turns: vec![CanonicalTurn {
            attribution: None,
            turn_index: 0,
            role: TurnRole::Assistant,
            timestamp: fixed_ts,
            latency_ms: 1200,
            text: Some("Inspecting workspace".into()),
            tool_calls: smallvec![CanonicalToolCall {
                call_id: CompactString::new("call_001"),
                tool_name: CompactString::new("view_file"),
                intent: ToolIntent::FileRead { path: CompactString::new("src/lib.rs"), line_range: Some((1, 50)) },
                raw_arguments: serde_json::json!({"AbsolutePath": "src/lib.rs"}),
            }],
            tool_results: smallvec![],
            usage: Some(TurnTokenUsage {
                input_tokens: 1000,
                output_tokens: 200,
                cache_creation_tokens: 4000,
                cache_read_tokens: 15000,
                reasoning_tokens: 0,
                cache_creation_1h_tokens: 0,
            }),
        }],
        subagents: vec![],
        extracted_schemas: smallvec![SchemaCitation {
            schema_id: CompactString::new("spec@1"),
            turn_index: 0,
            is_valid: true,
            summary: Some(CompactString::new("SPEC-PRISM-001")),
        }],
        detected_anomalies: smallvec![],
        otel_conversation_id: None,
        service_tier: None,
        parse_failures: smallvec![],
    };

    let serialized = serde_json::to_string(&transcript).unwrap();
    let deserialized: CanonicalTranscript = serde_json::from_str(&serialized).unwrap();

    assert_eq!(transcript, deserialized);
    assert_eq!(deserialized.turns.len(), 1);
    assert_eq!(deserialized.extracted_schemas[0].schema_id, "spec@1");
}

#[test]
fn test_canonical_transcript_full_hierarchy_roundtrip() {
    let fixed_ts_start = DateTime::from_timestamp(1771416000, 0).unwrap();
    let fixed_ts_end = DateTime::from_timestamp(1771416015, 0).unwrap();
    let pricing = PricingTable::seed();

    let child_subagent = CanonicalTranscript {
        session_id: CompactString::new("subagent-child-01"),
        parent_session_id: Some(CompactString::new("parent-root-00")),
        subagent_role: None,
        orchestrator: OrchestratorKind::Antigravity,
        model_family: CompactString::new("claude-3-5-haiku-20241022"),
        timing: ExecutionTiming {
            started_at: fixed_ts_start,
            ended_at: fixed_ts_end,
            wall_duration_ms: 3000,
            active_duration_ms: 3000,
            idle_duration_ms: 0,
            idle_gap_count: 0,
        },
        economics: TokenEconomics::calculate(
            &[TurnPricingInput {
                usage: TurnTokenUsage {
                    input_tokens: 5000,
                    output_tokens: 1000,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 10000,
                    reasoning_tokens: 0,
                    cache_creation_1h_tokens: 0,
                },
                timestamp: fixed_ts_start,
                tier: None,
            }],
            "claude-3-5-haiku-20241022",
            &pricing,
            None,
        ),
        turns: vec![CanonicalTurn {
            attribution: None,
            turn_index: 0,
            role: TurnRole::Assistant,
            timestamp: fixed_ts_start,
            latency_ms: 400,
            text: Some("Child subagent finished audit".into()),
            tool_calls: smallvec![],
            tool_results: smallvec![],
            usage: None,
        }],
        subagents: vec![],
        extracted_schemas: smallvec![],
        detected_anomalies: smallvec![],
        otel_conversation_id: None,
        service_tier: None,
        parse_failures: smallvec![],
    };

    let parent_transcript = CanonicalTranscript {
        session_id: CompactString::new("parent-root-00"),
        parent_session_id: None,
        subagent_role: None,
        orchestrator: OrchestratorKind::Antigravity,
        model_family: CompactString::new("claude-3-5-sonnet-20241022"),
        timing: ExecutionTiming {
            started_at: fixed_ts_start,
            ended_at: fixed_ts_end,
            wall_duration_ms: 15000,
            active_duration_ms: 12000,
            idle_duration_ms: 3000,
            idle_gap_count: 1,
        },
        economics: TokenEconomics::calculate(
            &[TurnPricingInput {
                usage: TurnTokenUsage {
                    input_tokens: 20000,
                    output_tokens: 4000,
                    cache_creation_tokens: 5000,
                    cache_read_tokens: 80000,
                    reasoning_tokens: 0,
                    cache_creation_1h_tokens: 0,
                },
                timestamp: fixed_ts_start,
                tier: None,
            }],
            "claude-3-5-sonnet-20241022",
            &pricing,
            None,
        ),
        turns: vec![
            CanonicalTurn {
                attribution: None,
                turn_index: 0,
                role: TurnRole::User,
                timestamp: fixed_ts_start,
                latency_ms: 0,
                text: Some("Audit workspace".into()),
                tool_calls: smallvec![],
                tool_results: smallvec![],
                usage: None,
            },
            CanonicalTurn {
                attribution: None,
                turn_index: 1,
                role: TurnRole::Assistant,
                timestamp: fixed_ts_start,
                latency_ms: 800,
                text: Some("Spawning subagent and reading config".into()),
                tool_calls: smallvec![
                    CanonicalToolCall {
                        call_id: CompactString::new("call_sub_1"),
                        tool_name: CompactString::new("invoke_subagent"),
                        intent: ToolIntent::SubagentSpawn {
                            agent_type: CompactString::new("auditor"),
                            description: CompactString::new("audit codebase"),
                        },
                        raw_arguments: serde_json::json!({"name": "auditor"}),
                    },
                    CanonicalToolCall {
                        call_id: CompactString::new("call_edit_1"),
                        tool_name: CompactString::new("replace_file_content"),
                        intent: ToolIntent::FileEdit {
                            path: CompactString::new("src/main.rs"),
                            lines_added: 10,
                            lines_removed: 3,
                        },
                        raw_arguments: serde_json::json!({}),
                    },
                ],
                tool_results: smallvec![],
                usage: Some(TurnTokenUsage {
                    input_tokens: 2000,
                    output_tokens: 400,
                    cache_creation_tokens: 500,
                    cache_read_tokens: 8000,
                    reasoning_tokens: 0,
                    cache_creation_1h_tokens: 0,
                }),
            },
            CanonicalTurn {
                attribution: None,
                turn_index: 2,
                role: TurnRole::ToolResult,
                timestamp: fixed_ts_end,
                latency_ms: 100,
                text: None,
                tool_calls: smallvec![],
                tool_results: smallvec![CanonicalToolResult {
                    call_id: CompactString::new("call_edit_1"),
                    output_bytes: 120,
                    line_count: 5,
                    is_error: false,
                    error_class: None,
                    truncated_output: None,
                    otel_span_id: None,
                }],
                usage: None,
            },
        ],
        subagents: vec![child_subagent],
        extracted_schemas: smallvec![
            SchemaCitation {
                schema_id: CompactString::new("spec@1"),
                turn_index: 1,
                is_valid: true,
                summary: Some(CompactString::new("SPEC-001")),
            },
            SchemaCitation {
                schema_id: CompactString::new("plan@1"),
                turn_index: 1,
                is_valid: true,
                summary: Some(CompactString::new("PLAN-001")),
            },
        ],
        detected_anomalies: smallvec![
            TrajectoryAnomaly::CircularLoop { symbol: CompactString::new("get_user"), cycle_depth: 3 },
            TrajectoryAnomaly::ContextFlood { turns: 5, uncompressed_tokens: 250000 },
            TrajectoryAnomaly::GateStall { agent_pair: CompactString::new("drafter->auditor"), observed_rounds: 3 },
            TrajectoryAnomaly::UngroundedDrafting {
                missing_symbol: CompactString::new("verify_token"),
                target_file: CompactString::new("src/auth.rs"),
            },
        ],
        otel_conversation_id: None,
        service_tier: None,
        parse_failures: smallvec![],
    };

    let json_str = serde_json::to_string_pretty(&parent_transcript).expect("Serialization failed");
    let deserialized: CanonicalTranscript = serde_json::from_str(&json_str).expect("Deserialization failed");

    // CRIT-LUMEN-001: struct equality with zero data loss
    assert_eq!(parent_transcript, deserialized);
    assert_eq!(deserialized.subagents.len(), 1);
    assert_eq!(deserialized.subagents[0].session_id, "subagent-child-01");
    assert_eq!(deserialized.detected_anomalies.len(), 4);
    assert_eq!(deserialized.extracted_schemas.len(), 2);
}

fn transcript_with_one_turn(
    session_id: &str,
    model: &str,
    input_tokens: u64,
    pricing: &PricingTable,
) -> CanonicalTranscript {
    let ts = DateTime::from_timestamp(1771416000, 0).unwrap();
    CanonicalTranscript {
        session_id: CompactString::new(session_id),
        parent_session_id: None,
        subagent_role: None,
        orchestrator: OrchestratorKind::ClaudeCode,
        model_family: CompactString::new(model),
        timing: ExecutionTiming {
            started_at: ts,
            ended_at: ts,
            wall_duration_ms: 1000,
            active_duration_ms: 1000,
            idle_duration_ms: 0,
            idle_gap_count: 0,
        },
        economics: TokenEconomics::calculate(
            &[TurnPricingInput {
                usage: TurnTokenUsage {
                    input_tokens,
                    output_tokens: 100,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    reasoning_tokens: 0,
                    cache_creation_1h_tokens: 0,
                },
                timestamp: ts,
                tier: None,
            }],
            model,
            pricing,
            None,
        ),
        turns: vec![],
        subagents: vec![],
        extracted_schemas: smallvec![],
        detected_anomalies: smallvec![],
        otel_conversation_id: None,
        service_tier: None,
        parse_failures: smallvec![],
    }
}

#[test]
fn test_rolled_up_economics_sums_subagent_cost_into_root() {
    // CRIT-LUMEN-176: root.economics alone silently excludes subagent spend -- confirms
    // rolled_up_economics closes that gap by summing root + every subagent (recursively).
    let pricing = PricingTable::seed();

    let grandchild = transcript_with_one_turn("grandchild", "claude-3-5-sonnet-20241022", 1000, &pricing);
    let mut child = transcript_with_one_turn("child", "claude-3-5-haiku-20241022", 2000, &pricing);
    child.subagents = vec![grandchild.clone()];
    let mut root = transcript_with_one_turn("root", "claude-3-5-sonnet-20241022", 4000, &pricing);
    root.subagents = vec![child.clone()];

    let rolled = root.rolled_up_economics();

    let expected_cost =
        root.economics.total_cost_usd + child.economics.total_cost_usd + grandchild.economics.total_cost_usd;
    assert!((rolled.total_cost_usd - expected_cost).abs() < 1e-9);
    assert_eq!(
        rolled.input_tokens,
        root.economics.input_tokens + child.economics.input_tokens + grandchild.economics.input_tokens
    );
    // root.economics alone (the pre-existing, still-available field) does NOT include the
    // subagent spend -- this is the exact undercount rolled_up_economics fixes.
    assert!(rolled.total_cost_usd > root.economics.total_cost_usd);

    // Root and grandchild share a model (sonnet); child uses haiku -- per_model merges same-model
    // entries across the tree rather than only tracking the root's own model.
    assert_eq!(rolled.per_model.len(), 2);
    let sonnet = rolled.per_model.get("claude-3-5-sonnet-20241022").unwrap();
    assert_eq!(sonnet.turns, 2);
    assert!((sonnet.cost_usd - (root.economics.total_cost_usd + grandchild.economics.total_cost_usd)).abs() < 1e-9);
}

#[test]
fn test_rolled_up_economics_is_fully_priced_is_and_of_whole_tree() {
    let pricing = PricingTable::seed();
    let unpriced_child = transcript_with_one_turn("child", "totally-unrecognized-model-xyz", 1000, &pricing);
    let mut root = transcript_with_one_turn("root", "claude-3-5-sonnet-20241022", 4000, &pricing);
    root.subagents = vec![unpriced_child];

    assert!(root.economics.is_fully_priced);
    assert!(!root.rolled_up_economics().is_fully_priced);
}

#[test]
fn test_rolled_up_economics_with_no_subagents_matches_own_economics_cost() {
    let pricing = PricingTable::seed();
    let root = transcript_with_one_turn("root", "claude-3-5-sonnet-20241022", 4000, &pricing);

    let rolled = root.rolled_up_economics();
    assert_eq!(rolled.total_cost_usd, root.economics.total_cost_usd);
    assert_eq!(rolled.input_tokens, root.economics.input_tokens);
}
