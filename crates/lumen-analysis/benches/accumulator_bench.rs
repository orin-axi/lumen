use chrono::Utc;
use compact_str::CompactString;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lumen_analysis::engine::AnalyticsEngine;
use lumen_model::{
    pricing, CanonicalToolCall, CanonicalTranscript, CanonicalTurn, ExecutionTiming, OrchestratorKind,
    TokenEconomics, ToolIntent, TurnPricingInput, TurnRole, TurnTokenUsage,
};
use smallvec::smallvec;

fn synthetic_transcript(turn_count: usize) -> CanonicalTranscript {
    let turns = (0..turn_count)
        .map(|i| CanonicalTurn {
            attribution: None,
            turn_index: i,
            role: if i % 3 == 0 { TurnRole::User } else { TurnRole::Assistant },
            timestamp: Utc::now(),
            latency_ms: 250,
            text: Some(format!("turn {i} synthetic text payload")),
            tool_calls: smallvec![CanonicalToolCall {
                call_id: CompactString::new(format!("call_{i}")),
                tool_name: CompactString::new("Read"),
                intent: ToolIntent::FileRead { path: CompactString::new("src/lib.rs"), line_range: None },
                raw_arguments: serde_json::json!({}),
            }],
            tool_results: smallvec![],
            usage: Some(TurnTokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                cache_creation_tokens: 0,
                cache_read_tokens: 900,
                reasoning_tokens: 0,
            }),
        })
        .collect();

    CanonicalTranscript {
        session_id: CompactString::new("bench-session"),
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
        economics: TokenEconomics::calculate(
            &[TurnPricingInput { usage: TurnTokenUsage::default(), timestamp: Utc::now(), tier: None }],
            "claude-3-5-sonnet-20241022",
            &pricing::SEEDED,
            None,
        ),
        turns,
        subagents: vec![],
        extracted_schemas: smallvec![],
        detected_anomalies: smallvec![],
        otel_conversation_id: None,
        service_tier: None,
        parse_failures: smallvec![],
    }
}

fn accumulator_benchmark(c: &mut Criterion) {
    let transcript_10k = synthetic_transcript(10_000);

    c.bench_function("analytics_engine_process_10k_turns", |b| {
        b.iter(|| {
            let engine = AnalyticsEngine::new();
            black_box(engine.process_transcript(black_box(&transcript_10k)))
        })
    });
}

criterion_group!(benches, accumulator_benchmark);
criterion_main!(benches);
