use lumen_model::OrchestratorKind;

/// Detects the orchestrator from a byte sample of the log file header (first 2048 bytes).
pub fn detect_orchestrator(sample_bytes: &[u8]) -> Option<OrchestratorKind> {
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

    if sample_str.contains("\"type\":\"event_msg\"") || sample_str.contains("\"type\":\"response_item\"") {
        return Some(OrchestratorKind::Codex);
    }

    if sample_str.contains("\"action\":\"run\"")
        || sample_str.contains("\"action\": \"run\"")
        || sample_str.contains("\"observation\":")
        || sample_str.contains("\"action\":\"message\"")
    {
        return Some(OrchestratorKind::OpenCode);
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
    fn test_detect_opencode_spaced_action_run() {
        // CRIT-LUMEN-108: detect_orchestrator must agree with OpenCodeAdapter::matches_fingerprint,
        // which already accepts the spaced JSON variant.
        let sample = b"{\"action\": \"run\", \"args\":{\"command\":\"cargo test\"}}";
        assert_eq!(detect_orchestrator(sample), Some(OrchestratorKind::OpenCode));
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
    fn test_detect_opencode_action_message() {
        // CRIT-LUMEN-108: "action":"message" is a real, meaningfully-parsed OpenCode event
        // (OpenCodeAdapter::parse_stream builds a CanonicalTurn from it) that detect_orchestrator
        // must also recognize so the two independently-coded paths cannot drift apart.
        let sample = b"{\"action\":\"message\",\"source\":\"assistant\",\"args\":{\"content\":\"done\"}}";
        assert_eq!(detect_orchestrator(sample), Some(OrchestratorKind::OpenCode));
    }
}
