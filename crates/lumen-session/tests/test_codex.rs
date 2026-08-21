use lumen_fixtures::real_codex_session_dump;
use lumen_model::*;
use lumen_session::*;
use std::io::Cursor;

#[test]
fn test_codex_fingerprint_detection() {
    // CRIT-LUMEN-106: a real event_msg envelope sample is detected as Codex, under 1ms.
    let sample = br#"{"timestamp":"2026-08-20T10:00:00Z","ordinal":1,"type":"event_msg","payload":{"type":"thread_settings_applied","thread_id":"thread-abc123","thread_settings":{"service_tier":"Standard"}}}"#;
    let start = std::time::Instant::now();
    let detected = detect_orchestrator(sample);
    let duration = start.elapsed();

    assert_eq!(detected, Some(OrchestratorKind::Codex));
    assert!(duration.as_micros() < 1000);
}

#[test]
fn test_codex_adapter_parses_real_event_msg_envelope() {
    // CRIT-LUMEN-109/110/162: turn role mapping, cumulative (last-write, not summed)
    // token accounting, reasoning_output_tokens kept distinct, and service_tier capture.
    let sample = real_codex_session_dump();

    let adapter = CodexAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Codex session");

    assert_eq!(transcript.session_id, "thread-abc123");
    assert_eq!(transcript.orchestrator, OrchestratorKind::Codex);

    assert_eq!(transcript.turns.len(), 3);
    assert_eq!(transcript.turns[0].role, TurnRole::User);
    assert_eq!(transcript.turns[0].text.as_deref(), Some("Fix the failing test"));
    assert_eq!(transcript.turns[1].role, TurnRole::Assistant);
    assert_eq!(transcript.turns[1].text.as_deref(), Some("Looking now."));
    assert_eq!(transcript.turns[2].role, TurnRole::ToolResult);
    assert_eq!(transcript.turns[2].text.as_deref(), Some("cargo test"));

    // Last-write, not summed: the second token_count line (1500/110/55) must win over the
    // first (1200/85/40). A naive sum would produce 2700/195.
    assert_eq!(transcript.economics.input_tokens, 1500);
    assert_eq!(transcript.economics.output_tokens, 110);
    assert_eq!(transcript.economics.reasoning_output_tokens, 55);

    assert_eq!(transcript.service_tier.as_deref(), Some("Standard"));
}

#[test]
fn test_codex_adapter_matches_fingerprint_parity_with_detect_orchestrator() {
    // CRIT-LUMEN-108: CodexAdapter::matches_fingerprint must agree with detect_orchestrator
    // on both a real matching sample and a non-matching sample.
    let adapter = CodexAdapter;

    let matching = real_codex_session_dump();
    assert_eq!(
        adapter.matches_fingerprint(matching),
        detect_orchestrator(matching.as_bytes()) == Some(OrchestratorKind::Codex)
    );
    assert!(adapter.matches_fingerprint(matching));

    let non_matching = r#"{"sessionId":"abc","parentUuid":"turn-0","type":"assistant"}"#;
    assert_eq!(
        adapter.matches_fingerprint(non_matching),
        detect_orchestrator(non_matching.as_bytes()) == Some(OrchestratorKind::Codex)
    );
    assert!(!adapter.matches_fingerprint(non_matching));
}
