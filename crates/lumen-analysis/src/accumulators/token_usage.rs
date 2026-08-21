use compact_str::CompactString;
use chrono::Utc;
use lumen_model::{ModelTokenSummary, PricingTable, TokenEconomics, TurnPricingInput, TurnTokenUsage};
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
        TokenEconomics::calculate(
            &[TurnPricingInput {
                usage: TurnTokenUsage {
                    input_tokens: self.input_tokens,
                    output_tokens: self.output_tokens,
                    cache_creation_tokens: self.cache_creation_tokens,
                    cache_read_tokens: self.cache_read_tokens,
                },
                timestamp: Utc::now(),
                tier: None,
            }],
            &self.default_model,
            &PricingTable::seed(),
            None,
        )
    }
}
