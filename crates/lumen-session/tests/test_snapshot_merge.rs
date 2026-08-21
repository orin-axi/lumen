use chrono::DateTime;
use compact_str::CompactString;
use lumen_model::*;
use lumen_session::*;
use smallvec::SmallVec;

#[test]
fn test_merge_precompact_snapshots_max_invariant() {
    let fixed_ts = DateTime::from_timestamp(1771416000, 0).unwrap();

    let final_transcript = CanonicalTranscript {
        session_id: CompactString::new("sess-compacted"),
        parent_session_id: None,
        orchestrator: OrchestratorKind::ClaudeCode,
        model_family: CompactString::new("claude-3-5-sonnet-20241022"),
        timing: ExecutionTiming {
            started_at: fixed_ts,
            ended_at: fixed_ts,
            wall_duration_ms: 20000,
            active_duration_ms: 20000,
            idle_duration_ms: 0,
            idle_gap_count: 0,
        },
        economics: TokenEconomics::calculate(5000, 1000, 2000, 15000, "claude-3-5-sonnet-20241022"),
        turns: Vec::new(),
        subagents: Vec::new(),
        extracted_schemas: SmallVec::new(),
        detected_anomalies: SmallVec::new(),
        otel_request_ids: SmallVec::new(),
    };

    let snapshots = vec![
        TurnTokenUsage {
            input_tokens: 30000, // Pre-compact snapshot had higher peak input
            output_tokens: 500,
            cache_creation_tokens: 1000,
            cache_read_tokens: 40000, // Higher peak cache read
        },
        TurnTokenUsage {
            input_tokens: 25000,
            output_tokens: 800,
            cache_creation_tokens: 2000,
            cache_read_tokens: 35000,
        },
    ];

    // CRIT-LUMEN-024: Merging pre-compaction snapshots applies max()
    let merged = merge_precompact_snapshots(final_transcript, &snapshots);

    assert_eq!(merged.economics.input_tokens, 30000);
    assert_eq!(merged.economics.cache_read_tokens, 40000);
}
