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

/// CRIT-LUMEN-172 unknown-model contract: when a `SessionAdapter` implementation cannot
/// determine a real model identity for a session -- the source data never carries a
/// recognizable model field, or the adapter's model-family variable defaults before any is
/// found -- `CanonicalTranscript::model_family` MUST be set to an honest, provider-specific
/// placeholder that `lumen_model::PricingTable::is_recognized` returns `false` for (equivalently:
/// `TokenEconomics.is_fully_priced` must come out `false`), by the convention
/// `"<provider>-unknown-model"` (e.g. `"claude-code-unknown-model"`,
/// `"codex-unknown-model"`, `"antigravity-unknown-model"`, `"opencode-unknown-model"`).
///
/// It must NEVER be a real, currently-seeded model name (including another adapter's, or an
/// older/different model this same provider has published elsewhere) -- doing so silently
/// mis-prices the session at that real model's rate instead of surfacing it as explicitly
/// unpriced. This is not optional/best-effort: every implementation of this trait, including
/// ones added in the future, must uphold it.
///
/// This is machine-checked, not just documented here: see
/// `lumen-session/tests/test_adapter_unknown_model_contract.rs`'s shared golden test, which
/// exercises every known adapter (including `OpenCodeAdapter`, which implements its own
/// `parse_database` rather than this trait -- see its doc comment) against a minimal input with
/// no model field and asserts the contract holds. A new adapter should add itself to that test.
pub trait SessionAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn matches_fingerprint(&self, sample: &str) -> bool;
    fn capabilities(&self) -> AdapterCapabilities;
    fn parse_stream<'a>(&self, reader: Box<dyn BufRead + 'a>) -> Result<CanonicalTranscript, IngestionError>;
}
