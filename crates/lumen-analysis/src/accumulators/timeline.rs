use chrono::{DateTime, Utc};
use lumen_model::{CanonicalTurn, TurnRole};
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

const IDLE_GAP_THRESHOLD_MS: i64 = 300_000;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TimelineAccumulator {
    pub current_streak: usize,
    pub assistant_streak_count: usize,
    pub longest_streak_turns: usize,
    pub idle_gap_count: usize,
    pub total_idle_ms: i64,
    pub longest_idle_gap_ms: i64,
    pub last_timestamp: Option<DateTime<Utc>>,
}

impl EntryAccumulator for TimelineAccumulator {
    type Output = Self;

    fn update(&mut self, entry: &CanonicalTurn) {
        if entry.role == TurnRole::Assistant {
            self.current_streak += 1;
        } else if self.current_streak > 0 {
            self.assistant_streak_count += 1;
            self.longest_streak_turns = self.longest_streak_turns.max(self.current_streak);
            self.current_streak = 0;
        }

        if let Some(prev) = self.last_timestamp {
            let gap = (entry.timestamp - prev).num_milliseconds();
            if gap > IDLE_GAP_THRESHOLD_MS {
                self.idle_gap_count += 1;
                self.total_idle_ms += gap;
                self.longest_idle_gap_ms = self.longest_idle_gap_ms.max(gap);
            }
        }
        self.last_timestamp = Some(entry.timestamp);
    }

    fn finalize(self) -> Self::Output {
        self
    }
}
