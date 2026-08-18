use lumen_model::CanonicalTurn;
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SelfCorrectionMetrics {
    pub tool_retry_corrections: u32,
    pub approach_pivot_corrections: u32,
    pub total_corrections: u32,
}

#[derive(Debug, Default, Clone)]
pub struct SelfCorrectionAccumulator {
    pub last_turn_had_error: bool,
    pub tool_retry_corrections: u32,
    pub approach_pivot_corrections: u32,
}

impl EntryAccumulator for SelfCorrectionAccumulator {
    type Output = SelfCorrectionMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        let current_has_error = entry.tool_results.iter().any(|r| r.is_error);

        if self.last_turn_had_error && !entry.tool_calls.is_empty() {
            if current_has_error {
                self.approach_pivot_corrections += 1;
            } else {
                self.tool_retry_corrections += 1;
            }
        }

        self.last_turn_had_error = current_has_error;
    }

    fn finalize(self) -> Self::Output {
        let total_corrections = self.tool_retry_corrections + self.approach_pivot_corrections;
        SelfCorrectionMetrics {
            tool_retry_corrections: self.tool_retry_corrections,
            approach_pivot_corrections: self.approach_pivot_corrections,
            total_corrections,
        }
    }
}
