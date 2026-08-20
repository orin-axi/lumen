use lumen_model::CanonicalTurn;
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlowMetrics {
    pub streak_lengths: Vec<usize>,
    pub longest_streak: usize,
    pub avg_streak_len: f64,
    pub permission_blocks: usize,
    pub total_tool_calls: usize,
    pub flow_ratio: f64,
}

#[derive(Debug, Default, Clone)]
pub struct FlowAccumulator {
    pub current_streak: usize,
    pub streak_lengths: Vec<usize>,
    pub permission_blocks: usize,
    pub total_tool_calls: usize,
}

impl FlowAccumulator {
    fn is_permission_break(entry: &CanonicalTurn) -> bool {
        entry.tool_results.iter().any(|result| {
            result.is_error && result.error_class.as_deref().is_some_and(|c| c.to_lowercase().contains("permission"))
        })
    }

    fn flush_streak(&mut self) {
        if self.current_streak > 0 {
            self.streak_lengths.push(self.current_streak);
            self.current_streak = 0;
        }
    }
}

impl EntryAccumulator for FlowAccumulator {
    type Output = FlowMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        self.total_tool_calls += entry.tool_calls.len();

        if Self::is_permission_break(entry) {
            self.flush_streak();
            self.permission_blocks += 1;
        } else {
            self.current_streak += entry.tool_calls.len();
        }
    }

    fn finalize(mut self) -> Self::Output {
        self.flush_streak();

        let longest_streak = self.streak_lengths.iter().max().copied().unwrap_or(0);
        let avg_streak_len = if self.streak_lengths.is_empty() {
            0.0
        } else {
            self.streak_lengths.iter().sum::<usize>() as f64 / self.streak_lengths.len() as f64
        };
        let flow_ratio = if self.total_tool_calls == 0 {
            0.0
        } else {
            self.streak_lengths.iter().filter(|&&n| n >= 2).sum::<usize>() as f64 / self.total_tool_calls as f64
        };

        FlowMetrics {
            streak_lengths: self.streak_lengths,
            longest_streak,
            avg_streak_len,
            permission_blocks: self.permission_blocks,
            total_tool_calls: self.total_tool_calls,
            flow_ratio,
        }
    }
}
