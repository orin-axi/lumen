use rusqlite::{params, Connection};

use crate::error::StoreError;
use crate::models::{SessionTrendPoint, TrendFilter};

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

    /// Base query only (CRIT-LUMEN-183, CRIT-LUMEN-190): provider filter, then a --limit cap to
    /// at most `limit` most recent sessions by `started_at DESC` with a `provider_session_id ASC`
    /// tie-break, then re-ordered oldest-to-newest for return. CompactionSummary population
    /// (CRIT-LUMEN-184) is deliberately deferred to a later task -- `.compaction` is always
    /// `None` here regardless of `filter.require_compaction`.
    pub fn list_session_trend(&self, filter: &TrendFilter) -> Result<Vec<SessionTrendPoint>, StoreError> {
        let limit = if filter.limit > 0 { filter.limit } else { 50 };
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, provider, provider_session_id, started_at, total_cost_usd, is_fully_priced,
                        cache_hit_ratio, turn_count, has_anomalies
                 FROM sessions
                 WHERE (?1 IS NULL OR provider = ?1)
                 ORDER BY started_at DESC, provider_session_id ASC
                 LIMIT ?2",
            )
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![filter.provider.as_deref(), limit as i64], |row| {
                let id: i64 = row.get(0)?;
                let provider: String = row.get(1)?;
                let session_id: String = row.get(2)?;
                let started_at = row.get(3)?;
                let total_cost_usd: f64 = row.get(4)?;
                let is_fully_priced: i64 = row.get(5)?;
                let cache_hit_ratio: f32 = row.get(6)?;
                let turn_count: i64 = row.get(7)?;
                let has_anomalies: i64 = row.get(8)?;
                // CRIT-LUMEN-183: cache_hit_ratio is already a [0.0, 100.0] percentage in the
                // sessions table -- round half-to-even to one decimal place, never rescale.
                let rounded_cache_hit = format!("{cache_hit_ratio:.1}").parse::<f32>().unwrap();
                let cost =
                    if is_fully_priced != 0 { lumen_model::Cost::Priced(total_cost_usd) } else { lumen_model::Cost::Unpriced };
                Ok((
                    id,
                    SessionTrendPoint {
                        provider,
                        session_id,
                        started_at,
                        cost,
                        cache_hit_ratio: rounded_cache_hit,
                        turn_count: turn_count as usize,
                        has_anomalies: has_anomalies != 0,
                        compaction: None,
                    },
                ))
            })
            .map_err(StoreError::Sqlite)?;

        let mut points: Vec<(i64, SessionTrendPoint)> = Vec::new();
        for r in rows {
            points.push(r.map_err(StoreError::Sqlite)?);
        }
        // Naively reversing the DESC-selected rows would also flip the provider_session_id
        // tie-break within equal-started_at groups; re-sort explicitly so oldest-to-newest
        // keeps the same ascending tie-break as the selection query.
        points.sort_by(|a, b| a.1.started_at.cmp(&b.1.started_at).then_with(|| a.1.session_id.cmp(&b.1.session_id)));

        Ok(points.into_iter().map(|(_id, p)| p).collect())
    }
}
