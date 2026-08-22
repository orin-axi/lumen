use camino::Utf8PathBuf;
use chrono::{TimeZone, Utc};
use lumen_model::*;
use lumen_store::*;
use tempfile::tempdir;

#[test]
fn test_sqlite_store_open_and_migrations_v1_to_v5() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("lumen_test.db")).unwrap();

    let store = SqliteStore::open(&db_path).expect("Failed to open SQLite store");
    assert!(!store.is_read_only());

    let conn = store.connection().expect("Failed to acquire connection");

    // Verify all 7 migrations were recorded
    let count: usize = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0))
        .expect("Failed to query schema_migrations");
    assert_eq!(count, 7);

    // Verify WAL journal mode
    let journal_mode: String =
        conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).expect("Failed to query journal_mode");
    assert_eq!(journal_mode.to_lowercase(), "wal");
}

#[test]
fn test_queue_repository_enqueue_fetch_and_dead_letter() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("queue_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = QueueRepository::new(&conn);

    // Enqueue 2 items
    let id1 = repo.enqueue("claude", "session-1", "/path/to/session1.jsonl", 12345).expect("Enqueue failed");
    let _id2 = repo.enqueue("agy", "session-2", "/path/to/session2.jsonl", 67890).expect("Enqueue failed");

    // Fetch pending
    let pending = repo.fetch_pending(10).expect("Fetch pending failed");
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].session_id, "session-1");
    assert_eq!(pending[0].status, "pending");

    // Mark failed twice -> status remains 'failed'
    repo.mark_failed(id1, "temporary network timeout").unwrap();
    repo.mark_failed(id1, "temporary network timeout 2").unwrap();

    // 3rd failure transitions to 'dead_letter'
    repo.mark_failed(id1, "fatal parsing error").unwrap();

    let dead_status: String =
        conn.query_row("SELECT status FROM ingestion_queue WHERE id = ?1", [id1], |row| row.get(0)).unwrap();
    assert_eq!(dead_status, "dead_letter");
}

#[test]
fn test_queue_repository_mark_failed_nonexistent_id_returns_error() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("queue_missing_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = QueueRepository::new(&conn);

    // No row with id 999 was ever enqueued; mark_failed must surface this as an
    // error rather than silently succeeding (the old implementation used
    // `.unwrap_or(0)` on the SELECT and then ran an UPDATE that matched zero
    // rows but still returned Ok(())).
    let result = repo.mark_failed(999, "some error");
    assert!(result.is_err(), "mark_failed on a nonexistent id must return Err, got {result:?}");
}

#[test]
fn test_session_repository_idempotent_upsert_and_list() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("session_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = SessionRepository::new(&conn);

    let mut record = SessionFactRecord {
        provider: "claude".to_string(),
        provider_session_id: "sess-abc-123".to_string(),
        model_family: "claude-3-5-sonnet-20241022".to_string(),
        orchestrator: OrchestratorKind::ClaudeCode,
        started_at: Utc::now(),
        ended_at: Utc::now(),
        wall_duration_ms: 5000,
        turn_count: 10,
        economics: TokenEconomics {
            input_tokens: 10000,
            output_tokens: 2000,
            cache_creation_tokens: 5000,
            cache_read_tokens: 15000,
            ephemeral_5m_tokens: 5000,
            ephemeral_1h_tokens: 0,
            cache_hit_ratio: 75.0,
            total_cost_usd: 0.085,
            provided_cost_usd: None,
            baseline_cost_no_cache_usd: 0.220,
            net_savings_usd: 0.135,
            efficiency_multiplier: 2.58,
            per_model: std::collections::HashMap::new(),
            reasoning_output_tokens: 0,
            is_fully_priced: true,
        },
        has_anomalies: false,
    };

    // First insert
    repo.upsert_session(&record).expect("Initial upsert failed");

    // Idempotent second upsert with modified turn count
    record.turn_count = 15;
    repo.upsert_session(&record).expect("Second upsert failed");

    let list = repo.list_recent(&SessionFilter { provider: None, limit: 10 }).expect("List recent failed");

    assert_eq!(list.len(), 1);
    assert_eq!(list[0].session_id, "sess-abc-123");
    assert_eq!(list[0].turn_count, 15);
    assert_eq!(list[0].cache_hit_ratio, 75.0);

    let detail = repo.get_session("claude", "sess-abc-123").expect("Get session failed").expect("Session not found");
    assert_eq!(detail.summary.session_id, "sess-abc-123");
    assert_eq!(detail.economics.net_savings_usd, 0.135);
}

#[test]
fn test_findings_repository_insert_and_query() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("findings_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = FindingsRepository::new(&conn);

    let findings = vec![
        FindingFactRecord {
            rule_id: "RTK-001".to_string(),
            severity: "warn".to_string(),
            confidence: 0.95,
            title: "Prefer ripgrep over grep".to_string(),
            message: "Use rg for 10x faster search".to_string(),
            evidence_json: Some(serde_json::json!({"command": "grep -rn foo ."})),
        },
        FindingFactRecord {
            rule_id: "CYCLE-001".to_string(),
            severity: "error".to_string(),
            confidence: 0.80,
            title: "Circular tool loop detected".to_string(),
            message: "Repeated read of main.rs".to_string(),
            evidence_json: None,
        },
    ];

    repo.insert_findings("sess-xyz", &findings).expect("Insert findings failed");

    let top = repo.list_top_findings(10).expect("Query findings failed");
    assert_eq!(top.len(), 2);
    // Highest confidence first
    assert_eq!(top[0].rule_id, "RTK-001");
    assert_eq!(top[0].confidence, 0.95);
    assert_eq!(top[1].rule_id, "CYCLE-001");
}

#[test]
fn test_tool_call_repository_insert_counts_rows_and_empty_slice_is_noop() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("tool_call_insert_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let session_repo = SessionRepository::new(&conn);
    let record = SessionFactRecord {
        provider: "claude".to_string(),
        provider_session_id: "sess-tool-1".to_string(),
        model_family: "claude-3-5-sonnet-20241022".to_string(),
        orchestrator: OrchestratorKind::ClaudeCode,
        started_at: Utc::now(),
        ended_at: Utc::now(),
        wall_duration_ms: 1000,
        turn_count: 3,
        economics: TokenEconomics {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            ephemeral_5m_tokens: 0,
            ephemeral_1h_tokens: 0,
            cache_hit_ratio: 0.0,
            total_cost_usd: 0.01,
            provided_cost_usd: None,
            baseline_cost_no_cache_usd: 0.02,
            net_savings_usd: 0.01,
            efficiency_multiplier: 2.0,
            per_model: std::collections::HashMap::new(),
            reasoning_output_tokens: 0,
            is_fully_priced: true,
        },
        has_anomalies: false,
    };
    session_repo.upsert_session(&record).unwrap();

    let internal_id: i64 = conn
        .query_row("SELECT id FROM sessions WHERE provider_session_id = ?1", ["sess-tool-1"], |row| row.get(0))
        .unwrap();

    let tool_repo = ToolCallRepository::new(&conn);

    // Empty slice must insert zero rows without error (CRIT-LUMEN-117).
    tool_repo.insert_tool_calls(internal_id, &[]).expect("empty slice insert failed");
    let count_after_empty: i64 = conn
        .query_row("SELECT COUNT(*) FROM tool_calls WHERE session_id = ?1", [internal_id], |row| row.get(0))
        .unwrap();
    assert_eq!(count_after_empty, 0);

    let calls = vec![
        ToolCallFactRecord {
            turn_index: 0,
            tool_name: "Read".to_string(),
            call_id: "call-1".to_string(),
            intent_kind: "read".to_string(),
            is_error: false,
            latency_ms: 10,
        },
        ToolCallFactRecord {
            turn_index: 1,
            tool_name: "Bash".to_string(),
            call_id: "call-2".to_string(),
            intent_kind: "exec".to_string(),
            is_error: true,
            latency_ms: 200,
        },
        ToolCallFactRecord {
            turn_index: 2,
            tool_name: "Read".to_string(),
            call_id: "call-3".to_string(),
            intent_kind: "read".to_string(),
            is_error: false,
            latency_ms: 5,
        },
    ];

    tool_repo.insert_tool_calls(internal_id, &calls).expect("Insert tool calls failed");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM tool_calls WHERE session_id = ?1", [internal_id], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 3);

    let listed = tool_repo.list_by_session(internal_id).expect("list_by_session failed");
    assert_eq!(listed.len(), 3);
    assert_eq!(listed[1].tool_name, "Bash");
    assert!(listed[1].is_error);
}

#[test]
fn test_tool_call_repository_counts_by_session() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("tool_call_counts_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let session_repo = SessionRepository::new(&conn);
    let record = SessionFactRecord {
        provider: "claude".to_string(),
        provider_session_id: "sess-tool-2".to_string(),
        model_family: "claude-3-5-sonnet-20241022".to_string(),
        orchestrator: OrchestratorKind::ClaudeCode,
        started_at: Utc::now(),
        ended_at: Utc::now(),
        wall_duration_ms: 1000,
        turn_count: 5,
        economics: TokenEconomics {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            ephemeral_5m_tokens: 0,
            ephemeral_1h_tokens: 0,
            cache_hit_ratio: 0.0,
            total_cost_usd: 0.01,
            provided_cost_usd: None,
            baseline_cost_no_cache_usd: 0.02,
            net_savings_usd: 0.01,
            efficiency_multiplier: 2.0,
            per_model: std::collections::HashMap::new(),
            reasoning_output_tokens: 0,
            is_fully_priced: true,
        },
        has_anomalies: false,
    };
    session_repo.upsert_session(&record).unwrap();
    let internal_id: i64 = conn
        .query_row("SELECT id FROM sessions WHERE provider_session_id = ?1", ["sess-tool-2"], |row| row.get(0))
        .unwrap();

    let tool_repo = ToolCallRepository::new(&conn);
    let calls = vec![
        ToolCallFactRecord {
            turn_index: 0,
            tool_name: "Read".to_string(),
            call_id: "c1".to_string(),
            intent_kind: "read".to_string(),
            is_error: false,
            latency_ms: 10,
        },
        ToolCallFactRecord {
            turn_index: 1,
            tool_name: "Read".to_string(),
            call_id: "c2".to_string(),
            intent_kind: "read".to_string(),
            is_error: true,
            latency_ms: 15,
        },
        ToolCallFactRecord {
            turn_index: 2,
            tool_name: "Bash".to_string(),
            call_id: "c3".to_string(),
            intent_kind: "exec".to_string(),
            is_error: true,
            latency_ms: 300,
        },
        ToolCallFactRecord {
            turn_index: 3,
            tool_name: "Write".to_string(),
            call_id: "c4".to_string(),
            intent_kind: "write".to_string(),
            is_error: false,
            latency_ms: 20,
        },
    ];
    tool_repo.insert_tool_calls(internal_id, &calls).unwrap();

    let tool_counts = tool_repo.tool_counts_by_session(internal_id).expect("tool_counts_by_session failed");
    assert_eq!(tool_counts.get("Read").copied(), Some(2));
    assert_eq!(tool_counts.get("Bash").copied(), Some(1));
    assert_eq!(tool_counts.get("Write").copied(), Some(1));
    assert_eq!(tool_counts.len(), 3);

    let error_counts = tool_repo.error_counts_by_session(internal_id).expect("error_counts_by_session failed");
    assert_eq!(error_counts.get("Read").copied(), Some(1));
    assert_eq!(error_counts.get("Bash").copied(), Some(1));
    assert_eq!(error_counts.get("Write"), None);
    assert_eq!(error_counts.len(), 2);
}

#[test]
fn test_session_repository_get_session_populates_tool_counts_from_internal_id() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("session_tool_counts_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let session_repo = SessionRepository::new(&conn);
    let record = SessionFactRecord {
        provider: "claude".to_string(),
        provider_session_id: "sess-tool-3".to_string(),
        model_family: "claude-3-5-sonnet-20241022".to_string(),
        orchestrator: OrchestratorKind::ClaudeCode,
        started_at: Utc::now(),
        ended_at: Utc::now(),
        wall_duration_ms: 1000,
        turn_count: 2,
        economics: TokenEconomics {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            ephemeral_5m_tokens: 0,
            ephemeral_1h_tokens: 0,
            cache_hit_ratio: 0.0,
            total_cost_usd: 0.01,
            provided_cost_usd: None,
            baseline_cost_no_cache_usd: 0.02,
            net_savings_usd: 0.01,
            efficiency_multiplier: 2.0,
            per_model: std::collections::HashMap::new(),
            reasoning_output_tokens: 0,
            is_fully_priced: true,
        },
        has_anomalies: false,
    };
    session_repo.upsert_session(&record).unwrap();
    let internal_id: i64 = conn
        .query_row("SELECT id FROM sessions WHERE provider_session_id = ?1", ["sess-tool-3"], |row| row.get(0))
        .unwrap();

    let tool_repo = ToolCallRepository::new(&conn);
    let calls = vec![
        ToolCallFactRecord {
            turn_index: 0,
            tool_name: "Read".to_string(),
            call_id: "c1".to_string(),
            intent_kind: "read".to_string(),
            is_error: false,
            latency_ms: 10,
        },
        ToolCallFactRecord {
            turn_index: 1,
            tool_name: "Bash".to_string(),
            call_id: "c2".to_string(),
            intent_kind: "exec".to_string(),
            is_error: true,
            latency_ms: 300,
        },
    ];
    tool_repo.insert_tool_calls(internal_id, &calls).unwrap();

    let detail =
        session_repo.get_session("claude", "sess-tool-3").expect("get_session failed").expect("session not found");
    assert_eq!(detail.tool_counts.get("Read").copied(), Some(1));
    assert_eq!(detail.tool_counts.get("Bash").copied(), Some(1));
    assert_eq!(detail.error_counts.get("Bash").copied(), Some(1));
    assert_eq!(detail.error_counts.get("Read"), None);
}

#[test]
fn test_read_only_mode_disallows_writes() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("readonly_test.db")).unwrap();

    // Create & initialize database first
    {
        let _writer = SqliteStore::open(&db_path).unwrap();
    }

    // Open read-only
    let ro_store = SqliteStore::open_read_only(&db_path).expect("Failed to open read-only");
    assert!(ro_store.is_read_only());

    // Migration attempt should fail on read-only store
    let mig_res = ro_store.run_migrations();
    assert!(mig_res.is_err());
}

#[test]
fn test_rollup_repository_upsert_is_idempotent_on_period_start_and_type() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("rollup_upsert_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = RollupRepository::new(&conn);

    let period_start = Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap();

    let first = RollupFactRecord {
        period_start,
        period_type: "daily".to_string(),
        session_count: 3,
        total_cost_usd: 1.5,
        total_savings_usd: 0.5,
        total_duration_ms: 60_000,
    };
    repo.upsert_rollup(&first).expect("first upsert failed");

    let second = RollupFactRecord {
        period_start,
        period_type: "daily".to_string(),
        session_count: 9,
        total_cost_usd: 4.25,
        total_savings_usd: 1.1,
        total_duration_ms: 120_000,
    };
    repo.upsert_rollup(&second).expect("second upsert failed");

    let row_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM rollups WHERE period_start = ?1 AND period_type = 'daily'",
            [period_start],
            |row| row.get(0),
        )
        .expect("failed to count rollups");
    assert_eq!(row_count, 1, "upsert_rollup must not create a duplicate row for the same period_start/period_type");

    let read = repo.get_rollup(period_start, "daily").expect("get_rollup failed").expect("expected a rollup row");
    assert_eq!(read.session_count, 9);
    assert_eq!(read.total_cost_usd, 4.25);
    assert_eq!(read.total_savings_usd, 1.1);
    assert_eq!(read.total_duration_ms, 120_000);
}

#[test]
fn test_rollup_repository_get_rollup_returns_none_for_missing_row() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("rollup_missing_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = RollupRepository::new(&conn);

    let period_start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let result = repo.get_rollup(period_start, "weekly").expect("get_rollup should not error on missing row");
    assert!(result.is_none());
}

#[test]
fn test_rollup_repository_list_rollups_filters_orders_and_limits() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("rollup_list_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = RollupRepository::new(&conn);

    let daily_1 = Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap();
    let daily_2 = Utc.with_ymd_and_hms(2026, 8, 2, 0, 0, 0).unwrap();
    let daily_3 = Utc.with_ymd_and_hms(2026, 8, 3, 0, 0, 0).unwrap();
    let weekly_1 = Utc.with_ymd_and_hms(2026, 7, 27, 0, 0, 0).unwrap();

    for (period_start, period_type, session_count) in
        [(daily_1, "daily", 1), (daily_2, "daily", 2), (daily_3, "daily", 3), (weekly_1, "weekly", 100)]
    {
        repo.upsert_rollup(&RollupFactRecord {
            period_start,
            period_type: period_type.to_string(),
            session_count,
            total_cost_usd: 1.0,
            total_savings_usd: 0.1,
            total_duration_ms: 1_000,
        })
        .expect("upsert failed");
    }

    let daily_rollups = repo.list_rollups("daily", 2).expect("list_rollups failed");
    assert_eq!(daily_rollups.len(), 2, "limit must be respected");
    assert!(
        daily_rollups.iter().all(|r| r.period_type == "daily"),
        "only matching period_type rows should be returned"
    );
    assert_eq!(daily_rollups[0].period_start, daily_3, "results must be ordered by period_start descending");
    assert_eq!(daily_rollups[1].period_start, daily_2);

    let weekly_rollups = repo.list_rollups("weekly", 10).expect("list_rollups failed");
    assert_eq!(weekly_rollups.len(), 1);
    assert_eq!(weekly_rollups[0].period_start, weekly_1);
    assert_eq!(weekly_rollups[0].session_count, 100);
}

fn make_test_session_record(provider_session_id: &str) -> SessionFactRecord {
    SessionFactRecord {
        provider: "claude".to_string(),
        provider_session_id: provider_session_id.to_string(),
        model_family: "claude-3-5-sonnet-20241022".to_string(),
        orchestrator: OrchestratorKind::ClaudeCode,
        started_at: Utc::now(),
        ended_at: Utc::now(),
        wall_duration_ms: 1000,
        turn_count: 3,
        economics: TokenEconomics {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            ephemeral_5m_tokens: 0,
            ephemeral_1h_tokens: 0,
            cache_hit_ratio: 0.0,
            total_cost_usd: 0.01,
            provided_cost_usd: None,
            baseline_cost_no_cache_usd: 0.02,
            net_savings_usd: 0.01,
            efficiency_multiplier: 2.0,
            per_model: std::collections::HashMap::new(),
            reasoning_output_tokens: 0,
            is_fully_priced: true,
        },
        has_anomalies: false,
    }
}

fn empty_token_economics() -> TokenEconomics {
    TokenEconomics {
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
        ephemeral_5m_tokens: 0,
        ephemeral_1h_tokens: 0,
        cache_hit_ratio: 0.0,
        total_cost_usd: 0.0,
        provided_cost_usd: None,
        baseline_cost_no_cache_usd: 0.0,
        net_savings_usd: 0.0,
        efficiency_multiplier: 1.0,
        per_model: std::collections::HashMap::new(),
        reasoning_output_tokens: 0,
        is_fully_priced: true,
    }
}

#[test]
fn test_command_event_repository_insert_counts_rows_and_empty_slice_is_noop() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("command_event_insert_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let session_repo = SessionRepository::new(&conn);
    session_repo.upsert_session(&make_test_session_record("sess-cmd-1")).unwrap();

    let cmd_repo = CommandEventRepository::new(&conn);

    // Empty slice must insert zero rows without error.
    cmd_repo.insert_command_events("claude", "sess-cmd-1", &[]).expect("empty slice insert failed");
    let count_after_empty: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM command_events ce JOIN sessions s ON ce.session_id = s.id WHERE s.provider_session_id = ?1",
            ["sess-cmd-1"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count_after_empty, 0);

    let events = vec![
        CommandEventFactRecord {
            command_base: "git".to_string(),
            sanitized_args: Some("commit -m <REDACTED>".to_string()),
            is_error: false,
        },
        CommandEventFactRecord { command_base: "rm".to_string(), sanitized_args: None, is_error: true },
    ];

    cmd_repo.insert_command_events("claude", "sess-cmd-1", &events).expect("insert_command_events failed");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM command_events ce JOIN sessions s ON ce.session_id = s.id WHERE s.provider_session_id = ?1",
            ["sess-cmd-1"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "one row per record must be inserted (CRIT-LUMEN-123)");
}

#[test]
fn test_command_event_repository_list_by_session_round_trips_sanitized_args() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("command_event_list_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let session_repo = SessionRepository::new(&conn);
    session_repo.upsert_session(&make_test_session_record("sess-cmd-2")).unwrap();

    let cmd_repo = CommandEventRepository::new(&conn);

    let events = vec![
        CommandEventFactRecord {
            command_base: "git".to_string(),
            sanitized_args: Some("push <REDACTED>".to_string()),
            is_error: false,
        },
        CommandEventFactRecord { command_base: "ls".to_string(), sanitized_args: None, is_error: false },
    ];
    cmd_repo.insert_command_events("claude", "sess-cmd-2", &events).expect("insert_command_events failed");

    let listed = cmd_repo.list_by_session("claude", "sess-cmd-2").expect("list_by_session failed");
    assert_eq!(listed.len(), 2);

    assert_eq!(listed[0].command_base, "git");
    assert_eq!(listed[0].sanitized_args, Some("push <REDACTED>".to_string()));
    assert!(!listed[0].is_error);

    assert_eq!(listed[1].command_base, "ls");
    assert_eq!(listed[1].sanitized_args, None, "sanitized_args must round-trip the None case correctly");
    assert!(!listed[1].is_error);
}

#[test]
fn test_command_event_repository_nonexistent_session_returns_error() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("command_event_missing_session_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let cmd_repo = CommandEventRepository::new(&conn);

    let events =
        vec![CommandEventFactRecord { command_base: "git".to_string(), sanitized_args: None, is_error: false }];

    let insert_result = cmd_repo.insert_command_events("claude", "no-such-session", &events);
    assert!(
        insert_result.is_err(),
        "insert_command_events against a nonexistent session must error, not silently corrupt data"
    );
    assert!(matches!(insert_result.unwrap_err(), StoreError::NotFound(_)));

    let list_result = cmd_repo.list_by_session("claude", "no-such-session");
    assert!(list_result.is_err(), "list_by_session against a nonexistent session must error");
    assert!(matches!(list_result.unwrap_err(), StoreError::NotFound(_)));
}

#[test]
fn test_snapshot_repository_save_and_get_latest_round_trips_bytes() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("snapshot_roundtrip_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = SnapshotRepository::new(&conn);

    let data = b"pre-compaction transcript bytes".to_vec();
    let id = repo.save_snapshot("claude", "sess-snap-1", &data).expect("save_snapshot failed");
    assert!(id > 0, "save_snapshot must return a real, usable row id (CRIT-LUMEN-124)");

    let latest = repo.get_latest_snapshot("claude", "sess-snap-1").expect("get_latest_snapshot failed");
    assert_eq!(latest, Some(data), "get_latest_snapshot must return the exact bytes saved (CRIT-LUMEN-124)");
}

#[test]
fn test_snapshot_repository_get_latest_returns_none_when_absent() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("snapshot_absent_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = SnapshotRepository::new(&conn);

    let result = repo.get_latest_snapshot("claude", "no-such-session").expect("get_latest_snapshot must not error");
    assert_eq!(
        result, None,
        "get_latest_snapshot must return Ok(None), not an error, when no snapshot exists (CRIT-LUMEN-124)"
    );
}

#[test]
fn test_snapshot_repository_get_latest_returns_most_recently_saved() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("snapshot_ordering_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = SnapshotRepository::new(&conn);

    repo.save_snapshot("claude", "sess-snap-order", b"first").unwrap();
    repo.save_snapshot("claude", "sess-snap-order", b"second").unwrap();
    repo.save_snapshot("claude", "sess-snap-order", b"third-and-latest").unwrap();

    let latest = repo.get_latest_snapshot("claude", "sess-snap-order").unwrap();
    assert_eq!(
        latest,
        Some(b"third-and-latest".to_vec()),
        "get_latest_snapshot must return the most recently saved snapshot (CRIT-LUMEN-124)"
    );
}

#[test]
fn test_snapshot_repository_scoped_by_provider_and_session_id() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("snapshot_scoping_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = SnapshotRepository::new(&conn);

    repo.save_snapshot("claude", "sess-scope-a", b"data-for-a").unwrap();
    repo.save_snapshot("codex", "sess-scope-a", b"data-for-codex-same-session-id").unwrap();
    repo.save_snapshot("claude", "sess-scope-b", b"data-for-b").unwrap();

    let a = repo.get_latest_snapshot("claude", "sess-scope-a").unwrap();
    assert_eq!(a, Some(b"data-for-a".to_vec()));

    let codex_same_session_id = repo.get_latest_snapshot("codex", "sess-scope-a").unwrap();
    assert_eq!(codex_same_session_id, Some(b"data-for-codex-same-session-id".to_vec()));

    let b = repo.get_latest_snapshot("claude", "sess-scope-b").unwrap();
    assert_eq!(b, Some(b"data-for-b".to_vec()));

    let nonexistent = repo.get_latest_snapshot("claude", "sess-scope-nonexistent").unwrap();
    assert_eq!(nonexistent, None);
}

#[test]
fn test_token_usage_repository_insert_one_row_per_model_with_direct_field_mapping() {
    use std::collections::HashMap;

    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("token_usage_insert_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let session_repo = SessionRepository::new(&conn);
    session_repo.upsert_session(&make_test_session_record("sess-token-1")).unwrap();

    let mut per_model = HashMap::new();
    per_model.insert(
        compact_str::CompactString::from("claude-3-5-sonnet-20241022"),
        ModelTokenSummary {
            input_tokens: 1000,
            output_tokens: 200,
            cache_creation_tokens: 50,
            cache_read_tokens: 30,
            reasoning_tokens: 0,
            cost_usd: 1.25,
            turns: 3,
            is_fully_priced: true,
        },
    );
    per_model.insert(
        compact_str::CompactString::from("claude-3-opus-20240229"),
        ModelTokenSummary {
            input_tokens: 2000,
            output_tokens: 400,
            cache_creation_tokens: 80,
            cache_read_tokens: 60,
            reasoning_tokens: 0,
            cost_usd: 2.50,
            turns: 5,
            is_fully_priced: true,
        },
    );

    let economics = TokenEconomics {
        input_tokens: 3000,
        output_tokens: 600,
        cache_creation_tokens: 130,
        cache_read_tokens: 90,
        ephemeral_5m_tokens: 0,
        ephemeral_1h_tokens: 0,
        cache_hit_ratio: 0.0,
        total_cost_usd: 3.75,
        provided_cost_usd: None,
        baseline_cost_no_cache_usd: 3.75,
        net_savings_usd: 0.0,
        efficiency_multiplier: 1.0,
        per_model,
        reasoning_output_tokens: 0,
        is_fully_priced: true,
    };

    let token_repo = TokenUsageRepository::new(&conn);
    token_repo.insert_token_usage("claude", "sess-token-1", &economics).expect("insert_token_usage failed");

    let internal_id: i64 = conn
        .query_row("SELECT id FROM sessions WHERE provider_session_id = ?1", ["sess-token-1"], |row| row.get(0))
        .unwrap();

    let mut stmt = conn
        .prepare(
            "SELECT model_name, input_tokens, cache_write_tokens, cache_read_tokens, output_tokens, cost_usd
             FROM token_usage WHERE session_id = ?1 ORDER BY model_name ASC",
        )
        .unwrap();
    let rows: Vec<(String, i64, i64, i64, i64, f64)> = stmt
        .query_map([internal_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert_eq!(rows.len(), 2, "must insert exactly one row per per_model entry");

    let sonnet = &rows[0];
    assert_eq!(sonnet.0, "claude-3-5-sonnet-20241022");
    assert_eq!(sonnet.1, 1000);
    assert_eq!(sonnet.2, 50, "cache_creation_tokens must map to cache_write_tokens column");
    assert_eq!(sonnet.3, 30);
    assert_eq!(sonnet.4, 200);
    assert!((sonnet.5 - 1.25).abs() < f64::EPSILON);

    let opus = &rows[1];
    assert_eq!(opus.0, "claude-3-opus-20240229");
    assert_eq!(opus.1, 2000);
    assert_eq!(opus.2, 80, "cache_creation_tokens must map to cache_write_tokens column");
    assert_eq!(opus.3, 60);
    assert_eq!(opus.4, 400);
    assert!((opus.5 - 2.50).abs() < f64::EPSILON);
}

#[test]
fn test_token_usage_repository_nonexistent_session_returns_error() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("token_usage_missing_session_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let token_repo = TokenUsageRepository::new(&conn);
    let economics = empty_token_economics();

    let insert_result = token_repo.insert_token_usage("claude", "no-such-session", &economics);
    assert!(insert_result.is_err(), "insert_token_usage against a nonexistent session must error");
    assert!(matches!(insert_result.unwrap_err(), StoreError::NotFound(_)));
}

#[test]
fn test_token_usage_repository_empty_per_model_inserts_zero_rows() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("token_usage_empty_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let session_repo = SessionRepository::new(&conn);
    session_repo.upsert_session(&make_test_session_record("sess-token-empty")).unwrap();

    let token_repo = TokenUsageRepository::new(&conn);
    let economics = empty_token_economics();

    token_repo.insert_token_usage("claude", "sess-token-empty", &economics).expect("empty per_model insert failed");

    let internal_id: i64 = conn
        .query_row("SELECT id FROM sessions WHERE provider_session_id = ?1", ["sess-token-empty"], |row| row.get(0))
        .unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM token_usage WHERE session_id = ?1", [internal_id], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

fn multi_model_economics(sonnet_turns: u64, opus_turns: u64) -> TokenEconomics {
    use std::collections::HashMap;
    let mut per_model = HashMap::new();
    per_model.insert(
        compact_str::CompactString::from("claude-3-5-sonnet-20241022"),
        ModelTokenSummary {
            input_tokens: 1000,
            output_tokens: 200,
            cache_creation_tokens: 50,
            cache_read_tokens: 30,
            reasoning_tokens: 0,
            cost_usd: 1.25,
            turns: sonnet_turns,
            is_fully_priced: true,
        },
    );
    per_model.insert(
        compact_str::CompactString::from("claude-3-opus-20240229"),
        ModelTokenSummary {
            input_tokens: 2000,
            output_tokens: 400,
            cache_creation_tokens: 80,
            cache_read_tokens: 60,
            reasoning_tokens: 0,
            cost_usd: 2.50,
            turns: opus_turns,
            is_fully_priced: true,
        },
    );

    TokenEconomics {
        input_tokens: 3000,
        output_tokens: 600,
        cache_creation_tokens: 130,
        cache_read_tokens: 90,
        ephemeral_5m_tokens: 0,
        ephemeral_1h_tokens: 0,
        cache_hit_ratio: 0.0,
        total_cost_usd: 3.75,
        provided_cost_usd: None,
        baseline_cost_no_cache_usd: 3.75,
        net_savings_usd: 0.0,
        efficiency_multiplier: 1.0,
        per_model,
        reasoning_output_tokens: 0,
        is_fully_priced: true,
    }
}

#[test]
fn test_upsert_session_persists_and_reads_back_per_model_token_usage() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("session_token_economics_roundtrip_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let session_repo = SessionRepository::new(&conn);

    let mut record = make_test_session_record("sess-economics-roundtrip");
    record.economics = multi_model_economics(3, 5);

    session_repo.upsert_session(&record).expect("upsert_session failed");

    let detail = session_repo
        .get_session("claude", "sess-economics-roundtrip")
        .expect("get_session failed")
        .expect("session not found");

    assert_eq!(detail.economics.per_model, record.economics.per_model, "per_model must round-trip exactly");
    assert_eq!(detail.economics.input_tokens, 3000, "input_tokens must be the sum across per_model entries");
    assert_eq!(detail.economics.output_tokens, 600, "output_tokens must be the sum across per_model entries");
    assert_eq!(
        detail.economics.cache_creation_tokens, 130,
        "cache_creation_tokens must be the sum across per_model entries"
    );
    assert_eq!(detail.economics.cache_read_tokens, 90, "cache_read_tokens must be the sum across per_model entries");
}

#[test]
fn test_upsert_session_reupsert_replaces_token_usage_without_duplicating_rows() {
    use std::collections::HashMap;

    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("session_token_economics_reupsert_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let session_repo = SessionRepository::new(&conn);

    let mut record = make_test_session_record("sess-economics-reupsert");
    record.economics = multi_model_economics(1, 1);
    session_repo.upsert_session(&record).expect("first upsert_session failed");

    // Second upsert for the SAME (provider, provider_session_id) with DIFFERENT per_model
    // content -- only one model this time, with different token counts.
    let mut second_per_model = HashMap::new();
    second_per_model.insert(
        compact_str::CompactString::from("claude-3-5-haiku-20241022"),
        ModelTokenSummary {
            input_tokens: 500,
            output_tokens: 100,
            cache_creation_tokens: 10,
            cache_read_tokens: 5,
            reasoning_tokens: 0,
            cost_usd: 0.10,
            turns: 2,
            is_fully_priced: true,
        },
    );
    record.economics = TokenEconomics {
        input_tokens: 500,
        output_tokens: 100,
        cache_creation_tokens: 10,
        cache_read_tokens: 5,
        ephemeral_5m_tokens: 0,
        ephemeral_1h_tokens: 0,
        cache_hit_ratio: 0.0,
        total_cost_usd: 0.10,
        provided_cost_usd: None,
        baseline_cost_no_cache_usd: 0.10,
        net_savings_usd: 0.0,
        efficiency_multiplier: 1.0,
        per_model: second_per_model.clone(),
        reasoning_output_tokens: 0,
        is_fully_priced: true,
    };
    session_repo.upsert_session(&record).expect("second upsert_session failed");

    let detail = session_repo
        .get_session("claude", "sess-economics-reupsert")
        .expect("get_session failed")
        .expect("session not found");

    assert_eq!(detail.economics.per_model, second_per_model, "get_session must reflect only the second upsert's data");
    assert_eq!(detail.economics.input_tokens, 500);
    assert_eq!(detail.economics.output_tokens, 100);
    assert_eq!(detail.economics.cache_creation_tokens, 10);
    assert_eq!(detail.economics.cache_read_tokens, 5);

    let internal_id: i64 = conn
        .query_row("SELECT id FROM sessions WHERE provider_session_id = ?1", ["sess-economics-reupsert"], |row| {
            row.get(0)
        })
        .unwrap();
    let row_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM token_usage WHERE session_id = ?1", [internal_id], |row| row.get(0))
        .unwrap();
    assert_eq!(row_count, 1, "re-upsert must fully replace token_usage rows, not accumulate duplicates");
}

#[test]
fn test_get_session_scoped_by_provider_avoids_cross_provider_collision() {
    // The `sessions` table's real uniqueness constraint is UNIQUE(provider, provider_session_id)
    // (see migrations.rs V2) -- a session is only truly identified by the PAIR, not by
    // provider_session_id alone. Two different providers can legitimately report the same
    // provider_session_id. get_session must resolve the correct row for the (provider,
    // session_id) pair given, not whichever row happens to match provider_session_id first.
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("session_provider_collision_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let session_repo = SessionRepository::new(&conn);

    let shared_session_id = "shared-session-id-123";

    let mut claude_record = make_test_session_record(shared_session_id);
    claude_record.provider = "claude-code".to_string();
    claude_record.model_family = "claude-3-5-sonnet-20241022".to_string();
    claude_record.economics.total_cost_usd = 1.11;
    claude_record.economics.net_savings_usd = 0.11;

    let mut codex_record = make_test_session_record(shared_session_id);
    codex_record.provider = "codex".to_string();
    codex_record.model_family = "gpt-5-codex".to_string();
    codex_record.economics.total_cost_usd = 9.99;
    codex_record.economics.net_savings_usd = 0.99;

    session_repo.upsert_session(&claude_record).expect("claude-code upsert failed");
    session_repo.upsert_session(&codex_record).expect("codex upsert failed");

    let claude_detail = session_repo
        .get_session("claude-code", shared_session_id)
        .expect("get_session for claude-code failed")
        .expect("claude-code session not found");
    assert_eq!(
        claude_detail.summary.model_family, "claude-3-5-sonnet-20241022",
        "get_session(\"claude-code\", ..) must return the claude-code row, not codex's"
    );
    assert_eq!(claude_detail.summary.total_cost_usd, 1.11);

    let codex_detail = session_repo
        .get_session("codex", shared_session_id)
        .expect("get_session for codex failed")
        .expect("codex session not found");
    assert_eq!(
        codex_detail.summary.model_family, "gpt-5-codex",
        "get_session(\"codex\", ..) must return the codex row, not claude-code's"
    );
    assert_eq!(codex_detail.summary.total_cost_usd, 9.99);
}
