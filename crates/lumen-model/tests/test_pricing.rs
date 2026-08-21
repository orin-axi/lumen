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

    // CRIT-LUMEN-008: genuinely unrecognized model falls back to Claude 3.5 Sonnet's rates
    for kind in [TokenRateKind::Input, TokenRateKind::CacheWrite, TokenRateKind::CacheRead, TokenRateKind::Output] {
        let unrecognized = table.rate_for("totally-unrecognized-model-xyz", None, kind, as_of);
        let sonnet = table.rate_for("claude-3-5-sonnet", None, kind, as_of);
        assert!((unrecognized - sonnet).abs() < 1e-9, "fallback rate for {kind:?} should equal Sonnet's rate");
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

    // Gemini 2.0 Flash
    assert_eq!(table.rate_for("gemini-2.0-flash", None, TokenRateKind::Input, as_of), 0.10);
    assert_eq!(table.rate_for("gemini-2.0-flash", None, TokenRateKind::CacheRead, as_of), 0.025);
    assert_eq!(table.rate_for("gemini-2.0-flash", None, TokenRateKind::Output, as_of), 0.40);

    // CRIT-LUMEN-008: Unrecognized model string defaults to Claude 3.5 Sonnet's rates
    for kind in [TokenRateKind::Input, TokenRateKind::CacheWrite, TokenRateKind::CacheRead, TokenRateKind::Output] {
        let unrecognized = table.rate_for("some-obscure-custom-llm-v1", None, kind, as_of);
        let sonnet = table.rate_for("claude-3-5-sonnet", None, kind, as_of);
        assert_eq!(unrecognized, sonnet);
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

    // CRIT-LUMEN-100: GPT-4o
    assert!((table.rate_for("gpt-4o", None, TokenRateKind::Input, as_of) - 2.50).abs() < 1e-9);
    assert!((table.rate_for("gpt-4o", None, TokenRateKind::CacheWrite, as_of) - 2.50).abs() < 1e-9);
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

    // CRIT-LUMEN-104: Gemini 2.0 Flash
    assert!((table.rate_for("gemini-2.0-flash", None, TokenRateKind::Input, as_of) - 0.10).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-flash", None, TokenRateKind::CacheWrite, as_of) - 0.10).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-flash", None, TokenRateKind::CacheRead, as_of) - 0.025).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-flash", None, TokenRateKind::Output, as_of) - 0.40).abs() < 1e-9);

    // CRIT-LUMEN-105: Gemini 2.0 Pro
    assert!((table.rate_for("gemini-2.0-pro", None, TokenRateKind::Input, as_of) - 1.25).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-pro", None, TokenRateKind::CacheWrite, as_of) - 1.25).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-pro", None, TokenRateKind::CacheRead, as_of) - 0.30).abs() < 1e-9);
    assert!((table.rate_for("gemini-2.0-pro", None, TokenRateKind::Output, as_of) - 5.00).abs() < 1e-9);

    // CacheWrite == Input for the six non-Anthropic models (Opus is Anthropic and has a
    // distinct 1.25x cache-write premium, so it is excluded from this check).
    for model in ["gpt-4o", "deepseek-r1", "kimi-k1.5", "glm-4-plus", "gemini-2.0-flash", "gemini-2.0-pro"] {
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

    // Real Gemini 2.0 Flash versioned strings (two different suffix shapes).
    assert!(
        (table.rate_for("gemini-2.0-flash-001", None, TokenRateKind::Input, as_of) - 0.10).abs() < 1e-9,
        "raw numbered Gemini Flash model string must normalize to gemini-2.0-flash's $0.10/M \
         input rate, not Sonnet's $3.00/M fallback"
    );
    assert!(
        (table.rate_for("gemini-2.0-flash-exp", None, TokenRateKind::Input, as_of) - 0.10).abs() < 1e-9,
        "raw -exp-suffixed Gemini Flash model string must also normalize to gemini-2.0-flash"
    );

    // CRIT-LUMEN-008 must still hold: a genuinely unrecognized model (normalizes to no seeded
    // key at all) still falls back to claude-3-5-sonnet's rates.
    for kind in [TokenRateKind::Input, TokenRateKind::CacheWrite, TokenRateKind::CacheRead, TokenRateKind::Output] {
        let unrecognized = table.rate_for("totally-fake-model-xyz-99999999", None, kind, as_of);
        let sonnet = table.rate_for("claude-3-5-sonnet", None, kind, as_of);
        assert!(
            (unrecognized - sonnet).abs() < 1e-9,
            "genuinely unrecognized model must still fall back to Sonnet's rate for {kind:?}"
        );
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
        ("totally-unrecognized-model-xyz", None, TokenRateKind::Input),   // exercises the sonnet fallback path
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
