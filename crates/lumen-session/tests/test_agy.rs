use lumen_model::*;
use lumen_session::*;
use std::io::Cursor;

#[test]
fn test_agy_fingerprint_detection() {
    let sample = b"{\"step_index\":0,\"source\":\"USER_EXPLICIT\",\"type\":\"USER_INPUT\",\"status\":\"DONE\"}";
    let start = std::time::Instant::now();
    let detected = detect_orchestrator(sample);
    let duration = start.elapsed();

    assert_eq!(detected, Some(OrchestratorKind::Antigravity));
    // CRIT-LUMEN-021: under 1ms
    assert!(duration.as_micros() < 1000);
}

#[test]
fn test_agy_adapter_thinking_blocks_and_tool_calls() {
    // CRIT-LUMEN-027: Extracts thinking reasoning into CanonicalTurn.text
    let sample = r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"Find all tests"}
{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"I need to scan tests directory","tool_calls":[{"name":"find_by_name","args":{"SearchDirectory":"tests","Pattern":"*.rs"}}]}
{"step_index":2,"source":"SYSTEM","type":"TOOL_RESULT","content":"tests/test_a.rs\ntests/test_b.rs"}
"#;

    let adapter = AgyAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse AGY session");

    assert_eq!(transcript.orchestrator, OrchestratorKind::Antigravity);
    assert_eq!(transcript.turns.len(), 3);
    assert_eq!(transcript.turns[0].role, TurnRole::User);
    assert_eq!(transcript.turns[0].text.as_deref(), Some("Find all tests"));

    // Verify thinking block extraction
    let assistant_turn = &transcript.turns[1];
    assert_eq!(assistant_turn.role, TurnRole::Assistant);
    assert_eq!(assistant_turn.text.as_deref(), Some("I need to scan tests directory"));
    assert_eq!(assistant_turn.tool_calls.len(), 1);
    assert_eq!(assistant_turn.tool_calls[0].tool_name, "find_by_name");
}

#[test]
fn test_agy_adapter_tool_call_args_double_json_parse() {
    // CRIT-LUMEN-027: each tool_call arg value is itself a JSON-encoded string requiring a
    // second parse pass; a value that fails the inner parse retains its raw string.
    let sample = r#"{"step_index":0,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"scanning","tool_calls":[{"name":"find_by_name","args":{"DirectoryPath":"\"/Users/gabe/lumen\"","Malformed":"not-json"}}]}"#;

    let adapter = AgyAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).unwrap();

    let args = &transcript.turns[0].tool_calls[0].raw_arguments;
    assert_eq!(args.get("DirectoryPath").and_then(|v| v.as_str()), Some("/Users/gabe/lumen"));
    assert_eq!(args.get("Malformed").and_then(|v| v.as_str()), Some("not-json"));
}

#[test]
fn test_agy_resolve_transcript_path_bypasses_symlink_layer() {
    // CRIT-LUMEN-165: resolves the real brain/ transcript path directly, not the
    // ~/.gemini/logs/<id>.jsonl symlink layer.
    let brain_root = std::path::Path::new("/Users/test/.gemini/antigravity-cli/brain");
    let path = AgyAdapter::resolve_transcript_path(brain_root, "conv-123");
    assert_eq!(
        path,
        std::path::PathBuf::from("/Users/test/.gemini/antigravity-cli/brain/conv-123/.system_generated/logs/transcript.jsonl")
    );
    assert!(!path.to_string_lossy().contains("/.gemini/logs/"));
}

#[test]
fn test_agy_adapter_matches_fingerprint_parity_with_detect_orchestrator() {
    let adapter = AgyAdapter;

    let matching = r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT"}"#;
    assert_eq!(
        adapter.matches_fingerprint(matching),
        detect_orchestrator(matching.as_bytes()) == Some(OrchestratorKind::Antigravity)
    );
    assert!(adapter.matches_fingerprint(matching));

    let non_matching = r#"{"type":"event_msg"}"#;
    assert_eq!(
        adapter.matches_fingerprint(non_matching),
        detect_orchestrator(non_matching.as_bytes()) == Some(OrchestratorKind::Antigravity)
    );
    assert!(!adapter.matches_fingerprint(non_matching));
}

#[test]
fn test_agy_adapter_does_not_claim_precedence_over_claude_code() {
    // detect_orchestrator resolves a sample with BOTH ClaudeCode and Antigravity markers to
    // ClaudeCode (checked first); AgyAdapter's own matches_fingerprint must not independently
    // claim it via its own standalone condition.
    let dual_marker = r#"{"sessionId":"x","parentUuid":"y","step_index":0,"source":"MODEL"}"#;
    assert_eq!(detect_orchestrator(dual_marker.as_bytes()), Some(OrchestratorKind::ClaudeCode));

    let adapter = AgyAdapter;
    assert_eq!(
        adapter.matches_fingerprint(dual_marker),
        detect_orchestrator(dual_marker.as_bytes()) == Some(OrchestratorKind::Antigravity)
    );
    assert!(!adapter.matches_fingerprint(dual_marker));
}
