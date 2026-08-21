use rusqlite::{params, Connection};

use crate::error::StoreError;
use crate::models::QueuedSessionRow;

pub struct QueueRepository<'a> {
    conn: &'a Connection,
}

impl<'a> QueueRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn enqueue(
        &self,
        provider: &str,
        session_id: &str,
        source_path: &str,
        source_hash: u64,
    ) -> Result<i64, StoreError> {
        self.conn
            .execute(
                "INSERT INTO ingestion_queue (provider, session_id, source_path, source_hash, status)
                 VALUES (?1, ?2, ?3, ?4, 'pending')",
                params![provider, session_id, source_path, source_hash as i64],
            )
            .map_err(StoreError::Sqlite)?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn fetch_pending(&self, limit: usize) -> Result<Vec<QueuedSessionRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, provider, session_id, source_path, source_hash, retry_count, status, created_at
                 FROM ingestion_queue
                 WHERE status = 'pending'
                 ORDER BY id ASC
                 LIMIT ?1",
            )
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let source_hash_i64: i64 = row.get(4)?;
                Ok(QueuedSessionRow {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    session_id: row.get(2)?,
                    source_path: row.get(3)?,
                    source_hash: source_hash_i64 as u64,
                    retry_count: row.get(5)?,
                    status: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .map_err(StoreError::Sqlite)?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(StoreError::Sqlite)?);
        }
        Ok(result)
    }

    pub fn mark_completed(&self, id: i64) -> Result<(), StoreError> {
        self.conn
            .execute(
                "UPDATE ingestion_queue SET status = 'completed', updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                params![id],
            )
            .map_err(StoreError::Sqlite)?;
        Ok(())
    }

    pub fn mark_failed(&self, id: i64, error: &str) -> Result<(), StoreError> {
        // Fetch current retry count
        let retry_count: u32 = self
            .conn
            .query_row("SELECT retry_count FROM ingestion_queue WHERE id = ?1", params![id], |row| row.get(0))
            .unwrap_or(0);

        let new_retry = retry_count + 1;
        let new_status = if new_retry >= 3 { "dead_letter" } else { "failed" };

        self.conn
            .execute(
                "UPDATE ingestion_queue 
                 SET status = ?1, retry_count = ?2, last_error = ?3, updated_at = CURRENT_TIMESTAMP 
                 WHERE id = ?4",
                params![new_status, new_retry, error, id],
            )
            .map_err(StoreError::Sqlite)?;

        Ok(())
    }
}
