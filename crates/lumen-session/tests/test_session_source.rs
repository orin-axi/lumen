use lumen_fixtures::real_opencode_session_db;
use lumen_session::*;
use std::io::Cursor;

const CLAUDE_SAMPLE: &str = r#"{"type":"assistant","sessionId":"s1","parentUuid":"t0","message":{"model":"claude-3-5-sonnet-20241022","role":"assistant","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":10,"output_tokens":5}}}
"#;

#[test]
fn test_stream_adapters_reject_database_source() {
    // CRIT-LUMEN-180: every stream-based adapter's `load` rejects `SessionSource::Database`
    // rather than misinterpreting a path as something it could stream.
    let path = std::path::Path::new("/nonexistent.db");

    let err = ClaudeCodeAdapter.load(SessionSource::Database(path)).unwrap_err();
    assert!(matches!(err, IngestionError::UnsupportedSourceKind { adapter: "claude-code", source_kind: "database" }));

    let err = CodexAdapter.load(SessionSource::Database(path)).unwrap_err();
    assert!(matches!(err, IngestionError::UnsupportedSourceKind { adapter: "codex", source_kind: "database" }));

    let err = AgyAdapter.load(SessionSource::Database(path)).unwrap_err();
    assert!(matches!(err, IngestionError::UnsupportedSourceKind { adapter: "antigravity", source_kind: "database" }));
}

#[test]
fn test_opencode_adapter_rejects_stream_source() {
    let err = OpenCodeAdapter.load(SessionSource::Stream(Box::new(Cursor::new("")))).unwrap_err();
    assert!(matches!(err, IngestionError::UnsupportedSourceKind { adapter: "opencode", source_kind: "stream" }));
}

#[test]
fn test_load_stream_source_matches_parse_stream_directly() {
    // load(Stream(..)) must produce the same transcript parse_stream would, just wrapped in a
    // one-element Vec -- it's a thin delegation, not a second parse path that could drift.
    // Compares content fields only, not full struct equality: parse_stream stamps
    // started_at/ended_at with Utc::now() when no explicit timestamp is present in the input, so
    // two independent calls never produce identical ExecutionTiming.
    let direct = ClaudeCodeAdapter.parse_stream(Box::new(Cursor::new(CLAUDE_SAMPLE))).unwrap();
    let via_load = ClaudeCodeAdapter.load(SessionSource::Stream(Box::new(Cursor::new(CLAUDE_SAMPLE)))).unwrap();

    assert_eq!(via_load.len(), 1);
    assert_eq!(via_load[0].session_id, direct.session_id);
    assert_eq!(via_load[0].model_family, direct.model_family);
    assert_eq!(via_load[0].economics, direct.economics);
    assert_eq!(via_load[0].turns.len(), direct.turns.len());
    assert_eq!(via_load[0].turns[0].role, direct.turns[0].role);
    assert_eq!(via_load[0].turns[0].text, direct.turns[0].text);
    assert_eq!(via_load[0].turns[0].usage, direct.turns[0].usage);
}

#[test]
fn test_all_four_adapters_dispatch_uniformly_through_dyn_session_adapter() {
    // The actual point of SessionSource/load (CRIT-LUMEN-180): a caller (lumen-cli's
    // load_sessions) can hold adapters behind one dyn SessionAdapter reference and call .load()
    // without a per-orchestrator special case for "how do I even call this adapter" --
    // previously impossible since OpenCodeAdapter couldn't implement SessionAdapter at all.
    let stream_adapters: Vec<Box<dyn SessionAdapter>> =
        vec![Box::new(ClaudeCodeAdapter), Box::new(CodexAdapter), Box::new(AgyAdapter)];

    for adapter in &stream_adapters {
        // An empty stream is a valid, if trivial, session for every stream-based adapter
        // (0 turns, no error) -- proves `load` really reaches this adapter's own parse logic
        // through the trait object, not a stub.
        let stream_result = adapter.load(SessionSource::Stream(Box::new(Cursor::new(""))));
        assert!(stream_result.is_ok(), "{} rejected a valid (empty) stream source", adapter.name());

        let db_result = adapter.load(SessionSource::Database(std::path::Path::new("/nonexistent.db")));
        assert!(db_result.is_err(), "{} accepted a database source it should reject", adapter.name());
    }

    let opencode: Box<dyn SessionAdapter> = Box::new(OpenCodeAdapter);
    let db = real_opencode_session_db();
    let db_result = opencode.load(SessionSource::Database(db.path.as_std_path()));
    assert!(db_result.is_ok(), "opencode rejected a real, valid database source");
    assert_eq!(db_result.unwrap().len(), 1);

    let stream_result = opencode.load(SessionSource::Stream(Box::new(Cursor::new(""))));
    assert!(stream_result.is_err(), "opencode accepted a stream source it should reject");
}
