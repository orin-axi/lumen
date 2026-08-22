use lumen_model::OrchestratorKind;

/// Detects the orchestrator from a byte sample of the log file header (first 2048 bytes). For
/// OpenCode this is a fast, cheap pre-filter only (the SQLite file-format magic prefix, present
/// on any SQLite database): it identifies "this is a SQLite file", not specifically "this is an
/// OpenCode database" -- callers routing a match to `OpenCodeAdapter::parse_database` should
/// still expect it to reflect the real schema (or use `OpenCodeAdapter::matches_database` for an
/// authoritative, schema-verified check, which needs file access this byte-sample function
/// doesn't have).
pub fn detect_orchestrator(sample_bytes: &[u8]) -> Option<OrchestratorKind> {
    // SQLite's on-disk format always starts with this exact 16-byte magic string -- real
    // OpenCode data lives in `~/.local/share/opencode/opencode.db`, not a JSONL stream (see
    // OpenCodeAdapter's doc comment). Checked first since it's a binary prefix match, not a
    // string search, and can never collide with the JSONL-based checks below.
    if sample_bytes.starts_with(b"SQLite format 3\0") {
        return Some(OrchestratorKind::OpenCode);
    }

    let sample_str = match std::str::from_utf8(sample_bytes) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&sample_bytes[..e.valid_up_to()]).unwrap_or(""),
    };

    if sample_str.contains("\"sessionId\"") && sample_str.contains("\"parentUuid\"") {
        return Some(OrchestratorKind::ClaudeCode);
    }

    if sample_str.contains("\"step_index\"")
        && (sample_str.contains("\"source\":\"USER_EXPLICIT\"")
            || sample_str.contains("\"source\":\"MODEL\"")
            || sample_str.contains("\"PLANNER_RESPONSE\""))
    {
        return Some(OrchestratorKind::Antigravity);
    }

    if sample_str.contains("\"type\":\"event_msg\"")
        || sample_str.contains("\"type\":\"response_item\"")
        || sample_str.contains("\"type\":\"session_meta\"")
    {
        return Some(OrchestratorKind::Codex);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_claude_code() {
        let sample = b"{\"type\":\"assistant\",\"sessionId\":\"ef175eb8-0825-4122-934b\",\"parentUuid\":\"turn-0\",\"message\":{}}";
        assert_eq!(detect_orchestrator(sample), Some(OrchestratorKind::ClaudeCode));
    }

    #[test]
    fn test_detect_claude_code_sessionid_alone_is_not_claude_code() {
        let sample = b"{\"sessionId\":\"ef175eb8-0825-4122-934b\"}";
        assert_ne!(detect_orchestrator(sample), Some(OrchestratorKind::ClaudeCode));
    }

    #[test]
    fn test_detect_agy() {
        let sample = b"{\"step_index\":0,\"source\":\"USER_EXPLICIT\",\"type\":\"USER_INPUT\",\"status\":\"DONE\"}";
        assert_eq!(detect_orchestrator(sample), Some(OrchestratorKind::Antigravity));
    }

    #[test]
    fn test_detect_opencode_sqlite_magic_prefix() {
        // Real OpenCode data is a SQLite database (~/.local/share/opencode/opencode.db), not a
        // JSONL stream -- confirmed against a real local database this session. SQLite's
        // on-disk format always starts with this exact 16-byte magic string.
        let mut sample = b"SQLite format 3\0".to_vec();
        sample.extend_from_slice(&[0u8; 32]); // trailing header bytes, irrelevant to detection
        assert_eq!(detect_orchestrator(&sample), Some(OrchestratorKind::OpenCode));
    }

    #[test]
    fn test_fictional_opencode_jsonl_shape_no_longer_matches_anything() {
        // The action/observation JSONL shape this crate previously assumed for OpenCode was
        // confirmed this session to have zero basis in real OpenCode output (real data is
        // SQLite). A sample in that shape must now fall through to None, not silently match a
        // format that never existed.
        let sample = b"{\"action\": \"run\", \"args\":{\"command\":\"cargo test\"}}";
        assert_eq!(detect_orchestrator(sample), None);
    }

    #[test]
    fn test_detect_orchestrator_survives_boundary_truncated_utf8() {
        // A well-formed ClaudeCode sample (sessionId+parentUuid markers present) followed by a
        // multi-byte UTF-8 character ('é' = 0xC3 0xA9) sliced at a byte offset that splits the
        // character -- simulating the 2048-byte header truncation landing mid-character. The
        // valid prefix's markers must still be found rather than the whole sample collapsing to "".
        let mut sample = b"{\"type\":\"assistant\",\"sessionId\":\"ef175eb8-0825-4122-934b\",\"parentUuid\":\"turn-0\",\"note\":\"caf".to_vec();
        sample.push(0xC3); // leading byte of 'é'; trailing byte 0xA9 intentionally omitted
        assert_eq!(detect_orchestrator(&sample), Some(OrchestratorKind::ClaudeCode));
    }

    #[test]
    fn test_detect_codex_session_meta_first_line() {
        // Real Codex CLI rollout files (confirmed against a real local
        // ~/.codex/sessions/**/rollout-*.jsonl file, CLI version 0.148.0) start with a
        // "session_meta" envelope line that can be tens of kilobytes long (it embeds the full
        // base_instructions system prompt) -- long enough that the 2048-byte fingerprint sample
        // never reaches a later "event_msg"/"response_item" line. Without recognizing
        // "session_meta" itself, a real Codex file can go entirely unrecognized.
        let sample = b"{\"timestamp\":\"2026-08-20T19:00:39.735Z\",\"ordinal\":0,\"type\":\"session_meta\",\"payload\":{\"session_id\":\"01a0208b-b21e-7c62-bc9b-7f386a299206\",\"cli_version\":\"0.148.0\"";
        assert_eq!(detect_orchestrator(sample), Some(OrchestratorKind::Codex));
    }
}
