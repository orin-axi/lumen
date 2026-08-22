use lumen_fixtures::real_opencode_session_db;
use lumen_model::*;
use lumen_session::*;
use rusqlite::Connection;
use tempfile::tempdir;

/// Real OpenCode data is a SQLite database (`session`/`message`/`part` tables), confirmed
/// against a real local `~/.local/share/opencode/opencode.db` this session -- this rewrites the
/// entire test file off the fictional `action`/`observation` JSONL format the adapter previously
/// assumed, which was confirmed to have zero basis in real OpenCode output.
#[test]
fn test_opencode_adapter_parses_real_session_end_to_end() {
    let db = real_opencode_session_db();
    let adapter = OpenCodeAdapter;

    let transcripts = adapter.parse_database(db.path.as_std_path()).expect("failed to parse real OpenCode fixture db");
    assert_eq!(transcripts.len(), 1);
    let transcript = &transcripts[0];

    assert_eq!(transcript.orchestrator, OrchestratorKind::OpenCode);
    assert_eq!(transcript.session_id, "ses_test001");
    assert_eq!(transcript.model_family, "gpt-5.6-terra-fast");
    assert_eq!(transcript.turns.len(), 2);

    assert_eq!(transcript.turns[0].role, TurnRole::User);
    assert_eq!(transcript.turns[0].text.as_deref(), Some("What are the docs and specs in this repo?"));

    let asst = &transcript.turns[1];
    assert_eq!(asst.role, TurnRole::Assistant);
    assert_eq!(asst.text.as_deref(), Some("I'll inspect the Cargo.toml workspace file."));
    assert_eq!(asst.tool_calls.len(), 2);
    assert_eq!(asst.tool_calls[0].tool_name, "read");
    assert_eq!(asst.tool_results.len(), 2);
    assert_eq!(asst.tool_calls[0].call_id, asst.tool_results[0].call_id, "call and result must share the real callID");

    let usage = asst.usage.expect("assistant turn must carry real per-message token usage");
    assert_eq!(usage.input_tokens, 700);
    assert_eq!(usage.output_tokens, 23);
    assert_eq!(usage.reasoning_tokens, 4);
    assert_eq!(usage.cache_read_tokens, 100);

    assert_eq!(transcript.economics.input_tokens, 700);
    assert_eq!(transcript.economics.output_tokens, 23);
    assert_eq!(transcript.economics.reasoning_output_tokens, 4);
    assert_eq!(transcript.economics.provided_cost_usd, Some(0.0021), "session.cost is the real provided total");
    assert!(
        transcript.economics.is_fully_priced,
        "gpt-5.6-terra-fast must normalize to the real seeded gpt-5.6-terra row"
    );
}

#[test]
fn test_opencode_matches_database_positive_and_negative() {
    let db = real_opencode_session_db();
    assert!(OpenCodeAdapter::matches_database(db.path.as_std_path()), "real OpenCode schema must be recognized");

    let dir = tempdir().unwrap();
    let unrelated_path = dir.path().join("unrelated.db");
    let conn = Connection::open(&unrelated_path).unwrap();
    conn.execute_batch("CREATE TABLE not_opencode (id INTEGER);").unwrap();
    drop(conn);
    assert!(
        !OpenCodeAdapter::matches_database(&unrelated_path),
        "an arbitrary SQLite file without session/message/part tables must not match"
    );

    let not_sqlite_path = dir.path().join("not_a_database.txt");
    std::fs::write(&not_sqlite_path, b"not a sqlite file at all").unwrap();
    assert!(!OpenCodeAdapter::matches_database(&not_sqlite_path), "a non-SQLite file must not match");
}

#[test]
fn test_opencode_detect_orchestrator_matches_sqlite_magic_prefix() {
    let db = real_opencode_session_db();
    let bytes = std::fs::read(&db.path).unwrap();
    let sample = &bytes[..bytes.len().min(2048)];
    assert_eq!(detect_orchestrator(sample), Some(OrchestratorKind::OpenCode));
}

#[test]
fn test_opencode_multiple_sessions_in_one_database_each_get_own_transcript() {
    // Real opencode.db files commonly hold many sessions (confirmed: 2 real sessions in a real
    // local database this session) -- parse_database must return one transcript per session,
    // not assume a 1-file-to-1-session mapping the way the JSONL adapters do.
    let db = real_opencode_session_db();
    let conn = Connection::open(&db.path).unwrap();
    conn.execute(
        "INSERT INTO session (id, cost, model, time_created, time_updated) VALUES (?1, ?2, ?3, ?4, ?4)",
        rusqlite::params!["ses_test002", 0.0, "null", 1_700_000_100_000i64],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?3, ?4)",
        rusqlite::params![
            "msg_test002_user",
            "ses_test002",
            1_700_000_100_000i64,
            r#"{"role":"user","time":{"created":1700000100000}}"#,
        ],
    )
    .unwrap();
    drop(conn);

    let adapter = OpenCodeAdapter;
    let transcripts = adapter.parse_database(db.path.as_std_path()).expect("failed to parse multi-session fixture db");

    assert_eq!(transcripts.len(), 2);
    assert_eq!(transcripts[0].session_id, "ses_test001");
    assert_eq!(transcripts[1].session_id, "ses_test002");
    assert_eq!(transcripts[1].turns.len(), 1);
}

#[test]
fn test_opencode_malformed_message_json_is_skipped_not_fatal() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("malformed.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, cost REAL DEFAULT 0 NOT NULL, model TEXT, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, cost, model, time_created, time_updated) VALUES ('ses_bad', 0, 'null', 1, 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('msg_bad', 'ses_bad', 1, 1, 'not valid json')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('msg_good', 'ses_bad', 2, 2, '{\"role\":\"user\"}')",
        [],
    )
    .unwrap();
    drop(conn);

    let adapter = OpenCodeAdapter;
    let transcripts = adapter.parse_database(&path).expect("a malformed message row must not abort the whole parse");
    assert_eq!(transcripts.len(), 1);
    assert_eq!(transcripts[0].turns.len(), 1, "the one well-formed message must still parse");
}

#[test]
fn test_opencode_pure_cache_read_and_write_turn_is_not_dropped() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cache_only.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, cost REAL DEFAULT 0 NOT NULL, model TEXT, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, cost, model, time_created, time_updated) VALUES ('ses1', 0, 'null', 1, 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('msg1', 'ses1', 1, 1, ?1)",
        [r#"{"role":"assistant","modelID":"claude-sonnet-5","tokens":{"total":5000,"input":0,"output":0,"reasoning":0,"cache":{"write":3000,"read":2000}}}"#],
    )
    .unwrap();
    drop(conn);

    let adapter = OpenCodeAdapter;
    let transcripts = adapter.parse_database(&path).unwrap();
    let usage = transcripts[0].turns[0].usage.expect("a pure cache-only turn must still record usage");
    assert_eq!(usage.cache_creation_tokens, 3000, "real cache write tokens must not be dropped");
    assert_eq!(usage.cache_read_tokens, 2000, "real cache read tokens must not be dropped");
    assert_eq!(transcripts[0].economics.cache_creation_tokens, 3000);
    assert_eq!(transcripts[0].economics.cache_read_tokens, 2000);
}

#[test]
fn test_opencode_zero_cost_session_yields_none_provided_cost() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("zero_cost.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, cost REAL DEFAULT 0 NOT NULL, model TEXT, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, cost, model, time_created, time_updated) VALUES ('ses1', 0.0, 'null', 1, 1)",
        [],
    )
    .unwrap();
    drop(conn);

    let adapter = OpenCodeAdapter;
    let transcripts = adapter.parse_database(&path).unwrap();
    assert_eq!(transcripts[0].economics.provided_cost_usd, None);
}
