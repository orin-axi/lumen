use chrono::{TimeZone, Utc};
use lumen_model::*;

#[test]
fn test_core_pricing_rates_and_unrecognized_model_fallback() {
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    // CRIT-LUMEN-002: Claude 3.5 Sonnet
    assert!((table.rate_for("claude-3-5-sonnet", None, TokenRateKind::Input, as_of) - 3.00).abs() < 1e-9);
    assert!((table.rate_for("claude-3-5-sonnet", None, TokenRateKind::CacheWrite, as_of) - 3.75).abs() < 1e-9);
    assert!((table.rate_for("claude-3-5-sonnet", None, TokenRateKind::CacheRead, as_of) - 0.30).abs() < 1e-9);
    assert!((table.rate_for("claude-3-5-sonnet", None, TokenRateKind::Output, as_of) - 15.00).abs() < 1e-9);

    // CRIT-LUMEN-003: Claude 3.5 Haiku
    assert!((table.rate_for("claude-3-5-haiku", None, TokenRateKind::Input, as_of) - 0.80).abs() < 1e-9);
    assert!((table.rate_for("claude-3-5-haiku", None, TokenRateKind::CacheWrite, as_of) - 1.00).abs() < 1e-9);
    assert!((table.rate_for("claude-3-5-haiku", None, TokenRateKind::CacheRead, as_of) - 0.08).abs() < 1e-9);
    assert!((table.rate_for("claude-3-5-haiku", None, TokenRateKind::Output, as_of) - 4.00).abs() < 1e-9);

    // CRIT-LUMEN-004: Qwen 2.5 Coder
    assert!((table.rate_for("qwen-2.5-coder", None, TokenRateKind::Input, as_of) - 0.20).abs() < 1e-9);
    assert!((table.rate_for("qwen-2.5-coder", None, TokenRateKind::CacheRead, as_of) - 0.05).abs() < 1e-9);
    assert!((table.rate_for("qwen-2.5-coder", None, TokenRateKind::Output, as_of) - 0.60).abs() < 1e-9);

    // CRIT-LUMEN-008 (revised): a genuinely unrecognized model returns 0.0 for every kind --
    // never another model's rate. Confirmed against real local session data that the earlier
    // silent-fallback-to-Sonnet behavior mispriced every current real model (none of which are
    // seeded under their old names), so callers must treat an unrecognized model as explicitly
    // unpriced (see PricingTable::is_recognized) rather than receive a plausible-looking number.
    assert!(!table.is_recognized("totally-unrecognized-model-xyz"));
    for kind in [TokenRateKind::Input, TokenRateKind::CacheWrite, TokenRateKind::CacheRead, TokenRateKind::Output] {
        let unrecognized = table.rate_for("totally-unrecognized-model-xyz", None, kind, as_of);
        assert_eq!(unrecognized, 0.0, "unrecognized model must price at 0.0 for {kind:?}, never a substituted rate");
    }
}

#[test]
fn test_claude_sonnet_pricing_calculation() {
    // Migrated off the removed ModelPricing::CLAUDE_3_5_SONNET constant (TASK-MODEL-008) onto
    // PricingTable::seed() + TokenEconomics::calculate, which carries the same dollar rates
    // forward unchanged.
    let pricing = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    let usage = TurnTokenUsage {
        input_tokens: 1_000_000,          // $3.00
        output_tokens: 1_000_000,         // $15.00
        cache_creation_tokens: 1_000_000, // $3.75 (1.25x)
        cache_read_tokens: 1_000_000,     // $0.30 (0.10x - 90% savings)
        reasoning_tokens: 0,
        cache_creation_1h_tokens: 0,
    };

    let turn = TurnPricingInput { usage, timestamp: as_of, tier: None };
    let econ = TokenEconomics::calculate(&[turn], "claude-3-5-sonnet", &pricing, None);

    let expected_cost = 3.00 + 3.75 + 0.30 + 15.00;
    assert!((econ.total_cost_usd - expected_cost).abs() < 1e-6);

    // Baseline prompt = 3M tokens @ $3.00/M ($9.00) + 1M output ($15.00) = $24.00
    let expected_baseline = 9.00 + 15.00;
    assert!((econ.baseline_cost_no_cache_usd - expected_baseline).abs() < 1e-6);
}

#[test]
fn test_all_model_pricing_rates_and_fallbacks() {
    // Migrated off the removed ModelPricing constants/from_model_name (TASK-MODEL-008) onto
    // PricingTable::seed() + rate_for. Model keys are the short canonical names seed() indexes
    // rows by, since rate_for does an exact match (no substring matching on date-suffixed
    // names the way ModelPricing::from_model_name did).
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    // CRIT-LUMEN-002: Claude 3.5 Sonnet
    assert_eq!(table.rate_for("claude-3-5-sonnet", None, TokenRateKind::Input, as_of), 3.00);
    assert_eq!(table.rate_for("claude-3-5-sonnet", None, TokenRateKind::CacheWrite, as_of), 3.75);
    assert_eq!(table.rate_for("claude-3-5-sonnet", None, TokenRateKind::CacheRead, as_of), 0.30);
    assert_eq!(table.rate_for("claude-3-5-sonnet", None, TokenRateKind::Output, as_of), 15.00);

    // CRIT-LUMEN-003: Claude 3.5 Haiku
    assert_eq!(table.rate_for("claude-3-5-haiku", None, TokenRateKind::Input, as_of), 0.80);
    assert_eq!(table.rate_for("claude-3-5-haiku", None, TokenRateKind::CacheWrite, as_of), 1.00);
    assert_eq!(table.rate_for("claude-3-5-haiku", None, TokenRateKind::CacheRead, as_of), 0.08);
    assert_eq!(table.rate_for("claude-3-5-haiku", None, TokenRateKind::Output, as_of), 4.00);

    // CRIT-LUMEN-004: Qwen 2.5 Coder. seed() has no CacheWrite row for Qwen (unlike the old
    // ModelPricing::QWEN_2_5_CODER constant, which set cache_write_per_m to 0.20 same as
    // input); per CRIT-LUMEN-161 a recognized model missing a specific rate kind returns 0.0,
    // it does not fall back to another model's or its own input rate.
    assert_eq!(table.rate_for("qwen-2.5-coder", None, TokenRateKind::Input, as_of), 0.20);
    assert_eq!(table.rate_for("qwen-2.5-coder", None, TokenRateKind::CacheWrite, as_of), 0.0);
    assert_eq!(table.rate_for("qwen-2.5-coder", None, TokenRateKind::CacheRead, as_of), 0.05);
    assert_eq!(table.rate_for("qwen-2.5-coder", None, TokenRateKind::Output, as_of), 0.60);

    // DeepSeek R1
    assert_eq!(table.rate_for("deepseek-r1", None, TokenRateKind::Input, as_of), 0.55);
    assert_eq!(table.rate_for("deepseek-r1", None, TokenRateKind::CacheRead, as_of), 0.14);
    assert_eq!(table.rate_for("deepseek-r1", None, TokenRateKind::Output, as_of), 2.19);

    // Gemini 2.0 Flash. Now vendored (CRIT-LUMEN-170): the dollars-per-token -> rate_per_m
    // conversion (`* 1_000_000.0`) is not always float-exact, so this uses tolerance like the
    // rest of the file's vendored-model assertions instead of assert_eq!.
    assert!((table.rate_for("gemini-2.0-flash", None, TokenRateKind::Input, as_of) - 0.10).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-flash", None, TokenRateKind::CacheRead, as_of) - 0.025).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-flash", None, TokenRateKind::Output, as_of) - 0.40).abs() < 1e-9);

    // CRIT-LUMEN-008 (revised): unrecognized model string prices at 0.0, never a substituted
    // rate -- see test_core_pricing_rates_and_unrecognized_model_fallback for the full rationale.
    assert!(!table.is_recognized("some-obscure-custom-llm-v1"));
    for kind in [TokenRateKind::Input, TokenRateKind::CacheWrite, TokenRateKind::CacheRead, TokenRateKind::Output] {
        let unrecognized = table.rate_for("some-obscure-custom-llm-v1", None, kind, as_of);
        assert_eq!(unrecognized, 0.0);
    }
}

#[test]
fn test_token_economics_zero_division_clamping_and_scale() {
    let pricing = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    // (a) CRIT-LUMEN-007: an empty turns slice must not divide by zero and returns a zeroed
    // TokenEconomics with cache_hit_ratio 0.0 and efficiency_multiplier 1.0.
    let empty_econ = TokenEconomics::calculate(&[], "claude-3-5-sonnet", &pricing, None);
    assert_eq!(empty_econ.cache_hit_ratio, 0.0);
    assert_eq!(empty_econ.efficiency_multiplier, 1.0);
    assert_eq!(empty_econ.total_cost_usd, 0.0);

    // (b) CRIT-LUMEN-005: cache_read_tokens exactly half of total prompt tokens must yield
    // cache_hit_ratio == 50.0 on the 0-100 percentage scale (not 0.5).
    let half_cache_turn = TurnPricingInput {
        usage: TurnTokenUsage {
            input_tokens: 100,
            cache_creation_tokens: 100,
            cache_read_tokens: 200,
            output_tokens: 50,
            reasoning_tokens: 0,
            cache_creation_1h_tokens: 0,
        },
        timestamp: as_of,
        tier: None,
    };
    let half_cache_econ = TokenEconomics::calculate(&[half_cache_turn], "claude-3-5-sonnet", &pricing, None);
    assert_eq!(half_cache_econ.cache_hit_ratio, 50.0);

    // (c) CRIT-LUMEN-009: a turn with zero cache activity has baseline_cost_no_cache_usd equal
    // to total_cost_usd, and net_savings_usd must clamp to 0.0 (never negative).
    let no_cache_turn = TurnPricingInput {
        usage: TurnTokenUsage {
            input_tokens: 1_000_000,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            output_tokens: 500_000,
            reasoning_tokens: 0,
            cache_creation_1h_tokens: 0,
        },
        timestamp: as_of,
        tier: None,
    };
    let no_cache_econ = TokenEconomics::calculate(&[no_cache_turn], "claude-3-5-sonnet", &pricing, None);
    assert!((no_cache_econ.baseline_cost_no_cache_usd - no_cache_econ.total_cost_usd).abs() < 1e-9);
    assert_eq!(no_cache_econ.net_savings_usd, 0.0);

    // (d) CRIT-LUMEN-010: TurnTokenUsage::prompt_tokens returns the sum of input, cache
    // creation, and cache read tokens.
    let usage = TurnTokenUsage {
        input_tokens: 100,
        cache_creation_tokens: 200,
        cache_read_tokens: 300,
        output_tokens: 400,
        reasoning_tokens: 0,
        cache_creation_1h_tokens: 0,
    };
    assert_eq!(usage.prompt_tokens(), 600);

    // (e) CRIT-LUMEN-006: a mixed-cache turn's efficiency_multiplier equals
    // baseline_cost_no_cache_usd / total_cost_usd exactly.
    let mixed_turn = TurnPricingInput {
        usage: TurnTokenUsage {
            input_tokens: 100_000,
            cache_creation_tokens: 50_000,
            cache_read_tokens: 50_000,
            output_tokens: 10_000,
            reasoning_tokens: 0,
            cache_creation_1h_tokens: 0,
        },
        timestamp: as_of,
        tier: None,
    };
    let mixed_econ = TokenEconomics::calculate(&[mixed_turn], "claude-3-5-sonnet", &pricing, None);
    let expected_multiplier = (mixed_econ.baseline_cost_no_cache_usd / mixed_econ.total_cost_usd) as f32;
    assert_eq!(mixed_econ.efficiency_multiplier, expected_multiplier);
}

#[test]
fn test_extended_pricing_matrix() {
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    // CRIT-LUMEN-099: Claude Opus
    assert!((table.rate_for("claude-opus", None, TokenRateKind::Input, as_of) - 15.00).abs() < 1e-9);
    assert!((table.rate_for("claude-opus", None, TokenRateKind::CacheWrite, as_of) - 18.75).abs() < 1e-9);
    assert!((table.rate_for("claude-opus", None, TokenRateKind::CacheRead, as_of) - 1.50).abs() < 1e-9);
    assert!((table.rate_for("claude-opus", None, TokenRateKind::Output, as_of) - 75.00).abs() < 1e-9);

    // CRIT-LUMEN-100: GPT-4o. Now vendored (CRIT-LUMEN-170): LiteLLM publishes no
    // cache_creation_input_token_cost for gpt-4o at all (unlike the prior hand-typed
    // same-as-input approximation), so CacheWrite correctly returns 0.0 per CRIT-LUMEN-161
    // (recognized model, this specific kind genuinely absent) rather than a synthesized value.
    assert!((table.rate_for("gpt-4o", None, TokenRateKind::Input, as_of) - 2.50).abs() < 1e-9);
    assert_eq!(table.rate_for("gpt-4o", None, TokenRateKind::CacheWrite, as_of), 0.0);
    assert!((table.rate_for("gpt-4o", None, TokenRateKind::CacheRead, as_of) - 1.25).abs() < 1e-9);
    assert!((table.rate_for("gpt-4o", None, TokenRateKind::Output, as_of) - 10.00).abs() < 1e-9);

    // CRIT-LUMEN-101: DeepSeek R1
    assert!((table.rate_for("deepseek-r1", None, TokenRateKind::Input, as_of) - 0.55).abs() < 1e-9);
    assert!((table.rate_for("deepseek-r1", None, TokenRateKind::CacheWrite, as_of) - 0.55).abs() < 1e-9);
    assert!((table.rate_for("deepseek-r1", None, TokenRateKind::CacheRead, as_of) - 0.14).abs() < 1e-9);
    assert!((table.rate_for("deepseek-r1", None, TokenRateKind::Output, as_of) - 2.19).abs() < 1e-9);

    // CRIT-LUMEN-102: Kimi K1.5
    assert!((table.rate_for("kimi-k1.5", None, TokenRateKind::Input, as_of) - 0.50).abs() < 1e-9);
    assert!((table.rate_for("kimi-k1.5", None, TokenRateKind::CacheWrite, as_of) - 0.50).abs() < 1e-9);
    assert!((table.rate_for("kimi-k1.5", None, TokenRateKind::CacheRead, as_of) - 0.10).abs() < 1e-9);
    assert!((table.rate_for("kimi-k1.5", None, TokenRateKind::Output, as_of) - 2.00).abs() < 1e-9);

    // CRIT-LUMEN-103: GLM-4-Plus
    assert!((table.rate_for("glm-4-plus", None, TokenRateKind::Input, as_of) - 1.40).abs() < 1e-9);
    assert!((table.rate_for("glm-4-plus", None, TokenRateKind::CacheWrite, as_of) - 1.40).abs() < 1e-9);
    assert!((table.rate_for("glm-4-plus", None, TokenRateKind::CacheRead, as_of) - 0.20).abs() < 1e-9);
    assert!((table.rate_for("glm-4-plus", None, TokenRateKind::Output, as_of) - 1.40).abs() < 1e-9);

    // CRIT-LUMEN-104: Gemini 2.0 Flash. Now vendored (CRIT-LUMEN-170): same correction as
    // gpt-4o above -- no published cache-write rate, so CacheWrite is genuinely 0.0.
    assert!((table.rate_for("gemini-2.0-flash", None, TokenRateKind::Input, as_of) - 0.10).abs() < 1e-9);
    assert_eq!(table.rate_for("gemini-2.0-flash", None, TokenRateKind::CacheWrite, as_of), 0.0);
    assert!((table.rate_for("gemini-2.0-flash", None, TokenRateKind::CacheRead, as_of) - 0.025).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-flash", None, TokenRateKind::Output, as_of) - 0.40).abs() < 1e-9);

    // CRIT-LUMEN-105: Gemini 2.0 Pro
    assert!((table.rate_for("gemini-2.0-pro", None, TokenRateKind::Input, as_of) - 1.25).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-pro", None, TokenRateKind::CacheWrite, as_of) - 1.25).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-pro", None, TokenRateKind::CacheRead, as_of) - 0.30).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-pro", None, TokenRateKind::Output, as_of) - 5.00).abs() < 1e-9);

    // CacheWrite == Input for the remaining legacy non-Anthropic models (Opus is Anthropic and
    // has a distinct 1.25x cache-write premium, so it is excluded; gpt-4o and gemini-2.0-flash
    // are excluded too -- now vendored, and CRIT-LUMEN-170 revealed neither publishes a
    // cache-write rate at all, so this same-as-input approximation no longer applies to them).
    for model in ["deepseek-r1", "kimi-k1.5", "glm-4-plus", "gemini-2.0-pro"] {
        let input_rate = table.rate_for(model, None, TokenRateKind::Input, as_of);
        let cache_write_rate = table.rate_for(model, None, TokenRateKind::CacheWrite, as_of);
        assert!((input_rate - cache_write_rate).abs() < 1e-9, "{model}: CacheWrite rate should equal Input rate");
    }
}

/// CRIT-LUMEN-159: PricingTable::rate_for must select the row whose [effective_from,
/// effective_until) window contains the CALLING TURN's own as_of timestamp -- not wall-clock
/// now -- and must resolve authoring-error overlaps deterministically: latest effective_from
/// wins, and among ties on effective_from, the row LAST in PricingTable.rates' declaration
/// order wins.
#[test]
fn test_rate_for_versioned_lookup_and_tie_break() {
    // (a) Two non-overlapping consecutive windows for the same (model, tier, kind)
    // representing a price change. as_of inside each window must select that window's row.
    let jan_1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let jun_1 = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
    let dec_1 = Utc.with_ymd_and_hms(2024, 12, 1, 0, 0, 0).unwrap();

    let windowed_table = PricingTable {
        rates: vec![
            PricingRate {
                model: "test-model".into(),
                tier: None,
                kind: TokenRateKind::Input,
                rate_per_m: 1.00,
                effective_from: jan_1,
                effective_until: Some(jun_1),
            },
            PricingRate {
                model: "test-model".into(),
                tier: None,
                kind: TokenRateKind::Input,
                rate_per_m: 2.00,
                effective_from: jun_1,
                effective_until: None,
            },
        ],
    };

    let mid_first_window = Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap();
    let mid_second_window = Utc.with_ymd_and_hms(2024, 9, 1, 0, 0, 0).unwrap();

    assert!(
        (windowed_table.rate_for("test-model", None, TokenRateKind::Input, mid_first_window) - 1.00).abs() < 1e-9,
        "as_of inside the first window must select the first rate, proving the CALLING TURN's \
         own timestamp -- not wall-clock now -- selects the row"
    );
    assert!(
        (windowed_table.rate_for("test-model", None, TokenRateKind::Input, mid_second_window) - 2.00).abs() < 1e-9,
        "as_of inside the second window must select the second rate"
    );

    // (b) Identical effective_from, no effective_until: an authoring-error overlap.
    // rate_for must return whichever row is LAST in PricingTable.rates' declaration order.
    let tie_table = PricingTable {
        rates: vec![
            PricingRate {
                model: "tie-model".into(),
                tier: None,
                kind: TokenRateKind::Input,
                rate_per_m: 5.00,
                effective_from: jan_1,
                effective_until: None,
            },
            PricingRate {
                model: "tie-model".into(),
                tier: None,
                kind: TokenRateKind::Input,
                rate_per_m: 9.00,
                effective_from: jan_1,
                effective_until: None,
            },
        ],
    };
    assert!(
        (tie_table.rate_for("tie-model", None, TokenRateKind::Input, dec_1) - 9.00).abs() < 1e-9,
        "identical effective_from must resolve to the LAST row in declaration order (index 1)"
    );

    // (c) Different effective_from values whose windows overlap. rate_for must return the
    // row with the LATER effective_from, regardless of declaration order.
    let overlap_table = PricingTable {
        rates: vec![
            PricingRate {
                model: "overlap-model".into(),
                tier: None,
                kind: TokenRateKind::Input,
                rate_per_m: 7.00,
                effective_from: jun_1,
                effective_until: None,
            },
            PricingRate {
                model: "overlap-model".into(),
                tier: None,
                kind: TokenRateKind::Input,
                rate_per_m: 4.00,
                effective_from: jan_1,
                effective_until: None,
            },
        ],
    };
    assert!(
        (overlap_table.rate_for("overlap-model", None, TokenRateKind::Input, dec_1) - 7.00).abs() < 1e-9,
        "overlapping windows must resolve to the row with the LATER effective_from (7.00, \
         declared first but effective later), not the row that appears later in the vector"
    );
}

/// CRIT-LUMEN-161: a RECOGNIZED model (matches at least one PricingRate row) that has no row
/// for the specific (tier, kind, as_of) requested must return 0.0 for that kind, never
/// substituting another model's rate -- distinct from CRIT-LUMEN-008's genuinely-unrecognized-
/// model-wide fallback to Sonnet.
#[test]
fn test_recognized_model_missing_kind_returns_zero() {
    let epoch = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    // Hand-constructed table: qwen-2.5-coder has Input/CacheRead/Output rows only, matching
    // CRIT-LUMEN-004's real gap (no seeded CacheWrite price for Qwen). Deliberately not built
    // from PricingTable::seed() so this test doesn't depend on seed()'s contents drifting.
    let table = PricingTable {
        rates: vec![
            PricingRate {
                model: "qwen-2.5-coder".into(),
                tier: None,
                kind: TokenRateKind::Input,
                rate_per_m: 0.20,
                effective_from: epoch,
                effective_until: None,
            },
            PricingRate {
                model: "qwen-2.5-coder".into(),
                tier: None,
                kind: TokenRateKind::CacheRead,
                rate_per_m: 0.05,
                effective_from: epoch,
                effective_until: None,
            },
            PricingRate {
                model: "qwen-2.5-coder".into(),
                tier: None,
                kind: TokenRateKind::Output,
                rate_per_m: 0.60,
                effective_from: epoch,
                effective_until: None,
            },
            // Sonnet row present in the table too, so a bug that falls through to the
            // CRIT-LUMEN-008 unrecognized-model-wide fallback has a real (wrong) value to
            // substitute instead of coincidentally also returning 0.0.
            PricingRate {
                model: "claude-3-5-sonnet".into(),
                tier: None,
                kind: TokenRateKind::CacheWrite,
                rate_per_m: 3.75,
                effective_from: epoch,
                effective_until: None,
            },
        ],
    };

    let result = table.rate_for("qwen-2.5-coder", None, TokenRateKind::CacheWrite, as_of);

    assert_eq!(result, 0.0, "recognized model missing a specific rate kind must return exactly 0.0");
    assert!(
        (result - 3.75).abs() > 1e-9,
        "must NOT substitute claude-3-5-sonnet's $3.75/M CacheWrite rate for a recognized \
         model's genuinely-absent rate kind"
    );
}

/// Blocker #2 from adversarial review: PricingTable::seed() pushes every row with tier: None,
/// but real adapters (e.g. CodexAdapter) pass tier: Some("Standard") when a real
/// thread_settings_applied event reports a service_tier. rate_for's exact tier filter matched
/// ZERO rows in that case, silently returning 0.0 for every kind. rate_for must fall back to a
/// model's own tier:None row when a specific tier is requested but no row for that tier exists.
#[test]
fn test_rate_for_tiered_query_falls_back_to_untiered_row() {
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    // seed() only has tier:None rows for gpt-4o. Querying with Some("Standard") must still
    // find the untiered row's rate rather than silently returning 0.0.
    let untiered = table.rate_for("gpt-4o", None, TokenRateKind::Input, as_of);
    let tiered_query = table.rate_for("gpt-4o", Some("Standard"), TokenRateKind::Input, as_of);
    assert_eq!(untiered, 2.50);
    assert_eq!(
        tiered_query, untiered,
        "a tiered query against a model with only tier:None rows must fall back to the \
         untiered rate, not silently return 0.0"
    );

    for kind in [TokenRateKind::Input, TokenRateKind::CacheWrite, TokenRateKind::CacheRead, TokenRateKind::Output] {
        assert_eq!(
            table.rate_for("gpt-4o", Some("Standard"), kind, as_of),
            table.rate_for("gpt-4o", None, kind, as_of),
            "tier fallback must hold for every {kind:?}"
        );
    }
}

/// When a tier-specific row DOES exist and matches, it must be preferred over the tier:None
/// fallback row -- tier-specific pricing takes priority when it exists.
#[test]
fn test_rate_for_prefers_matching_tiered_row_over_untiered_fallback() {
    let epoch = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    let table = PricingTable {
        rates: vec![
            PricingRate {
                model: "gpt-4o".into(),
                tier: None,
                kind: TokenRateKind::Input,
                rate_per_m: 2.50,
                effective_from: epoch,
                effective_until: None,
            },
            PricingRate {
                model: "gpt-4o".into(),
                tier: Some("Standard".into()),
                kind: TokenRateKind::Input,
                rate_per_m: 9.99,
                effective_from: epoch,
                effective_until: None,
            },
        ],
    };

    assert_eq!(
        table.rate_for("gpt-4o", Some("Standard"), TokenRateKind::Input, as_of),
        9.99,
        "a matching tier-specific row must win over the tier:None fallback"
    );
    assert_eq!(
        table.rate_for("gpt-4o", None, TokenRateKind::Input, as_of),
        2.50,
        "an untiered query must still return the untiered row untouched by the tiered row"
    );
}

/// CRIT-LUMEN-161 must still hold after the tier fallback: a recognized model missing a
/// specific rate kind entirely (no row for that kind under ANY tier) must still return exactly
/// 0.0, even when a tier is requested -- the tier fallback only kicks in when an untiered row
/// for that same kind exists, it must not silently substitute some other kind's rate.
#[test]
fn test_rate_for_tier_fallback_does_not_mask_missing_kind() {
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    assert_eq!(
        table.rate_for("qwen-2.5-coder", Some("Standard"), TokenRateKind::CacheWrite, as_of),
        0.0,
        "qwen-2.5-coder has no CacheWrite row under any tier, tiered or untiered -- must still \
         return exactly 0.0, not fall back to some other rate"
    );
}

/// CRIT-LUMEN-160: a provider-reported cost (e.g. Claude Code's real costUSD field) passed as
/// `provided_cost_usd` must be stored verbatim on `TokenEconomics.provided_cost_usd`, and must
/// never override, blend with, or short-circuit the independently computed `total_cost_usd` --
/// the two fields exist to be compared for drift, not merged.
#[test]
fn test_provided_cost_usd_twin_field() {
    let pricing = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    // 1,000,000 input tokens on claude-3-5-sonnet ($3.00/M) independently computes to $3.00,
    // which deliberately does NOT equal the provider-reported 4.20 supplied below.
    let turn = TurnPricingInput {
        usage: TurnTokenUsage {
            input_tokens: 1_000_000,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            cache_creation_1h_tokens: 0,
        },
        timestamp: as_of,
        tier: None,
    };

    // (a) A provided cost that disagrees with the independently computed cost must be stored
    // verbatim in provided_cost_usd, and must NOT override or blend into total_cost_usd.
    let econ_with_provided =
        TokenEconomics::calculate(std::slice::from_ref(&turn), "claude-3-5-sonnet", &pricing, Some(4.20));
    assert_eq!(econ_with_provided.provided_cost_usd, Some(4.20));
    assert!(
        (econ_with_provided.total_cost_usd - 3.00).abs() < 1e-9,
        "total_cost_usd must remain the independently computed value (3.00), unaffected by the \
         disagreeing provided_cost_usd of 4.20"
    );
    assert!(
        (econ_with_provided.provided_cost_usd.unwrap() - econ_with_provided.total_cost_usd).abs() > 1e-9,
        "provided_cost_usd and total_cost_usd must be free to disagree -- they are compared for \
         drift, not merged"
    );

    // (b) When no provided cost is supplied, provided_cost_usd must be None, not Some(0.0).
    let econ_without_provided =
        TokenEconomics::calculate(std::slice::from_ref(&turn), "claude-3-5-sonnet", &pricing, None);
    assert_eq!(econ_without_provided.provided_cost_usd, None);
}

/// High-severity adversarial finding: TokenRateKind had no Reasoning variant and
/// TurnTokenUsage had no reasoning_tokens field, so no adapter could ever price reasoning
/// tokens into total_cost_usd -- reasoning_output_tokens reached the final struct only via a
/// post-hoc field overwrite that never touched the pricing math. This test proves the DOLLAR
/// AMOUNT includes the reasoning contribution, not merely that a counter field is populated.
#[test]
fn test_reasoning_tokens_priced_into_total_cost() {
    let pricing = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    let usage = TurnTokenUsage {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
        reasoning_tokens: 500_000,
        cache_creation_1h_tokens: 0,
    };

    let turn = TurnPricingInput { usage, timestamp: as_of, tier: None };
    let econ = TokenEconomics::calculate(&[turn], "gpt-4o", &pricing, None);

    let reasoning_rate = pricing.rate_for("gpt-4o", None, TokenRateKind::Reasoning, as_of);
    assert!((reasoning_rate - 10.00).abs() < 1e-9, "gpt-4o's Reasoning rate must equal its Output rate ($10.00/M)");

    // input 1M @ $2.50 + output 1M @ $10.00 + reasoning 0.5M @ $10.00
    let expected_cost = 2.50 + 10.00 + 5.00;
    assert!(
        (econ.total_cost_usd - expected_cost).abs() < 1e-6,
        "total_cost_usd ({}) must include the reasoning tokens' dollar contribution, expected {}",
        econ.total_cost_usd,
        expected_cost
    );

    // reasoning tokens must be priced identically in the no-cache baseline, so the presence of
    // reasoning cost alone never skews net_savings_usd/efficiency_multiplier.
    let expected_baseline = 2.50 + 10.00 + 5.00;
    assert!((econ.baseline_cost_no_cache_usd - expected_baseline).abs() < 1e-6);
    assert_eq!(econ.net_savings_usd, 0.0);

    assert_eq!(econ.reasoning_output_tokens, 500_000);
}

/// Blocker #1 from adversarial review: rate_for must normalize raw, provider-versioned model
/// strings (as real adapters actually pass them, e.g. ClaudeCodeAdapter's `message.model`
/// field) down to seed()'s short canonical keys BEFORE doing the recognized/exact-match
/// lookup, per SPEC-LUMEN-001-MODEL.json's PricingTable api_surface entry. Without this
/// normalization, every one of these real strings falls through to the CRIT-LUMEN-008
/// unrecognized-model fallback and silently returns Sonnet's rates instead of the model's own.
#[test]
fn test_rate_for_normalizes_real_provider_versioned_model_strings() {
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    // Real ClaudeCodeAdapter message.model string for Haiku.
    assert!(
        (table.rate_for("claude-3-5-haiku-20241022", None, TokenRateKind::Input, as_of) - 0.80).abs() < 1e-9,
        "raw dated Haiku model string must normalize to claude-3-5-haiku's $0.80/M input rate"
    );
    assert!((table.rate_for("claude-3-5-haiku-20241022", None, TokenRateKind::Output, as_of) - 4.00).abs() < 1e-9);

    // Real ClaudeCodeAdapter message.model string for Opus.
    assert!(
        (table.rate_for("claude-opus-4-20250514", None, TokenRateKind::Input, as_of) - 15.00).abs() < 1e-9,
        "raw dated Opus model string must normalize to claude-opus's $15.00/M input rate"
    );
    assert!((table.rate_for("claude-opus-4-20250514", None, TokenRateKind::Output, as_of) - 75.00).abs() < 1e-9);

    // Real ClaudeCodeAdapter message.model string for Sonnet.
    assert!(
        (table.rate_for("claude-3-5-sonnet-20241022", None, TokenRateKind::Input, as_of) - 3.00).abs() < 1e-9,
        "raw dated Sonnet model string must normalize to claude-3-5-sonnet's own $3.00/M rate"
    );

    // Real GPT-4o versioned string.
    assert!(
        (table.rate_for("gpt-4o-2024-08-06", None, TokenRateKind::Input, as_of) - 2.50).abs() < 1e-9,
        "raw dated GPT-4o model string must normalize to gpt-4o's $2.50/M input rate, not \
         Sonnet's $3.00/M fallback"
    );
    assert!((table.rate_for("gpt-4o-2024-08-06", None, TokenRateKind::Output, as_of) - 10.00).abs() < 1e-9);

    // "gemini-2.0-flash-001": CRIT-LUMEN-170 discovered LiteLLM vendors this specific dated
    // release as its OWN entry (a real, distinct, slightly higher rate than the bare
    // "gemini-2.0-flash" alias -- Google's own pricing snapshot for this dated release predates
    // a later price cut reflected in the alias). normalize_model_key tries the unstripped
    // string first, so an exact vendored match like this one wins over family normalization --
    // that's a real precision improvement (a genuine historical rate), not a bug, so this no
    // longer equals gemini-2.0-flash's own $0.10/M rate. Recognized and nonzero is what matters.
    assert!(table.is_recognized("gemini-2.0-flash-001"));
    assert!(table.rate_for("gemini-2.0-flash-001", None, TokenRateKind::Input, as_of) > 0.0);
    assert!(
        (table.rate_for("gemini-2.0-flash-exp", None, TokenRateKind::Input, as_of) - 0.10).abs() < 1e-9,
        "raw -exp-suffixed Gemini Flash model string must also normalize to gemini-2.0-flash"
    );

    // CRIT-LUMEN-008 (revised) must still hold: a genuinely unrecognized model (normalizes to no
    // seeded key at all) prices at 0.0 for every kind, never a substituted rate.
    assert!(!table.is_recognized("totally-fake-model-xyz-99999999"));
    for kind in [TokenRateKind::Input, TokenRateKind::CacheWrite, TokenRateKind::CacheRead, TokenRateKind::Output] {
        let unrecognized = table.rate_for("totally-fake-model-xyz-99999999", None, kind, as_of);
        assert_eq!(unrecognized, 0.0, "genuinely unrecognized model must price at 0.0 for {kind:?}");
    }

    // CRIT-LUMEN-161 must still hold: qwen-2.5-coder's missing CacheWrite row still returns
    // exactly 0.0, not a substituted rate, even via a versioned qwen string.
    assert_eq!(
        table.rate_for("qwen-2.5-coder-32b-instruct", None, TokenRateKind::CacheWrite, as_of),
        0.0,
        "recognized-after-normalization model missing a specific rate kind must still return \
         exactly 0.0, never a substituted rate"
    );
    assert!((table.rate_for("qwen-2.5-coder-32b-instruct", None, TokenRateKind::Input, as_of) - 0.20).abs() < 1e-9);
}

/// Real, newly-discovered financial-correctness bug found via mutation testing during a fresh
/// spec re-verification: `normalize_model_key`'s progressively-shorter-prefix search let
/// "gpt-4o-mini" match the seeded "gpt-4o" key by stripping the trailing "mini" segment.
/// "gpt-4o-mini" is a REAL, DIFFERENT, materially cheaper OpenAI model -- normalizing it onto
/// gpt-4o's rate is a real overcharge for any Codex/Claude session that reports this model
/// string. The fix restricts what `normalize_model_key` may strip to segments that are clearly
/// version/date/scale markers (numeric-only segments, digit+single-letter parameter-scale
/// segments like "32b", and a small allowlist of non-differentiating tuning words), so "mini" --
/// which denotes a genuinely different priced product, not a version/scale suffix -- can never
/// be stripped.
///
/// Before CRIT-LUMEN-170 (vendored pricing data), no seeded key matched "gpt-4o-mini" at all
/// once "mini" was correctly rejected as strippable, so it fell through to the unrecognized-model
/// 0.0 fallback -- unpriced, but at least never mispriced at gpt-4o's rate. Vendoring supersedes
/// that: LiteLLM publishes gpt-4o-mini's own real, distinct rate directly, so it is now
/// genuinely recognized and priced at ITS OWN rate. This test's real invariant -- gpt-4o-mini
/// must never be billed at gpt-4o's rate -- still holds and is what's actually asserted; only
/// the previously-unpriced outcome has changed, to the better outcome of being correctly priced.
#[test]
fn test_gpt_4o_mini_does_not_normalize_to_gpt_4o() {
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    let mini_rate = table.rate_for("gpt-4o-mini", None, TokenRateKind::Input, as_of);
    let gpt4o_rate = table.rate_for("gpt-4o", None, TokenRateKind::Input, as_of);

    assert!((gpt4o_rate - 2.50).abs() < 1e-9, "sanity: gpt-4o's own seeded Input rate must still be $2.50/M");
    assert!(
        (mini_rate - 2.50).abs() > 1e-9,
        "gpt-4o-mini must NOT be billed at gpt-4o's $2.50/M rate -- it is a distinct, much \
         cheaper real model"
    );
    // CRIT-LUMEN-170: gpt-4o-mini is now a real vendored entry in its own right, priced at its
    // own rate rather than falling through to the unrecognized-model 0.0 fallback.
    assert!(table.is_recognized("gpt-4o-mini"), "gpt-4o-mini is a real vendored model, not unrecognized");
    assert!(mini_rate > 0.0, "gpt-4o-mini must be priced at its own real (nonzero) vendored rate, got {mini_rate}");
}

/// A second, genuine same-prefix-different-model risk among the seeded keys: "gemini-2.0-flash"
/// is seeded, and Google's real "gemini-2.0-flash-lite" is a distinct, differently-priced
/// product (not a version/date/scale suffix of flash). Confirms the fix generalizes beyond the
/// single gpt-4o-mini case rather than special-casing it. See
/// test_gpt_4o_mini_does_not_normalize_to_gpt_4o's doc comment for why CRIT-LUMEN-170 changed
/// this test's expected outcome from "unrecognized, 0.0" to "recognized at its own real rate".
#[test]
fn test_gemini_flash_lite_does_not_normalize_to_gemini_flash() {
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    let lite_rate = table.rate_for("gemini-2.0-flash-lite", None, TokenRateKind::Input, as_of);
    let flash_rate = table.rate_for("gemini-2.0-flash", None, TokenRateKind::Input, as_of);

    assert!((flash_rate - 0.10).abs() < 1e-9, "sanity: gemini-2.0-flash's own seeded Input rate must still be $0.10/M");
    assert!(
        (lite_rate - 0.10).abs() > 1e-9,
        "gemini-2.0-flash-lite must NOT be billed at gemini-2.0-flash's rate -- it is a \
         distinct real model, not a version/scale suffix of flash"
    );
    assert!(
        table.is_recognized("gemini-2.0-flash-lite"),
        "gemini-2.0-flash-lite is a real vendored model, not unrecognized"
    );
    assert!(
        lite_rate > 0.0,
        "gemini-2.0-flash-lite must be priced at its own real (nonzero) vendored rate, got {lite_rate}"
    );
}

/// Eight real model strings a prior drift-check confirmed must keep normalizing correctly after
/// the strippable-segment restriction is applied -- each stripped segment across these cases is
/// either numeric-only or a digit+single-letter parameter-scale token or an allowlisted tuning
/// word, so none of them should regress. ("gemini-2.0-flash-001" was dropped from this list:
/// CRIT-LUMEN-170 vendoring discovered it's actually its own distinct real vendored entry, not a
/// pure alias of "gemini-2.0-flash" -- see test_rate_for_normalizes_real_provider_versioned_model_strings
/// for that case's own dedicated coverage. "gemini-2.0-flash-exp" below still covers this
/// family's normalize-a-suffixed-string behavior, since it has no such dedicated vendored entry.)
#[test]
fn test_real_versioned_strings_still_normalize_after_safe_strip_restriction() {
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    let cases: &[(&str, &str, f64)] = &[
        ("claude-3-5-sonnet-20241022", "claude-3-5-sonnet", 3.00),
        ("claude-3-5-haiku-20241022", "claude-3-5-haiku", 0.80),
        ("claude-opus-4-20250514", "claude-opus", 15.00),
        ("claude-opus-4-1-20250805", "claude-opus", 15.00),
        ("gpt-4o-2024-08-06", "gpt-4o", 2.50),
        ("gemini-2.0-flash-exp", "gemini-2.0-flash", 0.10),
        ("deepseek-r1-0528", "deepseek-r1", 0.55),
        ("qwen-2.5-coder-32b-instruct", "qwen-2.5-coder", 0.20),
    ];

    for (raw, canonical, expected_input_rate) in cases {
        let raw_rate = table.rate_for(raw, None, TokenRateKind::Input, as_of);
        let canonical_rate = table.rate_for(canonical, None, TokenRateKind::Input, as_of);
        assert!(
            (raw_rate - expected_input_rate).abs() < 1e-9,
            "{raw} must normalize to {canonical}'s ${expected_input_rate}/M Input rate, got {raw_rate}"
        );
        assert!(
            (raw_rate - canonical_rate).abs() < 1e-9,
            "{raw}'s normalized rate must exactly equal {canonical}'s own rate"
        );
    }
}

/// Perf finding: adapters were calling `PricingTable::seed()` fresh on every single
/// `parse_stream` invocation instead of reusing one shared instance, even though
/// `TokenEconomics::calculate` takes `&PricingTable` specifically so callers could avoid that.
/// `pricing::SEEDED` is a `LazyLock<PricingTable>` built once and reused. This test proves the
/// shared static is a behaviorally identical drop-in for a fresh `PricingTable::seed()` call --
/// the actual safety property worth testing, since "was an allocation avoided" isn't practically
/// testable without a dedicated allocation-counting harness.
#[test]
fn test_seeded_static_matches_fresh_seed_table() {
    let fresh = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2024, 11, 1, 0, 0, 0).unwrap();

    let cases = [
        ("claude-3-5-sonnet", None, TokenRateKind::Input),
        ("claude-3-5-sonnet", None, TokenRateKind::Output),
        ("claude-3-5-haiku", None, TokenRateKind::CacheRead),
        ("gpt-4o", None, TokenRateKind::Reasoning),
        ("deepseek-r1", None, TokenRateKind::CacheWrite),
        ("qwen-2.5-coder-32b-instruct", None, TokenRateKind::CacheWrite), // exercises the 0.0 no-substitution path
        ("totally-unrecognized-model-xyz", None, TokenRateKind::Input),   // exercises the unrecognized-model 0.0 path
    ];

    for (model, tier, kind) in cases {
        let fresh_rate = fresh.rate_for(model, tier, kind, as_of);
        let shared_rate = pricing::SEEDED.rate_for(model, tier, kind, as_of);
        assert_eq!(
            shared_rate, fresh_rate,
            "pricing::SEEDED must match a fresh PricingTable::seed() for ({model}, {tier:?}, {kind:?})"
        );
    }
}

/// Current-generation models actually seen in real local session data this session (Claude
/// Code's `message.model`, Codex's `payload.thread_settings.model`, AGY's protobuf model field),
/// seeded from official first-party pricing pages (platform.claude.com, developers.openai.com,
/// ai.google.dev) fetched 2026-08-21 -- see PricingTable::seed's doc comments for the exact
/// source per model. Before this test existed, every one of these real model strings collapsed
/// onto claude-3-5-sonnet's stale fallback rate; this locks in that they're now genuinely priced.
#[test]
fn test_current_generation_real_models_are_seeded_and_recognized() {
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2026, 8, 21, 0, 0, 0).unwrap();

    let cases: &[(&str, f64, f64, f64)] = &[
        // (model, Input, Output, CacheRead) per MTok
        ("claude-fable-5", 10.00, 50.00, 1.00),
        ("claude-opus-5", 5.00, 25.00, 0.50),
        ("claude-sonnet-5", 2.00, 10.00, 0.20),
        ("claude-haiku-4-5", 1.00, 5.00, 0.10),
        ("gpt-5.6-terra", 2.00, 12.00, 0.20),
        ("gemini-3.7-flash", 0.75, 3.75, 0.075),
    ];

    for (model, input_rate, output_rate, cache_read_rate) in cases {
        assert!(table.is_recognized(model), "{model} must be recognized after seeding");
        assert!((table.rate_for(model, None, TokenRateKind::Input, as_of) - input_rate).abs() < 1e-9);
        assert!((table.rate_for(model, None, TokenRateKind::Output, as_of) - output_rate).abs() < 1e-9);
        assert!((table.rate_for(model, None, TokenRateKind::CacheRead, as_of) - cache_read_rate).abs() < 1e-9);
    }

    // Real request-time speed/effort suffixes observed in real OpenCode/AGY data must normalize
    // to their base seeded row, not fall through to unrecognized.
    assert!(table.is_recognized("gpt-5.6-terra-fast"));
    assert_eq!(
        table.rate_for("gpt-5.6-terra-fast", None, TokenRateKind::Input, as_of),
        table.rate_for("gpt-5.6-terra", None, TokenRateKind::Input, as_of),
    );
    assert!(table.is_recognized("gemini-3.7-flash-high"));
    assert_eq!(
        table.rate_for("gemini-3.7-flash-high", None, TokenRateKind::Input, as_of),
        table.rate_for("gemini-3.7-flash", None, TokenRateKind::Input, as_of),
    );
}

/// Anthropic's real published 5m/1h cache-write multipliers (1.25x/2x input) confirmed via
/// platform.claude.com/docs/en/about-claude/pricing -- each seeded Claude model must carry both
/// rates distinctly, not collapse the 1h tier onto the 5m rate.
#[test]
fn test_claude_1h_cache_write_rate_is_distinct_from_5m() {
    let table = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2026, 8, 21, 0, 0, 0).unwrap();

    let cases: &[(&str, f64, f64)] = &[
        // (model, 5m CacheWrite, 1h CacheWrite1h)
        ("claude-sonnet-5", 2.50, 4.00),
        ("claude-opus-5", 6.25, 10.00),
        ("claude-haiku-4-5", 1.25, 2.00),
        ("claude-fable-5", 12.50, 20.00),
    ];

    for (model, rate_5m, rate_1h) in cases {
        assert!((table.rate_for(model, None, TokenRateKind::CacheWrite, as_of) - rate_5m).abs() < 1e-9);
        assert!((table.rate_for(model, None, TokenRateKind::CacheWrite1h, as_of) - rate_1h).abs() < 1e-9);
    }
}

/// A turn whose cache_creation_tokens splits across both the 5m and 1h tiers must price each
/// portion at its own distinct rate, not the whole amount at either rate -- the real bug this
/// covers: Claude Code's real `usage.cache_creation.{ephemeral_5m_input_tokens,
/// ephemeral_1h_input_tokens}` was read into the flat sum only, discarding which portion was 1h.
#[test]
fn test_mixed_5m_and_1h_cache_write_priced_at_distinct_rates() {
    let pricing = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2026, 8, 21, 0, 0, 0).unwrap();

    let usage = TurnTokenUsage {
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_tokens: 1_000_000, // total write: 600k @ 5m rate + 400k @ 1h rate
        cache_creation_1h_tokens: 400_000,
        cache_read_tokens: 0,
        reasoning_tokens: 0,
    };

    let turn = TurnPricingInput { usage, timestamp: as_of, tier: None };
    let econ = TokenEconomics::calculate(&[turn], "claude-sonnet-5", &pricing, None);

    // 600k @ $2.50/M (5m) + 400k @ $4.00/M (1h)
    let expected = 0.600 * 2.50 + 0.400 * 4.00;
    assert!((econ.total_cost_usd - expected).abs() < 1e-6);
    assert_eq!(econ.ephemeral_5m_tokens, 600_000);
    assert_eq!(econ.ephemeral_1h_tokens, 400_000);
}

/// The core honesty fix this pass makes: an unrecognized model must surface as explicitly
/// unpriced via `is_fully_priced`, not report a cost that looks like a verified zero.
#[test]
fn test_unrecognized_model_sets_is_fully_priced_false() {
    let pricing = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2026, 8, 21, 0, 0, 0).unwrap();

    let usage = TurnTokenUsage {
        input_tokens: 1000,
        output_tokens: 500,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
        cache_read_tokens: 0,
        reasoning_tokens: 0,
    };
    let turn = TurnPricingInput { usage, timestamp: as_of, tier: None };

    let unrecognized =
        TokenEconomics::calculate(std::slice::from_ref(&turn), "some-brand-new-model-nobody-seeded", &pricing, None);
    assert!(!unrecognized.is_fully_priced);
    assert_eq!(unrecognized.total_cost_usd, 0.0);
    assert!(!unrecognized.per_model["some-brand-new-model-nobody-seeded"].is_fully_priced);

    let recognized = TokenEconomics::calculate(&[turn], "claude-sonnet-5", &pricing, None);
    assert!(recognized.is_fully_priced);
    assert!(recognized.total_cost_usd > 0.0);
    assert!(recognized.per_model["claude-sonnet-5"].is_fully_priced);
}

/// CRIT-LUMEN-171: `Cost` is the structural fix for the same class of bug
/// test_unrecognized_model_sets_is_fully_priced_false guards -- `TokenEconomics::cost()` and
/// `ModelTokenSummary::cost()` must produce `Unpriced`/`Priced` matching `is_fully_priced`
/// exactly, and `Cost::format_usd` must never print a dollar figure for `Unpriced`.
#[test]
fn test_cost_reflects_is_fully_priced_and_formats_correctly() {
    let pricing = PricingTable::seed();
    let as_of = Utc.with_ymd_and_hms(2026, 8, 21, 0, 0, 0).unwrap();

    let usage = TurnTokenUsage {
        input_tokens: 1_000_000,
        output_tokens: 0,
        cache_creation_tokens: 0,
        cache_creation_1h_tokens: 0,
        cache_read_tokens: 0,
        reasoning_tokens: 0,
    };
    let turn = TurnPricingInput { usage, timestamp: as_of, tier: None };

    let unrecognized =
        TokenEconomics::calculate(std::slice::from_ref(&turn), "another-brand-new-model-nobody-seeded", &pricing, None);
    assert_eq!(unrecognized.cost(), Cost::Unpriced);
    assert_eq!(unrecognized.cost().format_usd("unknown"), "unknown");
    assert_eq!(
        unrecognized.per_model["another-brand-new-model-nobody-seeded"].cost(),
        Cost::Unpriced,
        "ModelTokenSummary::cost() must agree with TokenEconomics::cost()"
    );

    let recognized = TokenEconomics::calculate(&[turn], "claude-sonnet-5", &pricing, None);
    assert_eq!(recognized.cost(), Cost::Priced(recognized.total_cost_usd));
    assert!((recognized.total_cost_usd - 2.00).abs() < 1e-9, "sanity: 1M input tokens @ $2.00/M");
    assert_eq!(recognized.cost().format_usd("unknown"), "$2.0000");
    assert_eq!(
        recognized.per_model["claude-sonnet-5"].cost(),
        Cost::Priced(recognized.per_model["claude-sonnet-5"].cost_usd)
    );
}
