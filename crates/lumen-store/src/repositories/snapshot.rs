use rusqlite::{params, Connection};

use crate::error::StoreError;

pub struct SnapshotRepository<'a> {
    conn: &'a Connection,
}

impl<'a> SnapshotRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn save_snapshot(&self, provider: &str, session_id: &str, data: &[u8]) -> Result<i64, StoreError> {
        self.conn
            .execute(
                "INSERT INTO session_snapshots (provider, session_id, snapshot_blob) VALUES (?1, ?2, ?3)",
                params![provider, session_id, data],
            )
            .map_err(StoreError::Sqlite)?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_latest_snapshot(&self, provider: &str, session_id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.conn
            .query_row(
                "SELECT snapshot_blob FROM session_snapshots
                 WHERE provider = ?1 AND session_id = ?2
                 ORDER BY id DESC LIMIT 1",
                params![provider, session_id],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(StoreError::Sqlite(other)),
            })
    }
}
