use chrono::Utc;
use compact_str::CompactString;
use lumen_model::{pricing, ModelTokenSummary, TokenEconomics, TurnPricingInput, TurnTokenUsage};
use std::collections::HashMap;

use crate::traits::RawMessageAccumulator;

#[derive(Debug, Default, Clone)]
pub struct TokenUsageAccumulator {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub per_model: HashMap<CompactString, ModelTokenSummary>,
    pub default_model: CompactString,
}

impl TokenUsageAccumulator {
    pub fn new(default_model: &str) -> Self {
        Self { default_model: CompactString::new(default_model), ..Default::default() }
    }
}

impl RawMessageAccumulator for TokenUsageAccumulator {
    type Output = TokenEconomics;

    fn update_raw(&mut self, message: &serde_json::Value) {
        if let Some(msg) = message.get("message") {
            let model_str = msg.get("model").and_then(|v| v.as_str()).unwrap_or(&self.default_model);

            if model_str.starts_with("<synthetic>") {
                return;
            }

            if let Some(u) = msg.get("usage") {
                let in_tok = u.get("input_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
                let out_tok = u.get("output_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
                let cache_write = u.get("cache_creation_input_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
                let cache_read = u.get("cache_read_input_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);

                self.input_tokens += in_tok;
                self.output_tokens += out_tok;
                self.cache_creation_tokens += cache_write;
                self.cache_read_tokens += cache_read;

                let entry = self.per_model.entry(CompactString::new(model_str)).or_insert(ModelTokenSummary {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                    reasoning_tokens: 0,
                    cost_usd: 0.0,
                    turns: 0,
                });

                entry.input_tokens += in_tok;
                entry.output_tokens += out_tok;
                entry.cache_creation_tokens += cache_write;
                entry.cache_read_tokens += cache_read;
                entry.turns += 1;
            }
        }
    }

    fn finalize(self) -> Self::Output {
        if self.per_model.is_empty() {
            return TokenEconomics::calculate(&[], &self.default_model, &pricing::SEEDED, None);
        }

        let mut merged_per_model: HashMap<CompactString, ModelTokenSummary> = HashMap::new();
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut total_cache_creation = 0u64;
        let mut total_cache_read = 0u64;
        let mut total_reasoning = 0u64;
        let mut total_cost = 0.0f64;
        let mut total_baseline = 0.0f64;

        for (model_name, summary) in &self.per_model {
            let priced = TokenEconomics::calculate(
                &[TurnPricingInput {
                    usage: TurnTokenUsage {
                        input_tokens: summary.input_tokens,
                        output_tokens: summary.output_tokens,
                        cache_creation_tokens: summary.cache_creation_tokens,
                        cache_read_tokens: summary.cache_read_tokens,
                        reasoning_tokens: summary.reasoning_tokens,
                    },
                    timestamp: Utc::now(),
                    tier: None,
                }],
                model_name,
                &pricing::SEEDED,
                None,
            );

            total_input += summary.input_tokens;
            total_output += summary.output_tokens;
            total_cache_creation += summary.cache_creation_tokens;
            total_cache_read += summary.cache_read_tokens;
            total_reasoning += summary.reasoning_tokens;
            total_cost += priced.total_cost_usd;
            total_baseline += priced.baseline_cost_no_cache_usd;

            merged_per_model.insert(
                model_name.clone(),
                ModelTokenSummary {
                    input_tokens: summary.input_tokens,
                    output_tokens: summary.output_tokens,
                    cache_creation_tokens: summary.cache_creation_tokens,
                    cache_read_tokens: summary.cache_read_tokens,
                    reasoning_tokens: summary.reasoning_tokens,
                    cost_usd: priced.total_cost_usd,
                    turns: summary.turns,
                },
            );
        }

        let prompt_total = total_input + total_cache_creation + total_cache_read;
        let cache_hit_ratio =
            if prompt_total > 0 { (total_cache_read as f32 / prompt_total as f32) * 100.0 } else { 0.0 };
        let net_savings_usd = (total_baseline - total_cost).max(0.0);
        let efficiency_multiplier = if total_cost > 0.0 { (total_baseline / total_cost) as f32 } else { 1.0 };

        TokenEconomics {
            input_tokens: total_input,
            output_tokens: total_output,
            cache_creation_tokens: total_cache_creation,
            cache_read_tokens: total_cache_read,
            ephemeral_5m_tokens: total_cache_creation,
            ephemeral_1h_tokens: 0,
            cache_hit_ratio,
            total_cost_usd: total_cost,
            provided_cost_usd: None,
            baseline_cost_no_cache_usd: total_baseline,
            net_savings_usd,
            efficiency_multiplier,
            per_model: merged_per_model,
            reasoning_output_tokens: total_reasoning,
        }
    }
}
