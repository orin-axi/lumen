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

    fn parse_stream<'a>(&self, _reader: Box<dyn BufRead + 'a>) -> Result<CanonicalTranscript, IngestionError> {
        let session_id = CompactString::new("codex-session");
        let model_family = CompactString::new("gpt-4o");
        let started_at = Utc::now();
        let ended_at = Utc::now();

        Ok(CanonicalTranscript {
            session_id,
            parent_session_id: None,
            orchestrator: OrchestratorKind::Codex,
            model_family: model_family.clone(),
            timing: ExecutionTiming {
                started_at,
                ended_at,
                wall_duration_ms: 0,
                active_duration_ms: 0,
                idle_duration_ms: 0,
                idle_gap_count: 0,
            },
            economics: TokenEconomics::calculate(0, 0, 0, 0, &model_family),
            turns: Vec::new(),
            subagents: Vec::new(),
            extracted_schemas: SmallVec::new(),
            detected_anomalies: SmallVec::new(),
        })
    }
}
