use rusqlite::{params, Connection};

use crate::error::StoreError;
use crate::models::CompactionFactRecord;

pub struct CompactionRepository<'a> {
    conn: &'a Connection,
}

impl<'a> CompactionRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn insert_compaction_facts(&self, session_id: i64, events: &[CompactionFactRecord]) -> Result<(), StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO compaction_events (session_id, sequence, trigger, pre_tokens, post_tokens, cumulative_dropped_tokens, duration_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(StoreError::Sqlite)?;

        for e in events {
            stmt.execute(params![
                session_id,
                e.sequence,
                e.trigger,
                e.pre_tokens as i64,
                e.post_tokens as i64,
                e.cumulative_dropped_tokens as i64,
                e.duration_ms as i64,
            ])
            .map_err(StoreError::Sqlite)?;
        }

        Ok(())
    }

    pub fn delete_for_session(&self, session_id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM compaction_events WHERE session_id = ?1", params![session_id])
            .map_err(StoreError::Sqlite)?;
        Ok(())
    }

    /// Reads back every persisted compaction event for `session_id`, ordered by `sequence`
    /// ascending -- for verifying end-to-end that `insert_compaction_facts` round-trips every
    /// field (not just the ones `compaction_summary_for_session` happens to aggregate).
    pub fn list_for_session(&self, session_id: i64) -> Result<Vec<CompactionFactRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT session_id, sequence, trigger, pre_tokens, post_tokens, cumulative_dropped_tokens, duration_ms
                 FROM compaction_events WHERE session_id = ?1 ORDER BY sequence ASC",
            )
            .map_err(StoreError::Sqlite)?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(CompactionFactRecord {
                    session_id: row.get(0)?,
                    sequence: row.get(1)?,
                    trigger: row.get(2)?,
                    pre_tokens: row.get::<_, i64>(3)? as u64,
                    post_tokens: row.get::<_, i64>(4)? as u64,
                    cumulative_dropped_tokens: row.get::<_, i64>(5)? as u64,
                    duration_ms: row.get::<_, i64>(6)? as u64,
                })
            })
            .map_err(StoreError::Sqlite)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StoreError::Sqlite)
    }
}
