use lumen_fixtures::real_codex_session_dump;
use lumen_model::*;
use lumen_session::*;
use std::io::Cursor;

#[test]
fn test_codex_fingerprint_detection() {
    // CRIT-LUMEN-106: a real event_msg envelope sample is detected as Codex, under 1ms.
    let sample = br#"{"timestamp":"2026-08-20T10:00:00Z","ordinal":1,"type":"event_msg","payload":{"type":"thread_settings_applied","thread_id":"thread-abc123","thread_settings":{"service_tier":"Standard"}}}"#;
    let start = std::time::Instant::now();
    let detected = detect_orchestrator(sample);
    let duration = start.elapsed();

    assert_eq!(detected, Some(OrchestratorKind::Codex));
    assert!(duration.as_micros() < 1000);
}

#[test]
fn test_codex_adapter_parses_real_event_msg_envelope() {
    // CRIT-LUMEN-109/110/162: turn role mapping, cumulative (last-write, not summed)
    // token accounting, reasoning_output_tokens kept distinct, and service_tier capture.
    let sample = real_codex_session_dump();

    let adapter = CodexAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Codex session");

    assert_eq!(transcript.session_id, "thread-abc123");
    assert_eq!(transcript.orchestrator, OrchestratorKind::Codex);

    assert_eq!(transcript.turns.len(), 3);
    assert_eq!(transcript.turns[0].role, TurnRole::User);
    // Real UserMessage shape: item.content is an array of blocks, each carrying its own
    // `text` field -- there is no top-level `item.text`.
    assert_eq!(transcript.turns[0].text.as_deref(), Some("Fix the failing test"));
    assert_eq!(transcript.turns[1].role, TurnRole::Assistant);
    // Real AgentMessage shape: same content-array-of-blocks structure as UserMessage.
    assert_eq!(transcript.turns[1].text.as_deref(), Some("Looking now."));
    assert_eq!(transcript.turns[2].role, TurnRole::ToolResult);
    // Real CommandExecution items have no text/content field at all; the real signal is the
    // `command` array (a shell argv list), joined with spaces.
    assert_eq!(transcript.turns[2].text.as_deref(), Some("/bin/zsh -lc cargo test"));

    // Last-write, not summed: the second token_count line (1500/110/55) must win over the
    // first (1200/85/40). A naive sum would produce 2700/195.
    assert_eq!(transcript.economics.input_tokens, 1500);
    assert_eq!(transcript.economics.output_tokens, 110);
    assert_eq!(transcript.economics.reasoning_output_tokens, 55);

    assert_eq!(transcript.service_tier.as_deref(), Some("Standard"));

    // Bug 4: thread_settings_applied's adjacent, equally-real `model` field must update
    // model_family away from the hardcoded "gpt-4o" default.
    assert_eq!(transcript.model_family.as_str(), "gpt-5.6-terra");
}

#[test]
fn test_codex_adapter_non_utf8_line_is_skipped_not_fatal() {
    // CRIT-LUMEN-025: a non-UTF8 line surfaces as an io::Error from BufRead::lines(), not a
    // serde_json parse error -- the read-error branch must skip+record like the parse-error
    // branch does, not abort the whole parse and discard the surrounding valid lines.
    let mut sample: Vec<u8> = Vec::new();
    sample.extend_from_slice(
        br#"{"timestamp":"2026-08-20T10:00:00Z","ordinal":1,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-abc123","item":{"type":"UserMessage","content":[{"text":"first"}]}}}"#,
    );
    sample.push(b'\n');
    sample.extend_from_slice(b"invalid utf8 follows: \xFF\xFE\n");
    sample.extend_from_slice(
        br#"{"timestamp":"2026-08-20T10:00:02Z","ordinal":2,"type":"event_msg","payload":{"type":"item_completed","thread_id":"thread-abc123","item":{"type":"AgentMessage","content":[{"text":"third"}]}}}"#,
    );
    sample.push(b'\n');

    let adapter = CodexAdapter;
    let transcript =
        adapter.parse_stream(Box::new(Cursor::new(sample))).expect("a non-UTF8 line must not abort the whole parse");

    assert_eq!(transcript.parse_failures.len(), 1);
    assert_eq!(transcript.turns.len(), 2);
    assert_eq!(transcript.turns[0].text.as_deref(), Some("first"));
    assert_eq!(transcript.turns[1].text.as_deref(), Some("third"));
}

#[test]
fn test_codex_adapter_extracts_cache_tokens_from_real_nested_shape() {
    // Bug 1 + Bug 2: real Codex token_count payloads nest total_token_usage one level deeper,
    // under payload.info.total_token_usage -- not directly on payload -- and the real cache
    // field names are cached_input_tokens / cache_write_input_tokens (Codex's own names, NOT
    // Claude Code's cache_read_input_tokens / cache_creation_input_tokens). Before the fix,
    // payload.get("total_token_usage") always returned None against real data, so
    // input/output/reasoning tokens were always 0, and cache fields were hardcoded to 0
    // regardless of payload content.
    let sample = real_codex_session_dump();

    let adapter = CodexAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Codex session");

    // Last-write: the second token_count line's cached_input_tokens=900 /
    // cache_write_input_tokens=25 must win over the first line's 400/10.
    assert_eq!(transcript.economics.cache_read_tokens, 900, "cached_input_tokens must map to cache_read_tokens");
    assert_eq!(
        transcript.economics.cache_creation_tokens, 25,
        "cache_write_input_tokens must map to cache_creation_tokens"
    );
}

#[test]
fn test_codex_adapter_prices_tiered_session_nonzero() {
    // Blocker #2 from adversarial review: real Codex sessions commonly report a service_tier
    // (this fixture carries service_tier: "Standard") via thread_settings_applied. Before the
    // rate_for tier-fallback fix, PricingTable::seed()'s tier:None rows never matched
    // TurnPricingInput's tier: Some("Standard"), so total_cost_usd silently came out as 0.0 for
    // any Codex session that reported a service tier -- regardless of real token volume.
    let sample = real_codex_session_dump();

    let adapter = CodexAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Codex session");

    assert_eq!(transcript.service_tier.as_deref(), Some("Standard"));
    assert_eq!(transcript.economics.input_tokens, 1500);
    assert_eq!(transcript.economics.output_tokens, 110);

    // model_family is "gpt-5.6-terra" (Bug 4 fix), extracted from the fixture's real
    // thread_settings.model field, and is now a real seeded model (official OpenAI pricing,
    // developers.openai.com/api/docs/pricing, fetched 2026-08-21): $2.00/M input, $2.00/M
    // cache-write (OpenAI has no separate cache-write charge), $0.20/M cache-read, $12.00/M
    // output, $12.00/M reasoning (same judgment-call convention as every other seeded model).
    let expected_cost = 1500.0 * 2.00 / 1_000_000.0
        + 25.0 * 2.00 / 1_000_000.0
        + 900.0 * 0.20 / 1_000_000.0
        + 110.0 * 12.00 / 1_000_000.0
        + 55.0 * 12.00 / 1_000_000.0;
    assert!(
        transcript.economics.total_cost_usd > 0.0,
        "a Codex session with a real service_tier must not silently price at $0.00"
    );
    assert!(
        (transcript.economics.total_cost_usd - expected_cost).abs() < 1e-9,
        "expected total_cost_usd {expected_cost}, got {}",
        transcript.economics.total_cost_usd
    );
    assert!(transcript.economics.is_fully_priced, "gpt-5.6-terra is a real seeded model, not an unpriced fallback");
}

#[test]
fn test_codex_adapter_reasoning_tokens_flow_into_pricing_math() {
    // High-severity adversarial finding: reasoning_output_tokens was tracked but structurally
    // could never be priced -- it was patched onto TokenEconomics AFTER calculate() had already
    // summed total_cost_usd, so the dollar amount never reflected it. This proves the fixture's
    // 55 reasoning tokens now flow through TurnTokenUsage into the actual pricing math, not
    // just that the disconnected reasoning_output_tokens counter is populated.
    let sample = real_codex_session_dump();

    let adapter = CodexAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Codex session");

    assert_eq!(transcript.economics.reasoning_output_tokens, 55);

    // model_family "gpt-5.6-terra" is a real seeded model (see
    // test_codex_adapter_prices_tiered_session_nonzero for the sourced rates).
    let cost_without_reasoning = 1500.0 * 2.00 / 1_000_000.0
        + 25.0 * 2.00 / 1_000_000.0
        + 900.0 * 0.20 / 1_000_000.0
        + 110.0 * 12.00 / 1_000_000.0;
    let reasoning_contribution = 55.0 * 12.00 / 1_000_000.0;

    assert!(
        (transcript.economics.total_cost_usd - (cost_without_reasoning + reasoning_contribution)).abs() < 1e-9,
        "total_cost_usd must include the reasoning tokens' own dollar contribution ({reasoning_contribution}), \
         not just the input/output cost ({cost_without_reasoning})"
    );
    assert!(
        (transcript.economics.total_cost_usd - cost_without_reasoning).abs() > 1e-9,
        "total_cost_usd must differ from the input/output-only cost -- proving reasoning tokens \
         genuinely reach the pricing math rather than being priced at an effective $0.00"
    );
}

#[test]
fn test_codex_adapter_matches_fingerprint_parity_with_detect_orchestrator() {
    // CRIT-LUMEN-108: CodexAdapter::matches_fingerprint must agree with detect_orchestrator
    // on both a real matching sample and a non-matching sample.
    let adapter = CodexAdapter;

    let matching = real_codex_session_dump();
    assert_eq!(
        adapter.matches_fingerprint(matching),
        detect_orchestrator(matching.as_bytes()) == Some(OrchestratorKind::Codex)
    );
    assert!(adapter.matches_fingerprint(matching));

    let non_matching = r#"{"sessionId":"abc","parentUuid":"turn-0","type":"assistant"}"#;
    assert_eq!(
        adapter.matches_fingerprint(non_matching),
        detect_orchestrator(non_matching.as_bytes()) == Some(OrchestratorKind::Codex)
    );
    assert!(!adapter.matches_fingerprint(non_matching));
}

#[test]
fn test_codex_adapter_records_response_item_as_parse_failure() {
    // response_item envelopes are accepted by fingerprint detection (detect_orchestrator /
    // matches_fingerprint) but parse_stream has no implemented parser for their internal
    // schema. A file composed of response_item lines must not silently parse into an
    // empty-looking transcript -- it must surface real signal via parse_failures.
    let sample = concat!(
        r#"{"timestamp":"2026-08-20T10:00:00Z","type":"response_item","payload":{"id":"item-1"}}"#,
        "\n",
        r#"{"timestamp":"2026-08-20T10:00:01Z","type":"response_item","payload":{"id":"item-2"}}"#,
        "\n",
    );

    let adapter = CodexAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Codex session");

    assert_eq!(transcript.turns.len(), 0);
    assert_eq!(transcript.parse_failures.len(), 2, "each response_item line must be recorded as a parse failure");
    for failure in &transcript.parse_failures {
        assert!(
            failure.error.contains("response_item"),
            "expected parse failure to mention response_item, got: {}",
            failure.error
        );
    }
}

#[test]
fn test_codex_adapter_event_msg_regression_unaffected_by_response_item_handling() {
    // Confirms the response_item dispatch arm doesn't disturb the existing event_msg parsing
    // path -- same assertions as test_codex_adapter_parses_real_event_msg_envelope.
    let sample = real_codex_session_dump();

    let adapter = CodexAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Codex session");

    assert_eq!(transcript.turns.len(), 3);
    assert_eq!(transcript.turns[0].role, TurnRole::User);
    assert_eq!(transcript.turns[1].role, TurnRole::Assistant);
    assert_eq!(transcript.turns[2].role, TurnRole::ToolResult);
    assert_eq!(transcript.economics.input_tokens, 1500);
    assert_eq!(transcript.economics.output_tokens, 110);
    assert_eq!(transcript.economics.reasoning_output_tokens, 55);
    assert!(transcript.parse_failures.is_empty(), "a clean event_msg-only session must have no parse failures");
}

#[test]
fn test_codex_adapter_parse_failure_byte_offset_tracks_real_position() {
    // A malformed line NOT at the start of the file must report a non-zero byte_offset
    // reflecting the real accumulated byte count of preceding lines; a malformed first line
    // must report byte_offset == 0.
    let first_line_malformed = "not valid json\n{\"timestamp\":\"2026-08-20T10:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"thread_settings_applied\",\"thread_id\":\"t\",\"thread_settings\":{\"service_tier\":\"Standard\"}}}\n";

    let adapter = CodexAdapter;
    let transcript =
        adapter.parse_stream(Box::new(Cursor::new(first_line_malformed))).expect("Failed to parse Codex session");
    assert_eq!(transcript.parse_failures.len(), 1);
    assert_eq!(transcript.parse_failures[0].byte_offset, 0, "a malformed first line has byte_offset 0");

    let valid_first_line = "{\"timestamp\":\"2026-08-20T10:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"thread_settings_applied\",\"thread_id\":\"t\",\"thread_settings\":{\"service_tier\":\"Standard\"}}}\n";
    let sample_with_later_malformed = format!("{valid_first_line}not valid json\n");
    let transcript2 = adapter
        .parse_stream(Box::new(Cursor::new(sample_with_later_malformed.as_str())))
        .expect("Failed to parse Codex session");
    assert_eq!(transcript2.parse_failures.len(), 1);
    assert_eq!(
        transcript2.parse_failures[0].byte_offset,
        valid_first_line.len(),
        "a malformed non-first line must report the real accumulated byte offset (LF-based approximation: stripped-line length + 1 per preceding line)"
    );
}

#[test]
fn test_codex_adapter_wall_duration_reflects_real_log_timestamps() {
    // Medium-severity adversarial finding: `started_at` was declared without `mut` and pinned
    // to Utc::now() at parse time, never updated from real log timestamps (unlike
    // claude.rs/agy.rs/opencode.rs, which all track a `has_start` flag and set `started_at` to
    // the FIRST real timestamp seen). Since real logs are historical, `ended_at - started_at`
    // (a later wall-clock time) was always negative and clamped to 0 via `.max(0)`, so every
    // Codex session's wall_duration_ms/active_duration_ms was silently always 0.
    //
    // The fixture's real timestamps span 2026-08-20T10:00:00Z (first line) through
    // 2026-08-20T10:00:15Z (last line) -- exactly 15 real seconds = 15000ms. This must be the
    // EXACT value, proving started_at is genuinely the first timestamp seen, not just
    // "not wall-clock-now".
    let sample = real_codex_session_dump();

    let adapter = CodexAdapter;
    let transcript = adapter.parse_stream(Box::new(Cursor::new(sample))).expect("Failed to parse Codex session");

    assert_eq!(
        transcript.timing.wall_duration_ms, 15000,
        "wall_duration_ms must reflect the real elapsed time between the fixture's first \
         (10:00:00Z) and last (10:00:15Z) timestamps, not be clamped to 0 by a started_at \
         pinned to wall-clock parse time"
    );
    assert_eq!(transcript.timing.active_duration_ms, 15000);
}

#[test]
fn test_codex_adapter_does_not_claim_precedence_over_claude_code() {
    // detect_orchestrator's ordered if-chain checks ClaudeCode markers first. A sample
    // containing BOTH ClaudeCode and Codex markers resolves to ClaudeCode; CodexAdapter's own
    // matches_fingerprint must not independently claim it via its own standalone condition.
    let dual_marker = r#"{"sessionId":"x","parentUuid":"y","type":"event_msg"}"#;
    assert_eq!(detect_orchestrator(dual_marker.as_bytes()), Some(OrchestratorKind::ClaudeCode));

    let adapter = CodexAdapter;
    assert_eq!(
        adapter.matches_fingerprint(dual_marker),
        detect_orchestrator(dual_marker.as_bytes()) == Some(OrchestratorKind::Codex)
    );
    assert!(!adapter.matches_fingerprint(dual_marker));
}
