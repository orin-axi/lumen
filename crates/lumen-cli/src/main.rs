use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};
use lumen_model::*;
use lumen_session::*;
use lumen_store::{SessionFactRecord, SessionFilter, SessionRepository, SqliteStore, ToolCallFactRecord};
use miette::{miette, IntoDiagnostic, Result};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "lumen", author, version, about = "Multi-orchestrator telemetry & session intelligence engine")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(long, global = true, help = "Emit machine-readable JSON format")]
    pub json: bool,

    /// SQLite store path. Defaults to ~/.lumen/lumen.db. Only used by ingest/sessions/session.
    #[arg(long, global = true)]
    pub db: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Render execution trajectory DAG and timeline for one session file (no store access)
    Trace {
        /// Path to a session log file
        session_path: PathBuf,
    },
    /// Audit token economics, prompt cache hit %, and USD cost for one session file (no store access)
    Audit {
        /// Path to a session log file
        session_path: PathBuf,
    },
    /// Parse real sessions from a file or directory and persist them to the SQLite store
    Ingest {
        /// Path to a session log file, an OpenCode SQLite database, or a directory to scan
        path: PathBuf,
    },
    /// List recent sessions from the store
    Sessions {
        /// Filter to one provider (claude-code, codex, antigravity, opencode)
        #[arg(long)]
        provider: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show one session's full detail from the store
    Session { provider: String, id: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = resolve_db_path(cli.db.as_deref())?;

    match cli.command {
        Commands::Trace { session_path } => cmd_trace(&session_path, cli.json)?,
        Commands::Audit { session_path } => cmd_audit(&session_path, cli.json)?,
        Commands::Ingest { path } => cmd_ingest(&path, &db_path, cli.json)?,
        Commands::Sessions { provider, limit } => cmd_sessions(&db_path, provider, limit, cli.json)?,
        Commands::Session { provider, id } => cmd_session(&db_path, &provider, &id, cli.json)?,
    }

    Ok(())
}

/// Resolves the SQLite store path: the explicit `--db` flag if given, otherwise
/// `~/.lumen/lumen.db`, creating its parent directory if needed.
fn resolve_db_path(explicit: Option<&Path>) -> Result<Utf8PathBuf> {
    let path = match explicit {
        Some(p) => p.to_path_buf(),
        None => {
            let home = std::env::var("HOME").into_diagnostic()?;
            PathBuf::from(home).join(".lumen").join("lumen.db")
        }
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).into_diagnostic()?;
    }

    Utf8PathBuf::from_path_buf(path).map_err(|p| miette!("database path is not valid UTF-8: {}", p.display()))
}

/// The `provider` string this session should be stored/queried under -- matches each adapter's
/// own `name()`, so `lumen session <provider> <id>` uses the same strings the adapters do.
fn provider_str(orchestrator: OrchestratorKind) -> &'static str {
    match orchestrator {
        OrchestratorKind::ClaudeCode => "claude-code",
        OrchestratorKind::Antigravity => "antigravity",
        OrchestratorKind::Codex => "codex",
        OrchestratorKind::OpenCode => "opencode",
        OrchestratorKind::Kimi => "kimi",
        OrchestratorKind::GenericOtel => "generic-otel",
    }
}

/// Loads every real session found at `path`: one transcript for the three JSONL adapters, and
/// one per real session row for OpenCode's SQLite store (a single `.db` file commonly holds many
/// real sessions -- see `OpenCodeAdapter`'s doc comment).
fn load_sessions(path: &Path) -> Result<Vec<CanonicalTranscript>> {
    // Read initial sample for fingerprinting
    let mut sample_file = File::open(path).into_diagnostic()?;
    let mut buffer = [0u8; 2048];
    use std::io::Read;
    let n = sample_file.read(&mut buffer).unwrap_or(0);
    let sample = &buffer[..n];

    let orchestrator = detect_orchestrator(sample).ok_or_else(|| {
        miette!(
            "{}: {} ({})",
            IngestionError::UnrecognizedFormat,
            path.display(),
            "no known orchestrator fingerprint matched"
        )
    })?;

    match orchestrator {
        OrchestratorKind::OpenCode => OpenCodeAdapter.parse_database(path).into_diagnostic(),
        _ => {
            let file = File::open(path).into_diagnostic()?;
            let reader = BufReader::new(file);
            let transcript = match orchestrator {
                OrchestratorKind::ClaudeCode => ClaudeCodeAdapter.parse_stream(Box::new(reader)).into_diagnostic(),
                OrchestratorKind::Antigravity => AgyAdapter.parse_stream(Box::new(reader)).into_diagnostic(),
                OrchestratorKind::Codex => CodexAdapter.parse_stream(Box::new(reader)).into_diagnostic(),
                other => Err(miette!(
                    "recognized orchestrator {:?} for {} but no adapter is implemented for it yet",
                    other,
                    path.display()
                )),
            }?;
            Ok(vec![transcript])
        }
    }
}

/// Loads exactly one session from `path`, for single-transcript commands (`trace`/`audit`).
/// When `path` is an OpenCode database holding multiple real sessions, returns the first.
fn load_session(path: &Path) -> Result<CanonicalTranscript> {
    load_sessions(path)?
        .into_iter()
        .next()
        .ok_or_else(|| miette!("{} contains no recognizable sessions", path.display()))
}

fn cmd_trace(path: &Path, json_mode: bool) -> Result<()> {
    let transcript = load_session(path)?;

    if json_mode {
        let json_out = serde_json::to_string_pretty(&transcript).into_diagnostic()?;
        println!("{}", json_out);
        return Ok(());
    }

    println!("\n Session Trajectory: {}", transcript.session_id);
    println!(" Orchestrator: {:?} | Model: {}", transcript.orchestrator, transcript.model_family);
    println!(" Turns: {} | Wall Time: {}ms\n", transcript.turns.len(), transcript.timing.wall_duration_ms);

    let mut table = Table::new();
    table.load_preset(UTF8_FULL).apply_modifier(UTF8_ROUND_CORNERS).set_header(Row::from(vec![
        "Turn",
        "Role",
        "Tool Invocations",
        "Tokens (In / Write / Read / Out)",
    ]));

    for turn in &transcript.turns {
        let tool_summary = if turn.tool_calls.is_empty() {
            "-".to_string()
        } else {
            turn.tool_calls.iter().map(|c| format!("{}({})", c.tool_name, c.call_id)).collect::<Vec<_>>().join(", ")
        };

        let token_summary = if let Some(u) = turn.usage {
            format!("{}/{}/{}/{}", u.input_tokens, u.cache_creation_tokens, u.cache_read_tokens, u.output_tokens)
        } else {
            "-".to_string()
        };

        table.add_row(Row::from(vec![
            Cell::new(turn.turn_index.to_string()),
            Cell::new(format!("{:?}", turn.role)),
            Cell::new(tool_summary),
            Cell::new(token_summary),
        ]));
    }

    println!("{table}");
    Ok(())
}

fn cmd_audit(path: &Path, json_mode: bool) -> Result<()> {
    let transcript = load_session(path)?;
    let eco = &transcript.economics;

    if json_mode {
        let json_out = serde_json::to_string_pretty(eco).into_diagnostic()?;
        println!("{}", json_out);
        return Ok(());
    }

    println!("\n Token Economics & Cache Audit: {}", transcript.session_id);
    println!(" Model: {}\n", transcript.model_family);

    print_economics_table(eco);
    Ok(())
}

fn print_economics_table(eco: &TokenEconomics) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL).apply_modifier(UTF8_ROUND_CORNERS).set_header(Row::from(vec!["Metric", "Value"]));

    table.add_row(Row::from(vec!["Uncached Input Tokens", &format!("{}", eco.input_tokens)]));
    table.add_row(Row::from(vec!["Cache Creation (5m Write)", &format!("{}", eco.ephemeral_5m_tokens)]));
    table.add_row(Row::from(vec!["Cache Creation (1h Write)", &format!("{}", eco.ephemeral_1h_tokens)]));
    table.add_row(Row::from(vec!["Cache Read (0.10x Discount)", &format!("{}", eco.cache_read_tokens)]));
    table.add_row(Row::from(vec!["Output Tokens", &format!("{}", eco.output_tokens)]));
    table.add_row(Row::from(vec!["Reasoning Tokens", &format!("{}", eco.reasoning_output_tokens)]));
    table.add_row(Row::from(vec![
        Cell::new("Cache Hit Ratio").fg(Color::Green),
        Cell::new(format!("{:.1}%", eco.cache_hit_ratio)).fg(Color::Green),
    ]));

    // CRIT-LUMEN-171: eco.cost() forces this match, so a future call site can't add a new
    // dollar figure here and forget the is_fully_priced check the way cmd_sessions once did.
    match eco.cost() {
        Cost::Priced(total) => {
            table.add_row(Row::from(vec![
                Cell::new("Actual USD Spend").fg(Color::Cyan),
                Cell::new(format!("${total:.4}")).fg(Color::Cyan),
            ]));
            table.add_row(Row::from(vec![
                "Baseline Cost (No Cache)",
                &format!("${:.4}", eco.baseline_cost_no_cache_usd),
            ]));
            table.add_row(Row::from(vec![
                Cell::new("Net Savings USD").fg(Color::Green),
                Cell::new(format!("${:.4}", eco.net_savings_usd)).fg(Color::Green),
            ]));
            table.add_row(Row::from(vec![
                Cell::new("Efficiency Multiplier").fg(Color::Yellow),
                Cell::new(format!("{:.2}x", eco.efficiency_multiplier)).fg(Color::Yellow),
            ]));
        }
        Cost::Unpriced => {
            // No seeded pricing row matched this model -- report cost as an explicit unknown
            // rather than the misleading $0.00 that a silently-wrong or silently-zero rate
            // would produce.
            table.add_row(Row::from(vec![
                Cell::new("USD Spend").fg(Color::Red),
                Cell::new("unknown (model not in pricing table)").fg(Color::Red),
            ]));
        }
    }

    println!("{table}");
}

/// Short, stable label for a `ToolIntent` variant, stored as `tool_calls.intent_kind`
/// (CRIT-LUMEN-174). A Lumen-specific naming judgment call -- there is no external
/// convention to follow here, unlike the adapter-facing field names elsewhere in this
/// codebase, which are dictated by each provider's real wire format.
fn intent_kind_str(intent: &ToolIntent) -> &'static str {
    match intent {
        ToolIntent::FileRead { .. } => "file_read",
        ToolIntent::FileEdit { .. } => "file_edit",
        ToolIntent::FileCreate { .. } => "file_create",
        ToolIntent::CodeSearch { .. } => "code_search",
        ToolIntent::FileDiscovery { .. } => "file_discovery",
        ToolIntent::TestExecution { .. } => "test_execution",
        ToolIntent::VersionControl { .. } => "version_control",
        ToolIntent::SubagentSpawn { .. } => "subagent_spawn",
        ToolIntent::McpCall { .. } => "mcp_call",
        ToolIntent::Other { .. } => "other",
    }
}

/// Builds one `ToolCallFactRecord` per real tool call across every turn (CRIT-LUMEN-174: the
/// data was always present on `CanonicalTranscript` but nothing ever persisted it, so
/// `SessionRepository::get_session`'s tool_counts/error_counts -- already shipped, already
/// serialized -- were silently empty for every real ingested session).
///
/// `is_error` is resolved by matching `call_id` against `tool_results` across the WHOLE
/// transcript, not just the call's own turn: some adapters (e.g. Claude Code) place a tool's
/// result in a later turn than its call. A call with no matching result anywhere defaults to
/// `false` (not known to have failed), since no result was ever observed for it.
///
/// `latency_ms` has no real per-call source in `CanonicalTurn` today (only a per-turn total) --
/// using the owning turn's `latency_ms` for every call within it is a documented approximation,
/// not real per-call timing; revisit if per-call latency is ever added to the canonical model.
fn build_tool_call_records(turns: &[CanonicalTurn]) -> Vec<ToolCallFactRecord> {
    let error_by_call_id: std::collections::HashMap<&str, bool> = turns
        .iter()
        .flat_map(|t| t.tool_results.iter())
        .map(|result| (result.call_id.as_str(), result.is_error))
        .collect();

    turns
        .iter()
        .flat_map(|turn| {
            turn.tool_calls.iter().map(|call| ToolCallFactRecord {
                turn_index: turn.turn_index,
                tool_name: call.tool_name.to_string(),
                call_id: call.call_id.to_string(),
                intent_kind: intent_kind_str(&call.intent).to_string(),
                is_error: error_by_call_id.get(call.call_id.as_str()).copied().unwrap_or(false),
                latency_ms: turn.latency_ms,
            })
        })
        .collect()
}

fn cmd_ingest(path: &Path, db_path: &Utf8PathBuf, json_mode: bool) -> Result<()> {
    let store = SqliteStore::open(db_path).into_diagnostic()?;
    let conn = store.connection().into_diagnostic()?;
    let repo = SessionRepository::new(&conn);

    let candidate_files: Vec<PathBuf> = if path.is_dir() {
        std::fs::read_dir(path)
            .into_diagnostic()?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|p| p.is_file())
            .collect()
    } else {
        vec![path.to_path_buf()]
    };

    let mut ingested = Vec::new();
    let mut failed = Vec::new();

    for file_path in &candidate_files {
        match load_sessions(file_path) {
            Ok(transcripts) => {
                for transcript in transcripts {
                    let record = SessionFactRecord {
                        provider: provider_str(transcript.orchestrator).to_string(),
                        provider_session_id: transcript.session_id.to_string(),
                        model_family: transcript.model_family.to_string(),
                        orchestrator: transcript.orchestrator,
                        started_at: transcript.timing.started_at,
                        ended_at: transcript.timing.ended_at,
                        wall_duration_ms: transcript.timing.wall_duration_ms,
                        turn_count: transcript.turns.len(),
                        economics: transcript.economics.clone(),
                        has_anomalies: !transcript.detected_anomalies.is_empty(),
                        tool_calls: build_tool_call_records(&transcript.turns),
                    };
                    match repo.upsert_session(&record) {
                        Ok(()) => ingested.push(record),
                        Err(e) => failed.push((file_path.clone(), e.to_string())),
                    }
                }
            }
            Err(e) => failed.push((file_path.clone(), e.to_string())),
        }
    }

    if json_mode {
        let json_out = serde_json::json!({
            "ingested": ingested.len(),
            "failed": failed.iter().map(|(p, e)| serde_json::json!({"path": p.display().to_string(), "error": e})).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&json_out).into_diagnostic()?);
        return Ok(());
    }

    println!("\n Ingested {} session(s) from {}\n", ingested.len(), path.display());

    if !ingested.is_empty() {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL).apply_modifier(UTF8_ROUND_CORNERS).set_header(Row::from(vec![
            "Provider",
            "Session ID",
            "Model",
            "Turns",
            "Cost USD",
        ]));
        for record in &ingested {
            let cost = record.economics.cost().format_usd("unknown");
            table.add_row(Row::from(vec![
                Cell::new(&record.provider),
                Cell::new(&record.provider_session_id),
                Cell::new(&record.model_family),
                Cell::new(record.turn_count.to_string()),
                Cell::new(cost),
            ]));
        }
        println!("{table}");
    }

    if !failed.is_empty() {
        println!("\n {} file(s) skipped:", failed.len());
        for (path, err) in &failed {
            println!("   {} -- {}", path.display(), err);
        }
    }

    Ok(())
}

fn cmd_sessions(db_path: &Utf8PathBuf, provider: Option<String>, limit: usize, json_mode: bool) -> Result<()> {
    let store = SqliteStore::open(db_path).into_diagnostic()?;
    let conn = store.connection().into_diagnostic()?;
    let repo = SessionRepository::new(&conn);
    let sessions = repo.list_recent(&SessionFilter { provider, limit }).into_diagnostic()?;

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&sessions).into_diagnostic()?);
        return Ok(());
    }

    println!("\n {} session(s) in {}\n", sessions.len(), db_path);

    let mut table = Table::new();
    table.load_preset(UTF8_FULL).apply_modifier(UTF8_ROUND_CORNERS).set_header(Row::from(vec![
        "Provider",
        "Session ID",
        "Model",
        "Turns",
        "Cache Hit %",
        "Cost USD",
    ]));
    for s in &sessions {
        table.add_row(Row::from(vec![
            Cell::new(&s.provider),
            Cell::new(&s.session_id),
            Cell::new(&s.model_family),
            Cell::new(s.turn_count.to_string()),
            Cell::new(format!("{:.1}%", s.cache_hit_ratio)),
            // CRIT-LUMEN-171 (real bug, found via this pass): this read s.total_cost_usd
            // directly with no is_fully_priced check at all, so an unpriced session (an
            // unrecognized model) displayed as an indistinguishable-from-real "$0.0000" here.
            Cell::new(s.cost().format_usd("unknown")),
        ]));
    }
    println!("{table}");
    Ok(())
}

fn cmd_session(db_path: &Utf8PathBuf, provider: &str, id: &str, json_mode: bool) -> Result<()> {
    let store = SqliteStore::open(db_path).into_diagnostic()?;
    let conn = store.connection().into_diagnostic()?;
    let repo = SessionRepository::new(&conn);
    let detail = repo
        .get_session(provider, id)
        .into_diagnostic()?
        .ok_or_else(|| miette!("no session found for provider={provider} id={id} in {db_path}"))?;

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&detail).into_diagnostic()?);
        return Ok(());
    }

    println!("\n Session: {} ({})", detail.summary.session_id, detail.summary.provider);
    println!(" Model: {} | Turns: {}\n", detail.summary.model_family, detail.summary.turn_count);
    print_economics_table(&detail.economics);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("create temp file");
        file.write_all(contents.as_bytes()).expect("write temp file");
        file.flush().expect("flush temp file");
        file
    }

    #[test]
    fn load_session_rejects_unrecognized_format() {
        // No sessionId/parentUuid, no step_index+source, no event_msg/response_item,
        // no SQLite magic prefix -- detect_orchestrator returns None.
        let file = write_temp_file(r#"{"totally":"unrecognized","shape":42}"#);

        let result = load_session(file.path());

        assert!(result.is_err(), "expected an Err for an unrecognized session format, got Ok");
    }

    #[test]
    fn load_session_parses_recognized_claude_code_transcript() {
        let file = write_temp_file(lumen_fixtures::real_claude_session_dump());

        let result = load_session(file.path());

        let transcript = result.expect("expected a real Claude Code transcript to parse successfully");
        assert_eq!(transcript.orchestrator, OrchestratorKind::ClaudeCode);
        assert!(!transcript.turns.is_empty(), "expected the recognized transcript to have turns");
    }

    #[test]
    fn ingest_then_sessions_then_session_round_trips_through_the_real_store() {
        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        let session_file = write_temp_file(lumen_fixtures::real_claude_session_dump());
        cmd_ingest(session_file.path(), &db_path, true).expect("ingest must succeed");

        let store = SqliteStore::open(&db_path).expect("reopen store");
        let conn = store.connection().unwrap();
        let repo = SessionRepository::new(&conn);
        let sessions = repo.list_recent(&SessionFilter::default()).expect("list_recent must succeed");
        assert_eq!(sessions.len(), 1, "the ingested session must be queryable back from the store");

        let detail = repo
            .get_session(&sessions[0].provider, &sessions[0].session_id)
            .expect("get_session must succeed")
            .expect("the ingested session must be found by (provider, session_id)");
        assert_eq!(detail.summary.provider, "claude-code");
        assert!(detail.summary.turn_count > 0);

        // CRIT-LUMEN-174: tool_counts was always empty on this exact real-fixture path before --
        // cmd_ingest never populated SessionFactRecord.tool_calls, so nothing was ever there for
        // get_session to read back. The real fixture's first session carries two real tool calls
        // (view_file, replace_file_content), neither erroring.
        assert!(!detail.tool_counts.is_empty(), "tool_counts must be populated from a real ingest, not empty");
        assert_eq!(detail.tool_counts.get("view_file").copied(), Some(1));
        assert_eq!(detail.tool_counts.get("replace_file_content").copied(), Some(1));
        assert!(detail.error_counts.is_empty(), "neither real tool call in this fixture errored");
    }
}
