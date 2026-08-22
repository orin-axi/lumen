use lumen_model::CanonicalTranscript;
use std::io::BufRead;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IngestionError {
    #[error("I/O error reading session log: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unrecognized log format")]
    UnrecognizedFormat,
    /// OpenCode's real store is a SQLite database, not a JSONL line stream -- errors opening or
    /// querying it (via `OpenCodeAdapter::parse_database`) surface here rather than as `Io`.
    #[error("SQLite error reading session database: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AdapterCapabilities {
    pub has_token_usage: bool,
    pub has_tool_results: bool,
    pub has_shell_commands: bool,
    pub has_file_events: bool,
    pub has_lifecycle_hooks: bool,
    pub supports_incremental_offsets: bool,
    pub supports_cost_estimation: bool,
}

pub trait SessionAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn matches_fingerprint(&self, sample: &str) -> bool;
    fn capabilities(&self) -> AdapterCapabilities;
    fn parse_stream<'a>(&self, reader: Box<dyn BufRead + 'a>) -> Result<CanonicalTranscript, IngestionError>;
}
