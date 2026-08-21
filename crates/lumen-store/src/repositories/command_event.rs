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

    pub fn insert_command_events(&self, session_id: &str, events: &[CommandEventFactRecord]) -> Result<(), StoreError> {
        let internal_id = self.resolve_session_id(session_id)?;

        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO command_events (session_id, command_base, sanitized_args, is_error)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(StoreError::Sqlite)?;

        for e in events {
            stmt.execute(params![internal_id, e.command_base, e.sanitized_args, if e.is_error { 1 } else { 0 },])
                .map_err(StoreError::Sqlite)?;
        }

        Ok(())
    }

    pub fn list_by_session(&self, session_id: &str) -> Result<Vec<CommandEventReadModel>, StoreError> {
        let internal_id = self.resolve_session_id(session_id)?;

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
