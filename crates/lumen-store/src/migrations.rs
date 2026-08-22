use rusqlite::Connection;

use crate::error::StoreError;

/// Applies Lumen's current SQLite schema. Pre-release, there is no installed base or existing
/// data to preserve across a schema change -- so instead of a versioned migration chain, this is
/// one idempotent script (every statement is CREATE ... IF NOT EXISTS) reflecting the schema's
/// current, single state. Changing a column or table means editing the relevant CREATE statement
/// directly; a local dev database that predates the change is safe to delete and let this
/// recreate, since nothing here needs to preserve real user data yet.
pub struct MigrationManager;

impl MigrationManager {
    pub const SCHEMA: &'static str = r#"
        CREATE TABLE IF NOT EXISTS ingestion_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            provider TEXT NOT NULL,
            session_id TEXT NOT NULL,
            source_path TEXT NOT NULL,
            source_hash INTEGER NOT NULL,
            retry_count INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'pending',
            last_error TEXT,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_ingestion_queue_status ON ingestion_queue(status);

        CREATE TABLE IF NOT EXISTS session_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            provider TEXT NOT NULL,
            session_id TEXT NOT NULL,
            snapshot_blob BLOB NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS pipeline_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_type TEXT NOT NULL,
            status TEXT NOT NULL,
            records_processed INTEGER NOT NULL DEFAULT 0,
            started_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            ended_at TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            provider TEXT NOT NULL,
            provider_session_id TEXT NOT NULL,
            model_family TEXT NOT NULL,
            orchestrator TEXT NOT NULL,
            started_at TIMESTAMP NOT NULL,
            ended_at TIMESTAMP NOT NULL,
            wall_duration_ms INTEGER NOT NULL,
            turn_count INTEGER NOT NULL,
            cache_hit_ratio REAL NOT NULL,
            total_cost_usd REAL NOT NULL,
            baseline_cost_usd REAL NOT NULL,
            net_savings_usd REAL NOT NULL,
            efficiency_multiplier REAL NOT NULL,
            has_anomalies INTEGER NOT NULL DEFAULT 0,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(provider, provider_session_id)
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_provider_id ON sessions(provider, provider_session_id);
        CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(started_at);

        CREATE TABLE IF NOT EXISTS token_usage (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            model_name TEXT NOT NULL,
            input_tokens INTEGER NOT NULL,
            cache_write_tokens INTEGER NOT NULL,
            cache_read_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            cost_usd REAL NOT NULL,
            turns INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_token_usage_session ON token_usage(session_id);

        CREATE TABLE IF NOT EXISTS tool_calls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            turn_index INTEGER NOT NULL,
            tool_name TEXT NOT NULL,
            call_id TEXT NOT NULL,
            intent_kind TEXT NOT NULL,
            is_error INTEGER NOT NULL DEFAULT 0,
            latency_ms INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_tool_calls_session ON tool_calls(session_id);

        CREATE TABLE IF NOT EXISTS command_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            command_base TEXT NOT NULL,
            sanitized_args TEXT,
            is_error INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS findings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            rule_id TEXT NOT NULL,
            severity TEXT NOT NULL,
            confidence REAL NOT NULL,
            title TEXT NOT NULL,
            message TEXT NOT NULL,
            evidence_json TEXT,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_findings_session ON findings(session_id);

        CREATE TABLE IF NOT EXISTS session_category_scores (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            category TEXT NOT NULL,
            score REAL NOT NULL
        );

        CREATE TABLE IF NOT EXISTS rollups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            period_start TIMESTAMP NOT NULL,
            period_type TEXT NOT NULL,
            session_count INTEGER NOT NULL,
            total_cost_usd REAL NOT NULL,
            total_savings_usd REAL NOT NULL,
            total_duration_ms INTEGER NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS ux_rollups_period ON rollups(period_start, period_type);

        CREATE TABLE IF NOT EXISTS discovered_categories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            confidence REAL NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
    "#;

    /// Applies the current schema inside one transaction. Every statement is idempotent, so this
    /// is safe to call on every `SqliteStore::open` regardless of whether the database is fresh
    /// or already up to date.
    pub fn apply_migrations(conn: &mut Connection) -> Result<(), StoreError> {
        let tx = conn.transaction().map_err(StoreError::Sqlite)?;
        tx.execute_batch(Self::SCHEMA).map_err(|e| StoreError::MigrationFailed { reason: e.to_string() })?;
        tx.commit().map_err(StoreError::Sqlite)?;
        Ok(())
    }
}
