use compact_str::CompactString;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

use crate::error::StoreError;
use crate::models::{ToolCallFactRecord, ToolCallReadModel};

pub struct ToolCallRepository<'a> {
    conn: &'a Connection,
}

impl<'a> ToolCallRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn insert_tool_calls(&self, session_id: i64, calls: &[ToolCallFactRecord]) -> Result<(), StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO tool_calls (session_id, turn_index, tool_name, call_id, intent_kind, is_error, latency_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(StoreError::Sqlite)?;

        for c in calls {
            stmt.execute(params![
                session_id,
                c.turn_index as i64,
                c.tool_name,
                c.call_id,
                c.intent_kind,
                if c.is_error { 1 } else { 0 },
                c.latency_ms as i64,
            ])
            .map_err(StoreError::Sqlite)?;
        }

        Ok(())
    }

    pub fn list_by_session(&self, session_id: i64) -> Result<Vec<ToolCallReadModel>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, turn_index, tool_name, call_id, intent_kind, is_error, latency_ms
                 FROM tool_calls
                 WHERE session_id = ?1
                 ORDER BY id ASC",
            )
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let turn_index: i64 = row.get(2)?;
                let is_error: i64 = row.get(6)?;
                let latency_ms: i64 = row.get(7)?;
                Ok(ToolCallReadModel {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    turn_index: turn_index as usize,
                    tool_name: row.get(3)?,
                    call_id: row.get(4)?,
                    intent_kind: row.get(5)?,
                    is_error: is_error != 0,
                    latency_ms: latency_ms as u64,
                })
            })
            .map_err(StoreError::Sqlite)?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(StoreError::Sqlite)?);
        }
        Ok(result)
    }

    pub fn tool_counts_by_session(&self, session_id: i64) -> Result<BTreeMap<CompactString, usize>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT tool_name, COUNT(*) FROM tool_calls WHERE session_id = ?1 GROUP BY tool_name")
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let tool_name: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((CompactString::from(tool_name), count as usize))
            })
            .map_err(StoreError::Sqlite)?;

        let mut result = BTreeMap::new();
        for r in rows {
            let (tool_name, count) = r.map_err(StoreError::Sqlite)?;
            result.insert(tool_name, count);
        }
        Ok(result)
    }

    pub fn error_counts_by_session(&self, session_id: i64) -> Result<BTreeMap<CompactString, usize>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT tool_name, COUNT(*) FROM tool_calls WHERE session_id = ?1 AND is_error = 1 GROUP BY tool_name",
            )
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let tool_name: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((CompactString::from(tool_name), count as usize))
            })
            .map_err(StoreError::Sqlite)?;

        let mut result = BTreeMap::new();
        for r in rows {
            let (tool_name, count) = r.map_err(StoreError::Sqlite)?;
            result.insert(tool_name, count);
        }
        Ok(result)
    }
}
