//! Dev-only test fixtures, real recorded transcripts, and test doubles for Lumen.

pub mod corpus;
pub mod db;

pub use corpus::*;
pub use db::*;

#[cfg(test)]
mod tests {
    use super::corpus::*;
    use super::db::*;

    #[test]
    fn test_corpus_samples_are_valid() {
        assert!(real_claude_session_dump().contains("claude-3-5-sonnet"));
        assert!(real_antigravity_session_dump().contains("PLANNER_RESPONSE"));
        let opencode_db = real_opencode_session_db();
        assert!(opencode_db.path.exists());
        assert!(corrupted_mixed_lines_sample().contains("\u{FEFF}"));
    }

    #[test]
    fn test_database_double_initialization() {
        // The schema is one idempotent script (no version-tracking table) -- applying it twice
        // must be a safe no-op, and the real tables must exist afterward.
        let db = create_migrated_test_db();
        db.store.run_migrations().expect("re-applying the schema must be a safe no-op");

        let conn = db.store.connection().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sessions'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1, "the sessions table must exist after schema setup");
    }
}
