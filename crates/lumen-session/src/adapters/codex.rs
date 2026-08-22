use chrono::{DateTime, Utc};
use compact_str::CompactString;
use lumen_model::*;
use smallvec::SmallVec;
use std::io::BufRead;

use crate::adapter::{AdapterCapabilities, IngestionError, SessionAdapter};
use crate::fingerprint::detect_orchestrator;

pub struct CodexAdapter;

impl SessionAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn matches_fingerprint(&self, sample: &str) -> bool {
        // CRIT-LUMEN-108: delegates to detect_orchestrator (single source of truth, including
        // precedence over earlier-checked orchestrators) instead of an independent condition.
        detect_orchestrator(sample.as_bytes()) == Some(OrchestratorKind::Codex)
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
        // Honest placeholder, not a real model name: "gpt-4o" was used here before, which was
        // harmless while every unrecognized model fell back to Sonnet's rate regardless. Now
        // that "gpt-4o" is itself a real seeded PricingTable row, defaulting to it would let a
        // session that never emits thread_settings_applied (unconfirmed against real data
        // whether this happens, but the failure mode is real) silently price as GPT-4o instead
        // of surfacing as unpriced. Same convention as AgyAdapter's "antigravity-unknown-model".
        let mut model_family = CompactString::new("codex-unknown-model");
        let mut started_at = Utc::now();
        let mut ended_at = started_at;
        let mut has_start = false;

        let mut turns = Vec::new();
        let mut parse_failures: SmallVec<[ParseFailureRecord; 2]> = SmallVec::new();
        let mut service_tier: Option<CompactString> = None;

        // CRIT-LUMEN-110: Codex token_count events carry cumulative running totals, not
        // per-line deltas -- these are last-write values, never summed.
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut reasoning_output_tokens = 0u64;
        // Bug 2: real Codex cache-token field names (cached_input_tokens /
        // cache_write_input_tokens) differ from Claude Code's (cache_read_input_tokens /
        // cache_creation_input_tokens) -- do not conflate the two adapters' vocabularies.
        let mut cache_read_tokens = 0u64;
        let mut cache_creation_tokens = 0u64;

        let mut byte_offset: usize = 0;

        for (idx, line_res) in reader.lines().enumerate() {
            let line = match line_res {
                Ok(l) => l,
                Err(e) => {
                    // CRIT-LUMEN-025: a non-UTF8 (or otherwise unreadable) line surfaces as an
                    // io::Error from BufRead::lines(), not a serde_json parse error -- treated
                    // the same as a corrupted-JSON line: skip + record, keep parsing. Same
                    // interpretation and byte_offset-non-advancement rationale as claude.rs.
                    parse_failures.push(ParseFailureRecord {
                        session_id: session_id.clone(),
                        line_number: idx + 1,
                        byte_offset,
                        error: CompactString::new(e.to_string()),
                    });
                    continue;
                }
            };

            // Offset at the START of the line currently being processed. `reader.lines()`
            // strips newlines and discards byte-position info, so we track it manually: this
            // is an LF-based approximation (`+1` per line) and will undercount by 1 byte per
            // line for CRLF-terminated input -- an acceptable known limitation for a
            // diagnostic field, not a byte-exact file-seek requirement. Same pattern as
            // claude.rs (commit 78f91cf).
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

            // Dispatch on the envelope's top-level type. Both event_msg and response_item are
            // recognized as valid Codex content by fingerprint detection (detect_orchestrator /
            // CodexAdapter::matches_fingerprint), but only event_msg's payload shapes are
            // understood by this parser today.
            match val.get("type").and_then(|v| v.as_str()) {
                Some("event_msg") => {}
                Some("response_item") => {
                    // Recognized by fingerprint but not yet implemented: there is no
                    // confirmed, real-data-verified schema for response_item's internal
                    // payload, so we do not guess at its fields. Record the skip as visible
                    // signal rather than silently dropping the line -- a transcript with zero
                    // turns must not look like a clean, successful empty parse.
                    parse_failures.push(ParseFailureRecord {
                        session_id: session_id.clone(),
                        line_number: idx + 1,
                        byte_offset: line_start_offset,
                        error: CompactString::new(
                            "response_item envelope recognized by fingerprint but not yet implemented by CodexAdapter::parse_stream",
                        ),
                    });
                    continue;
                }
                _ => continue,
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
                    // Bug 3: no real item shape has a top-level `text` field.
                    // UserMessage/AgentMessage carry an array of content blocks under
                    // `content`, each with its own `text`; multiple blocks are joined with a
                    // newline. CommandExecution has no text/content field at all -- its real
                    // signal is the `command` array (a shell argv list), joined with spaces
                    // into a readable representation. Reasoning's `summary_text`/`raw_content`
                    // were empty arrays in every real occurrence observed; their real populated
                    // shape is unconfirmed, so `text` is deliberately left `None` rather than
                    // guessed at.
                    let text = match item_type {
                        "UserMessage" | "AgentMessage" => {
                            item.and_then(|i| i.get("content")).and_then(|c| c.as_array()).map(|blocks| {
                                blocks
                                    .iter()
                                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                        }
                        "CommandExecution" => item
                            .and_then(|i| i.get("command"))
                            .and_then(|c| c.as_array())
                            .map(|argv| argv.iter().filter_map(|a| a.as_str()).collect::<Vec<_>>().join(" ")),
                        _ => None,
                    };

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
                    // Bug 1: real payloads nest total_token_usage one level deeper, under
                    // payload.info.total_token_usage -- not directly on payload.
                    if let Some(usage) = payload.get("info").and_then(|i| i.get("total_token_usage")) {
                        input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(input_tokens);
                        output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(output_tokens);
                        reasoning_output_tokens = usage
                            .get("reasoning_output_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(reasoning_output_tokens);
                        // Bug 2: real Codex cache field names, distinct from Claude Code's.
                        cache_read_tokens =
                            usage.get("cached_input_tokens").and_then(|v| v.as_u64()).unwrap_or(cache_read_tokens);
                        cache_creation_tokens = usage
                            .get("cache_write_input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(cache_creation_tokens);
                    }
                    // No total_token_usage on this payload: leave the running totals as-is.
                }
                Some("thread_settings_applied") => {
                    if let Some(ts) = payload.get("thread_settings") {
                        service_tier = ts.get("service_tier").and_then(|v| v.as_str()).map(CompactString::new);
                        // Bug 4: the adjacent, equally-real `model` field was never read.
                        if let Some(model) = ts.get("model").and_then(|v| v.as_str()) {
                            model_family = CompactString::new(model);
                        }
                    }
                }
                _ => {}
            }
        }

        let wall_duration = (ended_at - started_at).num_milliseconds().max(0) as u64;

        let pricing_input = TurnPricingInput {
            usage: TurnTokenUsage {
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens,
                reasoning_tokens: reasoning_output_tokens,
                cache_creation_1h_tokens: 0,
            },
            timestamp: ended_at,
            tier: service_tier.clone(),
        };

        let economics = TokenEconomics::calculate(&[pricing_input], &model_family, &pricing::SEEDED, None);

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
