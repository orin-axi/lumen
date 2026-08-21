use camino::Utf8PathBuf;
use chrono::Utc;
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

    // Verify all 5 migrations were recorded
    let count: usize = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0))
        .expect("Failed to query schema_migrations");
    assert_eq!(count, 5);

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

    let detail = repo.get_session("sess-abc-123").expect("Get session failed").expect("Session not found");
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
