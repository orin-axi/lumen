pub mod adapter;
pub mod adapters;
pub mod fingerprint;
pub mod jsonl;
pub mod snapshot;

pub use adapter::{AdapterCapabilities, IngestionError, SessionAdapter, SessionSource};
pub use adapters::{AgyAdapter, ClaudeCodeAdapter, CodexAdapter, OpenCodeAdapter};
pub use fingerprint::detect_orchestrator;
pub use snapshot::merge_precompact_snapshots;
