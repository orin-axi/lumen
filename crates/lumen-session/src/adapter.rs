use lumen_model::CanonicalTranscript;
use std::io::BufRead;
use std::path::Path;
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
    /// A `SessionAdapter::load` call was given a `SessionSource` variant this adapter doesn't
    /// support -- e.g. a `Database` source handed to a stream-based adapter, or vice versa
    /// (CRIT-LUMEN-180). A caller hits this only by constructing the wrong `SessionSource`
    /// variant for a given adapter; `lumen-cli`'s `load_sessions` picks the right one via
    /// `detect_orchestrator` before calling `load`, so this should never fire in practice --
    /// it exists so a caller that does get it wrong sees a clear error, not a panic.
    #[error("{adapter} adapter does not support a {source_kind} source")]
    UnsupportedSourceKind { adapter: &'static str, source_kind: &'static str },
}

/// The physical shape of a session data source -- CRIT-LUMEN-180. Every known adapter needs
/// exactly one of these two shapes: a one-file-to-one-session JSONL line stream (`ClaudeCode`,
/// `Codex`, `Antigravity`), or a one-file-to-many-sessions SQLite database (`OpenCode`). Exists
/// so `SessionAdapter::load` can be one method every adapter implements, letting a caller
/// dispatch through `&dyn SessionAdapter` uniformly instead of a per-orchestrator special case
/// for "how do I even call this adapter" -- previously `OpenCodeAdapter` couldn't implement
/// `SessionAdapter` at all, because its `parse_database(&Path)` was structurally incompatible
/// with `parse_stream(Box<dyn BufRead>)`.
pub enum SessionSource<'a> {
    Stream(Box<dyn BufRead + 'a>),
    Database(&'a Path),
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
    /// Loads every real session found in `source` (CRIT-LUMEN-180) -- the single entry point
    /// across both source shapes `SessionSource` covers. A stream-based adapter's
    /// implementation always returns exactly one transcript (wrapping its own `parse_stream`
    /// inherent method's result); `OpenCodeAdapter`'s may return several, since one real
    /// database commonly holds many real sessions. Returns
    /// `IngestionError::UnsupportedSourceKind` when given the source shape this adapter doesn't
    /// support.
    fn load(&self, source: SessionSource) -> Result<Vec<CanonicalTranscript>, IngestionError>;
}
