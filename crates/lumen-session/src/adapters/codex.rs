use chrono::{DateTime, Utc};
use compact_str::CompactString;
use lumen_model::*;
use smallvec::SmallVec;
use std::io::BufRead;

use crate::adapter::{AdapterCapabilities, IngestionError, SessionAdapter};

pub struct CodexAdapter;

impl SessionAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn matches_fingerprint(&self, sample: &str) -> bool {
        // CRIT-LUMEN-108: must agree with detect_orchestrator's Codex branch exactly.
        sample.contains("\"type\":\"event_msg\"") || sample.contains("\"type\":\"response_item\"")
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            has_token_usage: true,
            has_tool_results: true,
            has_shell_commands: true,
            has_file_events: true,
            has_lifecycle_hooks: false,
            supports_incremental_offsets: false,
            supports_cost_estimation: true,
        }
    }

    fn parse_stream<'a>(&self, reader: Box<dyn BufRead + 'a>) -> Result<CanonicalTranscript, IngestionError> {
        let mut session_id = CompactString::new("codex-session");
        let model_family = CompactString::new("gpt-4o");
        let started_at = Utc::now();
        let mut ended_at = started_at;

        let mut turns = Vec::new();
        let mut parse_failures: SmallVec<[ParseFailureRecord; 2]> = SmallVec::new();
        let mut service_tier: Option<CompactString> = None;

        // CRIT-LUMEN-110: Codex token_count events carry cumulative running totals, not
        // per-line deltas -- these are last-write values, never summed.
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut reasoning_output_tokens = 0u64;

        for (idx, line_res) in reader.lines().enumerate() {
            let line = match line_res {
                Ok(l) => l,
                Err(e) => return Err(IngestionError::Io(e)),
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let val: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    parse_failures.push(ParseFailureRecord {
                        session_id: session_id.clone(),
                        line_number: idx + 1,
                        byte_offset: 0,
                        error: CompactString::new(e.to_string()),
                    });
                    continue;
                }
            };

            if let Some(ts_str) = val.get("timestamp").and_then(|v| v.as_str()) {
                if let Ok(ts) = DateTime::parse_from_rfc3339(ts_str) {
                    ended_at = ts.with_timezone(&Utc);
                }
            }

            // Only event_msg envelope lines carry the payload shapes this adapter understands.
            if val.get("type").and_then(|v| v.as_str()) != Some("event_msg") {
                continue;
            }

            let Some(payload) = val.get("payload") else {
                continue;
            };

            match payload.get("type").and_then(|v| v.as_str()) {
                Some("item_completed") => {
                    if let Some(tid) = payload.get("thread_id").and_then(|v| v.as_str()) {
                        session_id = CompactString::new(tid);
                    }

                    let item = payload.get("item");
                    let item_type = item.and_then(|i| i.get("type")).and_then(|t| t.as_str()).unwrap_or("");
                    let role = match item_type {
                        "UserMessage" => TurnRole::User,
                        "AgentMessage" => TurnRole::Assistant,
                        "CommandExecution" | "Reasoning" => TurnRole::ToolResult,
                        _ => TurnRole::System,
                    };
                    let text = item.and_then(|i| i.get("text")).and_then(|t| t.as_str()).map(|s| s.to_string());

                    turns.push(CanonicalTurn {
                        attribution: None,
                        turn_index: turns.len(),
                        role,
                        timestamp: ended_at,
                        latency_ms: 0,
                        text,
                        tool_calls: SmallVec::new(),
                        tool_results: SmallVec::new(),
                        usage: None,
                    });
                }
                Some("token_count") => {
                    if let Some(usage) = payload.get("total_token_usage") {
                        input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(input_tokens);
                        output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(output_tokens);
                        reasoning_output_tokens = usage
                            .get("reasoning_output_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(reasoning_output_tokens);
                    }
                    // No total_token_usage on this payload: leave the running totals as-is.
                }
                Some("thread_settings_applied") => {
                    service_tier = payload
                        .get("thread_settings")
                        .and_then(|ts| ts.get("service_tier"))
                        .and_then(|v| v.as_str())
                        .map(CompactString::new);
                }
                _ => {}
            }
        }

        let wall_duration = (ended_at - started_at).num_milliseconds().max(0) as u64;

        let pricing_input = TurnPricingInput {
            usage: TurnTokenUsage {
                input_tokens,
                output_tokens,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
            timestamp: ended_at,
            tier: service_tier.clone(),
        };

        let mut economics =
            TokenEconomics::calculate(&[pricing_input], &model_family, &PricingTable::seed(), None);
        // TurnPricingInput/TurnTokenUsage carry no reasoning-tokens field, so this last-write
        // value (CRIT-LUMEN-110) is applied to the computed economics afterward.
        economics.reasoning_output_tokens = reasoning_output_tokens;

        Ok(CanonicalTranscript {
            session_id,
            parent_session_id: None,
            subagent_role: None,
            orchestrator: OrchestratorKind::Codex,
            model_family: model_family.clone(),
            timing: ExecutionTiming {
                started_at,
                ended_at,
                wall_duration_ms: wall_duration,
                active_duration_ms: wall_duration,
                idle_duration_ms: 0,
                idle_gap_count: 0,
            },
            economics,
            turns,
            subagents: Vec::new(),
            extracted_schemas: SmallVec::new(),
            detected_anomalies: SmallVec::new(),
            otel_conversation_id: None,
            service_tier,
            parse_failures,
        })
    }
}
