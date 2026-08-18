use lumen_model::*;

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
fn test_token_economics_summary_and_zero_division() {
    // CRIT-LUMEN-005 & CRIT-LUMEN-006: Standard TokenEconomics
    let economics = TokenEconomics::calculate(
        10_000,  // Uncached input
        2_000,   // Output
        20_000,  // Cache write
        170_000, // Cache read (85% of total prompt)
        "claude-3-5-sonnet-20241022",
    );

    assert_eq!(economics.input_tokens, 10_000);
    assert_eq!(economics.cache_creation_tokens, 20_000);
    assert_eq!(economics.cache_read_tokens, 170_000);
    assert_eq!(economics.output_tokens, 2_000);
    assert!((economics.cache_hit_ratio - 85.0).abs() < 1e-4);
    assert!(economics.net_savings_usd > 0.0);
    assert!(economics.efficiency_multiplier > 1.0);

    // CRIT-LUMEN-007: Zero prompt tokens must return 0.0 hit ratio and 1.0 efficiency without dividing by zero
    let zero_econ = TokenEconomics::calculate(0, 0, 0, 0, "claude-3-5-sonnet-20241022");
    assert_eq!(zero_econ.cache_hit_ratio, 0.0);
    assert_eq!(zero_econ.efficiency_multiplier, 1.0);
    assert_eq!(zero_econ.total_cost_usd, 0.0);
    assert_eq!(zero_econ.net_savings_usd, 0.0);

    // CRIT-LUMEN-009: Net savings clamped to >= 0.0
    assert!(zero_econ.net_savings_usd >= 0.0);
    assert!(economics.net_savings_usd >= 0.0);

    // CRIT-LUMEN-010: TurnTokenUsage::prompt_tokens returns sum
    let usage =
        TurnTokenUsage { input_tokens: 100, cache_creation_tokens: 200, cache_read_tokens: 300, output_tokens: 400 };
    assert_eq!(usage.prompt_tokens(), 600);
    assert_eq!(usage.total_tokens(), 1000);
}
