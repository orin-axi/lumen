use lumen_fixtures::corpus::*;
use lumen_model::*;
use lumen_session::*;
use rstest::rstest;
use std::io::Cursor;

#[rstest]
#[case(real_claude_session_dump(), Some(OrchestratorKind::ClaudeCode))]
#[case(claude_session_with_errors_and_rate_limits(), Some(OrchestratorKind::ClaudeCode))]
#[case(real_antigravity_session_dump(), Some(OrchestratorKind::Antigravity))]
#[case(real_opencode_session_dump(), Some(OrchestratorKind::OpenCode))]
#[case("random unformatted log line without agent markers", None)]
fn test_table_driven_fingerprint_detection(#[case] input: &str, #[case] expected: Option<OrchestratorKind>) {
    assert_eq!(detect_orchestrator(input.as_bytes()), expected);
}

#[test]
fn test_claude_fixture_end_to_end_parsing() {
    let adapter = ClaudeCodeAdapter;
    let transcript = adapter
        .parse_stream(Box::new(Cursor::new(real_claude_session_dump())))
        .expect("Failed to parse real Claude Code fixture");

    assert_eq!(transcript.orchestrator, OrchestratorKind::ClaudeCode);
    assert_eq!(transcript.turns.len(), 6);
    assert_eq!(transcript.turns[0].role, TurnRole::User);
    assert_eq!(transcript.turns[1].role, TurnRole::Assistant);
    assert_eq!(transcript.turns[2].role, TurnRole::ToolResult);
    assert_eq!(transcript.turns[3].role, TurnRole::Assistant);
    assert_eq!(transcript.turns[4].role, TurnRole::ToolResult);
    assert_eq!(transcript.turns[5].role, TurnRole::Assistant);

    assert!(transcript.economics.total_cost_usd > 0.0);
    assert!(transcript.economics.net_savings_usd > 0.0);
    assert!(transcript.economics.cache_hit_ratio > 70.0);
}

#[test]
fn test_antigravity_fixture_end_to_end_parsing() {
    let adapter = AgyAdapter;
    let transcript = adapter
        .parse_stream(Box::new(Cursor::new(real_antigravity_session_dump())))
        .expect("Failed to parse real Antigravity fixture");

    assert_eq!(transcript.orchestrator, OrchestratorKind::Antigravity);
    assert_eq!(transcript.turns.len(), 6);
    assert_eq!(transcript.turns[0].role, TurnRole::User);
    assert_eq!(transcript.turns[1].role, TurnRole::Assistant);
    assert_eq!(transcript.turns[2].role, TurnRole::ToolResult);
    assert_eq!(transcript.turns[3].role, TurnRole::Assistant);
    assert_eq!(transcript.turns[4].role, TurnRole::ToolResult);
    assert_eq!(transcript.turns[5].role, TurnRole::Assistant);

    // Verify thinking block extraction
    assert!(transcript.turns[1].text.as_ref().unwrap().contains("spawn a subagent"));
    assert!(transcript.turns[3].text.as_ref().unwrap().contains("inspect the cycle depth"));
    assert!(transcript.turns[5].text.as_ref().unwrap().contains("Audit complete"));
}

#[test]
fn test_opencode_fixture_end_to_end_parsing() {
    let adapter = OpenCodeAdapter;
    let transcript = adapter
        .parse_stream(Box::new(Cursor::new(real_opencode_session_dump())))
        .expect("Failed to parse real OpenCode fixture");

    assert_eq!(transcript.orchestrator, OrchestratorKind::OpenCode);
    assert_eq!(transcript.turns.len(), 7);
}

#[test]
fn test_corrupted_fixture_resilience() {
    let adapter = ClaudeCodeAdapter;
    let transcript = adapter
        .parse_stream(Box::new(Cursor::new(corrupted_mixed_lines_sample())))
        .expect("Corrupted fixture must parse surviving lines without failing");

    // Must successfully parse valid user/assistant turns despite BOM and corrupted JSON
    assert_eq!(transcript.turns.len(), 3);
    assert_eq!(transcript.turns[0].text.as_deref(), Some("Test begin"));
    assert_eq!(transcript.turns[1].role, TurnRole::Assistant);
    assert_eq!(transcript.turns[2].text.as_deref(), Some("Test end"));
}
