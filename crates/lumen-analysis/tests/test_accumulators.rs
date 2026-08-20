use chrono::Utc;
use compact_str::CompactString;
use lumen_analysis::*;
use lumen_model::*;
use smallvec::smallvec;

#[test]
fn test_circuit_breaker_trips_on_3_rounds() {
    let mut cb = CircuitBreakerAccumulator::default();

    for i in 0..3 {
        let turn = CanonicalTurn {
            turn_index: i,
            role: TurnRole::Assistant,
            timestamp: Utc::now(),
            latency_ms: 100,
            text: None,
            tool_calls: smallvec![CanonicalToolCall {
                call_id: CompactString::new("call_1"),
                tool_name: CompactString::new("invoke_subagent"),
                intent: ToolIntent::SubagentSpawn {
                    agent_type: CompactString::new("auditor"),
                    description: CompactString::new("audit this"),
                },
                raw_arguments: serde_json::json!({}),
            }],
            tool_results: smallvec![],
            usage: None,
        };
        cb.update(&turn);
    }

    let report = cb.finalize();
    assert_eq!(report.max_observed_rounds, 3);
    assert!(report.tripped);
    assert_eq!(report.stalls.len(), 1);
}

#[test]
fn test_turn_duration_p50_p95_percentiles() {
    let mut td = TurnDurationAccumulator::default();

    for lat in [100, 200, 300, 400, 500, 600, 700, 800, 900, 1000] {
        let turn = CanonicalTurn {
            turn_index: 0,
            role: TurnRole::Assistant,
            timestamp: Utc::now(),
            latency_ms: lat,
            text: None,
            tool_calls: smallvec![],
            tool_results: smallvec![],
            usage: None,
        };
        td.update(&turn);
    }

    let metrics = td.finalize();
    assert_eq!(metrics.total_turns, 10);
    assert_eq!(metrics.p50_ms, 600);
    assert_eq!(metrics.p95_ms, 1000);
}

#[test]
fn test_context_growth_and_autonomy_accumulators() {
    let mut cg = ContextGrowthAccumulator::default();
    let mut aut = AutonomyAccumulator::default();
    let mut art = ArtifactsAccumulator::default();
    let mut inv = ToolInventoryAccumulator::default();

    // Turn 0: User prompt
    let t0 = CanonicalTurn {
        turn_index: 0,
        role: TurnRole::User,
        timestamp: Utc::now(),
        latency_ms: 0,
        text: Some("Fix bug".into()),
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };
    cg.update(&t0);
    aut.update(&t0);
    art.update(&t0);
    inv.update(&t0);

    // Turn 1: Assistant reading file
    let t1 = CanonicalTurn {
        turn_index: 1,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 200,
        text: Some("Reading file".into()),
        tool_calls: smallvec![CanonicalToolCall {
            call_id: "c1".into(),
            tool_name: "view_file".into(),
            intent: ToolIntent::FileRead { path: "src/lib.rs".into(), line_range: None },
            raw_arguments: serde_json::json!({}),
        }],
        tool_results: smallvec![],
        usage: Some(TurnTokenUsage {
            input_tokens: 1000,
            cache_creation_tokens: 0,
            cache_read_tokens: 500,
            output_tokens: 50,
        }),
    };
    cg.update(&t1);
    aut.update(&t1);
    art.update(&t1);
    inv.update(&t1);

    // Turn 2: Assistant editing file
    let t2 = CanonicalTurn {
        turn_index: 2,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 300,
        text: Some("Editing file".into()),
        tool_calls: smallvec![CanonicalToolCall {
            call_id: "c2".into(),
            tool_name: "replace_file_content".into(),
            intent: ToolIntent::FileEdit { path: "src/lib.rs".into(), lines_added: 5, lines_removed: 2 },
            raw_arguments: serde_json::json!({}),
        }],
        tool_results: smallvec![],
        usage: Some(TurnTokenUsage {
            input_tokens: 1200,
            cache_creation_tokens: 0,
            cache_read_tokens: 500,
            output_tokens: 80,
        }),
    };
    cg.update(&t2);
    aut.update(&t2);
    art.update(&t2);
    inv.update(&t2);

    let cg_res = cg.finalize();
    assert_eq!(cg_res.initial_prompt_tokens, 1500);
    assert_eq!(cg_res.final_prompt_tokens, 1700);
    assert_eq!(cg_res.max_jump_tokens, 200);

    let aut_res = aut.finalize();
    assert_eq!(aut_res.max_autonomous_streak, 2);

    let art_res = art.finalize();
    assert_eq!(art_res.total_unique_files, 1);
    assert!(art_res.files_read.contains("src/lib.rs"));
    assert!(art_res.files_edited.contains("src/lib.rs"));

    let inv_res = inv.finalize();
    assert_eq!(inv_res.distinct_tools_count, 2);
    assert_eq!(inv_res.total_invocations, 2);
}

#[test]
fn test_api_health_and_mcp_affinity_accumulators() {
    let mut api = ApiHealthAccumulator::default();
    let mut mcp = McpAffinityAccumulator::default();
    let mut corr = SelfCorrectionAccumulator::default();

    // Turn 0: Failed tool with 429 rate limit
    let t0 = CanonicalTurn {
        turn_index: 0,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 1500,
        text: None,
        tool_calls: smallvec![CanonicalToolCall {
            call_id: "call_mcp_1".into(),
            tool_name: "mcp__github__get_issue".into(),
            intent: ToolIntent::McpCall { server: "github".into(), method: "get_issue".into() },
            raw_arguments: serde_json::json!({}),
        }],
        tool_results: smallvec![CanonicalToolResult {
            call_id: "call_mcp_1".into(),
            output_bytes: 50,
            line_count: 1,
            is_error: true,
            error_class: Some("429_rate_limit".into()),
            truncated_output: None,
        }],
        usage: None,
    };
    api.update(&t0);
    mcp.update(&t0);
    corr.update(&t0);

    // Turn 1: Fallback raw bash command that succeeds
    let t1 = CanonicalTurn {
        turn_index: 1,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 400,
        text: None,
        tool_calls: smallvec![CanonicalToolCall {
            call_id: "call_sh_1".into(),
            tool_name: "run_command".into(),
            intent: ToolIntent::Other { raw_name: "run_command".into() },
            raw_arguments: serde_json::json!({"command": "gh issue view 123"}),
        }],
        tool_results: smallvec![CanonicalToolResult {
            call_id: "call_sh_1".into(),
            output_bytes: 200,
            line_count: 5,
            is_error: false,
            error_class: None,
            truncated_output: None,
        }],
        usage: None,
    };
    api.update(&t1);
    mcp.update(&t1);
    corr.update(&t1);

    // CRIT-LUMEN-065: API Health metrics
    let api_res = api.finalize();
    assert_eq!(api_res.rate_limit_429_count, 1);
    assert_eq!(api_res.retry_count, 1);

    // CRIT-LUMEN-066: MCP Affinity metrics
    let mcp_res = mcp.finalize();
    assert_eq!(mcp_res.structured_mcp_count, 1);
    assert_eq!(mcp_res.raw_shell_count, 1);
    assert_eq!(mcp_res.mcp_adoption_ratio, 0.5);

    // CRIT-LUMEN-067: Self-correction (error followed by success)
    let corr_res = corr.finalize();
    assert_eq!(corr_res.tool_retry_corrections, 1);
}

#[test]
fn test_api_health_and_mcp_affinity_and_synthetic_exclusions() {
    let mut api = ApiHealthAccumulator::default();
    let mut mcp = McpAffinityAccumulator::default();
    let mut corr = SelfCorrectionAccumulator::default();

    // Turn 0: MCP call fails with 429 rate limit.
    let t0 = CanonicalTurn {
        turn_index: 0,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 1500,
        text: None,
        tool_calls: smallvec![CanonicalToolCall {
            call_id: "call_mcp_1".into(),
            tool_name: "mcp__jira__search".into(),
            intent: ToolIntent::McpCall { server: "jira".into(), method: "search".into() },
            raw_arguments: serde_json::json!({}),
        }],
        tool_results: smallvec![CanonicalToolResult {
            call_id: "call_mcp_1".into(),
            output_bytes: 40,
            line_count: 1,
            is_error: true,
            error_class: Some("rate_limit_429".into()),
            truncated_output: None,
        }],
        usage: None,
    };
    api.update(&t0);
    mcp.update(&t0);
    corr.update(&t0);

    // Turn 1: Retry via MCP fails with a 5xx server error -- also an approach pivot
    // (prior turn errored, this turn also errors after a retry attempt).
    let t1 = CanonicalTurn {
        turn_index: 1,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 800,
        text: None,
        tool_calls: smallvec![CanonicalToolCall {
            call_id: "call_mcp_2".into(),
            tool_name: "mcp__jira__search".into(),
            intent: ToolIntent::McpCall { server: "jira".into(), method: "search".into() },
            raw_arguments: serde_json::json!({}),
        }],
        tool_results: smallvec![CanonicalToolResult {
            call_id: "call_mcp_2".into(),
            output_bytes: 40,
            line_count: 1,
            is_error: true,
            error_class: Some("503_service_unavailable".into()),
            truncated_output: None,
        }],
        usage: None,
    };
    api.update(&t1);
    mcp.update(&t1);
    corr.update(&t1);

    // Turn 2: Falls back to a raw shell command which succeeds -- a tool_retry
    // self-correction (prior turn errored, this one succeeds).
    let t2 = CanonicalTurn {
        turn_index: 2,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 400,
        text: None,
        tool_calls: smallvec![CanonicalToolCall {
            call_id: "call_sh_1".into(),
            tool_name: "run_command".into(),
            intent: ToolIntent::Other { raw_name: "run_command".into() },
            raw_arguments: serde_json::json!({"command": "jira issue search"}),
        }],
        tool_results: smallvec![CanonicalToolResult {
            call_id: "call_sh_1".into(),
            output_bytes: 200,
            line_count: 5,
            is_error: false,
            error_class: None,
            truncated_output: None,
        }],
        usage: None,
    };
    api.update(&t2);
    mcp.update(&t2);
    corr.update(&t2);

    // CRIT-LUMEN-065: 429 and 5xx errors are both recorded with retry counts.
    let api_res = api.finalize();
    assert_eq!(api_res.rate_limit_429_count, 1);
    assert_eq!(api_res.server_error_5xx_count, 1);
    assert_eq!(api_res.retry_count, 2);

    // CRIT-LUMEN-066: ratio of structured MCP calls vs raw shell fallback.
    let mcp_res = mcp.finalize();
    assert_eq!(mcp_res.structured_mcp_count, 2);
    assert_eq!(mcp_res.raw_shell_count, 1);
    assert!((mcp_res.mcp_adoption_ratio - (2.0 / 3.0)).abs() < f32::EPSILON);

    // CRIT-LUMEN-067: tool failure immediately followed by an adjusted retry
    // is recorded as a tool_retry self-correction event.
    let corr_res = corr.finalize();
    assert_eq!(corr_res.tool_retry_corrections, 1);
    assert_eq!(corr_res.approach_pivot_corrections, 1);
    assert_eq!(corr_res.total_corrections, 2);

    // CRIT-LUMEN-061 / CRIT-LUMEN-068: TokenUsageAccumulator sums real usage into
    // running totals and silently excludes synthetic model usage from billing.
    let mut tok = TokenUsageAccumulator::new("claude-3-5-sonnet-20241022");
    tok.update_raw(&serde_json::json!({
        "message": {
            "model": "claude-3-5-sonnet-20241022",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_creation_input_tokens": 10,
                "cache_read_input_tokens": 5
            }
        }
    }));
    tok.update_raw(&serde_json::json!({
        "message": {
            "model": "<synthetic>title-generator",
            "usage": {
                "input_tokens": 9999,
                "output_tokens": 9999,
                "cache_creation_input_tokens": 9999,
                "cache_read_input_tokens": 9999
            }
        }
    }));

    let economics = tok.finalize();
    assert_eq!(economics.input_tokens, 100);
    assert_eq!(economics.output_tokens, 50);
    assert_eq!(economics.cache_creation_tokens, 10);
    assert_eq!(economics.cache_read_tokens, 5);
    assert!(!economics.per_model.contains_key("<synthetic>title-generator"));
}

#[test]
fn test_schema_extractor_citations() {
    let mut ext = SchemaExtractorAccumulator::default();

    // CRIT-LUMEN-063: Schema extraction and validation
    let t0 = CanonicalTurn {
        turn_index: 0,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 500,
        text: Some("Here is the spec:\n```json\n{\"$schema\": \"https://json-schema.org/draft/2020-12/schema\", \"id\": \"spec@1\"}\n```".into()),
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };
    ext.update(&t0);

    let citations = ext.finalize();
    assert_eq!(citations.len(), 1);
    assert_eq!(citations[0].schema_id, "spec@1");
    assert!(citations[0].is_valid);
}

#[test]
fn test_schema_extractor_plan_citation_valid_and_invalid() {
    let mut ext = SchemaExtractorAccumulator::default();

    // CRIT-LUMEN-063: plan@1 cited alongside an embedded JSON block is a valid citation.
    let valid_turn = CanonicalTurn {
        turn_index: 0,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 500,
        text: Some(
            "Here is the plan:\n```json\n{\"$schema\": \"http://json-schema.org/draft-07/schema#\", \"id\": \"plan@1\"}\n```"
                .into(),
        ),
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };
    ext.update(&valid_turn);

    // CRIT-LUMEN-063: plan@1 referenced in prose with no embedded JSON payload must still be
    // recorded as a citation, but marked invalid.
    let invalid_turn = CanonicalTurn {
        turn_index: 1,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 500,
        text: Some("We should follow the plan@1 format for this next.".into()),
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };
    ext.update(&invalid_turn);

    let citations = ext.finalize();
    assert_eq!(citations.len(), 2);

    let valid = citations.iter().find(|c| c.turn_index == 0).expect("valid citation missing");
    assert_eq!(valid.schema_id, "plan@1");
    assert!(valid.is_valid);

    let invalid = citations.iter().find(|c| c.turn_index == 1).expect("invalid citation missing");
    assert_eq!(invalid.schema_id, "plan@1");
    assert!(!invalid.is_valid);
}

#[test]
fn test_schema_extractor_wired_into_analysis_report() {
    let engine = AnalyticsEngine::new();

    let turn = CanonicalTurn {
        turn_index: 0,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 500,
        text: Some(
            "```json\n{\"$schema\": \"http://json-schema.org/draft-07/schema#\", \"id\": \"spec@1\"}\n```".into(),
        ),
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };

    let transcript = CanonicalTranscript {
        session_id: "s1".into(),
        parent_session_id: None,
        orchestrator: OrchestratorKind::ClaudeCode,
        model_family: "claude-3-5-sonnet-20241022".into(),
        timing: ExecutionTiming {
            started_at: Utc::now(),
            ended_at: Utc::now(),
            wall_duration_ms: 0,
            active_duration_ms: 0,
            idle_duration_ms: 0,
            idle_gap_count: 0,
        },
        economics: TokenEconomics::calculate(0, 0, 0, 0, "claude-3-5-sonnet-20241022"),
        turns: vec![turn],
        subagents: vec![],
        extracted_schemas: smallvec![],
        detected_anomalies: smallvec![],
    };

    // CRIT-LUMEN-063: SchemaExtractorAccumulator must be wired into AnalyticsEngine's
    // per-turn loop and its citations surfaced on AnalysisReport.
    let report = engine.process_transcript(&transcript);
    assert_eq!(report.schema_extractor.len(), 1);
    assert_eq!(report.schema_extractor[0].schema_id, "spec@1");
    assert!(report.schema_extractor[0].is_valid);
}
