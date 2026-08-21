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
