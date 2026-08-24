use chrono::{TimeZone, Utc};
use compact_str::CompactString;
use lumen_model::*;
use rusqlite::{Connection, OpenFlags};
use smallvec::SmallVec;
use std::path::Path;

use crate::adapter::{AdapterCapabilities, IngestionError, SessionAdapter, SessionSource};
use crate::fingerprint::detect_orchestrator;

/// (tool calls, tool results, joined text) extracted from one message's real `part` rows.
type MessageParts = (SmallVec<[CanonicalToolCall; 2]>, SmallVec<[CanonicalToolResult; 2]>, Option<String>);

/// OpenCode's real on-disk store is a SQLite database
/// (`~/.local/share/opencode/opencode.db`), not a JSONL line stream -- confirmed against a real
/// local database this session (`session`/`message`/`part` tables, JSON blobs in a `data`
/// column). This was structurally incompatible with the old `SessionAdapter::parse_stream`
/// contract (there is no meaningful way to stream a SQLite file line by line), so
/// `OpenCodeAdapter` used to not implement `SessionAdapter` at all -- `SessionSource` (CRIT-LUMEN-180)
/// resolved that by giving the trait's `load` method a source shape (`SessionSource::Database`)
/// this adapter can actually accept; `parse_database` below remains the real, direct entry
/// point when a caller already knows it's dealing with an OpenCode database specifically.
pub struct OpenCodeAdapter;

impl OpenCodeAdapter {
    pub fn name(&self) -> &'static str {
        "opencode"
    }

    /// Whether `path` is (probably) a real OpenCode SQLite database: readable as SQLite and
    /// carrying the real `session`/`message`/`part` tables this adapter depends on. A positive
    /// schema check, not just a `.db` extension guess, so an arbitrary unrelated SQLite file
    /// isn't misidentified as an OpenCode database.
    pub fn matches_database(path: &Path) -> bool {
        let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
            return false;
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('session', 'message', 'part')",
        ) else {
            return false;
        };
        matches!(stmt.query_row([], |row| row.get::<_, i64>(0)), Ok(3))
    }

    /// Parses every real session found in the database at `path` into one `CanonicalTranscript`
    /// each, ordered by `session.time_created`.
    pub fn parse_database(&self, path: &Path) -> Result<Vec<CanonicalTranscript>, IngestionError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

        let session_ids: Vec<String> = {
            let mut stmt = conn.prepare("SELECT id FROM session ORDER BY time_created")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
            rows
        };

        session_ids.iter().map(|session_id| self.parse_one_session(&conn, session_id)).collect()
    }

    fn parse_one_session(&self, conn: &Connection, session_id: &str) -> Result<CanonicalTranscript, IngestionError> {
        let session_cost: f64 =
            conn.query_row("SELECT cost FROM session WHERE id = ?1", [session_id], |row| row.get(0)).unwrap_or(0.0);

        let mut model_family = CompactString::new("opencode-unknown-model");
        let mut turns = Vec::new();
        let mut pricing_inputs: Vec<TurnPricingInput> = Vec::new();
        let mut started_at = Utc::now();
        let mut ended_at = Utc::now();
        let mut has_start = false;

        let mut msg_stmt =
            conn.prepare("SELECT id, time_created, data FROM message WHERE session_id = ?1 ORDER BY time_created, id")?;
        let messages: Vec<(String, i64, serde_json::Value)> = msg_stmt
            .query_map([session_id], |row| {
                let id: String = row.get(0)?;
                let time_created: i64 = row.get(1)?;
                let data: String = row.get(2)?;
                Ok((id, time_created, data))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|(id, time_created, raw)| serde_json::from_str(&raw).ok().map(|v| (id, time_created, v)))
            .collect();
        drop(msg_stmt);

        let mut part_stmt = conn.prepare("SELECT data FROM part WHERE message_id = ?1 ORDER BY time_created, id")?;

        for (message_id, time_created_ms, data) in &messages {
            let timestamp = Utc.timestamp_millis_opt(*time_created_ms).single().unwrap_or_else(Utc::now);
            if !has_start {
                started_at = timestamp;
                has_start = true;
            }
            ended_at = timestamp;

            let role_str = data.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let role = if role_str == "assistant" { TurnRole::Assistant } else { TurnRole::User };

            if role == TurnRole::Assistant {
                if let Some(model_id) = data.get("modelID").and_then(|v| v.as_str()) {
                    model_family = CompactString::new(model_id);
                }
            }

            // Real per-message usage: data.tokens.{input,output,reasoning,cache.{write,read}}.
            // OpenCode publishes no separate 5m/1h cache-write split (unlike Claude Code), so
            // cache_creation_1h_tokens stays 0 -- the whole write goes on the default rate.
            let usage = data.get("tokens").map(|t| TurnTokenUsage {
                input_tokens: t.get("input").and_then(|v| v.as_u64()).unwrap_or(0),
                output_tokens: t.get("output").and_then(|v| v.as_u64()).unwrap_or(0),
                cache_creation_tokens: t
                    .get("cache")
                    .and_then(|c| c.get("write"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cache_creation_1h_tokens: 0,
                cache_read_tokens: t.get("cache").and_then(|c| c.get("read")).and_then(|v| v.as_u64()).unwrap_or(0),
                reasoning_tokens: t.get("reasoning").and_then(|v| v.as_u64()).unwrap_or(0),
            });

            if let Some(usage) = usage {
                pricing_inputs.push(TurnPricingInput { usage, timestamp, tier: None });
            }

            let (tool_calls, tool_results, text) = self.parts_for_message(&mut part_stmt, message_id)?;

            turns.push(CanonicalTurn {
                attribution: None,
                turn_index: turns.len(),
                role,
                timestamp,
                latency_ms: 0,
                text,
                tool_calls,
                tool_results,
                usage,
            });
        }
        drop(part_stmt);

        let economics = TokenEconomics::calculate(
            &pricing_inputs,
            &model_family,
            &pricing::SEEDED,
            if session_cost > 0.0 { Some(session_cost) } else { None },
        );

        let wall_duration = (ended_at - started_at).num_milliseconds().max(0) as u64;

        Ok(CanonicalTranscript {
            session_id: CompactString::new(session_id),
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

    /// Real `part` rows for one message: `type: "text"` supplies the turn's readable text
    /// (joined across multiple parts, real for both user and assistant messages); `type: "tool"`
    /// supplies one call+result pair per row (real data carries call input/output/status
    /// together in a single row via `state`, unlike Claude Code's separate tool_use/tool_result
    /// entries, so correlation by call_id is always exact -- never inferred).
    fn parts_for_message(
        &self,
        stmt: &mut rusqlite::Statement,
        message_id: &str,
    ) -> Result<MessageParts, IngestionError> {
        let parts: Vec<serde_json::Value> = stmt
            .query_map([message_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|raw| serde_json::from_str(&raw).ok())
            .collect();

        let mut tool_calls = SmallVec::new();
        let mut tool_results = SmallVec::new();
        let mut text_segments = Vec::new();

        for part in &parts {
            match part.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                        text_segments.push(t.to_string());
                    }
                }
                Some("tool") => {
                    let tool_name = part.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                    let call_id = part.get("callID").and_then(|v| v.as_str()).unwrap_or("");
                    let state = part.get("state");
                    let input = state.and_then(|s| s.get("input")).cloned().unwrap_or(serde_json::Value::Null);
                    let status = state.and_then(|s| s.get("status")).and_then(|v| v.as_str()).unwrap_or("");
                    // Real data confirmed "completed" as the success status; no real error
                    // example was observed, so "error" is the defensible opposite (a common
                    // state-machine convention), not a confirmed exact string -- revisit if a
                    // real error-state row is observed with a different value.
                    let is_error = status == "error";
                    let output_str = state
                        .and_then(|s| s.get("output"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .unwrap_or_default();

                    let intent = classify_tool_intent(tool_name, &input);

                    tool_calls.push(CanonicalToolCall {
                        call_id: CompactString::new(call_id),
                        tool_name: CompactString::new(tool_name),
                        intent,
                        raw_arguments: input,
                    });
                    tool_results.push(CanonicalToolResult {
                        call_id: CompactString::new(call_id),
                        output_bytes: output_str.len(),
                        line_count: output_str.lines().count(),
                        is_error,
                        error_class: if is_error { Some(CompactString::new("ToolError")) } else { None },
                        truncated_output: None,
                        otel_span_id: None,
                    });
                }
                _ => {}
            }
        }

        let text = if text_segments.is_empty() { None } else { Some(text_segments.join("\n")) };
        Ok((tool_calls, tool_results, text))
    }
}

impl SessionAdapter for OpenCodeAdapter {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn matches_fingerprint(&self, sample: &str) -> bool {
        // Cheap binary-prefix pre-filter (the SQLite file-format magic bytes), same source of
        // truth the other three adapters delegate to -- see detect_orchestrator's own doc
        // comment for why this is only a pre-filter and `matches_database` is the real schema
        // check `load`/`parse_database` themselves rely on.
        detect_orchestrator(sample.as_bytes()) == Some(OrchestratorKind::OpenCode)
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

    fn load(&self, source: SessionSource) -> Result<Vec<CanonicalTranscript>, IngestionError> {
        match source {
            SessionSource::Database(path) => self.parse_database(path),
            SessionSource::Stream(_) => {
                Err(IngestionError::UnsupportedSourceKind { adapter: self.name(), source_kind: "stream" })
            }
        }
    }
}

/// Maps a real OpenCode tool name (confirmed against real data: `read`, `glob`, `task`, `grep`,
/// `bash`; others inferred by the same naming convention) to a `ToolIntent`.
fn classify_tool_intent(tool_name: &str, input: &serde_json::Value) -> ToolIntent {
    match tool_name {
        "read" => {
            let path = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::FileRead { path: CompactString::new(path), line_range: None }
        }
        "write" => {
            let path = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::FileCreate { path: CompactString::new(path) }
        }
        "edit" => {
            let path = input.get("filePath").and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::FileEdit { path: CompactString::new(path), lines_added: 0, lines_removed: 0 }
        }
        "grep" => {
            let query = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::CodeSearch { tool: CompactString::new("grep"), query: CompactString::new(query), is_ast: false }
        }
        "glob" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::FileDiscovery { tool: CompactString::new("glob"), pattern: CompactString::new(pattern) }
        }
        "bash" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.trim_start().starts_with("git") {
                ToolIntent::VersionControl { action: CompactString::new(cmd) }
            } else {
                ToolIntent::Other { raw_name: CompactString::new("bash") }
            }
        }
        "task" => {
            let description = input.get("description").and_then(|v| v.as_str()).unwrap_or("");
            ToolIntent::SubagentSpawn {
                agent_type: CompactString::new("task"),
                description: CompactString::new(description),
            }
        }
        other => ToolIntent::Other { raw_name: CompactString::new(other) },
    }
}
