use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};
use lumen_analysis::detect_trajectory_anomalies;
use lumen_model::*;
use lumen_session::*;
use lumen_store::{
    SessionFactRecord, SessionFilter, SessionRepository, SessionTrendPoint, SqliteStore, ToolCallFactRecord,
    TrendFilter, TrendRepository,
};
use miette::{miette, IntoDiagnostic, Result};
use serde::Serialize;
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
    /// Show cost/cache-hit/turn-count/anomaly-rate trends across tracked sessions
    Trends {
        /// Filter to one provider (claude-code, codex, antigravity, opencode)
        #[arg(long)]
        provider: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Include per-session compaction event data (Claude Code only)
        #[arg(long)]
        compaction: bool,
    },
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
        Commands::Trends { provider, limit, compaction } => {
            cmd_trends(&db_path, provider, limit, compaction, cli.json, &mut std::io::stdout())?
        }
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
    // Read initial sample for fingerprinting, then reuse the same handle (seeked back to the
    // start) for the real parse below instead of opening the file a second time.
    use std::io::{Read, Seek, SeekFrom};
    let mut sample_file = File::open(path).into_diagnostic()?;
    let mut buffer = [0u8; 2048];
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

    // ClaudeCode alone can have sibling `subagents/<worker>.jsonl` files (CRIT-LUMEN-026) --
    // parse_session_with_subagents links them into CanonicalTranscript.subagents, which
    // rolled_up_economics (CRIT-LUMEN-176) and detected_anomalies (CRIT-LUMEN-179) both need
    // populated to be meaningful. This can't go through the uniform SessionAdapter::load(source)
    // path below: `load` only ever receives an abstract Box<dyn BufRead> or a bare database
    // path, neither of which carries "the real directory this file lives in" -- sibling-file
    // discovery is inherently a real-filesystem concern, not something SessionSource can express
    // without coupling every adapter's trait contract to "you get a directory, not just a
    // source." A single, honest special case for the one adapter that actually needs it.
    let mut transcripts = if orchestrator == OrchestratorKind::ClaudeCode {
        let session_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| miette!("session file name is not valid UTF-8: {}", path.display()))?;
        vec![ClaudeCodeAdapter.parse_session_with_subagents(session_dir, file_name).into_diagnostic()?]
    } else {
        let adapter: Box<dyn SessionAdapter> = match orchestrator {
            OrchestratorKind::Antigravity => Box::new(AgyAdapter),
            OrchestratorKind::Codex => Box::new(CodexAdapter),
            OrchestratorKind::OpenCode => Box::new(OpenCodeAdapter),
            other => {
                return Err(miette!(
                    "recognized orchestrator {:?} for {} but no adapter is implemented for it yet",
                    other,
                    path.display()
                ))
            }
        };
        let source = if orchestrator == OrchestratorKind::OpenCode {
            SessionSource::Database(path)
        } else {
            sample_file.seek(SeekFrom::Start(0)).into_diagnostic()?;
            SessionSource::Stream(Box::new(BufReader::new(sample_file)))
        };
        adapter.load(source).into_diagnostic()?
    };

    for transcript in &mut transcripts {
        populate_detected_anomalies(transcript);
    }

    Ok(transcripts)
}

/// Detects CircularLoop/GateStall anomalies (CRIT-LUMEN-179) for `transcript` and every
/// subagent transitively, each scoped to its own `turns` -- not flattened across the tree, the
/// same per-transcript-node discipline `rolled_up_economics` documents: a cycle or stall found
/// inside one subagent's own trajectory is that subagent's fact, not the root's.
fn populate_detected_anomalies(transcript: &mut CanonicalTranscript) {
    transcript.detected_anomalies = detect_trajectory_anomalies(transcript);
    for subagent in &mut transcript.subagents {
        populate_detected_anomalies(subagent);
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
    // Includes subagent spend, not just the root transcript's own turns -- see
    // CanonicalTranscript::rolled_up_economics.
    let eco = transcript.rolled_up_economics();

    if json_mode {
        let json_out = serde_json::to_string_pretty(&eco).into_diagnostic()?;
        println!("{}", json_out);
        return Ok(());
    }

    println!("\n Token Economics & Cache Audit: {}", transcript.session_id);
    println!(" Model: {}\n", transcript.model_family);

    print_economics_table(&eco);
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

/// Maps parsed `compact_boundary` events to store-layer fact records (CRIT-LUMEN-185).
/// `session_id` is a placeholder `0` -- `upsert_session` resolves and uses the real internal
/// id via `CompactionRepository`; the field's value here is ignored in favor of
/// `insert_compaction_facts`'s explicit `session_id: i64` parameter.
fn build_compaction_fact_records(events: &[CompactionEvent]) -> Vec<lumen_store::CompactionFactRecord> {
    events
        .iter()
        .map(|e| lumen_store::CompactionFactRecord {
            session_id: 0,
            sequence: e.sequence,
            trigger: match e.trigger {
                CompactionTrigger::Auto => "auto".to_string(),
                CompactionTrigger::Manual => "manual".to_string(),
            },
            pre_tokens: e.pre_tokens,
            post_tokens: e.post_tokens,
            cumulative_dropped_tokens: e.cumulative_dropped_tokens,
            duration_ms: e.duration_ms,
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
                        // Includes subagent spend, not just the root transcript's own turns --
                        // see CanonicalTranscript::rolled_up_economics.
                        economics: transcript.rolled_up_economics(),
                        has_anomalies: !transcript.detected_anomalies.is_empty(),
                        tool_calls: build_tool_call_records(&transcript.turns),
                        compaction_events: build_compaction_fact_records(&transcript.compaction_events),
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
        "Anomalies",
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
            // CRIT-LUMEN-179's detected_anomalies was persisted but never read back out until
            // now -- this column was previously impossible to show at all.
            if s.has_anomalies { Cell::new("yes").fg(Color::Yellow) } else { Cell::new("-") },
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
    println!(
        " Model: {} | Turns: {} | Anomalies: {}\n",
        detail.summary.model_family,
        detail.summary.turn_count,
        if detail.summary.has_anomalies { "yes" } else { "none" }
    );
    print_economics_table(&detail.economics);
    Ok(())
}

fn cmd_trends(
    db_path: &Utf8PathBuf,
    provider: Option<String>,
    limit: usize,
    compaction: bool,
    json_mode: bool,
    writer: &mut dyn std::io::Write,
) -> Result<()> {
    let store = SqliteStore::open(db_path).into_diagnostic()?;
    let conn = store.connection().into_diagnostic()?;
    let repo = TrendRepository::new(&conn);
    let points = repo
        .list_session_trend(&TrendFilter { provider: provider.clone(), limit, require_compaction: compaction })
        .into_diagnostic()?;

    if points.len() < 2 {
        return Err(miette!("lumen trends requires at least 2 sessions after filtering; found {}", points.len()));
    }

    if compaction {
        if let Some(p) = provider.as_deref() {
            if p != "claude-code" {
                return Err(miette!("--compaction is only available for --provider claude-code (got '{p}')"));
            }
        }
        if (provider.is_none() || provider.as_deref() == Some("claude-code"))
            && repo.count_sessions("claude-code").into_diagnostic()? == 0
        {
            return Err(miette!("--compaction requires at least one Claude Code session; none found in the store"));
        }
    }

    let anomalous = points.iter().filter(|p| p.has_anomalies).count();
    let anomaly_rate = format!("{:.1}", (anomalous as f64 / points.len() as f64) * 100.0).parse::<f64>().unwrap();
    if json_mode {
        #[derive(Serialize)]
        struct TrendsJsonOutput<'a> {
            sessions: &'a [SessionTrendPoint],
            anomaly_rate: f64,
        }
        let json_out = TrendsJsonOutput { sessions: &points, anomaly_rate };
        writeln!(writer, "{}", serde_json::to_string_pretty(&json_out).into_diagnostic()?).into_diagnostic()?;
        return Ok(());
    }

    writeln!(writer, "\n {} session(s), anomaly rate: {:.1}%\n", points.len(), anomaly_rate).into_diagnostic()?;

    let mut table = Table::new();
    table.load_preset(UTF8_FULL).apply_modifier(UTF8_ROUND_CORNERS);
    let mut headers = vec!["Provider", "Session ID", "Started At", "Cost USD", "Cache Hit %", "Turns", "Anomalies"];
    if compaction {
        headers.extend(["Compaction Count", "Tokens Dropped", "Auto/Manual"]);
    }
    table.set_header(Row::from(headers));

    for p in &points {
        let mut cells = vec![
            Cell::new(&p.provider),
            Cell::new(&p.session_id),
            Cell::new(p.started_at.to_rfc3339()),
            Cell::new(p.cost.format_usd("unknown")),
            Cell::new(format!("{:.1}%", p.cache_hit_ratio)),
            Cell::new(p.turn_count.to_string()),
            if p.has_anomalies { Cell::new("yes") } else { Cell::new("-") },
        ];
        if compaction {
            match &p.compaction {
                Some(c) => {
                    cells.push(Cell::new(c.event_count.to_string()));
                    cells.push(Cell::new(c.tokens_dropped_total.to_string()));
                    cells.push(Cell::new(format!("{}/{}", c.auto_count, c.manual_count)));
                }
                None => {
                    cells.push(Cell::new("n/a"));
                    cells.push(Cell::new("n/a"));
                    cells.push(Cell::new("n/a"));
                }
            }
        }
        table.add_row(cells);
    }

    writeln!(writer, "{table}").into_diagnostic()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_store::CompactionFactRecord;
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
    fn load_session_populates_detected_anomalies_from_a_real_circular_loop() {
        // CRIT-LUMEN-179: end-to-end through load_session (fingerprint -> parse -> anomaly
        // detection), not just the lumen-analysis-level glue function directly.
        let sample = concat!(
            "{\"type\":\"assistant\",\"sessionId\":\"s1\",\"parentUuid\":\"t0\",\"message\":{\"model\":\"claude-3-5-sonnet-20241022\",\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"c0\",\"name\":\"grep_search\",\"input\":{\"Query\":\"get_balance\"}}],\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"s1\",\"parentUuid\":\"t1\",\"message\":{\"model\":\"claude-3-5-sonnet-20241022\",\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"c1\",\"name\":\"grep_search\",\"input\":{\"Query\":\"get_balance\"}}],\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"s1\",\"parentUuid\":\"t2\",\"message\":{\"model\":\"claude-3-5-sonnet-20241022\",\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"c2\",\"name\":\"grep_search\",\"input\":{\"Query\":\"get_balance\"}}],\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n",
        );
        let file = write_temp_file(sample);

        let transcript = load_session(file.path()).expect("expected a real transcript to parse");

        assert_eq!(transcript.detected_anomalies.len(), 1);
        assert!(matches!(
            &transcript.detected_anomalies[0],
            TrajectoryAnomaly::CircularLoop { symbol, cycle_depth } if symbol == "get_balance" && *cycle_depth == 3
        ));
    }

    #[test]
    fn load_session_links_sibling_subagent_files_and_rolls_up_their_cost() {
        // CRIT-LUMEN-176/179 were both inert via the CLI until now: load_sessions previously
        // called ClaudeCodeAdapter::parse_stream directly, never parse_session_with_subagents,
        // so CanonicalTranscript.subagents was always empty for every real `lumen ingest`/`lumen
        // audit` run regardless of how many sibling subagents/*.jsonl files a real session had.
        let dir = tempfile::tempdir().expect("create temp dir");

        let main_line = "{\"type\":\"assistant\",\"sessionId\":\"parent-1\",\"parentUuid\":\"t0\",\"message\":{\"model\":\"claude-3-5-sonnet-20241022\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"main\"}],\"usage\":{\"input_tokens\":1000,\"output_tokens\":100}}}\n";
        std::fs::write(dir.path().join("main.jsonl"), main_line).expect("write main.jsonl");

        let subagents_dir = dir.path().join("subagents");
        std::fs::create_dir(&subagents_dir).expect("create subagents dir");
        let worker_line = "{\"type\":\"assistant\",\"sessionId\":\"child-1\",\"parentUuid\":\"t0\",\"message\":{\"model\":\"claude-3-5-haiku-20241022\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"worker\"}],\"usage\":{\"input_tokens\":2000,\"output_tokens\":200}}}\n";
        std::fs::write(subagents_dir.join("worker-a.jsonl"), worker_line).expect("write worker-a.jsonl");

        let transcript = load_session(&dir.path().join("main.jsonl")).expect("expected a real transcript to parse");

        assert_eq!(transcript.subagents.len(), 1);
        assert_eq!(transcript.subagents[0].subagent_role, Some("worker-a".into()));

        let rolled = transcript.rolled_up_economics();
        assert!(
            rolled.total_cost_usd > transcript.economics.total_cost_usd,
            "rolled-up cost must exceed the root's own cost once a real subagent is linked"
        );
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

    #[test]
    fn cmd_trends_errors_when_fewer_than_2_sessions_remain() {
        // CRIT-LUMEN-187: fewer than 2 sessions after --provider+--limit must error clearly,
        // never render a degenerate table/JSON or panic.
        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        let store = SqliteStore::open(&db_path).expect("open store");
        let conn = store.connection().unwrap();
        let repo = SessionRepository::new(&conn);
        repo.upsert_session(&SessionFactRecord {
            provider: "claude-code".to_string(),
            provider_session_id: "s1".to_string(),
            ..Default::default()
        })
        .expect("upsert_session must succeed");
        drop(conn);
        drop(store);

        let err = cmd_trends(&db_path, None, 50, false, false, &mut Vec::new())
            .expect_err("expected an error with only 1 session");
        assert!(
            err.to_string().contains("at least 2 sessions"),
            "expected error to mention 'at least 2 sessions', got: {err}"
        );
    }

    /// Exit-gate blocker (2026-08-24, round 4): CRIT-LUMEN-187's own concrete example --
    /// `lumen trends --limit 1` against 10 matching sessions SHALL also error -- was untested.
    /// The only prior below-minimum test seeds exactly 1 session with limit 50, which cannot
    /// distinguish "minimum evaluated after --limit caps the set" (correct, per CRIT-LUMEN-190's
    /// required ordering) from "minimum evaluated before the cap" (a regression that would let
    /// `--limit 1` against a large matching set render a degenerate 1-row table instead of
    /// erroring).
    #[test]
    fn cmd_trends_limit_cutting_a_larger_matching_set_below_2_still_errors() {
        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        let store = SqliteStore::open(&db_path).expect("open store");
        let conn = store.connection().unwrap();
        let repo = SessionRepository::new(&conn);
        for i in 0..3 {
            repo.upsert_session(&SessionFactRecord {
                provider: "claude-code".to_string(),
                provider_session_id: format!("s{i}"),
                ..Default::default()
            })
            .expect("upsert_session must succeed");
        }
        drop(conn);
        drop(store);

        let err = cmd_trends(&db_path, None, 1, false, false, &mut Vec::new())
            .expect_err("expected an error when --limit 1 cuts 3 matching sessions down to 1");
        assert!(
            err.to_string().contains("at least 2 sessions"),
            "expected error to mention 'at least 2 sessions', got: {err}"
        );
    }

    #[test]
    fn cmd_trends_compaction_with_incompatible_provider_errors_naming_it() {
        // CRIT-LUMEN-189: --compaction with --provider set to something other than
        // "claude-code" must error, naming the incompatible provider, and this check must
        // take precedence over CRIT-LUMEN-188.
        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        let store = SqliteStore::open(&db_path).expect("open store");
        let conn = store.connection().unwrap();
        let repo = SessionRepository::new(&conn);
        for i in 0..2 {
            repo.upsert_session(&SessionFactRecord {
                provider: "codex".to_string(),
                provider_session_id: format!("s{i}"),
                ..Default::default()
            })
            .expect("upsert_session must succeed");
        }
        drop(conn);
        drop(store);

        let err = cmd_trends(&db_path, Some("codex".to_string()), 50, true, false, &mut Vec::new())
            .expect_err("expected an error for --compaction with an incompatible --provider");
        let msg = err.to_string();
        assert!(msg.contains("codex"), "expected error to name 'codex', got: {msg}");
        assert!(msg.contains("claude-code"), "expected error to mention 'claude-code', got: {msg}");
    }

    #[test]
    fn cmd_trends_renders_table_with_cost_cache_hit_and_anomaly_rate() {
        // CRIT-LUMEN-183/184/191: TABLE-mode rendering -- exact header set (no derived
        // trend-direction/shift-detection signal), "unknown" for an unpriced session's cost
        // (never a fabricated figure), half-to-even-rounded cache-hit-ratio and set-level
        // anomaly-rate percentages.
        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        let store = SqliteStore::open(&db_path).expect("open store");
        let conn = store.connection().unwrap();
        let repo = SessionRepository::new(&conn);

        repo.upsert_session(&SessionFactRecord {
            provider: "claude-code".to_string(),
            provider_session_id: "unpriced-session".to_string(),
            turn_count: 3,
            economics: TokenEconomics { cache_hit_ratio: 0.0, is_fully_priced: false, ..Default::default() },
            has_anomalies: false,
            ..Default::default()
        })
        .expect("upsert_session must succeed");

        repo.upsert_session(&SessionFactRecord {
            provider: "claude-code".to_string(),
            provider_session_id: "priced-session".to_string(),
            turn_count: 5,
            economics: TokenEconomics {
                cache_hit_ratio: 66.666_f32,
                total_cost_usd: 1.2345,
                is_fully_priced: true,
                ..Default::default()
            },
            has_anomalies: true,
            ..Default::default()
        })
        .expect("upsert_session must succeed");

        drop(conn);
        drop(store);

        let mut buf = Vec::new();
        cmd_trends(&db_path, None, 50, false, false, &mut buf).expect("cmd_trends must succeed with 2 sessions");
        let output = String::from_utf8(buf).expect("output must be valid utf8");

        for header in ["Provider", "Session ID", "Started At", "Cost USD", "Cache Hit %", "Turns", "Anomalies"] {
            assert!(output.contains(header), "expected header {header:?} in output, got:\n{output}");
        }
        assert!(!output.contains("Trend"), "must not emit a derived trend-direction label, got:\n{output}");
        assert!(!output.contains("Direction"), "must not emit a derived trend-direction label, got:\n{output}");
        assert!(!output.contains("Shift"), "must not emit a derived shift-detection label, got:\n{output}");
        assert!(output.contains("unknown"), "unpriced row's cost must render as 'unknown', got:\n{output}");
        assert!(
            output.contains("66.7%"),
            "priced row's cache hit ratio 66.666 must round half-to-even to 66.7%, got:\n{output}"
        );
        assert!(
            output.contains("50.0%"),
            "anomaly rate summary (1 of 2 sessions has anomalies) must render as 50.0%, got:\n{output}"
        );
    }

    #[test]
    fn cmd_trends_compaction_with_no_claude_code_sessions_errors() {
        // CRIT-LUMEN-188: --compaction with no/claude-code --provider and zero claude-code
        // sessions store-wide must error.
        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        let store = SqliteStore::open(&db_path).expect("open store");
        let conn = store.connection().unwrap();
        let repo = SessionRepository::new(&conn);
        for i in 0..2 {
            repo.upsert_session(&SessionFactRecord {
                provider: "codex".to_string(),
                provider_session_id: format!("s{i}"),
                ..Default::default()
            })
            .expect("upsert_session must succeed");
        }
        drop(conn);
        drop(store);

        let err = cmd_trends(&db_path, None, 50, true, false, &mut Vec::new())
            .expect_err("expected an error for --compaction with zero claude-code sessions");
        assert!(
            err.to_string().contains("Claude Code session"),
            "expected error to mention 'Claude Code session', got: {err}"
        );
    }

    /// Exit-gate blocker (2026-08-24, round 4): CRIT-LUMEN-184's table-mode compaction
    /// rendering -- including the normative "n/a" marker for a non-Claude-Code row -- had zero
    /// test coverage; every existing --compaction=true test either errors before rendering or
    /// runs in JSON mode. This seeds a claude-code session with real compaction events and a
    /// codex session with none, renders in table mode, and asserts both the real figures and
    /// the literal "n/a" marker (not merely that the codex row is non-zero).
    #[test]
    fn cmd_trends_table_mode_renders_compaction_figures_and_na_marker() {
        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        let store = SqliteStore::open(&db_path).expect("open store");
        let conn = store.connection().unwrap();
        let repo = SessionRepository::new(&conn);

        repo.upsert_session(&SessionFactRecord {
            provider: "claude-code".to_string(),
            provider_session_id: "cc-with-events".to_string(),
            compaction_events: vec![
                CompactionFactRecord {
                    session_id: 0,
                    sequence: 0,
                    trigger: "auto".to_string(),
                    pre_tokens: 100_000,
                    post_tokens: 20_000,
                    cumulative_dropped_tokens: 80_000,
                    duration_ms: 1500,
                },
                CompactionFactRecord {
                    session_id: 0,
                    sequence: 1,
                    trigger: "manual".to_string(),
                    pre_tokens: 50_000,
                    post_tokens: 10_000,
                    cumulative_dropped_tokens: 120_000,
                    duration_ms: 900,
                },
            ],
            ..Default::default()
        })
        .expect("upsert_session must succeed");

        repo.upsert_session(&SessionFactRecord {
            provider: "codex".to_string(),
            provider_session_id: "cx-no-events".to_string(),
            ..Default::default()
        })
        .expect("upsert_session must succeed");

        drop(conn);
        drop(store);

        let mut buf = Vec::new();
        cmd_trends(&db_path, None, 50, true, false, &mut buf).expect("cmd_trends must succeed with 2 sessions");
        let output = String::from_utf8(buf).expect("output must be valid utf8");

        for header in ["Compaction Count", "Tokens Dropped", "Auto/Manual"] {
            assert!(output.contains(header), "expected header {header:?} in output, got:\n{output}");
        }

        let lines: Vec<&str> = output.lines().collect();
        let cc_line = lines
            .iter()
            .find(|l| l.contains("cc-with-events"))
            .unwrap_or_else(|| panic!("expected a row for cc-with-events, got:\n{output}"));
        assert!(cc_line.contains('2'), "claude-code row must show event_count 2, got: {cc_line}");
        assert!(cc_line.contains("120000"), "claude-code row must show tokens_dropped_total 120000, got: {cc_line}");
        assert!(cc_line.contains("1/1"), "claude-code row must show auto/manual counts 1/1, got: {cc_line}");

        let cx_line = lines
            .iter()
            .find(|l| l.contains("cx-no-events"))
            .unwrap_or_else(|| panic!("expected a row for cx-no-events, got:\n{output}"));
        let cx_cells: Vec<&str> = cx_line.split(['│', '┆']).map(str::trim).filter(|c| !c.is_empty()).collect();
        let compaction_cells = &cx_cells[cx_cells.len() - 3..];
        assert_eq!(
            compaction_cells,
            &["n/a", "n/a", "n/a"],
            "non-Claude-Code row's three compaction columns must each render the literal 'n/a' marker, never a zero-valued figure, got: {cx_line}"
        );
    }

    #[test]
    fn cmd_trends_json_mode_emits_sessions_and_anomaly_rate_object_envelope() {
        // CRIT-LUMEN-186/191/184: JSON-mode rendering -- an explicit {sessions, anomaly_rate}
        // object root (never a bare array), no derived statistical field on any session, and a
        // "compaction" key present (with the correct event_count) only for the Claude Code
        // session that actually has persisted compaction events.
        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        let store = SqliteStore::open(&db_path).expect("open store");
        let conn = store.connection().unwrap();
        let repo = SessionRepository::new(&conn);

        repo.upsert_session(&SessionFactRecord {
            provider: "claude-code".to_string(),
            provider_session_id: "compacted-session".to_string(),
            compaction_events: vec![
                lumen_store::CompactionFactRecord {
                    session_id: 0,
                    sequence: 0,
                    trigger: "auto".to_string(),
                    pre_tokens: 100,
                    post_tokens: 20,
                    cumulative_dropped_tokens: 80,
                    duration_ms: 5,
                },
                lumen_store::CompactionFactRecord {
                    session_id: 0,
                    sequence: 1,
                    trigger: "manual".to_string(),
                    pre_tokens: 90,
                    post_tokens: 10,
                    cumulative_dropped_tokens: 160,
                    duration_ms: 9,
                },
            ],
            ..Default::default()
        })
        .expect("upsert_session must succeed");

        repo.upsert_session(&SessionFactRecord {
            provider: "claude-code".to_string(),
            provider_session_id: "plain-session".to_string(),
            ..Default::default()
        })
        .expect("upsert_session must succeed");

        drop(conn);
        drop(store);

        let mut buf = Vec::new();
        cmd_trends(&db_path, None, 50, true, true, &mut buf).expect("cmd_trends must succeed with 2 sessions");
        let output = String::from_utf8(buf).expect("output must be valid utf8");

        let value: serde_json::Value = serde_json::from_str(&output).expect("output must be valid JSON");
        let obj = value.as_object().expect("top-level JSON must be an object, not a bare array");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["anomaly_rate", "sessions"],
            "top-level object must have exactly the sessions and anomaly_rate keys, got:\n{output}"
        );

        let anomaly_rate = obj["anomaly_rate"].as_f64().expect("anomaly_rate must be a plain JSON number");
        assert!(
            (0.0..=100.0).contains(&anomaly_rate),
            "anomaly_rate must be a [0.0, 100.0] percentage, got {anomaly_rate}"
        );

        let sessions = obj["sessions"].as_array().expect("sessions must be a JSON array");
        assert_eq!(sessions.len(), 2);

        let allowed_keys = [
            "provider",
            "session_id",
            "started_at",
            "cost",
            "cache_hit_ratio",
            "turn_count",
            "has_anomalies",
            "compaction",
        ];
        for session in sessions {
            let session_obj = session.as_object().expect("each session entry must be a JSON object");
            for key in session_obj.keys() {
                assert!(
                    allowed_keys.contains(&key.as_str()),
                    "unexpected key {key:?} in session object -- no derived statistical field \
                     (trend_direction/cusum/classification/etc.) is allowed, got:\n{output}"
                );
            }
            let cost_obj = session_obj["cost"].as_object().expect("cost must be a JSON object");
            let mut cost_keys: Vec<&str> = cost_obj.keys().map(String::as_str).collect();
            cost_keys.sort_unstable();
            assert_eq!(cost_keys, vec!["priced", "usd"], "cost object must have exactly usd and priced keys");
        }

        let compacted = sessions
            .iter()
            .find(|s| s["session_id"] == "compacted-session")
            .expect("compacted-session must be present in sessions");
        assert_eq!(compacted["compaction"]["event_count"], 2);

        let plain = sessions
            .iter()
            .find(|s| s["session_id"] == "plain-session")
            .expect("plain-session must be present in sessions");
        assert_eq!(
            plain["compaction"]["event_count"], 0,
            "a Claude Code session with zero events must be Some(zeros), not absent"
        );
    }

    /// Exit-gate blocker (2026-08-24): the `allowed_keys` assertion above permits a "compaction"
    /// key but never asserts its ABSENCE, and its fixture is two Claude Code sessions that both
    /// legitimately carry a value -- so deleting SessionTrendPoint.compaction's
    /// `#[serde(skip_serializing_if = "Option::is_none")]` (which would emit `"compaction":
    /// null` in exactly the two shapes CRIT-LUMEN-186 forbids) left the full suite green. This
    /// test seeds a mixed claude-code + codex store and directly asserts key absence, not just
    /// value shape, in both forbidden cases: (a) --compaction unset, for every row; (b)
    /// --compaction set, for a non-Claude-Code row specifically.
    #[test]
    fn cmd_trends_json_mode_omits_compaction_key_never_emits_it_as_null() {
        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        let store = SqliteStore::open(&db_path).expect("open store");
        let conn = store.connection().unwrap();
        let repo = SessionRepository::new(&conn);

        repo.upsert_session(&SessionFactRecord {
            provider: "claude-code".to_string(),
            provider_session_id: "cc-session".to_string(),
            ..Default::default()
        })
        .expect("upsert_session must succeed");
        repo.upsert_session(&SessionFactRecord {
            provider: "codex".to_string(),
            provider_session_id: "cx-session".to_string(),
            ..Default::default()
        })
        .expect("upsert_session must succeed");

        drop(conn);
        drop(store);

        // (a) --compaction unset: no session, of any provider, may carry a "compaction" key.
        let mut buf = Vec::new();
        cmd_trends(&db_path, None, 50, false, true, &mut buf).expect("cmd_trends must succeed with 2 sessions");
        let output = String::from_utf8(buf).expect("output must be valid utf8");
        let value: serde_json::Value = serde_json::from_str(&output).expect("output must be valid JSON");
        for session in value["sessions"].as_array().expect("sessions must be an array") {
            assert!(
                !session.as_object().unwrap().contains_key("compaction"),
                "compaction key must be absent when --compaction is not set, got:\n{output}"
            );
        }

        // (b) --compaction set, no --provider filter: the codex row must omit the key entirely
        // (never `"compaction": null`) even though the claude-code row legitimately carries one.
        let mut buf = Vec::new();
        cmd_trends(&db_path, None, 50, true, true, &mut buf).expect("cmd_trends must succeed with 2 sessions");
        let output = String::from_utf8(buf).expect("output must be valid utf8");
        let value: serde_json::Value = serde_json::from_str(&output).expect("output must be valid JSON");
        let sessions = value["sessions"].as_array().expect("sessions must be an array");
        let cc = sessions.iter().find(|s| s["session_id"] == "cc-session").expect("cc-session must be present");
        assert!(cc.as_object().unwrap().contains_key("compaction"), "claude-code row must carry compaction");
        let cx = sessions.iter().find(|s| s["session_id"] == "cx-session").expect("cx-session must be present");
        assert!(
            !cx.as_object().unwrap().contains_key("compaction"),
            "non-Claude-Code row must omit the compaction key entirely, never emit it as null, got:\n{output}"
        );
    }

    #[test]
    fn cmd_trends_json_mode_preserves_f32_cache_hit_ratio_precision() {
        // CRIT-LUMEN-186: routing SessionTrendPoint through serde_json::json!/Value widens its
        // f32 cache_hit_ratio to f64, reintroducing the exact binary-precision garbage the
        // rounding at trend.rs:55 was meant to prevent -- e.g. 66.7_f32 renders as
        // "66.69999694824219" once passed through serde_json::Value's f64-only Number type,
        // instead of the correct shortest-round-trip "66.7" that serializing the typed
        // SessionTrendPoint struct directly would produce.
        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        let store = SqliteStore::open(&db_path).expect("open store");
        let conn = store.connection().unwrap();
        let repo = SessionRepository::new(&conn);

        repo.upsert_session(&SessionFactRecord {
            provider: "claude-code".to_string(),
            provider_session_id: "s1".to_string(),
            economics: lumen_model::TokenEconomics { cache_hit_ratio: 66.666, ..Default::default() },
            ..Default::default()
        })
        .expect("upsert_session must succeed");
        repo.upsert_session(&SessionFactRecord {
            provider: "claude-code".to_string(),
            provider_session_id: "s2".to_string(),
            ..Default::default()
        })
        .expect("upsert_session must succeed");

        drop(conn);
        drop(store);

        let mut buf = Vec::new();
        cmd_trends(&db_path, None, 50, false, true, &mut buf).expect("cmd_trends must succeed with 2 sessions");
        let output = String::from_utf8(buf).expect("output must be valid utf8");

        assert!(
            output.contains("66.7") && !output.contains("66.69999") && !output.contains("66.7000001"),
            "cache_hit_ratio must round-trip through JSON output as exactly 66.7, got:\n{output}"
        );
    }

    #[test]
    fn cmd_ingest_persists_a_real_compact_boundary_event_end_to_end() {
        // CRIT-LUMEN-185: no fixture anywhere carried a real compact_boundary event, so the full
        // adapter-parse -> build_compaction_fact_records -> CompactionRepository::insert path had
        // zero end-to-end coverage through the real cmd_ingest entry point (as opposed to the
        // adapter-level or repository-level unit tests, which each exercise only one hop).
        let sample = concat!(
            "{\"type\":\"user\",\"sessionId\":\"e2e-compact-1\",\"parentUuid\":null,\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"e2e-compact-1\",\"parentUuid\":\"turn-0\",\"message\":{\"model\":\"claude-3-5-sonnet-20241022\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}],\"usage\":{\"input_tokens\":100,\"output_tokens\":10}}}\n",
            "{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"compactMetadata\":{\"trigger\":\"auto\",\"preTokens\":100000,\"postTokens\":20000,\"cumulativeDroppedTokens\":80000,\"durationMs\":1500}}\n",
        );
        let file = write_temp_file(sample);

        let db_dir = tempfile::tempdir().expect("create temp db dir");
        let db_path = Utf8PathBuf::from_path_buf(db_dir.path().join("lumen_test.db")).unwrap();

        cmd_ingest(file.path(), &db_path, true).expect("cmd_ingest must succeed on a real compact_boundary sample");

        let store = SqliteStore::open(&db_path).expect("reopen store");
        let conn = store.connection().unwrap();
        let trend_repo = TrendRepository::new(&conn);
        let points = trend_repo
            .list_session_trend(&TrendFilter { provider: None, limit: 50, require_compaction: true })
            .unwrap();
        assert_eq!(points.len(), 1);
        let compaction = points[0].compaction.as_ref().expect("session must have a compaction summary");
        assert_eq!(compaction.event_count, 1, "the real compact_boundary event must have been persisted");
        assert_eq!(compaction.tokens_dropped_total, 80000);
        assert_eq!(compaction.auto_count, 1);
        assert_eq!(compaction.manual_count, 0);
    }
}
