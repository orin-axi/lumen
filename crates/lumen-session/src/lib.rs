pub mod adapter;
pub mod adapters;
pub mod fingerprint;
pub mod snapshot;

pub use adapter::{AdapterCapabilities, IngestionError, SessionAdapter};
pub use adapters::{AgyAdapter, ClaudeCodeAdapter, CodexAdapter, OpenCodeAdapter};
pub use fingerprint::detect_orchestrator;
pub use snapshot::merge_precompact_snapshots;
