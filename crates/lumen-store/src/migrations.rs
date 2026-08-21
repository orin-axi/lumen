use rusqlite::Connection;

use crate::error::StoreError;

pub struct MigrationManager;

impl MigrationManager {
    pub const MIGRATIONS: &'static [&'static str] = &[
        // V1: Ingestion queue, session snapshots, pipeline runs
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

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
        "#,
        // V2: Sessions and Token Usage
        r#"
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
            cost_usd REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_token_usage_session ON token_usage(session_id);
        "#,
        // V3: Tool calls and Command Events
        r#"
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
        "#,
        // V4: Findings and Category Scores
        r#"
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
        "#,
        // V5: Rollups and Discovered Categories
        r#"
        CREATE TABLE IF NOT EXISTS rollups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            period_start TIMESTAMP NOT NULL,
            period_type TEXT NOT NULL,
            session_count INTEGER NOT NULL,
            total_cost_usd REAL NOT NULL,
            total_savings_usd REAL NOT NULL,
            total_duration_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS discovered_categories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            confidence REAL NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        "#,
        // V6: Unique index on rollups(period_start, period_type) to support idempotent upsert
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS ux_rollups_period ON rollups(period_start, period_type);
        "#,
    ];

    pub fn apply_migrations(conn: &mut Connection) -> Result<usize, StoreError> {
        let tx = conn.transaction().map_err(StoreError::Sqlite)?;

        // Ensure migrations table exists
        tx.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
            );",
            [],
        )
        .map_err(StoreError::Sqlite)?;

        let mut applied_count = 0;

        for (idx, migration_sql) in Self::MIGRATIONS.iter().enumerate() {
            let version = idx + 1;

            let exists: bool = tx
                .query_row("SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)", [version], |row| {
                    row.get(0)
                })
                .unwrap_or(false);

            if !exists {
                tx.execute_batch(migration_sql)
                    .map_err(|e| StoreError::MigrationFailed { version, reason: e.to_string() })?;

                tx.execute("INSERT INTO schema_migrations (version) VALUES (?1)", [version])
                    .map_err(StoreError::Sqlite)?;

                applied_count += 1;
            }
        }

        tx.commit().map_err(StoreError::Sqlite)?;
        Ok(applied_count)
    }
}
