use lumen_model::*;
use lumen_session::*;
use std::io::Cursor;

#[test]
fn test_claude_code_fingerprint_detection() {
    let sample = b"{\"type\":\"mode\",\"mode\":\"normal\",\"sessionId\":\"ef175eb8-0825-4122-934b\"}\n{\"type\":\"permission-mode\"}";
    let start = std::time::Instant::now();
    let detected = detect_orchestrator(sample);
    let duration = start.elapsed();

    assert_eq!(detected, Some(OrchestratorKind::ClaudeCode));
    // CRIT-LUMEN-020: under 1ms
    assert!(duration.as_micros() < 1000);
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
