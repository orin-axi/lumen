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
    pub created_at: DateTime<Utc>,
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
