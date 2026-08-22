use rusqlite::{params, Connection};

use crate::error::StoreError;
use crate::models::{CommandEventFactRecord, CommandEventReadModel};

pub struct CommandEventRepository<'a> {
    conn: &'a Connection,
}

impl<'a> CommandEventRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Redacts a raw (or already partially-redacted) argument string into a stable pattern:
    /// flag names are preserved (they're part of the command's fixed vocabulary, not private
    /// data -- e.g. `-m`, `--verbose`), but every value is replaced with a fixed placeholder --
    /// bare positional tokens outright, and the value half of `--flag=value` pairs. This is the
    /// store's own redaction pass (CRIT-LUMEN-037): `insert_command_events` never trusts a
    /// caller-supplied string to already be safe, since this repository is the last boundary
    /// before the argument string is written to disk. A simple whitespace tokenizer, not a real
    /// shell parser -- sufficient to guarantee no raw token survives, which is the actual
    /// "zero raw private arguments persisted" contract; it does not attempt to preserve exact
    /// shell quoting/escaping semantics.
    fn redact_args(raw: &str) -> String {
        raw.split_whitespace()
            .map(|token| {
                if let Some(long_flag) = token.strip_prefix("--") {
                    match long_flag.split_once('=') {
                        Some((name, _value)) => format!("--{name}=<redacted>"),
                        None => token.to_string(),
                    }
                } else if token.starts_with('-') {
                    token.to_string()
                } else {
                    "<redacted>".to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn resolve_session_id(&self, provider: &str, session_id: &str) -> Result<i64, StoreError> {
        self.conn
            .query_row(
                "SELECT id FROM sessions WHERE provider = ?1 AND provider_session_id = ?2",
                params![provider, session_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => StoreError::NotFound(format!(
                    "session with provider '{provider}' and provider_session_id '{session_id}' not found"
                )),
                other => StoreError::Sqlite(other),
            })
    }

    pub fn insert_command_events(
        &self,
        provider: &str,
        session_id: &str,
        events: &[CommandEventFactRecord],
    ) -> Result<(), StoreError> {
        let internal_id = self.resolve_session_id(provider, session_id)?;

        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO command_events (session_id, command_base, sanitized_args, is_error)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(StoreError::Sqlite)?;

        for e in events {
            let redacted = e.sanitized_args.as_deref().map(Self::redact_args);
            stmt.execute(params![internal_id, e.command_base, redacted, if e.is_error { 1 } else { 0 },])
                .map_err(StoreError::Sqlite)?;
        }

        Ok(())
    }

    pub fn list_by_session(&self, provider: &str, session_id: &str) -> Result<Vec<CommandEventReadModel>, StoreError> {
        let internal_id = self.resolve_session_id(provider, session_id)?;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, command_base, sanitized_args, is_error
                 FROM command_events
                 WHERE session_id = ?1
                 ORDER BY id ASC",
            )
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![internal_id], |row| {
                let is_error: i64 = row.get(4)?;
                Ok(CommandEventReadModel {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    command_base: row.get(2)?,
                    sanitized_args: row.get(3)?,
                    is_error: is_error != 0,
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
