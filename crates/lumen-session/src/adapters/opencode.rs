use chrono::{DateTime, Utc};
use compact_str::CompactString;
use lumen_model::*;
use smallvec::SmallVec;
use std::io::BufRead;

use crate::adapter::{AdapterCapabilities, IngestionError, SessionAdapter};

pub struct OpenCodeAdapter;

impl SessionAdapter for OpenCodeAdapter {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn matches_fingerprint(&self, sample: &str) -> bool {
        sample.contains("\"action\":\"run\"")
            || sample.contains("\"action\": \"run\"")
            || sample.contains("\"observation\":")
            || sample.contains("\"action\":\"message\"")
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
        let session_id = CompactString::new("opencode-session");
        let model_family = CompactString::new("claude-3-5-sonnet-20241022");
        let mut turns = Vec::new();
        let mut started_at = Utc::now();
        let mut ended_at = Utc::now();
        let mut has_start = false;

        let mut current_input_tokens = 0u64;
        let mut current_output_tokens = 0u64;
        let mut current_cache_write = 0u64;
        let mut current_cache_read = 0u64;

        for line_res in reader.lines() {
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
                Err(_) => continue,
            };

            if let Some(ts_str) = val.get("timestamp").and_then(|v| v.as_str()) {
                if let Ok(ts) = DateTime::parse_from_rfc3339(ts_str) {
                    let utc_ts = ts.with_timezone(&Utc);
                    if !has_start {
                        started_at = utc_ts;
                        has_start = true;
                    }
                    ended_at = utc_ts;
                }
            }

            // Extract usage if present
            let mut turn_usage = None;
            if let Some(usage_obj) = val.get("metrics").or_else(|| val.get("usage")) {
                let in_tok = usage_obj
                    .get("accumulated_cost")
                    .or_else(|| usage_obj.get("input_tokens"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let out_tok = usage_obj.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let c_write = usage_obj.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let c_read = usage_obj.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

                if in_tok > 0 || out_tok > 0 {
                    turn_usage = Some(TurnTokenUsage {
                        input_tokens: in_tok,
                        cache_creation_tokens: c_write,
                        cache_read_tokens: c_read,
                        output_tokens: out_tok,
                    });
                    current_input_tokens += in_tok;
                    current_output_tokens += out_tok;
                    current_cache_write += c_write;
                    current_cache_read += c_read;
                }
            }

            let action = val.get("action").and_then(|v| v.as_str()).unwrap_or("");
            let source = val.get("source").and_then(|v| v.as_str()).unwrap_or("");

            if action == "message" {
                turns.push(CanonicalTurn {
                    attribution: None,
                    turn_index: turns.len(),
                    role: if source == "assistant" { TurnRole::Assistant } else { TurnRole::User },
                    timestamp: ended_at,
                    latency_ms: 0,
                    text: val.get("args").and_then(|a| a.get("content")).and_then(|c| c.as_str()).map(Into::into),
                    tool_calls: SmallVec::new(),
                    tool_results: SmallVec::new(),
                    usage: None,
                });
            } else if action == "run" || action == "read" || action == "edit" || action == "think" {
                let mut tool_calls = SmallVec::new();
                let args = val.get("args").cloned().unwrap_or(serde_json::Value::Null);

                let intent = match action {
                    "run" => {
                        let cmd = args.get("command").and_then(|c| c.as_str()).unwrap_or("");
                        if cmd.starts_with("git") {
                            ToolIntent::VersionControl { action: CompactString::new(cmd) }
                        } else if cmd.contains("test") {
                            ToolIntent::TestExecution { runner: CompactString::new("shell"), target_suite: None }
                        } else {
                            ToolIntent::Other { raw_name: CompactString::new("run") }
                        }
                    }
                    "read" => {
                        let path = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
                        ToolIntent::FileRead { path: CompactString::new(path), line_range: None }
                    }
                    "edit" => {
                        let path = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
                        ToolIntent::FileEdit { path: CompactString::new(path), lines_added: 0, lines_removed: 0 }
                    }
                    _ => ToolIntent::Other { raw_name: CompactString::new(action) },
                };

                tool_calls.push(CanonicalToolCall {
                    call_id: CompactString::new(format!("opencode_call_{}", turns.len())),
                    tool_name: CompactString::new(action),
                    intent,
                    raw_arguments: args,
                });

                turns.push(CanonicalTurn {
                    attribution: None,
                    turn_index: turns.len(),
                    role: TurnRole::Assistant,
                    timestamp: ended_at,
                    latency_ms: 0,
                    text: val.get("thought").and_then(|t| t.as_str()).map(Into::into),
                    tool_calls,
                    tool_results: SmallVec::new(),
                    usage: turn_usage,
                });
            } else if let Some(obs) = val.get("observation").and_then(|v| v.as_str()) {
                // Observation result
                let mut tool_results = SmallVec::new();
                let is_error = val.get("error").and_then(|e| e.as_bool()).unwrap_or(false);
                let content = val.get("content").and_then(|c| c.as_str()).unwrap_or("");

                tool_results.push(CanonicalToolResult {
                    call_id: CompactString::new(format!("opencode_call_{}", turns.len().saturating_sub(1))),
                    output_bytes: content.len(),
                    line_count: content.lines().count(),
                    is_error,
                    error_class: if is_error { Some(CompactString::new("ObservationError")) } else { None },
                    truncated_output: None,
                    otel_span_id: None,
                });

                turns.push(CanonicalTurn {
                    attribution: None,
                    turn_index: turns.len(),
                    role: TurnRole::ToolResult,
                    timestamp: ended_at,
                    latency_ms: 0,
                    text: Some(format!("Observation ({obs})")),
                    tool_calls: SmallVec::new(),
                    tool_results,
                    usage: None,
                });
            }
        }

        let total_prompt = current_input_tokens + current_cache_write + current_cache_read;
        let cache_hit_ratio =
            if total_prompt == 0 { 0.0 } else { (current_cache_read as f32 / total_prompt as f32) * 100.0 };

        let pricing = ModelPricing::from_model_name(&model_family);
        let total_cost = pricing.compute_cost(&TurnTokenUsage {
            input_tokens: current_input_tokens,
            cache_creation_tokens: current_cache_write,
            cache_read_tokens: current_cache_read,
            output_tokens: current_output_tokens,
        });

        let baseline_cost = pricing.compute_baseline_cost(&TurnTokenUsage {
            input_tokens: current_input_tokens,
            cache_creation_tokens: current_cache_write,
            cache_read_tokens: current_cache_read,
            output_tokens: current_output_tokens,
        });

        let economics = TokenEconomics {
            input_tokens: current_input_tokens,
            output_tokens: current_output_tokens,
            cache_creation_tokens: current_cache_write,
            cache_read_tokens: current_cache_read,
            ephemeral_5m_tokens: current_cache_write,
            ephemeral_1h_tokens: 0,
            cache_hit_ratio,
            total_cost_usd: total_cost,
            baseline_cost_no_cache_usd: baseline_cost,
            net_savings_usd: (baseline_cost - total_cost).max(0.0),
            efficiency_multiplier: if total_cost > 0.0 { (baseline_cost / total_cost) as f32 } else { 1.0 },
            per_model: std::collections::HashMap::new(),
        };

        let wall_duration = (ended_at - started_at).num_milliseconds().max(0) as u64;

        Ok(CanonicalTranscript {
            session_id,
            parent_session_id: None,
            subagent_role: None,
            orchestrator: OrchestratorKind::OpenCode,
            model_family,
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
            otel_conversation_id: None,
            service_tier: None,
            parse_failures: SmallVec::new(),
            subagents: Vec::new(),
            extracted_schemas: SmallVec::new(),
            detected_anomalies: SmallVec::new(),
        })
    }
}
