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
            input,
            output,
            cache_write,
            cache_read,
            "claude-3-5-sonnet-20241022",
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
    let models = [
        ("claude-3-5-sonnet-20241022", 3.00, 3.75, 0.30, 15.00),
        ("claude-3-5-haiku-20241022", 0.80, 1.00, 0.08, 4.00),
        ("claude-3-opus-20240229", 15.00, 18.75, 1.50, 75.00),
        ("gpt-4o", 2.50, 2.50, 1.25, 10.00),
        ("deepseek-r1", 0.55, 0.55, 0.14, 2.19),
        ("qwen-2.5-coder-32b", 0.20, 0.20, 0.05, 0.60),
        ("kimi-k1.5", 0.50, 0.50, 0.10, 2.00),
        ("glm-4-plus", 1.40, 1.40, 0.20, 1.40),
        ("gemini-2.0-flash", 0.10, 0.10, 0.025, 0.40),
        ("gemini-2.0-pro", 1.25, 1.25, 0.30, 5.00),
    ];

    for (model, in_rate, write_rate, read_rate, out_rate) in models {
        let pricing = ModelPricing::from_model_name(model);
        assert_eq!(pricing.input_base_per_m, in_rate, "Mismatch for {model}");
        assert_eq!(pricing.cache_write_per_m, write_rate, "Mismatch for {model}");
        assert_eq!(pricing.cache_read_per_m, read_rate, "Mismatch for {model}");
        assert_eq!(pricing.output_per_m, out_rate, "Mismatch for {model}");

        // Compute sample cost
        let usage = TurnTokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_creation_tokens: 1_000_000,
            cache_read_tokens: 1_000_000,
        };
        let cost = pricing.compute_cost(&usage);
        let expected = in_rate + write_rate + read_rate + out_rate;
        assert!((cost - expected).abs() < 1e-6);
    }
}

#[test]
fn test_extreme_boundary_token_values() {
    // Zero tokens across all fields
    let zero_econ = TokenEconomics::calculate(0, 0, 0, 0, "claude-3-5-sonnet-20241022");
    assert_eq!(zero_econ.cache_hit_ratio, 0.0);
    assert_eq!(zero_econ.efficiency_multiplier, 1.0);
    assert_eq!(zero_econ.total_cost_usd, 0.0);

    // 100% cache read (0 input, 0 cache write)
    let full_cache = TokenEconomics::calculate(0, 1000, 0, 500_000, "claude-3-5-sonnet-20241022");
    assert_eq!(full_cache.cache_hit_ratio, 100.0);
    assert!(full_cache.efficiency_multiplier > 9.0);

    // 100% uncached (0 cache read, 0 cache write)
    let zero_cache = TokenEconomics::calculate(500_000, 1000, 0, 0, "claude-3-5-sonnet-20241022");
    assert_eq!(zero_cache.cache_hit_ratio, 0.0);
    assert_eq!(zero_cache.net_savings_usd, 0.0);
    assert_eq!(zero_cache.efficiency_multiplier, 1.0);
}
