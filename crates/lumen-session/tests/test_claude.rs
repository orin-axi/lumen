use lumen_model::*;
use lumen_session::*;
use std::io::Cursor;
use std::io::Write;

#[test]
fn test_claude_code_fingerprint_detection() {
    let sample =
        b"{\"type\":\"assistant\",\"sessionId\":\"ef175eb8-0825-4122-934b\",\"parentUuid\":\"turn-0\",\"message\":{}}";
    let start = std::time::Instant::now();
    let detected = detect_orchestrator(sample);
    let duration = start.elapsed();

    assert_eq!(detected, Some(OrchestratorKind::ClaudeCode));
    // CRIT-LUMEN-020: under 1ms
    assert!(duration.as_micros() < 1000);
}

#[test]
fn test_claude_code_fingerprint_sessionid_alone_is_not_claude_code() {
    let sample = b"{\"sessionId\":\"ef175eb8-0825-4122-934b\"}";
    assert_ne!(detect_orchestrator(sample), Some(OrchestratorKind::ClaudeCode));
}

#[test]
fn test_claude_code_adapter_matches_fingerprint_parity_with_detect_orchestrator() {
    // CRIT-LUMEN drift fix: ClaudeCodeAdapter::matches_fingerprint must agree with
    // detect_orchestrator's ClaudeCode branch exactly (sessionId && parentUuid together).
    let adapter = ClaudeCodeAdapter;

    let matching = r#"{"type":"assistant","sessionId":"ef175eb8-0825-4122-934b","parentUuid":"turn-0"}"#;
    assert_eq!(
        adapter.matches_fingerprint(matching),
        detect_orchestrator(matching.as_bytes()) == Some(OrchestratorKind::ClaudeCode)
    );
    assert!(adapter.matches_fingerprint(matching));

    // Concrete drift case: sessionId + leafUuid, but no parentUuid. The old independent
    // condition accepted leafUuid as an alternative to parentUuid; detect_orchestrator requires
    // sessionId && parentUuid together, so this sample must NOT match.
    let drift_case = r#"{"sessionId":"x","leafUuid":"y"}"#;
    assert_eq!(
        adapter.matches_fingerprint(drift_case),
        detect_orchestrator(drift_case.as_bytes()) == Some(OrchestratorKind::ClaudeCode)
    );
    assert!(!adapter.matches_fingerprint(drift_case));

    let non_matching = r#"{"type":"event_msg"}"#;
    assert_eq!(
        adapter.matches_fingerprint(non_matching),
        detect_orchestrator(non_matching.as_bytes()) == Some(OrchestratorKind::ClaudeCode)
    );
    assert!(!adapter.matches_fingerprint(non_matching));
}

#[test]
fn test_claude_code_adapter_streaming_and_tool_extraction() {
    let sample = r#"{"type":"user","message":{"role":"user","content":"List files in project"}}
{"type":"assistant","message":{"model":"claude-3-5-sonnet-20241022","role":"assistant","content":[{"type":"tool_use","id":"tool_1","name":"view_file","input":{"file_path":"Cargo.toml"}}],"usage":{"input_tokens":1200,"output_tokens":150,"cache_creation_input_tokens":500,"cache_read_input_tokens":8000}}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool_1","content":"[workspace]\nmembers = []"}]}}
"#;

    let adapter = ClaudeCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Claude Code session");

    assert_eq!(transcript.orchestrator, OrchestratorKind::ClaudeCode);
    assert_eq!(transcript.model_family, "claude-3-5-sonnet-20241022");
    assert_eq!(transcript.turns.len(), 3);

    // CRIT-LUMEN-022: Tool call extraction & intent
    let assistant_turn = &transcript.turns[1];
    assert_eq!(assistant_turn.role, TurnRole::Assistant);
    assert_eq!(assistant_turn.tool_calls.len(), 1);
    assert_eq!(assistant_turn.tool_calls[0].call_id, "tool_1");
    assert_eq!(assistant_turn.tool_calls[0].tool_name, "view_file");
    assert!(matches!(assistant_turn.tool_calls[0].intent, ToolIntent::FileRead { .. }));

    // CRIT-LUMEN-023: Usage extraction
    let usage = assistant_turn.usage.unwrap();
    assert_eq!(usage.input_tokens, 1200);
    assert_eq!(usage.output_tokens, 150);
    assert_eq!(usage.cache_creation_tokens, 500);
    assert_eq!(usage.cache_read_tokens, 8000);

    // Economics summary
    assert_eq!(transcript.economics.input_tokens, 1200);
    assert_eq!(transcript.economics.cache_read_tokens, 8000);
    assert!(transcript.economics.cache_hit_ratio > 80.0);
}

#[test]
fn test_claude_code_adapter_corrupted_lines_resilience() {
    // CRIT-LUMEN-025: Malformed JSON lines should be skipped gracefully without panic
    let sample = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Hello\"}}\n\
                  {CORRUPTED TRUNCATED JSON LINE !@#$%\n\
                  {\"type\":\"assistant\",\"message\":{\"model\":\"claude-3-5-sonnet-20241022\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Hi there!\"}],\"usage\":{\"input_tokens\":500,\"output_tokens\":20,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}\n\
                  MALFORMED NON JSON LINE\n";

    let adapter = ClaudeCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Should not error on corrupted lines");

    assert_eq!(transcript.turns.len(), 2);
    assert_eq!(transcript.turns[0].role, TurnRole::User);
    assert_eq!(transcript.turns[1].role, TurnRole::Assistant);

    // CRIT-LUMEN-025: surviving parse-failure records
    assert_eq!(transcript.parse_failures.len(), 2);
    assert_eq!(transcript.parse_failures[0].line_number, 2);
    assert_eq!(transcript.parse_failures[1].line_number, 4);
    assert!(!transcript.parse_failures[0].error.is_empty());
}

#[test]
fn test_claude_code_adapter_empty_session() {
    // CRIT-LUMEN-028: Empty session returns valid 0-turn transcript without error
    let sample = "";
    let adapter = ClaudeCodeAdapter;
    let transcript =
        adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Empty session must parse successfully");

    assert_eq!(transcript.turns.len(), 0);
    assert_eq!(transcript.economics.input_tokens, 0);
    assert_eq!(transcript.economics.cache_hit_ratio, 0.0);
}

#[test]
fn test_claude_code_adapter_excludes_sidechain_entries_from_main_turns() {
    // CRIT-LUMEN-026: is_sidechain:true entries belong to a subagent branch, not the main chain.
    let sample = r#"{"type":"user","is_sidechain":true,"message":{"role":"user","content":"sidechain message"}}
{"type":"user","message":{"role":"user","content":"main message"}}
"#;

    let adapter = ClaudeCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Claude Code session");

    assert_eq!(transcript.turns.len(), 1);
    assert_eq!(transcript.turns[0].role, TurnRole::User);
    assert_eq!(transcript.turns[0].text.as_deref(), Some("main message"));
}

#[test]
fn test_claude_code_adapter_links_subagent_transcript_file() {
    // CRIT-LUMEN-026: sibling subagents/<worker>.jsonl files are linked as child transcripts.
    let dir = tempfile::tempdir().expect("Failed to create temp dir");

    let main_line = r#"{"type":"user","sessionId":"parent-1","message":{"role":"user","content":"main"}}
"#;
    std::fs::write(dir.path().join("main.jsonl"), main_line).expect("Failed to write main.jsonl");

    let subagents_dir = dir.path().join("subagents");
    std::fs::create_dir(&subagents_dir).expect("Failed to create subagents dir");
    let worker_line = r#"{"type":"user","message":{"role":"user","content":"worker"}}
"#;
    let mut worker_file =
        std::fs::File::create(subagents_dir.join("worker-a.jsonl")).expect("Failed to create worker-a.jsonl");
    worker_file.write_all(worker_line.as_bytes()).expect("Failed to write worker-a.jsonl");

    let adapter = ClaudeCodeAdapter;
    let transcript =
        adapter.parse_session_with_subagents(dir.path(), "main.jsonl").expect("Failed to parse session with subagents");

    assert_eq!(transcript.subagents.len(), 1);
    assert_eq!(transcript.subagents[0].parent_session_id, Some("parent-1".into()));
    assert_eq!(transcript.subagents[0].subagent_role, Some("worker-a".into()));
}

#[test]
fn test_claude_code_adapter_rejects_explicit_null_session_id() {
    // CRIT-LUMEN-163: an explicit JSON null on a known field is malformed, not a valid empty value.
    let sample = "{\"type\":\"user\",\"sessionId\":null,\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n";

    let adapter = ClaudeCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Should not error on explicit null");

    assert_eq!(transcript.parse_failures.len(), 1);
    assert_eq!(transcript.turns.len(), 0);
}

#[test]
fn test_claude_code_adapter_otel_conversation_id_first_non_sidechain_wins() {
    // CRIT-LUMEN-164: otel_conversation_id is set from the first non-sidechain entry's requestId,
    // never overwritten thereafter; sidechain entries are skipped entirely.
    let sample = r#"{"type":"user","is_sidechain":true,"requestId":"req-side","message":{"role":"user","content":"side"}}
{"type":"user","requestId":"req-first","message":{"role":"user","content":"first"}}
{"type":"user","requestId":"req-second","message":{"role":"user","content":"second"}}
"#;

    let adapter = ClaudeCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Claude Code session");

    assert_eq!(transcript.otel_conversation_id, Some("req-first".into()));
}
