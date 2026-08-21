use lumen_model::TokenEconomics;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

use crate::error::StoreError;
use crate::models::{SessionDetailReadModel, SessionFactRecord, SessionFilter, SessionSummaryReadModel};
use crate::repositories::token_usage::TokenUsageRepository;
use crate::repositories::tool_call::ToolCallRepository;

pub struct SessionRepository<'a> {
    conn: &'a Connection,
}

impl<'a> SessionRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn upsert_session(&self, record: &SessionFactRecord) -> Result<(), StoreError> {
        let orchestrator_str = format!("{:?}", record.orchestrator);

        self.conn
            .execute(
                "INSERT INTO sessions (
                    provider, provider_session_id, model_family, orchestrator,
                    started_at, ended_at, wall_duration_ms, turn_count,
                    cache_hit_ratio, total_cost_usd, baseline_cost_usd, net_savings_usd,
                    efficiency_multiplier, has_anomalies
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                ON CONFLICT(provider, provider_session_id) DO UPDATE SET
                    model_family = excluded.model_family,
                    orchestrator = excluded.orchestrator,
                    started_at = excluded.started_at,
                    ended_at = excluded.ended_at,
                    wall_duration_ms = excluded.wall_duration_ms,
                    turn_count = excluded.turn_count,
                    cache_hit_ratio = excluded.cache_hit_ratio,
                    total_cost_usd = excluded.total_cost_usd,
                    baseline_cost_usd = excluded.baseline_cost_usd,
                    net_savings_usd = excluded.net_savings_usd,
                    efficiency_multiplier = excluded.efficiency_multiplier,
                    has_anomalies = excluded.has_anomalies",
                params![
                    record.provider,
                    record.provider_session_id,
                    record.model_family,
                    orchestrator_str,
                    record.started_at,
                    record.ended_at,
                    record.wall_duration_ms as i64,
                    record.turn_count as i64,
                    record.economics.cache_hit_ratio,
                    record.economics.total_cost_usd,
                    record.economics.baseline_cost_no_cache_usd,
                    record.economics.net_savings_usd,
                    record.economics.efficiency_multiplier,
                    if record.has_anomalies { 1 } else { 0 },
                ],
            )
            .map_err(StoreError::Sqlite)?;

        let internal_id: i64 = self
            .conn
            .query_row(
                "SELECT id FROM sessions WHERE provider_session_id = ?1",
                params![record.provider_session_id],
                |row| row.get(0),
            )
            .map_err(StoreError::Sqlite)?;

        let token_usage_repo = TokenUsageRepository::new(self.conn);
        token_usage_repo.delete_for_session(internal_id)?;
        token_usage_repo.insert_token_usage(&record.provider_session_id, &record.economics)?;

        Ok(())
    }

    pub fn list_recent(&self, filter: &SessionFilter) -> Result<Vec<SessionSummaryReadModel>, StoreError> {
        let limit = if filter.limit > 0 { filter.limit } else { 50 };

        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, provider, provider_session_id, model_family, turn_count,
                        wall_duration_ms, cache_hit_ratio, total_cost_usd, net_savings_usd, created_at
                 FROM sessions
                 WHERE (?1 IS NULL OR provider = ?1)
                 ORDER BY started_at DESC
                 LIMIT ?2",
            )
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![filter.provider.as_deref(), limit as i64], |row| {
                let wall_ms: i64 = row.get(5)?;
                let turns: i64 = row.get(4)?;
                Ok(SessionSummaryReadModel {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    session_id: row.get(2)?,
                    model_family: row.get(3)?,
                    turn_count: turns as usize,
                    wall_duration_ms: wall_ms as u64,
                    cache_hit_ratio: row.get(6)?,
                    total_cost_usd: row.get(7)?,
                    net_savings_usd: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })
            .map_err(StoreError::Sqlite)?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(StoreError::Sqlite)?);
        }
        Ok(result)
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionDetailReadModel>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, provider, provider_session_id, model_family, turn_count,
                        wall_duration_ms, cache_hit_ratio, total_cost_usd, net_savings_usd, baseline_cost_usd,
                        efficiency_multiplier, created_at
                 FROM sessions
                 WHERE provider_session_id = ?1",
            )
            .map_err(StoreError::Sqlite)?;

        let mut rows = stmt
            .query_map(params![session_id], |row| {
                let id: i64 = row.get(0)?;
                let provider: String = row.get(1)?;
                let sess_id: String = row.get(2)?;
                let model_family: String = row.get(3)?;
                let turns: i64 = row.get(4)?;
                let wall_ms: i64 = row.get(5)?;
                let cache_hit: f32 = row.get(6)?;
                let cost_usd: f64 = row.get(7)?;
                let savings_usd: f64 = row.get(8)?;
                let baseline_usd: f64 = row.get(9)?;
                let efficiency: f32 = row.get(10)?;
                let created_at = row.get(11)?;

                Ok((
                    id,
                    SessionDetailReadModel {
                        summary: SessionSummaryReadModel {
                            id,
                            provider,
                            session_id: sess_id,
                            model_family: model_family.clone(),
                            turn_count: turns as usize,
                            wall_duration_ms: wall_ms as u64,
                            cache_hit_ratio: cache_hit,
                            total_cost_usd: cost_usd,
                            net_savings_usd: savings_usd,
                            created_at,
                        },
                        economics: TokenEconomics {
                            input_tokens: 0,
                            output_tokens: 0,
                            cache_creation_tokens: 0,
                            cache_read_tokens: 0,
                            ephemeral_5m_tokens: 0,
                            ephemeral_1h_tokens: 0,
                            cache_hit_ratio: cache_hit,
                            total_cost_usd: cost_usd,
                            provided_cost_usd: None,
                            baseline_cost_no_cache_usd: baseline_usd,
                            net_savings_usd: savings_usd,
                            efficiency_multiplier: efficiency,
                            per_model: std::collections::HashMap::new(),
                            reasoning_output_tokens: 0,
                        },
                        tool_counts: BTreeMap::new(),
                        error_counts: BTreeMap::new(),
                    },
                ))
            })
            .map_err(StoreError::Sqlite)?;

        let next_row = if let Some(res) = rows.next() { Some(res.map_err(StoreError::Sqlite)?) } else { None };
        drop(rows);
        drop(stmt);

        let (id, mut detail) = match next_row {
            Some(pair) => pair,
            None => return Ok(None),
        };

        let tool_call_repo = ToolCallRepository::new(self.conn);
        detail.tool_counts = tool_call_repo.tool_counts_by_session(id)?;
        detail.error_counts = tool_call_repo.error_counts_by_session(id)?;

        let token_usage_repo = TokenUsageRepository::new(self.conn);
        let per_model = token_usage_repo.per_model_by_session(id)?;

        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut cache_creation_tokens = 0u64;
        let mut cache_read_tokens = 0u64;
        for summary in per_model.values() {
            input_tokens += summary.input_tokens;
            output_tokens += summary.output_tokens;
            cache_creation_tokens += summary.cache_creation_tokens;
            cache_read_tokens += summary.cache_read_tokens;
        }

        detail.economics.input_tokens = input_tokens;
        detail.economics.output_tokens = output_tokens;
        detail.economics.cache_creation_tokens = cache_creation_tokens;
        detail.economics.cache_read_tokens = cache_read_tokens;
        detail.economics.per_model = per_model;

        Ok(Some(detail))
    }
}
