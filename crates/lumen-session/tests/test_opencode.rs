use lumen_model::*;
use lumen_session::*;
use std::io::Cursor;

#[test]
fn test_opencode_adapter_parses_actions_and_observations() {
    let sample = r#"{"timestamp":"2026-08-19T12:00:00Z","action":"message","source":"user","args":{"content":"Fix the bug in main.rs"}}
{"timestamp":"2026-08-19T12:00:02Z","action":"read","thought":"Let me inspect main.rs","args":{"path":"src/main.rs"},"metrics":{"input_tokens":1500,"output_tokens":80,"cache_read_input_tokens":1200,"cache_creation_input_tokens":0}}
{"timestamp":"2026-08-19T12:00:03Z","observation":"read","content":"fn main() {}"}
"#;

    let adapter = OpenCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse OpenCode session");

    assert_eq!(transcript.orchestrator, OrchestratorKind::OpenCode);
    assert_eq!(transcript.turns.len(), 3);
    assert_eq!(transcript.turns[0].role, TurnRole::User);
    assert_eq!(transcript.turns[1].role, TurnRole::Assistant);
    assert_eq!(transcript.turns[1].tool_calls.len(), 1);
    assert_eq!(transcript.turns[1].tool_calls[0].tool_name, "read");
    assert_eq!(transcript.turns[2].role, TurnRole::ToolResult);

    // Verify token economics
    assert_eq!(transcript.economics.input_tokens, 1500);
    assert_eq!(transcript.economics.cache_read_tokens, 1200);
    assert_eq!(transcript.economics.output_tokens, 80);
    assert!(transcript.economics.cache_hit_ratio > 40.0);
}

#[test]
fn test_opencode_adapter_matches_fingerprint_parity_with_detect_orchestrator() {
    let adapter = OpenCodeAdapter;

    let matching = r#"{"action":"run","args":{"command":"cargo test"}}"#;
    assert_eq!(
        adapter.matches_fingerprint(matching),
        detect_orchestrator(matching.as_bytes()) == Some(OrchestratorKind::OpenCode)
    );
    assert!(adapter.matches_fingerprint(matching));

    let non_matching = r#"{"type":"event_msg"}"#;
    assert_eq!(
        adapter.matches_fingerprint(non_matching),
        detect_orchestrator(non_matching.as_bytes()) == Some(OrchestratorKind::OpenCode)
    );
    assert!(!adapter.matches_fingerprint(non_matching));
}

#[test]
fn test_opencode_adapter_does_not_claim_precedence_over_claude_code() {
    // detect_orchestrator resolves a sample with BOTH ClaudeCode and OpenCode markers to
    // ClaudeCode (checked first); OpenCodeAdapter's own matches_fingerprint must not
    // independently claim it via its own standalone condition.
    let dual_marker = r#"{"sessionId":"x","parentUuid":"y","action":"run"}"#;
    assert_eq!(detect_orchestrator(dual_marker.as_bytes()), Some(OrchestratorKind::ClaudeCode));

    let adapter = OpenCodeAdapter;
    assert_eq!(
        adapter.matches_fingerprint(dual_marker),
        detect_orchestrator(dual_marker.as_bytes()) == Some(OrchestratorKind::OpenCode)
    );
    assert!(!adapter.matches_fingerprint(dual_marker));
}
