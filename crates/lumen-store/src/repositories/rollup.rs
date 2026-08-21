use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::error::StoreError;
use crate::models::{RollupFactRecord, RollupReadModel};

pub struct RollupRepository<'a> {
    conn: &'a Connection,
}

impl<'a> RollupRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn upsert_rollup(&self, record: &RollupFactRecord) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO rollups (
                    period_start, period_type, session_count,
                    total_cost_usd, total_savings_usd, total_duration_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(period_start, period_type) DO UPDATE SET
                    session_count = excluded.session_count,
                    total_cost_usd = excluded.total_cost_usd,
                    total_savings_usd = excluded.total_savings_usd,
                    total_duration_ms = excluded.total_duration_ms",
                params![
                    record.period_start,
                    record.period_type,
                    record.session_count,
                    record.total_cost_usd,
                    record.total_savings_usd,
                    record.total_duration_ms as i64,
                ],
            )
            .map_err(StoreError::Sqlite)?;

        Ok(())
    }

    pub fn get_rollup(
        &self,
        period_start: DateTime<Utc>,
        period_type: &str,
    ) -> Result<Option<RollupReadModel>, StoreError> {
        let result = self.conn.query_row(
            "SELECT id, period_start, period_type, session_count, total_cost_usd, total_savings_usd, total_duration_ms
                 FROM rollups
                 WHERE period_start = ?1 AND period_type = ?2",
            params![period_start, period_type],
            |row| {
                let total_duration_ms: i64 = row.get(6)?;
                Ok(RollupReadModel {
                    id: row.get(0)?,
                    period_start: row.get(1)?,
                    period_type: row.get(2)?,
                    session_count: row.get(3)?,
                    total_cost_usd: row.get(4)?,
                    total_savings_usd: row.get(5)?,
                    total_duration_ms: total_duration_ms as u64,
                })
            },
        );

        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StoreError::Sqlite(e)),
        }
    }

    pub fn list_rollups(&self, period_type: &str, limit: usize) -> Result<Vec<RollupReadModel>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, period_start, period_type, session_count, total_cost_usd, total_savings_usd, total_duration_ms
                 FROM rollups
                 WHERE period_type = ?1
                 ORDER BY period_start DESC
                 LIMIT ?2",
            )
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![period_type, limit as i64], |row| {
                let total_duration_ms: i64 = row.get(6)?;
                Ok(RollupReadModel {
                    id: row.get(0)?,
                    period_start: row.get(1)?,
                    period_type: row.get(2)?,
                    session_count: row.get(3)?,
                    total_cost_usd: row.get(4)?,
                    total_savings_usd: row.get(5)?,
                    total_duration_ms: total_duration_ms as u64,
                })
            })
            .map_err(StoreError::Sqlite)?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(StoreError::Sqlite)?);
        }
        Ok(result)
    }
}
