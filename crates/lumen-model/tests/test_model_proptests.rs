use chrono::Utc;
use lumen_model::*;
use proptest::prelude::*;

// Property-based testing for mathematical invariants of token accounting
proptest! {
    #[test]
    fn prop_token_economics_invariants(
        input in 0u64..1_000_000_000,
        output in 0u64..1_000_000_000,
        cache_write in 0u64..1_000_000_000,
        cache_read in 0u64..1_000_000_000,
    ) {
        let econ = TokenEconomics::calculate(
            &[TurnPricingInput {
                usage: TurnTokenUsage {
                    input_tokens: input,
                    output_tokens: output,
                    cache_creation_tokens: cache_write,
                    cache_read_tokens: cache_read,
                },
                timestamp: Utc::now(),
                tier: None,
            }],
            "claude-3-5-sonnet-20241022",
            &PricingTable::seed(),
            None,
        );

        // Invariant 1: Cache hit ratio is strictly bounded between 0.0% and 100.0%
        prop_assert!(econ.cache_hit_ratio >= 0.0);
        prop_assert!(econ.cache_hit_ratio <= 100.0);
        prop_assert!(!econ.cache_hit_ratio.is_nan());

        // Invariant 2: Net savings is non-negative and matches clamped difference
        prop_assert!(econ.net_savings_usd >= 0.0);
        prop_assert!(!econ.net_savings_usd.is_nan());
        let raw_savings = econ.baseline_cost_no_cache_usd - econ.total_cost_usd;
        let expected_savings = if raw_savings > 0.0 { raw_savings } else { 0.0 };
        prop_assert!((econ.net_savings_usd - expected_savings).abs() < 1e-6);

        // Invariant 3: Total cost is non-negative
        prop_assert!(econ.total_cost_usd >= 0.0);
        prop_assert!(!econ.total_cost_usd.is_nan());

        // Invariant 4: Efficiency multiplier is finite and non-negative
        prop_assert!(econ.efficiency_multiplier >= 0.0);
        prop_assert!(!econ.efficiency_multiplier.is_nan());
        if econ.total_cost_usd > 0.0 {
            let expected_efficiency = (econ.baseline_cost_no_cache_usd / econ.total_cost_usd) as f32;
            prop_assert!((econ.efficiency_multiplier - expected_efficiency).abs() < 1e-4);
        } else {
            prop_assert_eq!(econ.efficiency_multiplier, 1.0);
        }
    }

    #[test]
    fn prop_turn_token_usage_monotonicity(
        input in 0u64..10_000_000,
        output in 0u64..10_000_000,
        cache_creation in 0u64..10_000_000,
        cache_read in 0u64..10_000_000,
    ) {
        let usage = TurnTokenUsage {
            input_tokens: input,
            output_tokens: output,
            cache_creation_tokens: cache_creation,
            cache_read_tokens: cache_read,
        };

        prop_assert_eq!(usage.prompt_tokens(), input + cache_creation + cache_read);
        prop_assert_eq!(usage.total_tokens(), input + cache_creation + cache_read + output);
        prop_assert!(usage.total_tokens() >= usage.prompt_tokens());
    }
}

#[test]
fn test_all_commercial_model_pricing_matrix() {
    // NOTE: model keys here are the short canonical names PricingTable::seed() actually
    // indexes rows by (exact match, no substring fallback beyond "unrecognized model" ->
    // claude-3-5-sonnet) -- unlike the removed ModelPricing::from_model_name, which matched
    // via substring on date-suffixed names. Using the short keys here is the minimal
    // adaptation that keeps this test asserting real per-model rates instead of silently
    // exercising the "unrecognized model" sonnet fallback for every non-gpt/deepseek/etc row.
    let models = [
        ("claude-3-5-sonnet", 3.00, 3.75, 0.30, 15.00),
        ("claude-3-5-haiku", 0.80, 1.00, 0.08, 4.00),
        ("claude-opus", 15.00, 18.75, 1.50, 75.00),
        ("gpt-4o", 2.50, 2.50, 1.25, 10.00),
        ("deepseek-r1", 0.55, 0.55, 0.14, 2.19),
        // seed() has no CacheWrite row for Qwen (CRIT-LUMEN-004/161): a recognized model
        // missing a specific rate kind returns 0.0, it does not fall back to its input rate.
        ("qwen-2.5-coder", 0.20, 0.0, 0.05, 0.60),
        ("kimi-k1.5", 0.50, 0.50, 0.10, 2.00),
        ("glm-4-plus", 1.40, 1.40, 0.20, 1.40),
        ("gemini-2.0-flash", 0.10, 0.10, 0.025, 0.40),
        ("gemini-2.0-pro", 1.25, 1.25, 0.30, 5.00),
    ];

    let pricing = PricingTable::seed();
    let now = Utc::now();

    for (model, in_rate, write_rate, read_rate, out_rate) in models {
        assert_eq!(pricing.rate_for(model, None, TokenRateKind::Input, now), in_rate, "Mismatch for {model}");
        assert_eq!(pricing.rate_for(model, None, TokenRateKind::CacheWrite, now), write_rate, "Mismatch for {model}");
        assert_eq!(pricing.rate_for(model, None, TokenRateKind::CacheRead, now), read_rate, "Mismatch for {model}");
        assert_eq!(pricing.rate_for(model, None, TokenRateKind::Output, now), out_rate, "Mismatch for {model}");

        // Compute sample cost
        let usage = TurnTokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_creation_tokens: 1_000_000,
            cache_read_tokens: 1_000_000,
        };
        let econ =
            TokenEconomics::calculate(&[TurnPricingInput { usage, timestamp: now, tier: None }], model, &pricing, None);
        let expected = in_rate + write_rate + read_rate + out_rate;
        assert!((econ.total_cost_usd - expected).abs() < 1e-6);
    }
}

#[test]
fn test_extreme_boundary_token_values() {
    let pricing = PricingTable::seed();
    let now = Utc::now();

    let turn = |usage: TurnTokenUsage| TokenEconomics::calculate(
        &[TurnPricingInput { usage, timestamp: now, tier: None }],
        "claude-3-5-sonnet-20241022",
        &pricing,
        None,
    );

    // Zero tokens across all fields
    let zero_econ = turn(TurnTokenUsage {
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
    });
    assert_eq!(zero_econ.cache_hit_ratio, 0.0);
    assert_eq!(zero_econ.efficiency_multiplier, 1.0);
    assert_eq!(zero_econ.total_cost_usd, 0.0);

    // 100% cache read (0 input, 0 cache write)
    let full_cache = turn(TurnTokenUsage {
        input_tokens: 0,
        output_tokens: 1000,
        cache_creation_tokens: 0,
        cache_read_tokens: 500_000,
    });
    assert_eq!(full_cache.cache_hit_ratio, 100.0);
    assert!(full_cache.efficiency_multiplier > 9.0);

    // 100% uncached (0 cache read, 0 cache write)
    let zero_cache = turn(TurnTokenUsage {
        input_tokens: 500_000,
        output_tokens: 1000,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
    });
    assert_eq!(zero_cache.cache_hit_ratio, 0.0);
    assert_eq!(zero_cache.net_savings_usd, 0.0);
    assert_eq!(zero_cache.efficiency_multiplier, 1.0);
}
