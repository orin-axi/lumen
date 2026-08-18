use lumen_model::OrchestratorKind;

/// Detects the orchestrator from a byte sample of the log file header (first 2048 bytes).
pub fn detect_orchestrator(sample_bytes: &[u8]) -> Option<OrchestratorKind> {
    let sample_str = std::str::from_utf8(sample_bytes).unwrap_or("");

    if sample_str.contains("\"sessionId\"")
        && (sample_str.contains("\"permission-mode\"")
            || sample_str.contains("\"leafUuid\"")
            || sample_str.contains("\"parentUuid\"")
            || sample_str.contains("\"isSidechain\""))
    {
        return Some(OrchestratorKind::ClaudeCode);
    }

    if sample_str.contains("\"step_index\"")
        && (sample_str.contains("\"source\":\"USER_EXPLICIT\"")
            || sample_str.contains("\"source\":\"MODEL\"")
            || sample_str.contains("\"PLANNER_RESPONSE\""))
    {
        return Some(OrchestratorKind::Antigravity);
    }

    if sample_str.contains("\"choices\"")
        || sample_str.contains("\"prompt_tokens\"")
        || sample_str.contains("\"thread_id\"")
    {
        return Some(OrchestratorKind::Codex);
    }

    if sample_str.contains("\"action\":\"run\"") || sample_str.contains("\"observation\":") {
        return Some(OrchestratorKind::OpenCode);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_claude_code() {
        let sample = b"{\"type\":\"mode\",\"mode\":\"normal\",\"sessionId\":\"ef175eb8-0825-4122-934b\"}\n{\"type\":\"permission-mode\"}";
        assert_eq!(detect_orchestrator(sample), Some(OrchestratorKind::ClaudeCode));
    }

    #[test]
    fn test_detect_agy() {
        let sample = b"{\"step_index\":0,\"source\":\"USER_EXPLICIT\",\"type\":\"USER_INPUT\",\"status\":\"DONE\"}";
        assert_eq!(detect_orchestrator(sample), Some(OrchestratorKind::Antigravity));
    }
}
