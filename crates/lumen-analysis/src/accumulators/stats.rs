use lumen_model::{CanonicalTurn, TurnRole};
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatsMetrics {
    pub total_turns: usize,
    pub user_turns: usize,
    pub assistant_turns: usize,
    pub tool_result_turns: usize,
    pub system_turns: usize,
    pub total_tool_calls: usize,
    pub total_tool_results: usize,
    pub total_text_characters: usize,
}

#[derive(Debug, Default, Clone)]
pub struct StatsAccumulator {
    pub total_turns: usize,
    pub user_turns: usize,
    pub assistant_turns: usize,
    pub tool_result_turns: usize,
    pub system_turns: usize,
    pub total_tool_calls: usize,
    pub total_tool_results: usize,
    pub total_text_characters: usize,
}

impl EntryAccumulator for StatsAccumulator {
    type Output = StatsMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        self.total_turns += 1;
        match entry.role {
            TurnRole::User => self.user_turns += 1,
            TurnRole::Assistant => self.assistant_turns += 1,
            TurnRole::ToolResult => self.tool_result_turns += 1,
            TurnRole::System => self.system_turns += 1,
        }

        self.total_tool_calls += entry.tool_calls.len();
        self.total_tool_results += entry.tool_results.len();

        if let Some(text) = &entry.text {
            self.total_text_characters += text.len();
        }
    }

    fn finalize(self) -> Self::Output {
        StatsMetrics {
            total_turns: self.total_turns,
            user_turns: self.user_turns,
            assistant_turns: self.assistant_turns,
            tool_result_turns: self.tool_result_turns,
            system_turns: self.system_turns,
            total_tool_calls: self.total_tool_calls,
            total_tool_results: self.total_tool_results,
            total_text_characters: self.total_text_characters,
        }
    }
}
