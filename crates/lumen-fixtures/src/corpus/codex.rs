/// A real-shape Codex CLI rollout dump, matching the verbatim envelope structure confirmed
/// against a real Codex CLI 0.148.0 session file on 2026-08-20
/// (~/.codex/sessions/2026/08/20/rollout-2026-08-20T12-00-21-01a0208b-b21e-7c62-bc9b-7f386a299206.jsonl):
///
/// - `thread_settings_applied` carries both `service_tier` and `model` under `thread_settings`.
/// - `item_completed`'s `item.content` is an array of content blocks with a `text` field
///   (`UserMessage`/`AgentMessage`); there is no top-level `item.text`.
/// - `CommandExecution` items have no text/content field at all -- their real signal is the
///   `command` array (a shell argv list).
/// - `token_count`'s `total_token_usage` (and cache fields `cached_input_tokens` /
///   `cache_write_input_tokens`) live under `payload.info.total_token_usage`, not directly on
///   `payload` -- a real, confirmed nesting level the old fixture omitted.
/// - `total_token_usage` is a cumulative running total (not a per-line delta) -- the second
///   line's reading is the one that should survive last-write accounting (CRIT-LUMEN-110).
pub fn real_codex_session_dump() -> &'static str {
    r#"{"timestamp":"2026-08-20T10:00:00Z","ordinal":1,"type":"event_msg","payload":{"type":"thread_settings_applied","thread_id":"thread-abc123","thread_settings":{"model":"gpt-5.6-terra","service_tier":"Standard"}}}
{"timestamp":"2026-08-20T10:00:01Z","ordinal":2,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-abc123","item":{"type":"UserMessage","id":"item-1","content":[{"type":"text","text":"Fix the failing test","text_elements":[]}]}}}
{"timestamp":"2026-08-20T10:00:05Z","ordinal":3,"type":"event_msg","payload":{"type":"token_count","thread_id":"thread-abc123","info":{"total_token_usage":{"input_tokens":1200,"cached_input_tokens":400,"cache_write_input_tokens":10,"output_tokens":85,"reasoning_output_tokens":40,"total_tokens":1325}}}}
{"timestamp":"2026-08-20T10:00:06Z","ordinal":4,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-abc123","item":{"type":"AgentMessage","id":"item-2","content":[{"type":"Text","text":"Looking now."}]}}}
{"timestamp":"2026-08-20T10:00:10Z","ordinal":5,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-abc123","item":{"type":"CommandExecution","id":"exec-1","process_id":"1234","command":["/bin/zsh","-lc","cargo test"],"cwd":"file:///repo"}}}
{"timestamp":"2026-08-20T10:00:15Z","ordinal":6,"type":"event_msg","payload":{"type":"token_count","thread_id":"thread-abc123","info":{"total_token_usage":{"input_tokens":1500,"cached_input_tokens":900,"cache_write_input_tokens":25,"output_tokens":110,"reasoning_output_tokens":55,"total_tokens":1665}}}}
"#
}
