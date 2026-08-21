use chrono::{DateTime, Utc};
use compact_str::CompactString;
use lumen_model::*;
use smallvec::SmallVec;
use std::io::BufRead;

use crate::adapter::{AdapterCapabilities, IngestionError, SessionAdapter};
use crate::fingerprint::detect_orchestrator;

pub struct AgyAdapter;

impl AgyAdapter {
    /// CRIT-LUMEN-165: resolves the real transcript path directly, bypassing the
    /// ~/.gemini/logs symlink farm.
    pub fn resolve_transcript_path(brain_root: &std::path::Path, conversation_id: &str) -> std::path::PathBuf {
        brain_root.join(conversation_id).join(".system_generated").join("logs").join("transcript.jsonl")
    }
}

impl SessionAdapter for AgyAdapter {
    fn name(&self) -> &'static str {
        "antigravity"
    }

    fn matches_fingerprint(&self, sample: &str) -> bool {
        // Delegates to detect_orchestrator (single source of truth for orchestrator precedence)
        // instead of an independently-coded condition that can drift.
        detect_orchestrator(sample.as_bytes()) == Some(OrchestratorKind::Antigravity)
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
        let session_id = CompactString::new("agy-session");
        // Bug 1 fix: AGY's real on-disk transcript.jsonl schema (step_index, source, type,
        // status, created_at, content, plus thinking/tool_calls[] on PLANNER_RESPONSE entries
        // -- confirmed against real fixtures) has no per-session model-name field anywhere.
        // Antigravity is a Gemini-lineage orchestrator, not Claude, so the old hardcoded
        // "claude-3-5-sonnet-20241022" actively lied about the model. There is no real signal
        // to extract, so this is an honest generic placeholder instead of a specific model
        // name we cannot verify. Pricing still falls back to Sonnet's rate via PricingTable's
        // existing CRIT-LUMEN-008 fallback for unrecognized model strings -- unchanged here.
        let model_family = CompactString::new("antigravity-unknown-model");
        let mut turns = Vec::new();
        let mut started_at = Utc::now();
        let mut ended_at = Utc::now();
        let mut has_start = false;

        // Bug 2 fix: a persistent (not per-turn) monotonic counter and FIFO queue so that
        // TOOL_RESULT entries correlate to the tool_call that produced them, instead of two
        // incompatible ad-hoc numbering schemes that essentially never matched.
        let mut next_call_id: usize = 0;
        let mut pending_call_ids: std::collections::VecDeque<CompactString> = std::collections::VecDeque::new();

        // Bug 3 fix: mirrors claude.rs (78f91cf) / codex.rs (f0826a4) -- enumerate lines and
        // track a running byte offset so malformed lines are recorded, not silently discarded.
        let mut parse_failures: SmallVec<[ParseFailureRecord; 2]> = SmallVec::new();
        let mut byte_offset: usize = 0;

        for (idx, line_res) in reader.lines().enumerate() {
            let line = match line_res {
                Ok(l) => l,
                Err(e) => return Err(IngestionError::Io(e)),
            };

            // LF-based approximation, same documented limitation as claude.rs/codex.rs.
            let line_start_offset = byte_offset;
            byte_offset += line.len() + 1;

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
                        byte_offset: line_start_offset,
                        error: CompactString::new(e.to_string()),
                    });
                    continue;
                }
            };

            if let Some(created_str) = val.get("created_at").and_then(|v| v.as_str()) {
                if let Ok(ts) = DateTime::parse_from_rfc3339(created_str) {
                    let utc_ts = ts.with_timezone(&Utc);
                    if !has_start {
                        started_at = utc_ts;
                        has_start = true;
                    }
                    ended_at = utc_ts;
                }
            }

            let step_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

            if step_type == "USER_INPUT" {
                turns.push(CanonicalTurn {
                    attribution: None,
                    turn_index: turns.len(),
                    role: TurnRole::User,
                    timestamp: ended_at,
                    latency_ms: 0,
                    text: val.get("content").and_then(|v| v.as_str()).map(Into::into),
                    tool_calls: SmallVec::new(),
                    tool_results: SmallVec::new(),
                    usage: None,
                });
            } else if step_type == "PLANNER_RESPONSE" {
                let thinking = val.get("thinking").and_then(|v| v.as_str()).map(Into::into);

                let mut tool_calls = SmallVec::new();
                if let Some(calls) = val.get("tool_calls").and_then(|c| c.as_array()) {
                    for call in calls {
                        let name = call.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let mut parsed_args = serde_json::Map::new();
                        if let Some(obj) = call.get("args").and_then(|a| a.as_object()) {
                            for (k, v) in obj {
                                if let Some(s) = v.as_str() {
                                    match serde_json::from_str::<serde_json::Value>(s) {
                                        Ok(inner) => {
                                            parsed_args.insert(k.clone(), inner);
                                        }
                                        Err(_) => {
                                            parsed_args.insert(k.clone(), v.clone());
                                        }
                                    }
                                } else {
                                    parsed_args.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        let args = serde_json::Value::Object(parsed_args);

                        let call_id = CompactString::new(format!("agy_call_{next_call_id}"));
                        next_call_id += 1;
                        pending_call_ids.push_back(call_id.clone());

                        tool_calls.push(CanonicalToolCall {
                            call_id,
                            tool_name: CompactString::new(name),
                            intent: ToolIntent::Other { raw_name: CompactString::new(name) },
                            raw_arguments: args,
                        });
                    }
                }

                turns.push(CanonicalTurn {
                    attribution: None,
                    turn_index: turns.len(),
                    role: TurnRole::Assistant,
                    timestamp: ended_at,
                    latency_ms: 0,
                    text: thinking,
                    tool_calls,
                    tool_results: SmallVec::new(),
                    usage: None,
                });
            } else if step_type == "TOOL_RESULT" {
                let content = val.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let is_error = val.get("status").and_then(|v| v.as_str()) == Some("ERROR");

                let call_id = pending_call_ids
                    .pop_front()
                    .unwrap_or_else(|| CompactString::new(format!("agy_unmatched_result_{}", turns.len())));

                let mut tool_results = SmallVec::new();
                tool_results.push(CanonicalToolResult {
                    call_id,
                    output_bytes: content.len(),
                    line_count: content.lines().count(),
                    is_error,
                    error_class: if is_error { Some(CompactString::new("ToolError")) } else { None },
                    truncated_output: None,
                    otel_span_id: None,
                });

                turns.push(CanonicalTurn {
                    attribution: None,
                    turn_index: turns.len(),
                    role: TurnRole::ToolResult,
                    timestamp: ended_at,
                    latency_ms: 0,
                    text: Some(content.to_string()),
                    tool_calls: SmallVec::new(),
                    tool_results,
                    usage: None,
                });
            }
        }

        let wall_duration_ms = (ended_at - started_at).num_milliseconds().max(0) as u64;

        Ok(CanonicalTranscript {
            session_id,
            parent_session_id: None,
            subagent_role: None,
            orchestrator: OrchestratorKind::Antigravity,
            model_family: model_family.clone(),
            timing: ExecutionTiming {
                started_at,
                ended_at,
                wall_duration_ms,
                active_duration_ms: wall_duration_ms,
                idle_duration_ms: 0,
                idle_gap_count: 0,
            },
            economics: TokenEconomics::calculate(&[], &model_family, &PricingTable::seed(), None),
            turns,
            subagents: Vec::new(),
            extracted_schemas: SmallVec::new(),
            otel_conversation_id: None,
            service_tier: None,
            parse_failures,
            detected_anomalies: SmallVec::new(),
        })
    }
}
