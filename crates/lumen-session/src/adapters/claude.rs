use chrono::{DateTime, Utc};
use compact_str::CompactString;
use lumen_model::*;
use smallvec::SmallVec;
use std::io::BufRead;

use crate::adapter::{AdapterCapabilities, IngestionError, SessionAdapter};
use crate::fingerprint::detect_orchestrator;

pub struct ClaudeCodeAdapter;

impl SessionAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn matches_fingerprint(&self, sample: &str) -> bool {
        // Delegates to detect_orchestrator (the single source of truth for orchestrator
        // precedence) instead of maintaining an independently-coded condition that can drift.
        detect_orchestrator(sample.as_bytes()) == Some(OrchestratorKind::ClaudeCode)
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            has_token_usage: true,
            has_tool_results: true,
            has_shell_commands: true,
            has_file_events: true,
            has_lifecycle_hooks: true,
            supports_incremental_offsets: true,
            supports_cost_estimation: true,
        }
    }

    fn parse_stream<'a>(&self, reader: Box<dyn BufRead + 'a>) -> Result<CanonicalTranscript, IngestionError> {
        let mut session_id = CompactString::new("unknown");
        let mut model_family = CompactString::new("claude-3-5-sonnet-20241022");
        let mut turns = Vec::new();
        let mut started_at = Utc::now();
        let mut ended_at = Utc::now();
        let mut has_start = false;

        let mut total_input: u64 = 0;
        let mut total_output: u64 = 0;
        let mut total_cache_creation: u64 = 0;
        let mut total_cache_read: u64 = 0;
        let mut parse_failures: SmallVec<[ParseFailureRecord; 2]> = SmallVec::new();
        let mut otel_conversation_id: Option<CompactString> = None;

        // costUSD is Claude Code's own reported cost, wired through TokenEconomics::calculate's
        // provided_cost_usd twin-field (CRIT-LUMEN-160) for drift detection against the
        // independently-computed total_cost_usd. It's unclear whether costUSD is a per-message
        // delta or a cumulative running total, so treated last-write-wins, same as OpenCode's
        // accumulated_cost (commit c87e801) and Codex's token_count.
        let mut last_cost_usd: Option<f64> = None;

        let mut byte_offset: usize = 0;

        'lines: for (idx, line_res) in reader.lines().enumerate() {
            let line = match line_res {
                Ok(l) => l,
                Err(e) => {
                    // CRIT-LUMEN-025: `BufRead::lines()` surfaces a non-UTF8 (or otherwise
                    // unreadable) line as an `io::Error`, not a serde_json parse error -- but
                    // the criterion treats "corrupted, truncated, or non-UTF8 lines" the same
                    // way regardless of which stage rejected them: skip the line, record a
                    // parse-failure entry, keep parsing. There is no way to distinguish "this
                    // one line had bad bytes" from "the whole underlying reader is broken" at
                    // this type level -- `lines()` yields the same `io::Error` shape for both --
                    // so every line-read error is treated as a skippable bad line, matching the
                    // criterion's literal wording.
                    //
                    // The line's true byte length is unknown (it never became a `String`), so
                    // `byte_offset` is deliberately NOT advanced for this iteration -- the next
                    // successfully-read line's reported offset will undercount by this line's
                    // real length. This is a documented limitation of an already-approximate
                    // diagnostic field (see the LF-based `+1` approximation below), not a
                    // byte-exact guarantee.
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
            // diagnostic field, not a byte-exact file-seek requirement.
            let line_start_offset = byte_offset;
            byte_offset += line.len() + 1;

            let trimmed = line.trim();
            let clean_line = trimmed.strip_prefix('\u{FEFF}').unwrap_or(trimmed).trim();
            if clean_line.is_empty() {
                continue;
            }

            let val: serde_json::Value = match serde_json::from_str(clean_line) {
                Ok(v) => v,
                Err(e) => {
                    // Skip corrupted or incomplete JSON lines gracefully, but record the
                    // failure so it survives the parse_stream call (CRIT-LUMEN-025).
                    parse_failures.push(ParseFailureRecord {
                        session_id: session_id.clone(),
                        line_number: idx + 1,
                        byte_offset: line_start_offset,
                        error: CompactString::new(e.to_string()),
                    });
                    continue;
                }
            };

            if !val.is_object() {
                continue;
            }

            // CRIT-LUMEN-163: an explicit JSON null on one of these known top-level fields is
            // malformed, not a valid empty/zero value -- reject the whole line rather than
            // silently treating the null the same as an absent field.
            for field in ["id", "cwd", "sessionId", "requestId", "version", "costUSD"] {
                if val.get(field).map(|v| v.is_null()).unwrap_or(false) {
                    parse_failures.push(ParseFailureRecord {
                        session_id: session_id.clone(),
                        line_number: idx + 1,
                        byte_offset: line_start_offset,
                        error: CompactString::new(format!("explicit null on field '{field}'")),
                    });
                    continue 'lines;
                }
            }

            // CRIT-LUMEN-163 (nested paths): model and the two cache token fields are actually
            // read from nested paths (message.model, message.usage.cache_*_input_tokens), not
            // the entry's top level -- check them there. Only meaningful when a `message`
            // object is present at all; a missing message object is a different, already
            // handled case (no usage/model to misparse).
            if let Some(message) = val.get("message").and_then(|m| m.as_object()) {
                if message.get("model").map(|v| v.is_null()).unwrap_or(false) {
                    parse_failures.push(ParseFailureRecord {
                        session_id: session_id.clone(),
                        line_number: idx + 1,
                        byte_offset: line_start_offset,
                        error: CompactString::new("explicit null on field 'message.model'"),
                    });
                    continue 'lines;
                }

                if let Some(usage) = message.get("usage").and_then(|u| u.as_object()) {
                    for field in ["cache_creation_input_tokens", "cache_read_input_tokens"] {
                        if usage.get(field).map(|v| v.is_null()).unwrap_or(false) {
                            parse_failures.push(ParseFailureRecord {
                                session_id: session_id.clone(),
                                line_number: idx + 1,
                                byte_offset: line_start_offset,
                                error: CompactString::new(format!("explicit null on field 'message.usage.{field}'")),
                            });
                            continue 'lines;
                        }
                    }
                }
            }

            // CRIT-LUMEN-026: entries with isSidechain=true belong to a subagent/parallel
            // branch, distinct from the main chain -- exclude them from the main transcript's
            // turns entirely. Real Claude Code session field is camelCase "isSidechain"
            // (verified against 31 real local ~/.claude/projects/*/*.jsonl transcripts;
            // snake_case "is_sidechain" never occurs in any real file).
            let is_sidechain = val.get("isSidechain").and_then(|v| v.as_bool()).unwrap_or(false);
            if is_sidechain {
                continue;
            }

            // CRIT-LUMEN-164: otel_conversation_id is set from the first non-sidechain entry's
            // requestId encountered in file order, and never overwritten thereafter.
            if otel_conversation_id.is_none() {
                if let Some(rid) = val.get("requestId").and_then(|v| v.as_str()) {
                    otel_conversation_id = Some(CompactString::new(rid));
                }
            }

            // costUSD's explicit-null case was already rejected (and the line skipped) by the
            // CRIT-LUMEN-163 null-guard above; here we only need to handle a real number or an
            // absent field.
            if let Some(cost) = val.get("costUSD").and_then(|v| v.as_f64()) {
                last_cost_usd = Some(cost);
            }

            // Extract session ID
            if let Some(s) = val.get("sessionId").and_then(|v| v.as_str()) {
                if session_id == "unknown" {
                    session_id = CompactString::new(s);
                }
            }

            // Extract timestamp from message or attachment
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

            let line_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

            if line_type == "assistant" {
                if let Some(message) = val.get("message").and_then(|m| m.as_object()) {
                    if let Some(m) = message.get("model").and_then(|v| v.as_str()) {
                        if !m.starts_with("<synthetic>") {
                            model_family = CompactString::new(m);
                        }
                    }

                    // Extract token usage
                    let mut turn_usage = None;
                    if let Some(u) = message.get("usage").and_then(|u| u.as_object()) {
                        let in_tok = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let out_tok = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let cache_write = u.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                        let cache_read = u.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

                        total_input += in_tok;
                        total_output += out_tok;
                        total_cache_creation += cache_write;
                        total_cache_read += cache_read;

                        turn_usage = Some(TurnTokenUsage {
                            input_tokens: in_tok,
                            output_tokens: out_tok,
                            cache_creation_tokens: cache_write,
                            cache_read_tokens: cache_read,
                            reasoning_tokens: 0,
                        });
                    }

                    // Extract tool calls and text
                    let mut tool_calls = SmallVec::new();
                    let mut turn_text = None;

                    if let Some(contents) = message.get("content").and_then(|c| c.as_array()) {
                        for block in contents {
                            if let Some(block_obj) = block.as_object() {
                                let block_type = block_obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                if block_type == "tool_use" {
                                    let id = block_obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                    let name = block_obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                    if !id.is_empty() || !name.is_empty() {
                                        let input = block_obj.get("input").cloned().unwrap_or(serde_json::Value::Null);
                                        let intent = parse_tool_intent(name, &input);

                                        tool_calls.push(CanonicalToolCall {
                                            call_id: CompactString::new(id),
                                            tool_name: CompactString::new(name),
                                            intent,
                                            raw_arguments: input,
                                        });
                                    }
                                } else if block_type == "text" {
                                    if let Some(txt) = block_obj.get("text").and_then(|v| v.as_str()) {
                                        turn_text = Some(txt.to_string());
                                    }
                                }
                            }
                        }
                    } else if let Some(txt) = message.get("content").and_then(|c| c.as_str()) {
                        turn_text = Some(txt.to_string());
                    }

                    // Only push if turn has substantive content, tools, or usage
                    if turn_usage.is_some() || !tool_calls.is_empty() || turn_text.is_some() {
                        turns.push(CanonicalTurn {
                            attribution: None,
                            turn_index: turns.len(),
                            role: TurnRole::Assistant,
                            timestamp: ended_at,
                            latency_ms: 0,
                            text: turn_text,
                            tool_calls,
                            tool_results: SmallVec::new(),
                            usage: turn_usage,
                        });
                    }
                }
            } else if line_type == "user" {
                let mut tool_results = SmallVec::new();
                let mut user_text = None;

                if let Some(msg) = val.get("message") {
                    if let Some(txt) = msg.get("content").and_then(|c| c.as_str()) {
                        user_text = Some(txt.to_string());
                    } else if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
                        for b in arr {
                            let block_type = b.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            if block_type == "tool_result" {
                                let tool_use_id = b.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                                let is_error = b.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                                let content_str = b.get("content").and_then(|v| v.as_str()).unwrap_or("");
                                tool_results.push(CanonicalToolResult {
                                    call_id: CompactString::new(tool_use_id),
                                    output_bytes: content_str.len(),
                                    line_count: content_str.lines().count(),
                                    is_error,
                                    error_class: if is_error { Some(CompactString::new("ToolError")) } else { None },
                                    truncated_output: None,
                                    otel_span_id: None,
                                });
                            } else if block_type == "text" {
                                if let Some(txt) = b.get("text").and_then(|t| t.as_str()) {
                                    user_text = Some(txt.to_string());
                                }
                            }
                        }
                    }
                } else if let Some(txt) = val.get("text").and_then(|v| v.as_str()) {
                    user_text = Some(txt.to_string());
                }

                if !tool_results.is_empty() {
                    turns.push(CanonicalTurn {
                        attribution: None,
                        turn_index: turns.len(),
                        role: TurnRole::ToolResult,
                        timestamp: ended_at,
                        latency_ms: 0,
                        text: None,
                        tool_calls: SmallVec::new(),
                        tool_results,
                        usage: None,
                    });
                } else if let Some(text) = user_text {
                    turns.push(CanonicalTurn {
                        attribution: None,
                        turn_index: turns.len(),
                        role: TurnRole::User,
                        timestamp: ended_at,
                        latency_ms: 0,
                        text: Some(text),
                        tool_calls: SmallVec::new(),
                        tool_results: SmallVec::new(),
                        usage: None,
                    });
                }
            }
        }

        let wall_duration_ms = (ended_at - started_at).num_milliseconds().max(0) as u64;

        let economics = TokenEconomics::calculate(
            &[TurnPricingInput {
                usage: TurnTokenUsage {
                    input_tokens: total_input,
                    output_tokens: total_output,
                    cache_creation_tokens: total_cache_creation,
                    cache_read_tokens: total_cache_read,
                    reasoning_tokens: 0,
                },
                timestamp: ended_at,
                tier: None,
            }],
            &model_family,
            &pricing::SEEDED,
            last_cost_usd,
        );

        Ok(CanonicalTranscript {
            session_id,
            parent_session_id: None,
            subagent_role: None,
            orchestrator: OrchestratorKind::ClaudeCode,
            model_family,
            timing: ExecutionTiming {
                started_at,
                ended_at,
                wall_duration_ms,
                active_duration_ms: wall_duration_ms,
                idle_duration_ms: 0,
                idle_gap_count: 0,
            },
            economics,
            turns,
            subagents: Vec::new(),
            extracted_schemas: SmallVec::new(),
            detected_anomalies: SmallVec::new(),
            otel_conversation_id,
            service_tier: None,
            parse_failures,
        })
    }
}

impl ClaudeCodeAdapter {
    /// Parses a session's main transcript file plus any sibling `subagents/<worker>.jsonl`
    /// files found alongside it, linking each as a child transcript via `parent_session_id`
    /// and `subagent_role` (CRIT-LUMEN-026). A missing `subagents/` directory, or a
    /// `subagents/<worker>.jsonl` file with no corresponding `isSidechain` entries in the
    /// main file, is not an error -- it is simply linked (or, for a missing directory, simply
    /// absent) without failing the parse.
    pub fn parse_session_with_subagents(
        &self,
        session_dir: &std::path::Path,
        main_file_name: &str,
    ) -> Result<CanonicalTranscript, IngestionError> {
        let main_path = session_dir.join(main_file_name);
        let main_file = std::fs::File::open(&main_path).map_err(IngestionError::Io)?;
        let mut transcript = self.parse_stream(Box::new(std::io::BufReader::new(main_file)))?;

        let subagents_dir = session_dir.join("subagents");
        if let Ok(entries) = std::fs::read_dir(&subagents_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let worker = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
                let file = std::fs::File::open(&path).map_err(IngestionError::Io)?;
                let mut child = self.parse_stream(Box::new(std::io::BufReader::new(file)))?;
                child.parent_session_id = Some(transcript.session_id.clone());
                child.subagent_role = Some(CompactString::new(worker));
                transcript.subagents.push(child);
            }
        }

        Ok(transcript)
    }
}

fn parse_tool_intent(name: &str, input: &serde_json::Value) -> ToolIntent {
    match name {
        "view_file" | "Read" | "read_file" => {
            let path =
                input.get("AbsolutePath").or_else(|| input.get("file_path")).and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::FileRead { path: CompactString::new(path), line_range: None }
        }
        "replace_file_content" | "Edit" | "edit_file" => {
            let path =
                input.get("TargetFile").or_else(|| input.get("file_path")).and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::FileEdit { path: CompactString::new(path), lines_added: 0, lines_removed: 0 }
        }
        "write_to_file" | "Write" => {
            let path =
                input.get("TargetFile").or_else(|| input.get("file_path")).and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::FileCreate { path: CompactString::new(path) }
        }
        "grep_search" | "rg" => {
            let q = input.get("Query").or_else(|| input.get("pattern")).and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::CodeSearch { tool: CompactString::new(name), query: CompactString::new(q), is_ast: false }
        }
        "find_by_name" | "fd" => {
            let p = input.get("Pattern").and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::FileDiscovery { tool: CompactString::new(name), pattern: CompactString::new(p) }
        }
        "run_command" | "Bash" | "bash" => {
            let cmd = input.get("CommandLine").or_else(|| input.get("command")).and_then(|v| v.as_str()).unwrap_or("");
            if cmd.starts_with("git") {
                ToolIntent::VersionControl { action: CompactString::new(cmd) }
            } else if cmd.contains("test") {
                ToolIntent::TestExecution { runner: CompactString::new("shell"), target_suite: None }
            } else {
                ToolIntent::Other { raw_name: CompactString::new(name) }
            }
        }
        _ => ToolIntent::Other { raw_name: CompactString::new(name) },
    }
}
