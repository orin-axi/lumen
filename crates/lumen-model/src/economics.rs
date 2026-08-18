use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::pricing::ModelPricing;
use crate::turn::TurnTokenUsage;

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
    pub baseline_cost_no_cache_usd: f64,
    pub net_savings_usd: f64,
    pub efficiency_multiplier: f32,
    pub per_model: HashMap<CompactString, ModelTokenSummary>,
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
    /// Computes full economic summary from raw counters.
    pub fn calculate(
        input_tokens: u64,
        output_tokens: u64,
        cache_creation_tokens: u64,
        cache_read_tokens: u64,
        model_name: &str,
    ) -> Self {
        let prompt_total = input_tokens + cache_creation_tokens + cache_read_tokens;
        let cache_hit_ratio =
            if prompt_total > 0 { (cache_read_tokens as f32 / prompt_total as f32) * 100.0 } else { 0.0 };

        let pricing = ModelPricing::from_model_name(model_name);
        let usage = TurnTokenUsage { input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens };

        let total_cost_usd = pricing.compute_cost(&usage);
        let baseline_cost_no_cache_usd = pricing.compute_baseline_cost(&usage);
        let net_savings_usd = (baseline_cost_no_cache_usd - total_cost_usd).max(0.0);

        let efficiency_multiplier =
            if total_cost_usd > 0.0 { (baseline_cost_no_cache_usd / total_cost_usd) as f32 } else { 1.0 };

        let mut per_model = HashMap::new();
        per_model.insert(
            CompactString::new(model_name),
            ModelTokenSummary {
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens,
                cost_usd: total_cost_usd,
                turns: 1,
            },
        );

        Self {
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            ephemeral_5m_tokens: cache_creation_tokens,
            ephemeral_1h_tokens: 0,
            cache_hit_ratio,
            total_cost_usd,
            baseline_cost_no_cache_usd,
            net_savings_usd,
            efficiency_multiplier,
            per_model,
        }
    }
}
