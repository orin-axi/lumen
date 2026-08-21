use clap::{Parser, Subcommand};
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};
use lumen_model::*;
use lumen_session::*;
use miette::{IntoDiagnostic, Result, miette};
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
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Render execution trajectory DAG and timeline
    Trace {
        /// Path to JSONL session log
        session_path: PathBuf,
    },
    /// Audit token economics, prompt cache hit %, and USD cost
    Audit {
        /// Path to JSONL session log
        session_path: PathBuf,
    },
    /// Parallel scan across all sessions in a directory
    Scan {
        /// Directory to scan
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Trace { session_path } => cmd_trace(&session_path, cli.json)?,
        Commands::Audit { session_path } => cmd_audit(&session_path, cli.json)?,
        Commands::Scan { dir } => cmd_scan(&dir, cli.json)?,
    }

    Ok(())
}

fn load_session(path: &Path) -> Result<CanonicalTranscript> {
    let file = File::open(path).into_diagnostic()?;
    let reader = BufReader::new(file);

    // Read initial sample for fingerprinting
    let mut sample_file = File::open(path).into_diagnostic()?;
    let mut buffer = [0u8; 2048];
    use std::io::Read;
    let n = sample_file.read(&mut buffer).unwrap_or(0);
    let sample = &buffer[..n];

    let orchestrator = detect_orchestrator(sample).ok_or_else(|| {
        miette!("{}: {} ({})", IngestionError::UnrecognizedFormat, path.display(), "no known orchestrator fingerprint matched")
    })?;

    match orchestrator {
        OrchestratorKind::ClaudeCode => ClaudeCodeAdapter.parse_stream(Box::new(reader)).into_diagnostic(),
        OrchestratorKind::Antigravity => AgyAdapter.parse_stream(Box::new(reader)).into_diagnostic(),
        OrchestratorKind::Codex => CodexAdapter.parse_stream(Box::new(reader)).into_diagnostic(),
        OrchestratorKind::OpenCode => OpenCodeAdapter.parse_stream(Box::new(reader)).into_diagnostic(),
        other => Err(miette!(
            "recognized orchestrator {:?} for {} but no adapter is implemented for it yet",
            other,
            path.display()
        )),
    }
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

    let mut table = Table::new();
    table.load_preset(UTF8_FULL).apply_modifier(UTF8_ROUND_CORNERS).set_header(Row::from(vec!["Metric", "Value"]));

    table.add_row(Row::from(vec!["Uncached Input Tokens", &format!("{}", eco.input_tokens)]));
    table.add_row(Row::from(vec!["Cache Creation (5m Write)", &format!("{}", eco.cache_creation_tokens)]));
    table.add_row(Row::from(vec!["Cache Read (0.10x Discount)", &format!("{}", eco.cache_read_tokens)]));
    table.add_row(Row::from(vec!["Output Tokens", &format!("{}", eco.output_tokens)]));
    table.add_row(Row::from(vec![
        Cell::new("Cache Hit Ratio").fg(Color::Green),
        Cell::new(format!("{:.1}%", eco.cache_hit_ratio)).fg(Color::Green),
    ]));
    table.add_row(Row::from(vec![
        Cell::new("Actual USD Spend").fg(Color::Cyan),
        Cell::new(format!("${:.4}", eco.total_cost_usd)).fg(Color::Cyan),
    ]));
    table.add_row(Row::from(vec!["Baseline Cost (No Cache)", &format!("${:.4}", eco.baseline_cost_no_cache_usd)]));
    table.add_row(Row::from(vec![
        Cell::new("Net Savings USD").fg(Color::Green),
        Cell::new(format!("${:.4}", eco.net_savings_usd)).fg(Color::Green),
    ]));
    table.add_row(Row::from(vec![
        Cell::new("Efficiency Multiplier").fg(Color::Yellow),
        Cell::new(format!("{:.2}x", eco.efficiency_multiplier)).fg(Color::Yellow),
    ]));

    println!("{table}");
    Ok(())
}

fn cmd_scan(dir: &Path, json_mode: bool) -> Result<()> {
    use std::fs;

    let mut sessions = Vec::new();

    if dir.is_dir() {
        for entry in fs::read_dir(dir).into_diagnostic()? {
            let entry = entry.into_diagnostic()?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                if let Ok(transcript) = load_session(&path) {
                    sessions.push(transcript);
                }
            }
        }
    }

    if json_mode {
        let json_out = serde_json::to_string_pretty(&sessions).into_diagnostic()?;
        println!("{}", json_out);
        return Ok(());
    }

    println!("\n Multi-Session Directory Scan: {}", dir.display());
    println!(" Total Sessions Discovered: {}\n", sessions.len());

    let mut table = Table::new();
    table.load_preset(UTF8_FULL).apply_modifier(UTF8_ROUND_CORNERS).set_header(Row::from(vec![
        "Session ID",
        "Orchestrator",
        "Turns",
        "Cache Hit %",
        "Cost USD",
        "Savings USD",
    ]));

    for sess in &sessions {
        table.add_row(Row::from(vec![
            Cell::new(sess.session_id.as_str()),
            Cell::new(format!("{:?}", sess.orchestrator)),
            Cell::new(sess.turns.len().to_string()),
            Cell::new(format!("{:.1}%", sess.economics.cache_hit_ratio)),
            Cell::new(format!("${:.4}", sess.economics.total_cost_usd)),
            Cell::new(format!("${:.4}", sess.economics.net_savings_usd)),
        ]));
    }

    println!("{table}");
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
        // no action:run/observation/action:message markers -- detect_orchestrator returns None.
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
}
