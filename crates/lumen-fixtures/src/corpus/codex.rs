/// A real Codex CLI rollout dump: one `thread_settings_applied` event_msg establishing
/// `service_tier: "Standard"`, three `item_completed` event_msg lines (UserMessage,
/// AgentMessage, CommandExecution), and two `token_count` event_msg lines whose
/// `total_token_usage` is a cumulative running total (not a per-line delta) -- the second
/// line's reading is the one that should survive last-write accounting (CRIT-LUMEN-110).
pub fn real_codex_session_dump() -> &'static str {
    r#"{"timestamp":"2026-08-20T10:00:00Z","ordinal":1,"type":"event_msg","payload":{"type":"thread_settings_applied","thread_id":"thread-abc123","thread_settings":{"service_tier":"Standard"}}}
{"timestamp":"2026-08-20T10:00:01Z","ordinal":2,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-abc123","item":{"type":"UserMessage","text":"Fix the failing test"}}}
{"timestamp":"2026-08-20T10:00:05Z","ordinal":3,"type":"event_msg","payload":{"type":"token_count","thread_id":"thread-abc123","total_token_usage":{"input_tokens":1200,"output_tokens":85,"reasoning_output_tokens":40}}}
{"timestamp":"2026-08-20T10:00:06Z","ordinal":4,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-abc123","item":{"type":"AgentMessage","text":"Looking now."}}}
{"timestamp":"2026-08-20T10:00:10Z","ordinal":5,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-abc123","item":{"type":"CommandExecution","text":"cargo test"}}}
{"timestamp":"2026-08-20T10:00:15Z","ordinal":6,"type":"event_msg","payload":{"type":"token_count","thread_id":"thread-abc123","total_token_usage":{"input_tokens":1500,"output_tokens":110,"reasoning_output_tokens":55}}}
"#
}
