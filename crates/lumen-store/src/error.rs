use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("Database connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("SQLite database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Database schema setup failed: {reason}")]
    MigrationFailed { reason: String },

    #[error("Write operation attempted on read-only database store")]
    ReadOnlyViolation,

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Record not found: {0}")]
    NotFound(String),
}
