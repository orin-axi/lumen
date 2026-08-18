use lumen_model::CanonicalTurn;
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TurnDurationMetrics {
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub avg_ms: u64,
    pub total_turns: usize,
}

#[derive(Debug, Default, Clone)]
pub struct TurnDurationAccumulator {
    pub latencies: Vec<u64>,
}

impl EntryAccumulator for TurnDurationAccumulator {
    type Output = TurnDurationMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        if entry.latency_ms > 0 {
            self.latencies.push(entry.latency_ms);
        }
    }

    fn finalize(mut self) -> Self::Output {
        if self.latencies.is_empty() {
            return TurnDurationMetrics::default();
        }

        self.latencies.sort_unstable();
        let total_turns = self.latencies.len();
        let sum: u64 = self.latencies.iter().sum();
        let avg_ms = sum / (total_turns as u64);

        let p50_idx = (total_turns as f64 * 0.50).floor() as usize;
        let p95_idx = ((total_turns as f64 * 0.95).floor() as usize).min(total_turns - 1);

        TurnDurationMetrics { p50_ms: self.latencies[p50_idx], p95_ms: self.latencies[p95_idx], avg_ms, total_turns }
    }
}
