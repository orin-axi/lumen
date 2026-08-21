use chrono::{DateTime, Utc};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::economics::TokenEconomics;
use crate::schema::SchemaCitation;
use crate::turn::CanonicalTurn;

/// Universal canonical intermediate representation (IR) for an agent session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalTranscript {
    pub session_id: CompactString,
    pub parent_session_id: Option<CompactString>,
    /// Some(<worker>) for a transcript reached via `subagents`, where `<worker>` is the real
    /// path segment from the sibling `subagents/<worker>.jsonl` filename that produced it
    /// (SPEC-LUMEN-002-SESSION's CRIT-LUMEN-026). `None` for a root/non-subagent transcript.
    pub subagent_role: Option<CompactString>,
    pub orchestrator: OrchestratorKind,
    pub model_family: CompactString,
    pub timing: ExecutionTiming,
    pub economics: TokenEconomics,
    pub turns: Vec<CanonicalTurn>,
    pub subagents: Vec<CanonicalTranscript>,
    pub extracted_schemas: SmallVec<[SchemaCitation; 4]>,
    pub detected_anomalies: SmallVec<[TrajectoryAnomaly; 4]>,
    pub otel_conversation_id: Option<CompactString>,
    pub service_tier: Option<CompactString>,
    pub parse_failures: SmallVec<[ParseFailureRecord; 2]>,
}

/// A record of one JSONL line that failed to parse (corrupted, truncated, or non-UTF8),
/// surviving the `parse_stream` call so a caller can inspect it (CRIT-LUMEN-025).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParseFailureRecord {
    pub session_id: CompactString,
    pub line_number: usize,
    pub byte_offset: usize,
    pub error: CompactString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrchestratorKind {
    ClaudeCode,
    Antigravity,
    Codex,
    OpenCode,
    Kimi,
    GenericOtel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTiming {
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub wall_duration_ms: u64,
    pub active_duration_ms: u64,
    pub idle_duration_ms: u64,
    pub idle_gap_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrajectoryAnomaly {
    CircularLoop { symbol: CompactString, cycle_depth: usize },
    ContextFlood { turns: usize, uncompressed_tokens: u64 },
    GateStall { agent_pair: CompactString, observed_rounds: usize },
    UngroundedDrafting { missing_symbol: CompactString, target_file: CompactString },
}
