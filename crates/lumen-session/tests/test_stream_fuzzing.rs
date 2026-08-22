use lumen_session::*;
use proptest::prelude::*;
use std::io::Cursor;

proptest! {
    #[test]
    fn prop_fuzz_random_bytes_does_not_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        // Fingerprinting must never panic on arbitrary bytes
        let _detected = detect_orchestrator(&bytes);

        // Parsers must never panic on arbitrary bytes. CRIT-LUMEN-025: as of the
        // skip-and-record fix for non-UTF8/unreadable lines, none of these adapters have any
        // remaining `return Err(...)` path inside parse_stream -- every line-level failure
        // (bad JSON or bad bytes) is recorded as a ParseFailureRecord and parsing continues.
        // So parse_stream must now always return Ok(...) for arbitrary byte input, never Err.
        let claude = ClaudeCodeAdapter;
        let claude_res = claude.parse_stream(Box::new(Cursor::new(&bytes)));
        prop_assert!(claude_res.is_ok(), "ClaudeCodeAdapter must never return Err on arbitrary bytes");

        let agy = AgyAdapter;
        let agy_res = agy.parse_stream(Box::new(Cursor::new(&bytes)));
        prop_assert!(agy_res.is_ok(), "AgyAdapter must never return Err on arbitrary bytes");

        // OpenCodeAdapter reads a SQLite file by path, not a byte stream -- arbitrary bytes are
        // not valid SQLite, so an Err (SQLite failing to open/query it) is the correct, expected
        // outcome here, unlike the three byte-stream adapters above. The only guarantee that
        // still applies is: never panic. See prop_fuzz_opencode_database_path_does_not_panic.
    }

    #[test]
    fn prop_fuzz_opencode_database_path_does_not_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fuzz.db");
        std::fs::write(&path, &bytes).unwrap();

        // Must never panic on arbitrary file content. Err is an acceptable, expected outcome
        // for non-SQLite garbage -- only a panic would be a real bug.
        let _parse_result = OpenCodeAdapter.parse_database(&path);
        let _matches_result = OpenCodeAdapter::matches_database(&path);
    }

    #[test]
    fn prop_fuzz_random_strings_does_not_panic(s in "\\PC*") {
        let bytes = s.into_bytes();
        let _detected = detect_orchestrator(&bytes);

        let claude = ClaudeCodeAdapter;
        let _claude_res = claude.parse_stream(Box::new(Cursor::new(&bytes)));

        let agy = AgyAdapter;
        let _agy_res = agy.parse_stream(Box::new(Cursor::new(&bytes)));
    }
}

#[test]
fn test_multiline_mixed_line_endings_and_utf8_bom() {
    // Session prefixed with UTF-8 BOM and using mixed \r\n and \n
    let sample = "\u{FEFF}{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Hello\"}}\r\n\
                  \r\n\
                  {\"type\":\"assistant\",\"message\":{\"model\":\"claude-3-5-sonnet-20241022\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Hi\"}],\"usage\":{\"input_tokens\":10,\"output_tokens\":5,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}\n\
                  \n\
                  {\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Thanks\"}}\r\n";

    let adapter = ClaudeCodeAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("BOM and CRLF must parse cleanly");

    assert_eq!(transcript.turns.len(), 3);
    assert_eq!(transcript.turns[0].text.as_deref(), Some("Hello"));
    assert_eq!(transcript.turns[1].text.as_deref(), Some("Hi"));
    assert_eq!(transcript.turns[2].text.as_deref(), Some("Thanks"));
}

#[test]
fn test_missing_and_corrupted_fields_graceful_recovery() {
    let corrupted_samples = [
        r#"{"type":"assistant","message":null}"#,
        r#"{"type":"assistant","message":{"content":12345}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use"}]}}"#,
        r#"{"type":"unknown_future_event_type","data":{"foo":"bar"}}"#,
        r#"{"invalid_json": true"#,
        r#"null"#,
        r#"12345"#,
        r#""""#,
    ];

    let combined = corrupted_samples.join("\n");
    let adapter = ClaudeCodeAdapter;
    let transcript = adapter
        .parse_stream(Box::new(Cursor::new(combined)))
        .expect("Parser should discard malformed records without failing");

    // All invalid records should be ignored without panicking
    assert_eq!(transcript.turns.len(), 0);
}
