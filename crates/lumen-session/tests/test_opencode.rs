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
fn test_opencode_adapter_non_utf8_line_is_skipped_not_fatal() {
    // CRIT-LUMEN-025: a non-UTF8 line surfaces as an io::Error from BufRead::lines(), not a
    // serde_json parse error -- the read-error branch must skip+record like the parse-error
    // branch does, not abort the whole parse and discard the surrounding valid lines.
    let mut sample: Vec<u8> = Vec::new();
    sample.extend_from_slice(
        br#"{"timestamp":"2026-08-19T12:00:00Z","action":"message","source":"user","args":{"content":"first"}}"#,
    );
    sample.push(b'\n');
    sample.extend_from_slice(b"invalid utf8 follows: \xFF\xFE\n");
    sample.extend_from_slice(br#"{"timestamp":"2026-08-19T12:00:03Z","observation":"read","content":"third"}"#);
    sample.push(b'\n');

    let adapter = OpenCodeAdapter;
    let transcript =
        adapter.parse_stream(Box::new(Cursor::new(sample))).expect("a non-UTF8 line must not abort the whole parse");

    assert_eq!(transcript.parse_failures.len(), 1);
    assert_eq!(transcript.turns.len(), 2);
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

#[test]
fn test_opencode_accumulated_cost_does_not_shadow_input_tokens() {
    // `metrics.accumulated_cost` is a dollar float and `metrics.input_tokens` is a real
    // non-zero integer in the same object. The float must not shadow the token count.
    let sample = r#"{"timestamp":"2026-08-19T12:00:00Z","action":"read","args":{"path":"src/main.rs"},"metrics":{"accumulated_cost":0.0234,"input_tokens":1500,"output_tokens":80}}
"#;

    let adapter = OpenCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse OpenCode session");

    assert_eq!(transcript.turns.len(), 1);
    let usage = transcript.turns[0].usage.as_ref().expect("expected usage on turn");
    assert_eq!(usage.input_tokens, 1500, "accumulated_cost float must not shadow real input_tokens");
}

#[test]
fn test_opencode_accumulated_cost_last_write_wins_as_provided_cost() {
    // accumulated_cost is a per-line cumulative running total (like Codex's token_count),
    // so the final economics.provided_cost_usd must equal the LAST value seen, not the
    // first and not a sum.
    let sample = r#"{"timestamp":"2026-08-19T12:00:00Z","action":"read","args":{"path":"a.rs"},"metrics":{"accumulated_cost":0.01,"input_tokens":100,"output_tokens":10}}
{"timestamp":"2026-08-19T12:00:01Z","action":"read","args":{"path":"b.rs"},"metrics":{"accumulated_cost":0.05,"input_tokens":200,"output_tokens":20}}
"#;

    let adapter = OpenCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse OpenCode session");

    assert_eq!(transcript.economics.provided_cost_usd, Some(0.05));
}

#[test]
fn test_opencode_no_accumulated_cost_yields_none_provided_cost() {
    let sample = r#"{"timestamp":"2026-08-19T12:00:00Z","action":"read","args":{"path":"a.rs"},"metrics":{"input_tokens":100,"output_tokens":10}}
"#;

    let adapter = OpenCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse OpenCode session");

    assert_eq!(transcript.economics.provided_cost_usd, None);
}

#[test]
fn test_opencode_malformed_line_produces_real_parse_failure_record() {
    let sample = "{\"timestamp\":\"2026-08-19T12:00:00Z\",\"action\":\"message\",\"source\":\"user\",\"args\":{\"content\":\"hi\"}}\nnot valid json at all\n{\"timestamp\":\"2026-08-19T12:00:02Z\",\"observation\":\"read\",\"content\":\"ok\"}\n";

    let adapter = OpenCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse OpenCode session");

    assert_eq!(transcript.parse_failures.len(), 1);
    let failure = &transcript.parse_failures[0];
    assert_eq!(failure.line_number, 2);
    assert!(!failure.error.is_empty(), "error message must be real, not hardcoded/empty");
    assert!(failure.byte_offset > 0, "second line's byte offset must be non-zero");
}

#[test]
fn test_opencode_pure_cache_read_turn_is_not_dropped() {
    // A genuine pure cache-hit turn reports input_tokens: 0, output_tokens: 0, but a real
    // non-zero cache_read_input_tokens. The usage-capture gate must not require in_tok/out_tok
    // to be non-zero -- otherwise the entire 5000-token cache read is silently discarded: no
    // turn_usage is recorded and none of the running totals are incremented.
    let sample = r#"{"timestamp":"2026-08-19T12:00:00Z","action":"read","args":{"path":"src/main.rs"},"metrics":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":5000,"cache_creation_input_tokens":0}}
"#;

    let adapter = OpenCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse OpenCode session");

    let usage = transcript.turns[0].usage.as_ref().expect("pure cache-read turn must still record usage");
    assert_eq!(usage.cache_read_tokens, 5000, "real cache read tokens must not be dropped");
    assert_eq!(
        transcript.economics.cache_read_tokens, 5000,
        "cache read tokens must reach the running total/economics"
    );
}

#[test]
fn test_opencode_pure_cache_write_turn_is_not_dropped() {
    // Same defect, mirrored for cache_creation_input_tokens (a pure cache-write turn with
    // zero input_tokens/output_tokens).
    let sample = r#"{"timestamp":"2026-08-19T12:00:00Z","action":"read","args":{"path":"src/main.rs"},"metrics":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":3000}}
"#;

    let adapter = OpenCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse OpenCode session");

    let usage = transcript.turns[0].usage.as_ref().expect("pure cache-write turn must still record usage");
    assert_eq!(usage.cache_creation_tokens, 3000, "real cache write tokens must not be dropped");
    assert_eq!(
        transcript.economics.cache_creation_tokens, 3000,
        "cache write tokens must reach the running total/economics"
    );
}
