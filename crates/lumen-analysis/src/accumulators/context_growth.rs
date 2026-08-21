use lumen_model::CanonicalTurn;
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextGrowthMetrics {
    pub initial_prompt_tokens: u64,
    pub peak_prompt_tokens: u64,
    pub final_prompt_tokens: u64,
    pub avg_growth_per_turn: f64,
    pub max_jump_tokens: u64,
}

#[derive(Debug, Default, Clone)]
pub struct ContextGrowthAccumulator {
    pub initial_prompt_tokens: Option<u64>,
    pub previous_prompt_tokens: u64,
    pub peak_prompt_tokens: u64,
    pub final_prompt_tokens: u64,
    pub total_growth: u64,
    pub turn_count: usize,
    pub max_jump_tokens: u64,
}

impl EntryAccumulator for ContextGrowthAccumulator {
    type Output = ContextGrowthMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        if let Some(usage) = entry.usage {
            let prompt = usage.prompt_tokens();
            if self.initial_prompt_tokens.is_none() {
                self.initial_prompt_tokens = Some(prompt);
            }

            if prompt > self.previous_prompt_tokens && self.previous_prompt_tokens > 0 {
                let jump = prompt - self.previous_prompt_tokens;
                self.total_growth += jump;
                self.max_jump_tokens = self.max_jump_tokens.max(jump);
            }

            self.peak_prompt_tokens = self.peak_prompt_tokens.max(prompt);
            self.final_prompt_tokens = prompt;
            self.previous_prompt_tokens = prompt;
            self.turn_count += 1;
        }
    }

    fn finalize(self) -> Self::Output {
        let avg_growth =
            if self.turn_count > 1 { self.total_growth as f64 / (self.turn_count - 1) as f64 } else { 0.0 };

        ContextGrowthMetrics {
            initial_prompt_tokens: self.initial_prompt_tokens.unwrap_or(0),
            peak_prompt_tokens: self.peak_prompt_tokens,
            final_prompt_tokens: self.final_prompt_tokens,
            avg_growth_per_turn: avg_growth,
            max_jump_tokens: self.max_jump_tokens,
        }
    }
}
