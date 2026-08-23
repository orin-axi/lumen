//! CRIT-LUMEN-172: a machine-checked, Weaver-style golden test tying every known adapter's
//! unknown-model fallback behavior to one shared expectation, instead of each adapter's own
//! inline comment restating the convention independently (the drift risk that let
//! ClaudeCodeAdapter default to a real, currently-seeded model name go unnoticed -- see
//! SessionAdapter's own doc comment in `adapter.rs` for the full contract this test enforces).
//!
//! Each case below feeds its adapter a minimal, structurally valid input that carries NO
//! recognizable model field, then asserts the SAME two invariants for every one of them:
//! 1. `model_family` is never a real, currently-seeded PricingTable model
//!    (`!pricing::SEEDED.is_recognized(...)`).
//! 2. `economics.is_fully_priced` is `false` -- `TokenEconomics::calculate` derives this
//!    directly from `is_recognized`, so this is really the same invariant surfaced on the field
//!    real callers actually branch on.
//!
//! A new adapter (JSONL-based, added to `CASES` below; SQLite-based like OpenCode, added as its
//! own `#[test]` following `test_opencode_adapter_unknown_model_contract`'s pattern) must pass
//! this same check.

use lumen_model::{pricing, CanonicalTranscript};
use lumen_session::*;
use rusqlite::Connection;
use std::io::Cursor;
use tempfile::tempdir;

struct Case {
    adapter_name: &'static str,
    /// Minimal structurally-valid input for this adapter that carries no model field at all.
    sample: &'static str,
    parse: fn(&str) -> CanonicalTranscript,
}

fn parse_claude_code(sample: &str) -> CanonicalTranscript {
    ClaudeCodeAdapter.parse_stream(Box::new(Cursor::new(sample.to_string()))).expect("must parse successfully")
}

fn parse_codex(sample: &str) -> CanonicalTranscript {
    CodexAdapter.parse_stream(Box::new(Cursor::new(sample.to_string()))).expect("must parse successfully")
}

fn parse_agy(sample: &str) -> CanonicalTranscript {
    AgyAdapter.parse_stream(Box::new(Cursor::new(sample.to_string()))).expect("must parse successfully")
}

const CASES: &[Case] = &[
    Case {
        adapter_name: "claude-code",
        sample: "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}],\"usage\":{\"input_tokens\":10,\"output_tokens\":5,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}\n",
        parse: parse_claude_code,
    },
    Case {
        adapter_name: "codex",
        // A real token_count event with no preceding/following thread_settings_applied at all --
        // the session genuinely never reports a model, not merely an explicit null.
        sample: r#"{"timestamp":"2026-08-23T10:00:00Z","ordinal":1,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":5}}}}
"#,
        parse: parse_codex,
    },
    Case {
        adapter_name: "antigravity",
        sample: r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"hi"}"#,
        parse: parse_agy,
    },
];

#[test]
fn test_jsonl_adapters_unknown_model_contract() {
    for case in CASES {
        let transcript = (case.parse)(case.sample);

        assert!(
            transcript.model_family.ends_with("-unknown-model"),
            "{}: model_family must follow the \"<provider>-unknown-model\" convention, got {:?}",
            case.adapter_name,
            transcript.model_family
        );
        assert!(
            !pricing::SEEDED.is_recognized(&transcript.model_family),
            "{}: model_family {:?} must not be a real, currently-seeded PricingTable model",
            case.adapter_name,
            transcript.model_family
        );
        assert!(
            !transcript.economics.is_fully_priced,
            "{}: a session with no recognizable model field must surface as explicitly unpriced \
             (is_fully_priced: false), not a plausible-looking cost",
            case.adapter_name
        );
    }
}

/// OpenCodeAdapter is SQLite-backed and implements `parse_database`, not `SessionAdapter`
/// (see its own doc comment), so it can't share `CASES`' `parse_stream` signature -- same
/// contract, exercised via its own real entry point.
#[test]
fn test_opencode_adapter_unknown_model_contract() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("no_model.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, cost REAL DEFAULT 0 NOT NULL, model TEXT, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);",
    )
    .unwrap();
    // A real assistant message with no "modelID" field at all -- the session genuinely never
    // reports a model, distinct from an explicit null.
    conn.execute(
        "INSERT INTO session (id, cost, model, time_created, time_updated) VALUES ('ses_no_model', 0, 'null', 1, 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('msg1', 'ses_no_model', 1, 1, ?1)",
        [r#"{"role":"assistant","tokens":{"input":10,"output":5,"reasoning":0,"cache":{"write":0,"read":0}}}"#],
    )
    .unwrap();
    drop(conn);

    let transcripts = OpenCodeAdapter.parse_database(&path).expect("must parse successfully");
    let transcript = &transcripts[0];

    assert!(
        transcript.model_family.ends_with("-unknown-model"),
        "opencode: model_family must follow the \"<provider>-unknown-model\" convention, got {:?}",
        transcript.model_family
    );
    assert!(
        !pricing::SEEDED.is_recognized(&transcript.model_family),
        "opencode: model_family {:?} must not be a real, currently-seeded PricingTable model",
        transcript.model_family
    );
    assert!(
        !transcript.economics.is_fully_priced,
        "opencode: a session with no recognizable model field must surface as explicitly \
         unpriced (is_fully_priced: false), not a plausible-looking cost"
    );
}
