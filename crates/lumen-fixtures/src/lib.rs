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
        assert!(real_opencode_session_dump().contains("Cargo.toml"));
        assert!(corrupted_mixed_lines_sample().contains("\u{FEFF}"));
    }

    #[test]
    fn test_database_double_initialization() {
        let db = create_migrated_test_db();
        let conn = db.store.connection().unwrap();
        let count: usize = conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 5);
    }
}
