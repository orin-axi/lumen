use lumen_model::*;
use lumen_session::*;
use std::io::Cursor;

#[test]
fn test_codex_adapter_parses_thread_id_choices_and_cumulative_usage() {
    // CRIT-LUMEN-109/110: session_id from thread_id, one CanonicalTurn per choices[]
    // element, role mapped from choices[i].message.role, text from
    // choices[i].message.content, and usage summed (not overwritten) across every
    // usage-bearing line.
    let sample = r#"{"thread_id":"codex-abc123","choices":[{"message":{"role":"assistant","content":"Let me look at the file"}}],"usage":{"prompt_tokens":100,"completion_tokens":20}}
{"thread_id":"codex-abc123","choices":[{"message":{"role":"user","content":"Fix the bug"}},{"message":{"role":"assistant","content":"Done"}}],"usage":{"prompt_tokens":50,"completion_tokens":10}}
"#;

    let adapter = CodexAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Codex session");

    assert_eq!(transcript.session_id, "codex-abc123");
    assert_eq!(transcript.orchestrator, OrchestratorKind::Codex);
    assert_eq!(transcript.turns.len(), 3);

    assert_eq!(transcript.turns[0].role, TurnRole::Assistant);
    assert_eq!(transcript.turns[0].text.as_deref(), Some("Let me look at the file"));

    assert_eq!(transcript.turns[1].role, TurnRole::User);
    assert_eq!(transcript.turns[1].text.as_deref(), Some("Fix the bug"));

    assert_eq!(transcript.turns[2].role, TurnRole::Assistant);
    assert_eq!(transcript.turns[2].text.as_deref(), Some("Done"));

    // Cumulative sum across both usage-bearing lines, not last-write-overwrite:
    // prompt_tokens: 100 + 50 = 150, completion_tokens: 20 + 10 = 30
    assert_eq!(transcript.economics.input_tokens, 150);
    assert_eq!(transcript.economics.output_tokens, 30);
}

#[test]
fn test_codex_adapter_skips_malformed_lines() {
    let sample = "not valid json\n{\"thread_id\":\"codex-xyz\",\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"hi\"}}]}\n\n";
    let adapter = CodexAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Codex session");

    assert_eq!(transcript.session_id, "codex-xyz");
    assert_eq!(transcript.turns.len(), 1);
}
