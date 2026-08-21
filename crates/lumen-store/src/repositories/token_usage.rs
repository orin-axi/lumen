use compact_str::CompactString;
use rusqlite::{params, Connection};
use std::collections::HashMap;

use crate::error::StoreError;

pub struct TokenUsageRepository<'a> {
    conn: &'a Connection,
}

impl<'a> TokenUsageRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    fn resolve_session_id(&self, session_id: &str) -> Result<i64, StoreError> {
        self.conn
            .query_row("SELECT id FROM sessions WHERE provider_session_id = ?1", params![session_id], |row| row.get(0))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StoreError::NotFound(format!("session with provider_session_id '{session_id}' not found"))
                }
                other => StoreError::Sqlite(other),
            })
    }

    pub fn insert_token_usage(
        &self,
        session_id: &str,
        economics: &lumen_model::TokenEconomics,
    ) -> Result<(), StoreError> {
        let internal_id = self.resolve_session_id(session_id)?;

        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO token_usage (session_id, model_name, input_tokens, cache_write_tokens, cache_read_tokens, output_tokens, cost_usd, turns)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .map_err(StoreError::Sqlite)?;

        for (model_name, summary) in economics.per_model.iter() {
            stmt.execute(params![
                internal_id,
                model_name.as_str(),
                summary.input_tokens,
                summary.cache_creation_tokens,
                summary.cache_read_tokens,
                summary.output_tokens,
                summary.cost_usd,
                summary.turns,
            ])
            .map_err(StoreError::Sqlite)?;
        }

        Ok(())
    }

    /// Deletes all `token_usage` rows for the given internal session id. Used by
    /// `SessionRepository::upsert_session` to make repeated upserts for the same
    /// (provider, provider_session_id) idempotent -- the per-model breakdown is fully
    /// replaced rather than accumulated across calls.
    pub fn delete_for_session(&self, session_id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM token_usage WHERE session_id = ?1", params![session_id])
            .map_err(StoreError::Sqlite)?;
        Ok(())
    }

    /// Reads back the per-model token usage breakdown for an internal session id, keyed by
    /// model name. Note: `reasoning_tokens` has no backing column in `token_usage` and is not
    /// tracked by `insert_token_usage`, so it is always read back as 0 -- a separate,
    /// already-known gap not addressed here.
    pub fn per_model_by_session(
        &self,
        session_id: i64,
    ) -> Result<HashMap<CompactString, lumen_model::ModelTokenSummary>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT model_name, input_tokens, cache_write_tokens, cache_read_tokens, output_tokens, cost_usd, turns
                 FROM token_usage WHERE session_id = ?1",
            )
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let model_name: String = row.get(0)?;
                let input_tokens: i64 = row.get(1)?;
                let cache_creation_tokens: i64 = row.get(2)?;
                let cache_read_tokens: i64 = row.get(3)?;
                let output_tokens: i64 = row.get(4)?;
                let cost_usd: f64 = row.get(5)?;
                let turns: i64 = row.get(6)?;

                Ok((
                    CompactString::new(model_name),
                    lumen_model::ModelTokenSummary {
                        input_tokens: input_tokens as u64,
                        output_tokens: output_tokens as u64,
                        cache_creation_tokens: cache_creation_tokens as u64,
                        cache_read_tokens: cache_read_tokens as u64,
                        reasoning_tokens: 0,
                        cost_usd,
                        turns: turns as u64,
                    },
                ))
            })
            .map_err(StoreError::Sqlite)?;

        let mut result = HashMap::new();
        for r in rows {
            let (model_name, summary) = r.map_err(StoreError::Sqlite)?;
            result.insert(model_name, summary);
        }
        Ok(result)
    }
}
