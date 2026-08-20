use chrono::{Duration, Utc};
use compact_str::CompactString;
use lumen_analysis::*;
use lumen_model::*;
use smallvec::smallvec;

#[test]
fn test_circuit_breaker_trips_on_3_rounds() {
    let mut cb = CircuitBreakerAccumulator::default();

    for i in 0..3 {
        let turn = CanonicalTurn {
            attribution: None,
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
fn test_circuit_breaker_round_counter_boundary_and_pair_change_reset() {
    // CRIT-LUMEN-062: consecutive review handoffs between the same agent pair
    // increment the round counter, and a CircuitStallEvent is only flagged once
    // rounds exceed 2 -- not at exactly 2. Handing off to a *different* agent
    // pair resets the counter rather than continuing the streak.
    let mut cb = CircuitBreakerAccumulator::default();

    let spawn_turn = |turn_index: usize, agent_type: &str| CanonicalTurn {
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
    };

    // Round 1 and round 2 with "auditor" -- must not exceed the threshold yet.
    cb.update(&spawn_turn(0, "auditor"));
    cb.update(&spawn_turn(1, "auditor"));
    assert_eq!(cb.consecutive_rounds, 2);
    assert!(cb.stalls.is_empty(), "2 consecutive rounds must not flag a stall (threshold is >2)");

    // Handoff to a different agent pair resets the streak back to round 1.
    cb.update(&spawn_turn(2, "reviewer"));
    assert_eq!(cb.consecutive_rounds, 1);
    assert!(cb.stalls.is_empty());

    // "reviewer" continues for 2 more consecutive rounds, crossing the threshold on round 3.
    cb.update(&spawn_turn(3, "reviewer"));
    cb.update(&spawn_turn(4, "reviewer"));

    let report = cb.finalize();
    assert_eq!(report.max_observed_rounds, 3);
    assert!(report.tripped);
    assert_eq!(report.stalls.len(), 1);
    assert_eq!(report.stalls[0].agent_pair.as_str(), "parent->reviewer");
    assert_eq!(report.stalls[0].observed_rounds, 3);
    assert_eq!(report.stalls[0].turn_index, 4);
}

#[test]
fn test_turn_duration_p50_p95_percentiles() {
    let mut td = TurnDurationAccumulator::default();

    for lat in [100, 200, 300, 400, 500, 600, 700, 800, 900, 1000] {
        let turn = CanonicalTurn {
            attribution: None,
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
    assert_eq!(metrics.avg_ms, 550);
}

#[test]
fn test_turn_duration_excludes_non_assistant_roles() {
    // CRIT-LUMEN-064: p50/p95/avg turn latency must be computed "across all
    // assistant completions" -- non-assistant turns (User, System, ToolResult)
    // must not contribute even if they carry a nonzero latency_ms.
    let mut td = TurnDurationAccumulator::default();

    let make_turn = |turn_index: usize, role: TurnRole, latency_ms: u64| CanonicalTurn {
        attribution: None,
        turn_index,
        role,
        timestamp: Utc::now(),
        latency_ms,
        text: None,
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };

    td.update(&make_turn(0, TurnRole::Assistant, 100));
    td.update(&make_turn(1, TurnRole::User, 99999));
    td.update(&make_turn(2, TurnRole::Assistant, 300));

    let metrics = td.finalize();
    assert_eq!(metrics.total_turns, 2, "User turn latency must not be counted");
    assert_eq!(metrics.avg_ms, 200, "User turn's 99999ms latency must not skew the average");
    assert_eq!(metrics.p50_ms, 300);
    assert_eq!(metrics.p95_ms, 300);
}

#[test]
fn test_context_growth_and_autonomy_accumulators() {
    let mut cg = ContextGrowthAccumulator::default();
    let mut aut = AutonomyAccumulator::default();
    let mut art = ArtifactsAccumulator::default();
    let mut inv = ToolInventoryAccumulator::default();

    // Turn 0: User prompt
    let t0 = CanonicalTurn {
        attribution: None,
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
        attribution: None,
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
        attribution: None,
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
fn test_context_growth_skips_missing_usage_and_zero_growth_floor() {
    // CRIT-LUMEN-130: a turn with usage=None must be excluded from turn_count
    // entirely and must not set/overwrite initial_prompt_tokens.
    // CRIT-LUMEN-131: the first usage-bearing turn (previous_prompt_tokens
    // starts at 0) contributes zero growth even with a positive prompt_tokens,
    // and a later usage turn whose prompt_tokens does not exceed the prior
    // usage turn's also contributes zero growth. A None-usage turn interleaved
    // between usage turns must not disturb the previous-usage-turn tracking.
    // CRIT-LUMEN-132: avg_growth_per_turn == total_growth / (turn_count - 1),
    // or 0.0 when turn_count <= 1.
    let mut cg = ContextGrowthAccumulator::default();

    let usage_turn = |turn_index: usize, prompt_tokens: u64| CanonicalTurn {
        attribution: None,
        turn_index,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 0,
        text: None,
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: Some(TurnTokenUsage {
            input_tokens: prompt_tokens,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            output_tokens: 0,
        }),
    };

    let no_usage_turn = |turn_index: usize| CanonicalTurn {
        attribution: None,
        turn_index,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 0,
        text: None,
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };

    // Turn 0: no usage -- must be skipped entirely, must not set
    // initial_prompt_tokens or increment turn_count.
    cg.update(&no_usage_turn(0));

    // Turn 1: first usage-bearing turn. previous_prompt_tokens is still 0, so
    // even though prompt_tokens (500) is positive, it must contribute zero
    // growth. This also sets initial_prompt_tokens to 500 -- proving the
    // earlier None-usage turn did not already set it (e.g. to 0).
    cg.update(&usage_turn(1, 500));
    assert_eq!(cg.turn_count, 1, "None-usage turn 0 must not increment turn_count");
    assert_eq!(cg.initial_prompt_tokens, Some(500));
    assert_eq!(cg.total_growth, 0, "first usage turn must not count its own prompt_tokens as growth");

    // Turn 2: usage-bearing, prompt_tokens (400) does not exceed the prior
    // usage turn's (500) -- must contribute zero growth, not a negative jump.
    cg.update(&usage_turn(2, 400));
    assert_eq!(cg.turn_count, 2);
    assert_eq!(cg.total_growth, 0);
    assert_eq!(cg.max_jump_tokens, 0);

    // Turn 3: no usage again, interleaved -- must be skipped entirely and must
    // not disturb previous_prompt_tokens tracking (still 400 from turn 2).
    cg.update(&no_usage_turn(3));
    assert_eq!(cg.turn_count, 2, "interleaved None-usage turn must not increment turn_count");

    // Turn 4: usage-bearing, prompt_tokens (700) exceeds the prior *usage*
    // turn's (400, from turn 2 -- the intervening None-usage turn 3 must not
    // have reset previous_prompt_tokens) -- a real jump of 300.
    cg.update(&usage_turn(4, 700));
    assert_eq!(cg.turn_count, 3);
    assert_eq!(cg.total_growth, 300);
    assert_eq!(cg.max_jump_tokens, 300);

    let metrics = cg.finalize();
    assert_eq!(metrics.initial_prompt_tokens, 500);
    assert_eq!(metrics.final_prompt_tokens, 700);
    assert_eq!(metrics.max_jump_tokens, 300);
    // avg_growth_per_turn = total_growth / (turn_count - 1) = 300 / (3 - 1) = 150.0
    assert_eq!(metrics.avg_growth_per_turn, 150.0);

    // CRIT-LUMEN-132: turn_count <= 1 must floor avg_growth_per_turn at 0.0
    // rather than dividing by zero or a negative denominator.
    let mut single = ContextGrowthAccumulator::default();
    single.update(&usage_turn(0, 1000));
    let single_metrics = single.finalize();
    assert_eq!(single_metrics.avg_growth_per_turn, 0.0, "turn_count == 1 must yield avg_growth_per_turn 0.0");

    let mut empty = ContextGrowthAccumulator::default();
    empty.update(&no_usage_turn(0));
    let empty_metrics = empty.finalize();
    assert_eq!(empty_metrics.avg_growth_per_turn, 0.0, "turn_count == 0 must yield avg_growth_per_turn 0.0");
    assert_eq!(empty_metrics.initial_prompt_tokens, 0, "no usage-bearing turns means initial_prompt_tokens defaults to 0");
}

#[test]
fn test_api_health_and_mcp_affinity_accumulators() {
    let mut api = ApiHealthAccumulator::default();
    let mut mcp = McpAffinityAccumulator::default();
    let mut corr = SelfCorrectionAccumulator::default();

    // Turn 0: Failed tool with 429 rate limit
    let t0 = CanonicalTurn {
        attribution: None,
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
        attribution: None,
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
        attribution: None,
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
        attribution: None,
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
        attribution: None,
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
                attribution: None,
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
                attribution: None,
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
        attribution: None,
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
        attribution: None,
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

#[test]
fn test_subagent_and_plugin_skill_attribution() {
    // CRIT-LUMEN-147/148: turns attributed to a Plugin or Skill bucket their token
    // usage under by_plugin/by_skill; turns with None or Root attribution both land
    // in unattributed_tokens.
    let usage_turn = |turn_index: usize, attribution: Option<AttributionSource>, tokens: u64| CanonicalTurn {
        attribution,
        turn_index,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 100,
        text: None,
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: Some(TurnTokenUsage {
            input_tokens: tokens,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            output_tokens: 0,
        }),
    };

    let turn_a = usage_turn(0, Some(AttributionSource::Plugin { name: "foo".into() }), 100);
    let turn_b = usage_turn(1, Some(AttributionSource::Skill { name: "bar".into(), plugin: None }), 50);
    let turn_c = usage_turn(2, None, 30);
    let turn_d = usage_turn(3, Some(AttributionSource::Root), 20);

    let mut attribution = AttributionAccumulator::default();
    attribution.update(&turn_a);
    attribution.update(&turn_b);
    attribution.update(&turn_c);
    attribution.update(&turn_d);
    let metrics = attribution.finalize();

    assert_eq!(metrics.by_plugin.get("foo"), Some(&100));
    assert_eq!(metrics.by_skill.get("bar"), Some(&50));
    assert_eq!(metrics.unattributed_tokens, 50);

    // CRIT-LUMEN-069/149: subagent transcripts must be recursed into and their
    // token totals aggregated into by_subagent, keyed by the subagent's session_id.
    let child_turn = usage_turn(0, Some(AttributionSource::Plugin { name: "reviewer-tool".into() }), 40);

    let child_transcript = CanonicalTranscript {
        session_id: "reviewer".into(),
        parent_session_id: Some("parent".into()),
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
        turns: vec![child_turn],
        subagents: vec![],
        extracted_schemas: smallvec![],
        detected_anomalies: smallvec![],
    };

    let parent_turn = usage_turn(0, None, 10);

    let parent_transcript = CanonicalTranscript {
        session_id: "parent".into(),
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
        turns: vec![parent_turn],
        subagents: vec![child_transcript],
        extracted_schemas: smallvec![],
        detected_anomalies: smallvec![],
    };

    let engine = AnalyticsEngine::new();
    let report = engine.process_transcript(&parent_transcript);

    assert_eq!(report.by_subagent.get("reviewer").map(|m| m.by_plugin.get("reviewer-tool").copied()), Some(Some(40)));
}

#[test]
fn test_artifacts_accumulator_dedup_and_skip_empty() {
    // CRIT-LUMEN-126: non-empty FileRead/FileCreate/FileEdit paths land in their
    // respective sets, while empty-path variants and non-file ToolIntent variants
    // (CodeSearch, McpCall) are silently skipped.
    let mut art = ArtifactsAccumulator::default();

    let turn = CanonicalTurn {
        attribution: None,
        turn_index: 0,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 100,
        text: None,
        tool_calls: smallvec![
            CanonicalToolCall {
                call_id: "c1".into(),
                tool_name: "view_file".into(),
                intent: ToolIntent::FileRead { path: "src/lib.rs".into(), line_range: None },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "c2".into(),
                tool_name: "view_file".into(),
                intent: ToolIntent::FileRead { path: "".into(), line_range: None },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "c3".into(),
                tool_name: "create_file".into(),
                intent: ToolIntent::FileCreate { path: "src/new.rs".into() },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "c4".into(),
                tool_name: "create_file".into(),
                intent: ToolIntent::FileCreate { path: "".into() },
                raw_arguments: serde_json::json!({}),
            },
            // A path that also appears in files_read -- must be deduped in
            // total_unique_files while remaining present in both category sets.
            CanonicalToolCall {
                call_id: "c5".into(),
                tool_name: "replace_file_content".into(),
                intent: ToolIntent::FileEdit { path: "src/lib.rs".into(), lines_added: 1, lines_removed: 1 },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "c6".into(),
                tool_name: "replace_file_content".into(),
                intent: ToolIntent::FileEdit { path: "".into(), lines_added: 0, lines_removed: 0 },
                raw_arguments: serde_json::json!({}),
            },
            // Non-file ToolIntent variants must not leak into any category set.
            CanonicalToolCall {
                call_id: "c7".into(),
                tool_name: "grep".into(),
                intent: ToolIntent::CodeSearch { tool: "ripgrep".into(), query: "foo".into(), is_ast: false },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "c8".into(),
                tool_name: "mcp__github__get_issue".into(),
                intent: ToolIntent::McpCall { server: "github".into(), method: "get_issue".into() },
                raw_arguments: serde_json::json!({}),
            },
        ],
        tool_results: smallvec![],
        usage: None,
    };
    art.update(&turn);

    let metrics = art.finalize();

    assert_eq!(metrics.files_read.len(), 1);
    assert!(metrics.files_read.contains("src/lib.rs"));
    assert_eq!(metrics.files_created.len(), 1);
    assert!(metrics.files_created.contains("src/new.rs"));
    assert_eq!(metrics.files_edited.len(), 1);
    assert!(metrics.files_edited.contains("src/lib.rs"));

    // CRIT-LUMEN-127: "src/lib.rs" appears in both files_read and files_edited but
    // must be counted exactly once in total_unique_files.
    assert_eq!(metrics.total_unique_files, 2);
}

#[test]
fn test_autonomy_streak_flush_and_index() {
    // CRIT-LUMEN-128/129: Assistant and ToolResult both extend the streak, System
    // is a no-op that neither breaks nor extends it, User flushes the open streak
    // into streak_lengths and resets it, and finalize flushes any trailing open
    // streak before computing max/avg/total_streaks and a full-role autonomy_index.
    let mut aut = AutonomyAccumulator::default();

    let make_turn = |turn_index: usize, role: TurnRole| CanonicalTurn {
        attribution: None,
        turn_index,
        role,
        timestamp: Utc::now(),
        latency_ms: 0,
        text: None,
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };

    aut.update(&make_turn(0, TurnRole::User));

    aut.update(&make_turn(1, TurnRole::Assistant));
    assert_eq!(aut.current_streak, 1);
    assert_eq!(aut.assistant_turns, 1);

    // A System turn sandwiched between two Assistant turns must neither break
    // nor extend the streak on its own.
    aut.update(&make_turn(2, TurnRole::System));
    assert_eq!(aut.current_streak, 1, "System turn must not reset current_streak");

    aut.update(&make_turn(3, TurnRole::Assistant));
    assert_eq!(
        aut.current_streak, 2,
        "streak must extend across the intervening System turn, proving it was a true no-op"
    );

    // TurnRole::ToolResult must extend current_streak and increment assistant_turns
    // identically to TurnRole::Assistant (both share one match arm in autonomy.rs).
    aut.update(&make_turn(4, TurnRole::ToolResult));
    assert_eq!(aut.current_streak, 3, "ToolResult must extend current_streak exactly like Assistant");
    assert_eq!(aut.assistant_turns, 3, "ToolResult must increment assistant_turns exactly like Assistant");
    assert_eq!(aut.max_streak, 3);

    // A User turn flushes the non-zero open streak into streak_lengths and resets it.
    aut.update(&make_turn(5, TurnRole::User));
    assert_eq!(aut.current_streak, 0, "User turn must reset current_streak to 0");
    assert_eq!(aut.streak_lengths, vec![3], "User turn must flush the open streak into streak_lengths");

    // One more Assistant turn opens a second, trailing streak that is still open
    // when finalize() is called.
    aut.update(&make_turn(6, TurnRole::Assistant));

    let metrics = aut.finalize();

    // finalize() must flush the trailing open streak (length 1) before computing
    // aggregates, yielding streak_lengths == [3, 1].
    assert_eq!(metrics.max_autonomous_streak, 3);
    assert_eq!(metrics.total_streaks, 2);
    assert!((metrics.avg_autonomous_streak - 2.0).abs() < f32::EPSILON);

    // autonomy_index = assistant_turns / total_turns computed over ALL roles
    // (User, Assistant, System, ToolResult): 4 assistant/tool-result turns out of
    // 7 total turns (indices 0..=6).
    assert!((metrics.autonomy_index - (4.0 / 7.0)).abs() < f32::EPSILON);
}

#[test]
fn test_stats_mutually_exclusive_roles_and_byte_length() {
    // CRIT-LUMEN-133: exactly one of user_turns/assistant_turns/tool_result_turns/
    // system_turns increments per turn (matching entry.role), while total_turns
    // increments for every turn regardless of role.
    // CRIT-LUMEN-134: total_text_characters accumulates text.len() -- UTF-8 BYTE
    // length, not char count -- for Some(text) turns, and adds nothing for None.
    let mut stats = StatsAccumulator::default();

    let make_turn = |turn_index: usize, role: TurnRole, text: Option<&str>| CanonicalTurn {
        attribution: None,
        turn_index,
        role,
        timestamp: Utc::now(),
        latency_ms: 0,
        text: text.map(|s| s.to_string()),
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };

    // "héllo 👍" is 7 chars but 11 UTF-8 bytes (é = 2 bytes, 👍 = 4 bytes).
    // If the implementation used text.chars().count() instead of text.len(),
    // this turn alone would under-count total_text_characters by 4.
    let multibyte = "héllo 👍";
    assert_eq!(multibyte.chars().count(), 7);
    assert_eq!(multibyte.len(), 11);

    stats.update(&make_turn(0, TurnRole::User, Some(multibyte)));
    stats.update(&make_turn(1, TurnRole::Assistant, None));
    stats.update(&make_turn(2, TurnRole::ToolResult, Some("ok")));
    stats.update(&make_turn(3, TurnRole::System, Some("hi")));

    let metrics = stats.finalize();

    assert_eq!(metrics.total_turns, 4, "total_turns must increment for every turn regardless of role");
    assert_eq!(metrics.user_turns, 1, "exactly one User turn was processed");
    assert_eq!(metrics.assistant_turns, 1, "exactly one Assistant turn was processed");
    assert_eq!(metrics.tool_result_turns, 1, "exactly one ToolResult turn was processed");
    assert_eq!(metrics.system_turns, 1, "exactly one System turn was processed");
    assert_eq!(
        metrics.user_turns + metrics.assistant_turns + metrics.tool_result_turns + metrics.system_turns,
        metrics.total_turns,
        "per-role counters must be mutually exclusive and sum to total_turns"
    );

    // 11 (multibyte, byte length) + 0 (None) + 2 ("ok") + 2 ("hi") = 15.
    // A char-count implementation would instead yield 7 + 0 + 2 + 2 = 11.
    assert_eq!(
        metrics.total_text_characters, 15,
        "total_text_characters must sum UTF-8 byte lengths, not char counts, and skip None text"
    );
}

#[test]
fn test_tool_inventory_last_call_wins_and_running_error_key() {
    // CRIT-LUMEN-135: last_tool_name must reflect the last call in a turn's
    // tool_calls list, not the first -- even when multiple calls occur in one turn.
    // CRIT-LUMEN-136: an is_error result is attributed to the accumulator's running
    // last_tool_name (not any per-result identifier like call_id), and an error
    // observed before any tool_call has ever run (last_tool_name still None) is
    // dropped without incrementing any errors_by_tool entry.
    let mut inv = ToolInventoryAccumulator::default();

    let make_call = |call_id: &str, tool_name: &str| CanonicalToolCall {
        call_id: call_id.into(),
        tool_name: tool_name.into(),
        intent: ToolIntent::Other { raw_name: tool_name.into() },
        raw_arguments: serde_json::json!({}),
    };

    let make_result = |call_id: &str, is_error: bool| CanonicalToolResult {
        call_id: call_id.into(),
        output_bytes: 0,
        line_count: 0,
        is_error,
        error_class: None,
        truncated_output: None,
    };

    // Turn 0: an error result arrives before any tool_call has ever run.
    // last_tool_name is still None, so this error must be dropped entirely.
    let t0 = CanonicalTurn {
        attribution: None,
        turn_index: 0,
        role: TurnRole::ToolResult,
        timestamp: Utc::now(),
        latency_ms: 0,
        text: None,
        tool_calls: smallvec![],
        tool_results: smallvec![make_result("orphan_call", true)],
        usage: None,
    };
    inv.update(&t0);
    assert!(
        inv.last_tool_name.is_none(),
        "no tool_call has run yet, so last_tool_name must remain None"
    );
    assert!(
        inv.errors_by_tool.is_empty(),
        "an error observed before any tool_call must be dropped, not recorded under a None key"
    );

    // Turn 1: multiple tool_calls in one turn -- last_tool_name must end up as
    // the *last* call's tool_name ("edit_file"), not the first ("read_file").
    // A trailing error result must then be keyed by that running last_tool_name,
    // and specifically NOT by its own call_id ("call_b"), which is not a tool name.
    let t1 = CanonicalTurn {
        attribution: None,
        turn_index: 1,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 100,
        text: None,
        tool_calls: smallvec![make_call("call_a", "read_file"), make_call("call_b", "edit_file")],
        tool_results: smallvec![make_result("call_b", true)],
        usage: None,
    };
    inv.update(&t1);

    assert_eq!(
        inv.last_tool_name.as_deref(),
        Some("edit_file"),
        "last_tool_name must reflect the final call in list order, not the first"
    );
    assert_eq!(
        inv.invocations_by_tool.get("read_file").copied(),
        Some(1),
        "invocations_by_tool must still count the earlier call in the turn"
    );
    assert_eq!(
        inv.invocations_by_tool.get("edit_file").copied(),
        Some(1),
        "invocations_by_tool must count the final call in the turn"
    );
    assert_eq!(
        inv.errors_by_tool.get("edit_file").copied(),
        Some(1),
        "the error must be keyed by the running last_tool_name (edit_file), not by call_id"
    );
    assert!(
        inv.errors_by_tool.get("call_b").is_none(),
        "the error must never be keyed by the result's own call_id"
    );
    assert_eq!(
        inv.errors_by_tool.len(),
        1,
        "only one tool must have an error entry after this turn"
    );

    let metrics = inv.finalize();
    assert_eq!(metrics.total_invocations, 2);
    assert_eq!(metrics.distinct_tools_count, 2);
    assert_eq!(metrics.errors_by_tool.get("edit_file").copied(), Some(1));
}

#[test]
fn test_flow_accumulator_permission_break_and_ratio() {
    // CRIT-LUMEN-137/138
    let mut flow = FlowAccumulator::default();

    let make_turn = |turn_index: usize, n_calls: usize, tool_results: smallvec::SmallVec<[CanonicalToolResult; 2]>| {
        let tool_calls: smallvec::SmallVec<[CanonicalToolCall; 2]> = (0..n_calls)
            .map(|i| CanonicalToolCall {
                call_id: CompactString::new(format!("call_{turn_index}_{i}")),
                tool_name: "run_command".into(),
                intent: ToolIntent::Other { raw_name: "run_command".into() },
                raw_arguments: serde_json::json!({}),
            })
            .collect();
        CanonicalTurn {
            attribution: None,
            turn_index,
            role: TurnRole::Assistant,
            timestamp: Utc::now(),
            latency_ms: 100,
            text: None,
            tool_calls,
            tool_results,
            usage: None,
        }
    };

    // 3 turns of 2 tool_calls each -> streak reaches 6.
    for i in 0..3 {
        flow.update(&make_turn(i, 2, smallvec![]));
    }
    assert_eq!(flow.current_streak, 6);

    // Permission-error turn with 1 tool_call whose result is a permission error (mixed case).
    let perm_result = smallvec![CanonicalToolResult {
        call_id: "call_perm".into(),
        output_bytes: 10,
        line_count: 1,
        is_error: true,
        error_class: Some("Permission_Denied".into()),
        truncated_output: None,
    }];
    flow.update(&make_turn(3, 1, perm_result));

    assert!(flow.streak_lengths.contains(&6));
    assert_eq!(flow.permission_blocks, 1);
    assert_eq!(flow.current_streak, 0, "the error turn's own tool_calls must not be added to current_streak");
    assert_eq!(flow.total_tool_calls, 7, "total_tool_calls is unconditional, so the error turn's 1 call counts");

    // 2 more turns of 1 tool_call each -> trailing streak of 2.
    flow.update(&make_turn(4, 1, smallvec![]));
    flow.update(&make_turn(5, 1, smallvec![]));

    let metrics = flow.finalize();
    assert_eq!(metrics.longest_streak, 6);
    assert_eq!(metrics.avg_streak_len, 4.0);
    assert_eq!(metrics.total_tool_calls, 9);
    assert!((metrics.flow_ratio - (8.0 / 9.0)).abs() < 1e-9);
}

#[test]
fn test_hook_activity_accumulator_block_rate_and_duration() {
    // CRIT-LUMEN-139 / CRIT-LUMEN-140: HookActivityAccumulator tallies hook
    // invocations by event name and duration, and separately tracks blocked
    // decisions to compute a block rate and average duration on finalize.
    let mut hooks = HookActivityAccumulator::default();

    hooks.update_raw(&serde_json::json!({
        "hookEventName": "PreToolUse",
        "durationMs": 100
    }));
    hooks.update_raw(&serde_json::json!({
        "hookEventName": "PreToolUse",
        "durationMs": 150,
        "permissionDecision": "deny"
    }));
    hooks.update_raw(&serde_json::json!({
        "hookEventName": "PostToolUse",
        "durationMs": 50,
        "decision": "block"
    }));
    hooks.update_raw(&serde_json::json!({
        "someOtherField": "ignored"
    }));

    let metrics = hooks.finalize();
    assert_eq!(metrics.hook_invocations, 3);
    assert_eq!(metrics.by_event.get("PreToolUse").copied(), Some(2));
    assert_eq!(metrics.by_event.get("PostToolUse").copied(), Some(1));
    assert_eq!(metrics.total_duration_ms, 300);
    assert_eq!(metrics.blocked_count, 2);
    assert!((metrics.block_rate - (2.0 / 3.0)).abs() < 1e-9);
    assert!((metrics.avg_duration_ms - 100.0).abs() < 1e-9);

    let empty = HookActivityAccumulator::default().finalize();
    assert_eq!(empty.block_rate, 0.0);
    assert_eq!(empty.avg_duration_ms, 0.0);
}

#[test]
fn test_pr_link_accumulator_vcs_vs_text_source() {
    // CRIT-LUMEN-141 / CRIT-LUMEN-142: PrLinkAccumulator scans VersionControl-paired
    // tool_results and entry.text for github.com PR URLs, dedupes them into pr_urls,
    // records the first turn a PR was seen, and only counts a match toward
    // linked_via_vcs_tool when it came from a VersionControl-paired tool_result --
    // text-only restatements of the same PR must not double count or bump the counter.
    let mut pr_link = PrLinkAccumulator::default();

    let turn0 = CanonicalTurn {
        attribution: None,
        turn_index: 0,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 100,
        text: None,
        tool_calls: smallvec![CanonicalToolCall {
            call_id: CompactString::new("call_1"),
            tool_name: CompactString::new("bash"),
            intent: ToolIntent::VersionControl { action: CompactString::new("push") },
            raw_arguments: serde_json::json!({}),
        }],
        tool_results: smallvec![CanonicalToolResult {
            call_id: CompactString::new("call_1"),
            output_bytes: 64,
            line_count: 1,
            is_error: false,
            error_class: None,
            truncated_output: Some(CompactString::new(
                "Created https://github.com/acme/widgets/pull/42 successfully"
            )),
        }],
        usage: None,
    };
    pr_link.update(&turn0);

    let turn1 = CanonicalTurn {
        attribution: None,
        turn_index: 1,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 100,
        text: Some("see github.com/acme/widgets/pull/42?tab=files for details".into()),
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };
    pr_link.update(&turn1);

    let turn2 = CanonicalTurn {
        attribution: None,
        turn_index: 2,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 100,
        text: Some("also check github.com/acme/other/pull/7".into()),
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };
    pr_link.update(&turn2);

    let metrics = pr_link.finalize();

    let expected: std::collections::BTreeSet<CompactString> =
        ["acme/widgets#42", "acme/other#7"].into_iter().map(CompactString::new).collect();
    assert_eq!(metrics.pr_urls, expected);
    assert_eq!(metrics.first_pr_turn_index, Some(0));
    assert_eq!(
        metrics.linked_via_vcs_tool, 1,
        "only turn 0's VersionControl-paired tool_result match counts; text-only matches do not"
    );
}

#[test]
fn test_fuzzy_tools_clustering_and_typo_rate() {
    // CRIT-LUMEN-143/144/154: counts and total_tool_calls are populated during
    // update(); finalize() greedily clusters by (-count, name) using Levenshtein
    // distance with a length-dependent threshold (1 if candidate.len() < 5, else 2).
    let mut ft = FuzzyToolsAccumulator::default();

    let make_call = |call_id: &str, tool_name: &str| CanonicalToolCall {
        call_id: call_id.into(),
        tool_name: tool_name.into(),
        intent: ToolIntent::Other { raw_name: tool_name.into() },
        raw_arguments: serde_json::json!({}),
    };

    let make_turn = |turn_index: usize, tool_calls: smallvec::SmallVec<[CanonicalToolCall; 2]>| CanonicalTurn {
        attribution: None,
        turn_index,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 100,
        text: None,
        tool_calls,
        tool_results: smallvec![],
        usage: None,
    };

    // Turn 0: 5x view_file, 2x veiw_file (transposition typo, distance 2, len>=5).
    ft.update(&make_turn(
        0,
        smallvec![
            make_call("c1", "view_file"),
            make_call("c2", "view_file"),
            make_call("c3", "view_file"),
            make_call("c4", "view_file"),
            make_call("c5", "view_file"),
            make_call("c6", "veiw_file"),
            make_call("c7", "veiw_file"),
        ],
    ));

    // Turn 1: 3x grep, 1x grpe (transposition typo, distance 2, but len<5 so threshold=1).
    ft.update(&make_turn(
        1,
        smallvec![
            make_call("c8", "grep"),
            make_call("c9", "grep"),
            make_call("c10", "grep"),
            make_call("c11", "grpe"),
        ],
    ));

    // Turn 2: 1x bash.
    ft.update(&make_turn(2, smallvec![make_call("c12", "bash")]));

    // CRIT-LUMEN-154: counts and total_tool_calls are populated during update().
    assert_eq!(ft.counts.get("view_file").copied(), Some(5));
    assert_eq!(ft.counts.get("veiw_file").copied(), Some(2));
    assert_eq!(ft.counts.get("grep").copied(), Some(3));
    assert_eq!(ft.counts.get("grpe").copied(), Some(1));
    assert_eq!(ft.counts.get("bash").copied(), Some(1));
    assert_eq!(ft.total_tool_calls, 12);

    let metrics = ft.finalize();

    // CRIT-LUMEN-143: greedy clustering by (-count, name) with the length-dependent
    // Levenshtein threshold. Sorted order is view_file(5), grep(3), veiw_file(2),
    // bash(1), grpe(1) -- bash sorts before grpe alphabetically at equal count.
    assert_eq!(metrics.clusters.len(), 4, "expected 4 clusters: view_file, grep, bash, grpe");

    assert_eq!(metrics.clusters[0].canonical.as_str(), "view_file");
    assert_eq!(
        metrics.clusters[0].variants,
        vec![(CompactString::new("veiw_file"), 2)],
        "veiw_file is distance 2 from view_file and len>=5 so threshold=2 -- clusters as a variant"
    );

    assert_eq!(metrics.clusters[1].canonical.as_str(), "grep");
    assert!(
        metrics.clusters[1].variants.is_empty(),
        "grpe is distance 2 from grep but len<5 so threshold=1 -- must NOT cluster"
    );

    assert_eq!(metrics.clusters[2].canonical.as_str(), "bash");
    assert!(metrics.clusters[2].variants.is_empty());

    assert_eq!(metrics.clusters[3].canonical.as_str(), "grpe");
    assert!(metrics.clusters[3].variants.is_empty(), "grpe must become its own singleton canonical");

    // CRIT-LUMEN-144: typo_call_count is the sum of variant counts across all clusters.
    assert_eq!(metrics.typo_call_count, 2);
    assert_eq!(metrics.typo_rate, 2.0 / 12.0);
}

#[test]
fn test_trajectory_dag_intent_mapping_and_had_error() {
    // CRIT-LUMEN-145/146: intent-to-node mapping and had_error joined from tool_results
    // by matching call_id within the same turn.
    let mut td = TrajectoryDagAccumulator::default();

    let turn = CanonicalTurn {
        attribution: None,
        turn_index: 0,
        role: TurnRole::Assistant,
        timestamp: Utc::now(),
        latency_ms: 100,
        text: None,
        tool_calls: smallvec![
            CanonicalToolCall {
                call_id: "call_edit".into(),
                tool_name: "edit_file".into(),
                intent: ToolIntent::FileEdit { path: "a.rs".into(), lines_added: 1, lines_removed: 1 },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "call_search".into(),
                tool_name: "grep".into(),
                intent: ToolIntent::CodeSearch { tool: "ripgrep".into(), query: "foo".into(), is_ast: false },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "call_commit".into(),
                tool_name: "git".into(),
                intent: ToolIntent::VersionControl { action: "commit".into() },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "call_status".into(),
                tool_name: "git".into(),
                intent: ToolIntent::VersionControl { action: "status".into() },
                raw_arguments: serde_json::json!({}),
            },
        ],
        tool_results: smallvec![CanonicalToolResult {
            call_id: "call_edit".into(),
            output_bytes: 0,
            line_count: 0,
            is_error: true,
            error_class: Some("edit_failed".into()),
            truncated_output: None,
        }],
        usage: None,
    };

    td.update(&turn);

    let nodes = td.finalize();
    assert_eq!(nodes.len(), 4);

    let edit_node = nodes.iter().find(|n| n.call_id.as_str() == "call_edit").unwrap();
    assert_eq!(edit_node.target_file.as_deref(), Some("a.rs"));
    assert_eq!(edit_node.target_symbol, None);
    assert!(edit_node.is_mutation);
    assert!(edit_node.had_error, "matching tool_result has is_error=true");

    let search_node = nodes.iter().find(|n| n.call_id.as_str() == "call_search").unwrap();
    assert_eq!(search_node.target_file, None);
    assert_eq!(search_node.target_symbol.as_deref(), Some("foo"));
    assert!(!search_node.is_mutation);
    assert!(!search_node.had_error, "no matching tool_result defaults had_error to false");

    let commit_node = nodes.iter().find(|n| n.call_id.as_str() == "call_commit").unwrap();
    assert_eq!(commit_node.target_file, None);
    assert_eq!(commit_node.target_symbol, None);
    assert!(commit_node.is_mutation);
    assert!(!commit_node.had_error);

    let status_node = nodes.iter().find(|n| n.call_id.as_str() == "call_status").unwrap();
    assert_eq!(status_node.target_file, None);
    assert_eq!(status_node.target_symbol, None);
    assert!(!status_node.is_mutation);
    assert!(!status_node.had_error);
}

#[test]
fn test_timeline_accumulator_assistant_streaks_and_idle_gaps() {
    // CRIT-LUMEN-150/151: consecutive Assistant turns extend an open streak; a
    // non-Assistant turn closes it, bumping assistant_streak_count and updating
    // longest_streak_turns. Independently, a gap exceeding 5 minutes since
    // last_timestamp increments idle_gap_count/total_idle_ms/longest_idle_gap_ms,
    // while last_timestamp always advances regardless of gap size.
    let mut tl = TimelineAccumulator::default();

    let t0 = Utc::now();

    let make_turn = |turn_index: usize, role: TurnRole, timestamp: chrono::DateTime<Utc>| CanonicalTurn {
        attribution: None,
        turn_index,
        role,
        timestamp,
        latency_ms: 0,
        text: None,
        tool_calls: smallvec![],
        tool_results: smallvec![],
        usage: None,
    };

    // 3 consecutive Assistant turns 1s apart -> streak of 3.
    tl.update(&make_turn(0, TurnRole::Assistant, t0));
    tl.update(&make_turn(1, TurnRole::Assistant, t0 + Duration::seconds(1)));
    tl.update(&make_turn(2, TurnRole::Assistant, t0 + Duration::seconds(2)));
    assert_eq!(tl.current_streak, 3);
    assert_eq!(tl.idle_gap_count, 0);

    // A User turn 6 minutes after the last Assistant turn -- gap exceeds 5min
    // threshold, closes the streak.
    let user_ts = t0 + Duration::seconds(2) + Duration::minutes(6);
    tl.update(&make_turn(3, TurnRole::User, user_ts));

    assert_eq!(tl.current_streak, 0, "streak must close on role transition away from Assistant");
    assert_eq!(tl.assistant_streak_count, 1);
    assert_eq!(tl.longest_streak_turns, 3);
    assert_eq!(tl.idle_gap_count, 1);

    let expected_gap_ms = Duration::minutes(6).num_milliseconds();
    assert_eq!(tl.total_idle_ms, expected_gap_ms);
    assert_eq!(tl.longest_idle_gap_ms, expected_gap_ms);
    assert_eq!(tl.last_timestamp, Some(user_ts));

    // 2 more consecutive Assistant turns 1s apart -- no new idle gap since <5min.
    let a1_ts = user_ts + Duration::seconds(1);
    let a2_ts = a1_ts + Duration::seconds(1);
    tl.update(&make_turn(4, TurnRole::Assistant, a1_ts));
    tl.update(&make_turn(5, TurnRole::Assistant, a2_ts));

    assert_eq!(tl.current_streak, 2);
    assert_eq!(tl.idle_gap_count, 1, "sub-5min gaps must not increment idle_gap_count");
    assert_eq!(tl.total_idle_ms, expected_gap_ms, "sub-5min gaps must not add to total_idle_ms");
    assert_eq!(tl.longest_idle_gap_ms, expected_gap_ms);
    assert_eq!(tl.last_timestamp, Some(a2_ts), "last_timestamp must always advance to the most recent turn");

    let report = tl.finalize();
    assert_eq!(report.assistant_streak_count, 1);
    assert_eq!(report.longest_streak_turns, 3);
    assert_eq!(report.idle_gap_count, 1);
    assert_eq!(report.total_idle_ms, expected_gap_ms);
    assert_eq!(report.longest_idle_gap_ms, expected_gap_ms);
}
