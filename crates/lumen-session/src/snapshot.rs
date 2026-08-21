use lumen_model::{CanonicalTranscript, TurnTokenUsage};

/// Merges pre-compaction snapshots with the final parse result using max() semantics.
pub fn merge_precompact_snapshots(
    final_transcript: CanonicalTranscript,
    snapshots: &[TurnTokenUsage],
) -> CanonicalTranscript {
    if snapshots.is_empty() {
        return final_transcript;
    }

    let mut merged = final_transcript;

    let mut max_input = merged.economics.input_tokens;
    let mut max_output = merged.economics.output_tokens;
    let mut max_cache_creation = merged.economics.cache_creation_tokens;
    let mut max_cache_read = merged.economics.cache_read_tokens;

    for snap in snapshots {
        max_input = max_input.max(snap.input_tokens);
        max_output = max_output.max(snap.output_tokens);
        max_cache_creation = max_cache_creation.max(snap.cache_creation_tokens);
        max_cache_read = max_cache_read.max(snap.cache_read_tokens);
    }

    merged.economics = lumen_model::TokenEconomics::calculate(
        max_input,
        max_output,
        max_cache_creation,
        max_cache_read,
        &merged.model_family,
    );

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use compact_str::CompactString;
    use lumen_model::*;

    #[test]
    fn test_merge_snapshots_uses_max_not_sum() {
        let transcript = CanonicalTranscript {
            session_id: CompactString::new("sess-1"),
            parent_session_id: None,
            subagent_role: None,
            orchestrator: OrchestratorKind::ClaudeCode,
            model_family: CompactString::new("claude-3-5-sonnet-20241022"),
            timing: ExecutionTiming {
                started_at: Utc::now(),
                ended_at: Utc::now(),
                wall_duration_ms: 1000,
                active_duration_ms: 1000,
                idle_duration_ms: 0,
                idle_gap_count: 0,
            },
            economics: TokenEconomics::calculate(100, 50, 500, 2000, "claude-3-5-sonnet-20241022"),
            turns: vec![],
            subagents: vec![],
            extracted_schemas: smallvec::smallvec![],
            detected_anomalies: smallvec::smallvec![],
            otel_request_ids: smallvec::smallvec![],
        };

        let snapshots = vec![
            TurnTokenUsage {
                input_tokens: 300,
                output_tokens: 150,
                cache_creation_tokens: 1000,
                cache_read_tokens: 5000,
            },
            TurnTokenUsage {
                input_tokens: 200,
                output_tokens: 100,
                cache_creation_tokens: 800,
                cache_read_tokens: 4000,
            },
        ];

        let merged = merge_precompact_snapshots(transcript, &snapshots);

        assert_eq!(merged.economics.input_tokens, 300);
        assert_eq!(merged.economics.output_tokens, 150);
        assert_eq!(merged.economics.cache_read_tokens, 5000);
    }
}
