use rusqlite::{params, Connection};

use crate::error::StoreError;

pub struct TrendRepository<'a> {
    conn: &'a Connection,
}

impl<'a> TrendRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn count_sessions(&self, provider: &str) -> Result<usize, StoreError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions WHERE provider = ?1", params![provider], |row| row.get(0))
            .map_err(StoreError::Sqlite)?;
        Ok(count as usize)
    }
}
