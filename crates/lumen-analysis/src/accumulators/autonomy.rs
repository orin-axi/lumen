use lumen_model::{CanonicalTurn, TurnRole};
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutonomyMetrics {
    pub max_autonomous_streak: usize,
    pub avg_autonomous_streak: f32,
    pub total_streaks: usize,
    pub autonomy_index: f32,
}

#[derive(Debug, Default, Clone)]
pub struct AutonomyAccumulator {
    pub current_streak: usize,
    pub max_streak: usize,
    pub streak_lengths: Vec<usize>,
    pub assistant_turns: usize,
    pub total_turns: usize,
}

impl EntryAccumulator for AutonomyAccumulator {
    type Output = AutonomyMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        self.total_turns += 1;
        match entry.role {
            TurnRole::Assistant | TurnRole::ToolResult => {
                self.current_streak += 1;
                self.assistant_turns += 1;
                self.max_streak = self.max_streak.max(self.current_streak);
            }
            TurnRole::User => {
                if self.current_streak > 0 {
                    self.streak_lengths.push(self.current_streak);
                    self.current_streak = 0;
                }
            }
            TurnRole::System => {}
        }
    }

    fn finalize(mut self) -> Self::Output {
        if self.current_streak > 0 {
            self.streak_lengths.push(self.current_streak);
        }

        let total_streaks = self.streak_lengths.len();
        let avg_streak = if total_streaks > 0 {
            self.streak_lengths.iter().sum::<usize>() as f32 / total_streaks as f32
        } else {
            0.0
        };

        let autonomy_index =
            if self.total_turns > 0 { self.assistant_turns as f32 / self.total_turns as f32 } else { 0.0 };

        AutonomyMetrics {
            max_autonomous_streak: self.max_streak,
            avg_autonomous_streak: avg_streak,
            total_streaks,
            autonomy_index,
        }
    }
}
