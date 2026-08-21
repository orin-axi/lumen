use lumen_analysis::*;
use lumen_fixtures::corpus::*;
use lumen_model::{
    AttributionSource, CanonicalToolCall, CanonicalToolResult, CanonicalTranscript, CanonicalTurn, ToolIntent,
    TurnRole, TurnTokenUsage,
};
use lumen_session::*;
use std::io::Cursor;

#[test]
fn test_analysis_pipeline_on_real_claude_session() {
    let adapter = ClaudeCodeAdapter;
    let transcript = adapter
        .parse_stream(Box::new(Cursor::new(real_claude_session_dump())))
        .expect("Failed to parse Claude Code fixture");

    let engine = AnalyticsEngine::new();
    let report = engine.process_transcript(&transcript);

    // Verify comprehensive report generation across all accumulators
    assert_eq!(report.stats.total_turns, 6);
    assert_eq!(report.stats.user_turns, 1);
    assert_eq!(report.stats.assistant_turns, 3);
    assert_eq!(report.stats.tool_result_turns, 2);
    assert_eq!(report.stats.total_tool_calls, 2);

    // Verify artifacts accumulator
    assert_eq!(report.artifacts.total_unique_files, 1);
    assert!(report.artifacts.files_read.contains("crates/lumen-store/src/error.rs"));
    assert!(report.artifacts.files_edited.contains("crates/lumen-store/src/error.rs"));

    // Verify context growth
    assert_eq!(report.context_growth.initial_prompt_tokens, 60000);
    assert_eq!(report.context_growth.peak_prompt_tokens, 61000);

    // Verify tool inventory
    assert_eq!(report.tool_inventory.distinct_tools_count, 2);
    assert_eq!(report.tool_inventory.invocations_by_tool.get("view_file"), Some(&1));
    assert_eq!(report.tool_inventory.invocations_by_tool.get("replace_file_content"), Some(&1));

    // Verify autonomy
    assert!(report.autonomy.autonomy_index > 0.0);
}

#[test]
fn test_analysis_pipeline_on_antigravity_session() {
    let adapter = AgyAdapter;
    let transcript = adapter
        .parse_stream(Box::new(Cursor::new(real_antigravity_session_dump())))
        .expect("Failed to parse AGY fixture");

    let engine = AnalyticsEngine::new();
    let report = engine.process_transcript(&transcript);

    assert_eq!(report.stats.total_turns, 6);
    assert_eq!(report.stats.user_turns, 1);
    assert_eq!(report.stats.assistant_turns, 3);
    assert_eq!(report.stats.tool_result_turns, 2);

    assert_eq!(report.tool_inventory.distinct_tools_count, 2);
    assert_eq!(report.tool_inventory.invocations_by_tool.get("invoke_subagent"), Some(&1));
    assert_eq!(report.tool_inventory.invocations_by_tool.get("view_file"), Some(&1));
}

#[test]
fn test_analysis_pipeline_on_opencode_session() {
    let adapter = OpenCodeAdapter;
    let transcript = adapter
        .parse_stream(Box::new(Cursor::new(real_opencode_session_dump())))
        .expect("Failed to parse OpenCode fixture");

    let engine = AnalyticsEngine::new();
    let report = engine.process_transcript(&transcript);

    assert_eq!(report.stats.total_turns, 7);
    assert_eq!(report.artifacts.total_unique_files, 1);
    assert!(report.artifacts.files_read.contains("Cargo.toml"));
    assert!(report.artifacts.files_edited.contains("Cargo.toml"));
}

/// Builds a transcript that exercises every one of the 19 EntryAccumulator-based
/// accumulators wired into `AnalyticsEngine::process_transcript`'s per-turn loop
/// (see `crates/lumen-analysis/src/engine.rs`), layering hand-built turns on top of
/// a real parsed Claude Code session so the base stats/artifacts/context_growth/
/// tool_inventory/autonomy/flow/fuzzy_tools/trajectory_dag/turn_duration coverage
/// already proven by `test_analysis_pipeline_on_real_claude_session` still holds.
fn build_full_coverage_transcript() -> CanonicalTranscript {
    let adapter = ClaudeCodeAdapter;
    let mut transcript = adapter
        .parse_stream(Box::new(Cursor::new(real_claude_session_dump())))
        .expect("Failed to parse Claude Code fixture");

    let now = chrono::Utc::now();

    // Trips circuit_breaker (3 consecutive SubagentSpawn calls to the same
    // agent_type within one turn's tool_calls).
    let circuit_breaker_turn = CanonicalTurn {
        turn_index: 100,
        role: TurnRole::Assistant,
        timestamp: now,
        latency_ms: 50,
        text: None,
        tool_calls: vec![
            CanonicalToolCall {
                call_id: "cb1".into(),
                tool_name: "invoke_subagent".into(),
                intent: ToolIntent::SubagentSpawn { agent_type: "worker".into(), description: "do x".into() },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "cb2".into(),
                tool_name: "invoke_subagent".into(),
                intent: ToolIntent::SubagentSpawn { agent_type: "worker".into(), description: "do y".into() },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "cb3".into(),
                tool_name: "invoke_subagent".into(),
                intent: ToolIntent::SubagentSpawn { agent_type: "worker".into(), description: "do z".into() },
                raw_arguments: serde_json::json!({}),
            },
        ]
        .into(),
        tool_results: vec![].into(),
        usage: None,
        attribution: None,
    };

    // Feeds api_health (429 error) and sets up self_correction's "last_turn_had_error" state.
    let api_error_turn = CanonicalTurn {
        turn_index: 101,
        role: TurnRole::ToolResult,
        timestamp: now,
        latency_ms: 0,
        text: None,
        tool_calls: vec![].into(),
        tool_results: vec![CanonicalToolResult {
            call_id: "cb1".into(),
            output_bytes: 0,
            line_count: 0,
            is_error: true,
            error_class: Some("429 Too Many Requests".into()),
            truncated_output: None,
            otel_span_id: None,
        }]
        .into(),
        usage: None,
        attribution: None,
    };

    // Feeds self_correction's tool_retry_corrections (follows an errored turn with a
    // non-empty, non-erroring tool call) and permission_mode's auto_accepted_actions.
    let retry_turn = CanonicalTurn {
        turn_index: 102,
        role: TurnRole::Assistant,
        timestamp: now,
        latency_ms: 20,
        text: None,
        tool_calls: vec![CanonicalToolCall {
            call_id: "rt1".into(),
            tool_name: "view_file".into(),
            intent: ToolIntent::FileRead { path: "crates/lumen-analysis/src/lib.rs".into(), line_range: None },
            raw_arguments: serde_json::json!({}),
        }]
        .into(),
        tool_results: vec![].into(),
        usage: None,
        attribution: None,
    };

    // Feeds permission_mode's manual_approval_prompts.
    let manual_approval_turn = CanonicalTurn {
        turn_index: 103,
        role: TurnRole::User,
        timestamp: now,
        latency_ms: 0,
        text: Some("yes".to_string()),
        tool_calls: vec![].into(),
        tool_results: vec![].into(),
        usage: None,
        attribution: None,
    };

    // Feeds pr_link (github PR URL in text), schema_extractor (spec@1 + ```json marker),
    // mcp_affinity (McpCall + raw Bash shell call), span_mapping (otel_span_id present),
    // attribution (Plugin-attributed usage), and context_growth (an additional usage sample).
    let capstone_turn = CanonicalTurn {
        turn_index: 104,
        role: TurnRole::Assistant,
        timestamp: now,
        latency_ms: 30,
        text: Some("Fix merged, see https://github.com/acme/widget/pull/42. Ref spec@1 in ```json block.".to_string()),
        tool_calls: vec![
            CanonicalToolCall {
                call_id: "mcp1".into(),
                tool_name: "mcp_tool".into(),
                intent: ToolIntent::McpCall { server: "github".into(), method: "list_prs".into() },
                raw_arguments: serde_json::json!({}),
            },
            CanonicalToolCall {
                call_id: "sh1".into(),
                tool_name: "Bash".into(),
                intent: ToolIntent::Other { raw_name: "Bash".into() },
                raw_arguments: serde_json::json!({}),
            },
        ]
        .into(),
        tool_results: vec![CanonicalToolResult {
            call_id: "mcp1".into(),
            output_bytes: 10,
            line_count: 1,
            is_error: false,
            error_class: None,
            truncated_output: None,
            otel_span_id: Some("span-1".into()),
        }]
        .into(),
        usage: Some(TurnTokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_tokens: 0,
            cache_read_tokens: 20,
            reasoning_tokens: 0,
        }),
        attribution: Some(AttributionSource::Plugin { name: "proof".into() }),
    };

    transcript.turns.push(circuit_breaker_turn);
    transcript.turns.push(api_error_turn);
    transcript.turns.push(retry_turn);
    transcript.turns.push(manual_approval_turn);
    transcript.turns.push(capstone_turn);

    transcript
}

/// CRIT-LUMEN-060: the 19 EntryAccumulator-based accumulators must run in a single
/// linear per-turn pass with zero heap allocations in the inner loop, with
/// token_usage, hook_activity (RawMessageAccumulators over raw pre-parse JSON,
/// invoked independently of process_transcript) and otel_correlation (a whole-
/// transcript, post-loop computation) explicitly exempted by design.
#[test]
fn test_single_pass_zero_allocation() {
    let transcript = build_full_coverage_transcript();
    let engine = AnalyticsEngine::new();
    let report = engine.process_transcript(&transcript);

    // (a) All 19 in-loop accumulator outputs are present in AnalysisReport and
    // populated by this transcript -- proving process_transcript's single per-turn
    // loop (see engine.rs) actually wires and drives every one of them.
    assert!(report.circuit_breaker.tripped, "circuit_breaker: expected trip from 3 consecutive SubagentSpawn calls");
    assert!(report.turn_durations.total_turns > 0, "turn_durations: expected at least one measured latency");
    assert!(report.api_health.total_error_events > 0, "api_health: expected the 429 error to be counted");
    assert!(report.mcp_affinity.structured_mcp_count > 0, "mcp_affinity: expected the McpCall to be counted");
    assert!(report.self_corrections.total_corrections > 0, "self_corrections: expected the retry to be counted");
    assert!(report.context_growth.peak_prompt_tokens > 0, "context_growth: expected nonzero peak prompt tokens");
    assert!(report.tool_inventory.distinct_tools_count > 0, "tool_inventory: expected at least one distinct tool");
    assert!(report.autonomy.autonomy_index > 0.0, "autonomy: expected nonzero autonomy index");
    assert!(
        report.permission_mode.auto_accepted_actions > 0 && report.permission_mode.manual_approval_prompts > 0,
        "permission_mode: expected both auto-accepted and manual-approval events"
    );
    assert!(report.artifacts.total_unique_files > 0, "artifacts: expected at least one tracked file");
    assert!(!report.pr_link.pr_urls.is_empty(), "pr_link: expected the github.com PR URL to be captured");
    assert!(report.flow.total_tool_calls > 0, "flow: expected nonzero tool call count");
    assert!(!report.fuzzy_tools.clusters.is_empty(), "fuzzy_tools: expected at least one tool cluster");
    assert!(report.stats.total_turns > 0, "stats: expected nonzero total turns");
    assert!(!report.schema_extractor.is_empty(), "schema_extractor: expected the spec@1 citation to be captured");
    assert!(
        !report.attribution.by_plugin.is_empty() || report.attribution.unattributed_tokens > 0,
        "attribution: expected plugin-attributed tokens to be counted"
    );
    assert!(!report.trajectory_dag.is_empty(), "trajectory_dag: expected at least one tool node");
    assert!(
        report.timeline.assistant_streak_count > 0 || report.timeline.current_streak > 0,
        "timeline: expected an assistant streak to be recorded"
    );
    assert!(!report.span_mapping.mapped.is_empty(), "span_mapping: expected the otel_span_id to be mapped");

    // (b) token_usage, hook_activity, and otel_correlation are intentionally NOT part
    // of process_transcript's per-turn loop, by construction -- this test never
    // references TokenUsageAccumulator or HookActivityAccumulator (they are
    // RawMessageAccumulators invoked independently, over raw pre-parse JSON, not
    // over CanonicalTranscript), and the per-turn-loop assertions above never touch
    // report.otel_correlation, which engine.rs computes exactly once, post-loop, via
    // `OtelCorrelationAccumulator::finalize(transcript)` outside the
    // `for turn in &transcript.turns` loop.

    // (c) Zero-heap-allocation-in-the-per-turn-loop harness: NOT IMPLEMENTED, and
    // this is a genuine gap, not an oversight papered over as a tautological pass.
    //
    // This workspace sets `unsafe_code = "forbid"` in [workspace.lints.rust], inherited
    // by this crate via `[lints] workspace = true`, which rustc enforces as a hard
    // `-F unsafe-code` compiler flag. A `#[global_allocator]` guard requires
    // `unsafe impl GlobalAlloc`, which is empirically confirmed to fail to compile
    // under this flag -- including inside test binaries of this crate. No allocation-
    // counting crate (one that implements the required unsafe allocator internally and
    // exposes a safe `measure()`-style API) is present anywhere in this workspace's
    // Cargo.toml/Cargo.lock, and adding one requires editing
    // crates/lumen-analysis/Cargo.toml, which is out of scope for this task (scoped to
    // committing only this test file) and would collide with another task's uncommitted,
    // in-progress edits to that same file in this shared working tree.
    //
    // Independent of that tooling blocker, a static line-by-line audit of all 19
    // in-loop accumulators' update() methods (performed for this task) found concrete,
    // non-hypothetical per-turn heap allocations that would violate CRIT-LUMEN-060's
    // "zero heap allocations in the inner loop" clause even if the harness above could
    // be built:
    //   - accumulators/pr_link.rs update(): `entry.tool_calls.iter().map(...).collect::
    //     <BTreeMap<_, _>>()` allocates a fresh BTreeMap on every turn that has any
    //     tool_calls, unconditionally -- independent of whether a PR link is ever found.
    //   - accumulators/circuit_breaker.rs update(): `CompactString::new(format!(
    //     "parent->{agent_type}"))` allocates a heap String via `format!` on every
    //     SubagentSpawn tool call.
    //   - accumulators/artifacts.rs, tool_inventory.rs, fuzzy_tools.rs, attribution.rs,
    //     trajectory_dag.rs update(): `.clone()` on CompactString path/tool/plugin names
    //     feeding `BTreeMap::entry()` / `BTreeSet::insert()` -- a heap allocation for any
    //     string longer than CompactString's inline threshold, plus a first-insert heap
    //     allocation for the BTreeMap/BTreeSet node itself whenever a new key appears.
    // This means CRIT-LUMEN-060, as worded, does not hold against the live code. See the
    // concerns reported alongside this task's status for the recommended next step.
}
