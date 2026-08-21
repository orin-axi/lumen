use chrono::Utc;
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
        sample.contains("\"choices\"") || sample.contains("\"prompt_tokens\"") || sample.contains("\"thread_id\"")
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
        let mut total_input_tokens = 0u64;
        let mut total_output_tokens = 0u64;

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

            if let Some(tid) = val.get("thread_id").and_then(|v| v.as_str()) {
                session_id = CompactString::new(tid);
            }

            // CRIT-LUMEN-110: cumulative sum across every usage-bearing line, not overwrite.
            if let Some(usage) = val.get("usage") {
                let prompt_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let completion_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                total_input_tokens += prompt_tokens;
                total_output_tokens += completion_tokens;
            }

            // CRIT-LUMEN-109: one CanonicalTurn per choices[] element.
            if let Some(choices) = val.get("choices").and_then(|v| v.as_array()) {
                for choice in choices {
                    let message = choice.get("message");
                    let role_str = message.and_then(|m| m.get("role")).and_then(|r| r.as_str()).unwrap_or("");
                    let role = if role_str == "assistant" { TurnRole::Assistant } else { TurnRole::User };
                    let text = message.and_then(|m| m.get("content")).and_then(|c| c.as_str()).map(|s| s.to_string());

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
            }
        }

        ended_at = Utc::now();
        let wall_duration = (ended_at - started_at).num_milliseconds().max(0) as u64;

        Ok(CanonicalTranscript {
            session_id,
            parent_session_id: None,
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
            economics: TokenEconomics::calculate(total_input_tokens, total_output_tokens, 0, 0, &model_family),
            turns,
            subagents: Vec::new(),
            extracted_schemas: SmallVec::new(),
            detected_anomalies: SmallVec::new(),
            otel_conversation_id: None,
            service_tier: None,
            parse_failures: SmallVec::new(),
        })
    }
}
