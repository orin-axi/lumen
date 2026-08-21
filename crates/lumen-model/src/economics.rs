use chrono::{DateTime, Utc};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::pricing::{PricingTable, TokenRateKind};
use crate::turn::TurnTokenUsage;

/// One turn's token usage plus the pricing context (timestamp, tier) needed to price it
/// independently -- required so a session whose turns straddle a price-change boundary (or a
/// service_tier change) is priced correctly on both sides of the boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnPricingInput {
    pub usage: TurnTokenUsage,
    pub timestamp: DateTime<Utc>,
    pub tier: Option<CompactString>,
}

/// Aggregate session token economics and financial savings metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenEconomics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub ephemeral_5m_tokens: u64,
    pub ephemeral_1h_tokens: u64,
    pub cache_hit_ratio: f32,
    pub total_cost_usd: f64,
    pub provided_cost_usd: Option<f64>,
    pub baseline_cost_no_cache_usd: f64,
    pub net_savings_usd: f64,
    pub efficiency_multiplier: f32,
    pub per_model: HashMap<CompactString, ModelTokenSummary>,
    pub reasoning_output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTokenSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
    pub turns: u64,
}

impl TokenEconomics {
    /// Computes full economic summary from one [`TurnPricingInput`] per turn. Each turn is
    /// priced independently via `pricing.rate_for` at that turn's own timestamp/tier, and the
    /// per-turn costs are summed into `total_cost_usd` -- required so a session whose turns
    /// straddle a price-change boundary is priced correctly on both sides. Token counters
    /// remain simple sums across turns, unaffected by the per-turn pricing. An empty `turns`
    /// slice returns a zeroed `TokenEconomics`, not an error.
    pub fn calculate(
        turns: &[TurnPricingInput],
        model_name: &str,
        pricing: &PricingTable,
        provided_cost_usd: Option<f64>,
    ) -> Self {
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut cache_creation_tokens = 0u64;
        let mut cache_read_tokens = 0u64;
        let mut total_cost_usd = 0.0f64;
        let mut baseline_cost_no_cache_usd = 0.0f64;

        for turn in turns {
            let usage = &turn.usage;
            let tier = turn.tier.as_deref();

            let input_rate = pricing.rate_for(model_name, tier, TokenRateKind::Input, turn.timestamp);
            let cache_write_rate = pricing.rate_for(model_name, tier, TokenRateKind::CacheWrite, turn.timestamp);
            let cache_read_rate = pricing.rate_for(model_name, tier, TokenRateKind::CacheRead, turn.timestamp);
            let output_rate = pricing.rate_for(model_name, tier, TokenRateKind::Output, turn.timestamp);

            let turn_cost = (usage.input_tokens as f64 / 1_000_000.0) * input_rate
                + (usage.cache_creation_tokens as f64 / 1_000_000.0) * cache_write_rate
                + (usage.cache_read_tokens as f64 / 1_000_000.0) * cache_read_rate
                + (usage.output_tokens as f64 / 1_000_000.0) * output_rate;

            let turn_baseline_cost = (usage.prompt_tokens() as f64 / 1_000_000.0) * input_rate
                + (usage.output_tokens as f64 / 1_000_000.0) * output_rate;

            input_tokens += usage.input_tokens;
            output_tokens += usage.output_tokens;
            cache_creation_tokens += usage.cache_creation_tokens;
            cache_read_tokens += usage.cache_read_tokens;
            total_cost_usd += turn_cost;
            baseline_cost_no_cache_usd += turn_baseline_cost;
        }

        let prompt_total = input_tokens + cache_creation_tokens + cache_read_tokens;
        let cache_hit_ratio =
            if prompt_total > 0 { (cache_read_tokens as f32 / prompt_total as f32) * 100.0 } else { 0.0 };

        let net_savings_usd = (baseline_cost_no_cache_usd - total_cost_usd).max(0.0);

        let efficiency_multiplier =
            if total_cost_usd > 0.0 { (baseline_cost_no_cache_usd / total_cost_usd) as f32 } else { 1.0 };

        let mut per_model = HashMap::new();
        if !turns.is_empty() {
            per_model.insert(
                CompactString::new(model_name),
                ModelTokenSummary {
                    input_tokens,
                    output_tokens,
                    cache_creation_tokens,
                    cache_read_tokens,
                    cost_usd: total_cost_usd,
                    turns: turns.len() as u64,
                },
            );
        }

        Self {
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            ephemeral_5m_tokens: cache_creation_tokens,
            ephemeral_1h_tokens: 0,
            cache_hit_ratio,
            total_cost_usd,
            provided_cost_usd,
            baseline_cost_no_cache_usd,
            net_savings_usd,
            efficiency_multiplier,
            per_model,
            reasoning_output_tokens: 0,
        }
    }
}
