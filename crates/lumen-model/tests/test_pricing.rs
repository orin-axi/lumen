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
    let pricing = ModelPricing::CLAUDE_3_5_SONNET;

    let usage = TurnTokenUsage {
        input_tokens: 1_000_000,          // $3.00
        output_tokens: 1_000_000,         // $15.00
        cache_creation_tokens: 1_000_000, // $3.75 (1.25x)
        cache_read_tokens: 1_000_000,     // $0.30 (0.10x - 90% savings)
    };

    let actual_cost = pricing.compute_cost(&usage);
    let expected_cost = 3.00 + 3.75 + 0.30 + 15.00;
    assert!((actual_cost - expected_cost).abs() < 1e-6);

    let baseline_cost = pricing.compute_baseline_cost(&usage);
    // Baseline prompt = 3M tokens @ $3.00/M ($9.00) + 1M output ($15.00) = $24.00
    let expected_baseline = 9.00 + 15.00;
    assert!((baseline_cost - expected_baseline).abs() < 1e-6);
}

#[test]
fn test_all_model_pricing_rates_and_fallbacks() {
    // CRIT-LUMEN-002: Claude 3.5 Sonnet
    assert_eq!(ModelPricing::CLAUDE_3_5_SONNET.input_base_per_m, 3.00);
    assert_eq!(ModelPricing::CLAUDE_3_5_SONNET.cache_write_per_m, 3.75);
    assert_eq!(ModelPricing::CLAUDE_3_5_SONNET.cache_read_per_m, 0.30);
    assert_eq!(ModelPricing::CLAUDE_3_5_SONNET.output_per_m, 15.00);

    // CRIT-LUMEN-003: Claude 3.5 Haiku
    assert_eq!(ModelPricing::CLAUDE_3_5_HAIKU.input_base_per_m, 0.80);
    assert_eq!(ModelPricing::CLAUDE_3_5_HAIKU.cache_write_per_m, 1.00);
    assert_eq!(ModelPricing::CLAUDE_3_5_HAIKU.cache_read_per_m, 0.08);
    assert_eq!(ModelPricing::CLAUDE_3_5_HAIKU.output_per_m, 4.00);

    // CRIT-LUMEN-004: Qwen 2.5 Coder
    assert_eq!(ModelPricing::QWEN_2_5_CODER.input_base_per_m, 0.20);
    assert_eq!(ModelPricing::QWEN_2_5_CODER.cache_write_per_m, 0.20);
    assert_eq!(ModelPricing::QWEN_2_5_CODER.cache_read_per_m, 0.05);
    assert_eq!(ModelPricing::QWEN_2_5_CODER.output_per_m, 0.60);

    // DeepSeek R1
    assert_eq!(ModelPricing::DEEPSEEK_R1.input_base_per_m, 0.55);
    assert_eq!(ModelPricing::DEEPSEEK_R1.cache_read_per_m, 0.14);
    assert_eq!(ModelPricing::DEEPSEEK_R1.output_per_m, 2.19);

    // Gemini 2.0 Flash
    assert_eq!(ModelPricing::GEMINI_2_0_FLASH.input_base_per_m, 0.10);
    assert_eq!(ModelPricing::GEMINI_2_0_FLASH.cache_read_per_m, 0.025);
    assert_eq!(ModelPricing::GEMINI_2_0_FLASH.output_per_m, 0.40);

    // CRIT-LUMEN-008: Unrecognized model string defaults to Claude 3.5 Sonnet
    let unknown_pricing = ModelPricing::from_model_name("some-obscure-custom-llm-v1");
    assert_eq!(unknown_pricing, ModelPricing::CLAUDE_3_5_SONNET);
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
        },
        timestamp: as_of,
        tier: None,
    };
    let no_cache_econ = TokenEconomics::calculate(&[no_cache_turn], "claude-3-5-sonnet", &pricing, None);
    assert!((no_cache_econ.baseline_cost_no_cache_usd - no_cache_econ.total_cost_usd).abs() < 1e-9);
    assert_eq!(no_cache_econ.net_savings_usd, 0.0);

    // (d) CRIT-LUMEN-010: TurnTokenUsage::prompt_tokens returns the sum of input, cache
    // creation, and cache read tokens.
    let usage =
        TurnTokenUsage { input_tokens: 100, cache_creation_tokens: 200, cache_read_tokens: 300, output_tokens: 400 };
    assert_eq!(usage.prompt_tokens(), 600);

    // (e) CRIT-LUMEN-006: a mixed-cache turn's efficiency_multiplier equals
    // baseline_cost_no_cache_usd / total_cost_usd exactly.
    let mixed_turn = TurnPricingInput {
        usage: TurnTokenUsage {
            input_tokens: 100_000,
            cache_creation_tokens: 50_000,
            cache_read_tokens: 50_000,
            output_tokens: 10_000,
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
        assert!(
            (input_rate - cache_write_rate).abs() < 1e-9,
            "{model}: CacheWrite rate should equal Input rate"
        );
    }
}
