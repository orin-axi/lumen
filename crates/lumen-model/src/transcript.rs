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
    /// `Some(<worker>)` for a transcript reached via `subagents`, where `<worker>` is the real
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

impl CanonicalTranscript {
    /// Sums this transcript's own `economics` with every `subagents` entry's (recursively
    /// rolled-up) economics. `economics` alone is root-only: each `SessionAdapter` builds it
    /// from only its own `turns`, and nothing sums a subagent's spend into its parent -- so a
    /// session that delegates heavily via subagents silently under-reports `total_cost_usd` if
    /// a caller reads `economics` directly. Callers that want "this session's full economic
    /// picture" (`cmd_ingest`'s persisted record, `cmd_audit`'s displayed total) must call this
    /// instead of reading `economics` directly; callers that specifically want only the root
    /// transcript's own turns (e.g. rendering just its own trajectory) should keep using
    /// `economics` as-is.
    ///
    /// `provided_cost_usd` (a provider-reported figure, when present) is dropped in the rolled-up
    /// result rather than summed or picked from one side -- a provider only ever reports a figure
    /// for the transcript it was attached to, and fabricating a combined one would misattribute
    /// it. `is_fully_priced` is the AND of every transcript in the tree, matching its existing
    /// meaning ("the whole reported total is trustworthy") -- one unpriced subagent model makes
    /// the combined total just as untrustworthy as an unpriced root model.
    pub fn rolled_up_economics(&self) -> TokenEconomics {
        let mut rolled = self.economics.clone();
        rolled.provided_cost_usd = None;

        for subagent in &self.subagents {
            let child = subagent.rolled_up_economics();

            rolled.input_tokens += child.input_tokens;
            rolled.output_tokens += child.output_tokens;
            rolled.cache_creation_tokens += child.cache_creation_tokens;
            rolled.cache_read_tokens += child.cache_read_tokens;
            rolled.ephemeral_5m_tokens += child.ephemeral_5m_tokens;
            rolled.ephemeral_1h_tokens += child.ephemeral_1h_tokens;
            rolled.total_cost_usd += child.total_cost_usd;
            rolled.baseline_cost_no_cache_usd += child.baseline_cost_no_cache_usd;
            rolled.reasoning_output_tokens += child.reasoning_output_tokens;
            rolled.is_fully_priced = rolled.is_fully_priced && child.is_fully_priced;

            for (model, child_summary) in child.per_model {
                rolled
                    .per_model
                    .entry(model)
                    .and_modify(|existing| {
                        existing.input_tokens += child_summary.input_tokens;
                        existing.output_tokens += child_summary.output_tokens;
                        existing.cache_creation_tokens += child_summary.cache_creation_tokens;
                        existing.cache_read_tokens += child_summary.cache_read_tokens;
                        existing.reasoning_tokens += child_summary.reasoning_tokens;
                        existing.cost_usd += child_summary.cost_usd;
                        existing.turns += child_summary.turns;
                        existing.is_fully_priced = existing.is_fully_priced && child_summary.is_fully_priced;
                    })
                    .or_insert(child_summary);
            }
        }

        let prompt_total = rolled.input_tokens + rolled.cache_creation_tokens + rolled.cache_read_tokens;
        rolled.cache_hit_ratio =
            if prompt_total > 0 { (rolled.cache_read_tokens as f32 / prompt_total as f32) * 100.0 } else { 0.0 };
        rolled.net_savings_usd = (rolled.baseline_cost_no_cache_usd - rolled.total_cost_usd).max(0.0);
        rolled.efficiency_multiplier = if rolled.total_cost_usd > 0.0 {
            (rolled.baseline_cost_no_cache_usd / rolled.total_cost_usd) as f32
        } else {
            1.0
        };

        rolled
    }
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

impl Default for OrchestratorKind {
    /// Arbitrary but stable choice for `#[derive(Default)]`-adjacent call sites (e.g.
    /// `SessionFactRecord::default()`) that need *a* variant, not a semantically meaningful one.
    fn default() -> Self {
        Self::ClaudeCode
    }
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
