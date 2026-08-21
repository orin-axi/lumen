use rusqlite::{params, Connection};

use crate::error::StoreError;
use crate::models::{FindingFactRecord, FindingReadModel};

pub struct FindingsRepository<'a> {
    conn: &'a Connection,
}

impl<'a> FindingsRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn insert_findings(&self, session_id: &str, findings: &[FindingFactRecord]) -> Result<(), StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO findings (session_id, rule_id, severity, confidence, title, message, evidence_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(StoreError::Sqlite)?;

        for f in findings {
            let evidence_str = f.evidence_json.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());

            stmt.execute(params![session_id, f.rule_id, f.severity, f.confidence, f.title, f.message, evidence_str,])
                .map_err(StoreError::Sqlite)?;
        }

        Ok(())
    }

    pub fn list_top_findings(&self, limit: usize) -> Result<Vec<FindingReadModel>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, rule_id, severity, confidence, title, message
                 FROM findings
                 ORDER BY confidence DESC, id DESC
                 LIMIT ?1",
            )
            .map_err(StoreError::Sqlite)?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(FindingReadModel {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    rule_id: row.get(2)?,
                    severity: row.get(3)?,
                    confidence: row.get(4)?,
                    title: row.get(5)?,
                    message: row.get(6)?,
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
