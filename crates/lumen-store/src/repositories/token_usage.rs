use rusqlite::{params, Connection};

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
            .query_row("SELECT id FROM sessions WHERE provider_session_id = ?1", params![session_id], |row| {
                row.get(0)
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StoreError::NotFound(format!("session with provider_session_id '{session_id}' not found"))
                }
                other => StoreError::Sqlite(other),
            })
    }

    pub fn insert_token_usage(&self, session_id: &str, economics: &lumen_model::TokenEconomics) -> Result<(), StoreError> {
        let internal_id = self.resolve_session_id(session_id)?;

        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO token_usage (session_id, model_name, input_tokens, cache_write_tokens, cache_read_tokens, output_tokens, cost_usd)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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
            ])
            .map_err(StoreError::Sqlite)?;
        }

        Ok(())
    }
}
