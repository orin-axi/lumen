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
}
