use chrono::{DateTime, Utc};
use compact_str::CompactString;
use lumen_model::{OrchestratorKind, TokenEconomics};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedSessionRow {
    pub id: i64,
    pub provider: String,
    pub session_id: String,
    pub source_path: String,
    pub source_hash: u64,
    pub retry_count: u32,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFactRecord {
    pub provider: String,
    pub provider_session_id: String,
    pub model_family: String,
    pub orchestrator: OrchestratorKind,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub wall_duration_ms: u64,
    pub turn_count: usize,
    pub economics: TokenEconomics,
    pub has_anomalies: bool,
    /// Per-tool-call facts for this session (CRIT-LUMEN-174). Persisted internally by
    /// `SessionRepository::upsert_session` via `ToolCallRepository`, same pattern as
    /// `economics.per_model` being persisted via `TokenUsageRepository` -- an empty `Vec` is
    /// valid (a session with no tool calls, or a caller that doesn't track them).
    pub tool_calls: Vec<ToolCallFactRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummaryReadModel {
    pub id: i64,
    pub provider: String,
    pub session_id: String,
    pub model_family: String,
    pub turn_count: usize,
    pub wall_duration_ms: u64,
    pub cache_hit_ratio: f32,
    pub total_cost_usd: f64,
    pub net_savings_usd: f64,
    /// See `lumen_model::TokenEconomics::is_fully_priced`. Persisted per CRIT-LUMEN-171 --
    /// previously lost on every store round-trip (never written by `upsert_session`, always
    /// read back hardcoded `true`), which meant a genuinely unpriced session displayed as an
    /// indistinguishable-from-real `$0.0000` everywhere the store's read models were used.
    pub is_fully_priced: bool,
    pub created_at: DateTime<Utc>,
}

impl SessionSummaryReadModel {
    /// See `lumen_model::TokenEconomics::cost`.
    pub fn cost(&self) -> lumen_model::Cost {
        if self.is_fully_priced {
            lumen_model::Cost::Priced(self.total_cost_usd)
        } else {
            lumen_model::Cost::Unpriced
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetailReadModel {
    pub summary: SessionSummaryReadModel,
    pub economics: TokenEconomics,
    pub tool_counts: BTreeMap<CompactString, usize>,
    pub error_counts: BTreeMap<CompactString, usize>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    pub provider: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingFactRecord {
    pub rule_id: String,
    pub severity: String,
    pub confidence: f32,
    pub title: String,
    pub message: String,
    pub evidence_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingReadModel {
    pub id: i64,
    pub session_id: String,
    pub rule_id: String,
    pub severity: String,
    pub confidence: f32,
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFactRecord {
    pub turn_index: usize,
    pub tool_name: String,
    pub call_id: String,
    pub intent_kind: String,
    pub is_error: bool,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallReadModel {
    pub id: i64,
    pub session_id: i64,
    pub turn_index: usize,
    pub tool_name: String,
    pub call_id: String,
    pub intent_kind: String,
    pub is_error: bool,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEventFactRecord {
    pub command_base: String,
    pub sanitized_args: Option<String>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEventReadModel {
    pub id: i64,
    pub session_id: i64,
    pub command_base: String,
    pub sanitized_args: Option<String>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupFactRecord {
    pub period_start: DateTime<Utc>,
    pub period_type: String,
    pub session_count: i64,
    pub total_cost_usd: f64,
    pub total_savings_usd: f64,
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupReadModel {
    pub id: i64,
    pub period_start: DateTime<Utc>,
    pub period_type: String,
    pub session_count: i64,
    pub total_cost_usd: f64,
    pub total_savings_usd: f64,
    pub total_duration_ms: u64,
}
