use camino::Utf8PathBuf;
use chrono::Utc;
use lumen_model::*;
use lumen_store::*;
use std::sync::Arc;
use std::thread;
use tempfile::tempdir;

#[test]
fn test_concurrent_multithreaded_readers_and_writers() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("concurrency_test.db")).unwrap();

    let store = Arc::new(SqliteStore::open(&db_path).expect("Failed to open SQLite store"));

    let mut handles = Vec::new();

    // Spawn 4 writer threads
    for thread_idx in 0..4 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            let conn = store_clone.connection().unwrap();
            let repo = SessionRepository::new(&conn);

            for i in 0..10 {
                let record = SessionFactRecord {
                    provider: "claude".to_string(),
                    provider_session_id: format!("thread-{thread_idx}-sess-{i}"),
                    model_family: "claude-3-5-sonnet-20241022".to_string(),
                    orchestrator: OrchestratorKind::ClaudeCode,
                    started_at: Utc::now(),
                    ended_at: Utc::now(),
                    wall_duration_ms: 1000,
                    turn_count: 5,
                    economics: TokenEconomics::calculate(100, 50, 0, 200, "claude-3-5-sonnet-20241022"),
                    has_anomalies: false,
                };
                repo.upsert_session(&record).unwrap();
            }
        });
        handles.push(handle);
    }

    // Spawn 4 reader threads
    for _ in 0..4 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            let conn = store_clone.connection().unwrap();
            let repo = SessionRepository::new(&conn);

            for _ in 0..10 {
                let _res = repo.list_recent(&SessionFilter { provider: None, limit: 50 });
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let conn = store.connection().unwrap();
    let repo = SessionRepository::new(&conn);
    let all = repo.list_recent(&SessionFilter { provider: None, limit: 100 }).unwrap();
    assert_eq!(all.len(), 40);
}

#[test]
fn test_queue_dead_letter_exact_thresholds() {
    let dir = tempdir().unwrap();
    let db_path = Utf8PathBuf::from_path_buf(dir.path().join("dl_test.db")).unwrap();
    let store = SqliteStore::open(&db_path).unwrap();
    let conn = store.connection().unwrap();

    let repo = QueueRepository::new(&conn);
    let id = repo.enqueue("claude", "sess-dl", "/path/to/sess.jsonl", 1111).unwrap();

    // Initial state: retry_count = 0, status = 'pending'
    let pending = repo.fetch_pending(10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].retry_count, 0);

    // 1st failure: retry_count = 1, status = 'failed'
    repo.mark_failed(id, "err1").unwrap();
    let status1: (String, u32) = conn
        .query_row("SELECT status, retry_count FROM ingestion_queue WHERE id = ?1", [id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(status1.0, "failed");
    assert_eq!(status1.1, 1);

    // 2nd failure: retry_count = 2, status = 'failed'
    repo.mark_failed(id, "err2").unwrap();
    let status2: (String, u32) = conn
        .query_row("SELECT status, retry_count FROM ingestion_queue WHERE id = ?1", [id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(status2.0, "failed");
    assert_eq!(status2.1, 2);

    // 3rd failure: retry_count = 3, status = 'dead_letter' (CRIT-LUMEN-034)
    repo.mark_failed(id, "err3").unwrap();
    let status3: (String, u32) = conn
        .query_row("SELECT status, retry_count FROM ingestion_queue WHERE id = ?1", [id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(status3.0, "dead_letter");
    assert_eq!(status3.1, 3);

    // Dead letter item is excluded from fetch_pending
    let pending_after = repo.fetch_pending(10).unwrap();
    assert_eq!(pending_after.len(), 0);
}
