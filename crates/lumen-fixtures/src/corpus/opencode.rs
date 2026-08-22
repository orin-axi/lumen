use camino::Utf8PathBuf;
use rusqlite::Connection;
use tempfile::{tempdir, TempDir};

/// A real-schema OpenCode SQLite database, built on disk for a test to point `OpenCodeAdapter`
/// at. `_dir` is held only to keep the temp directory (and therefore `path`) alive for the
/// caller's lifetime -- dropping it deletes the database file.
pub struct RealOpenCodeDb {
    _dir: TempDir,
    pub path: Utf8PathBuf,
}

/// Builds a real-schema OpenCode SQLite database (`session`/`message`/`part` tables, confirmed
/// against a real local `~/.local/share/opencode/opencode.db` this session) with one real-shaped
/// session: a user message, and an assistant message carrying real per-message token usage
/// (`tokens.{input,output,reasoning,cache.{read,write}}`) plus two real-shaped tool-use `part`
/// rows (`read` and `bash`), so adapter tests exercise text extraction, tool calls, and pricing
/// against the real schema instead of a fictional JSONL format.
pub fn real_opencode_session_db() -> RealOpenCodeDb {
    let dir = tempdir().expect("failed to create temp dir for OpenCode fixture db");
    let path = Utf8PathBuf::from_path_buf(dir.path().join("opencode.db")).expect("non-utf8 temp path");

    let conn = Connection::open(&path).expect("failed to create OpenCode fixture db");
    conn.execute_batch(
        "CREATE TABLE session (
            id TEXT PRIMARY KEY,
            cost REAL DEFAULT 0 NOT NULL,
            model TEXT,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL
        );
        CREATE TABLE part (
            id TEXT PRIMARY KEY,
            message_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            time_created INTEGER NOT NULL,
            time_updated INTEGER NOT NULL,
            data TEXT NOT NULL
        );",
    )
    .expect("failed to create OpenCode fixture schema");

    conn.execute(
        "INSERT INTO session (id, cost, model, time_created, time_updated) VALUES (?1, ?2, ?3, ?4, ?4)",
        rusqlite::params![
            "ses_test001",
            0.0021,
            r#"{"id":"gpt-5.6-terra-fast","providerID":"openai","variant":"high"}"#,
            1_700_000_000_000i64,
        ],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?3, ?4)",
        rusqlite::params![
            "msg_test_user",
            "ses_test001",
            1_700_000_000_000i64,
            r#"{"role":"user","time":{"created":1700000000000},"modelID":"gpt-5.6-terra-fast","providerID":"openai"}"#,
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
        rusqlite::params![
            "prt_test_user_text",
            "msg_test_user",
            "ses_test001",
            1_700_000_000_000i64,
            r#"{"type":"text","text":"What are the docs and specs in this repo?"}"#,
        ],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?3, ?4)",
        rusqlite::params![
            "msg_test_asst",
            "ses_test001",
            1_700_000_010_000i64,
            r#"{"role":"assistant","modelID":"gpt-5.6-terra-fast","providerID":"openai","cost":0.0021,
               "tokens":{"total":730,"input":700,"output":23,"reasoning":4,"cache":{"write":0,"read":100}},
               "time":{"created":1700000010000,"completed":1700000015000},"finish":"tool-calls"}"#,
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
        rusqlite::params![
            "prt_test_asst_text",
            "msg_test_asst",
            "ses_test001",
            1_700_000_010_000i64,
            r#"{"type":"text","text":"I'll inspect the Cargo.toml workspace file."}"#,
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
        rusqlite::params![
            "prt_test_asst_tool_read",
            "msg_test_asst",
            "ses_test001",
            1_700_000_011_000i64,
            r#"{"type":"tool","tool":"read","callID":"call_test_read",
               "state":{"status":"completed","input":{"filePath":"Cargo.toml"},
               "output":"[workspace]\nmembers = [\"crates/*\"]"}}"#,
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
        rusqlite::params![
            "prt_test_asst_tool_bash",
            "msg_test_asst",
            "ses_test001",
            1_700_000_012_000i64,
            r#"{"type":"tool","tool":"bash","callID":"call_test_bash",
               "state":{"status":"completed","input":{"command":"cargo check"},
               "output":"Finished dev profile"}}"#,
        ],
    )
    .unwrap();

    RealOpenCodeDb { _dir: dir, path }
}
