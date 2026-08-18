use compact_str::CompactString;
use lumen_model::{CanonicalTurn, ToolIntent};
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitStallEvent {
    pub agent_pair: CompactString,
    pub observed_rounds: usize,
    pub turn_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CircuitBreakerReport {
    pub max_observed_rounds: usize,
    pub stalls: Vec<CircuitStallEvent>,
    pub tripped: bool,
}

#[derive(Debug, Default, Clone)]
pub struct CircuitBreakerAccumulator {
    pub current_agent_pair: Option<CompactString>,
    pub consecutive_rounds: usize,
    pub max_observed_rounds: usize,
    pub stalls: Vec<CircuitStallEvent>,
}

impl EntryAccumulator for CircuitBreakerAccumulator {
    type Output = CircuitBreakerReport;

    fn update(&mut self, entry: &CanonicalTurn) {
        for call in &entry.tool_calls {
            if let ToolIntent::SubagentSpawn { agent_type, .. } = &call.intent {
                let pair = CompactString::new(format!("parent->{agent_type}"));

                if self.current_agent_pair.as_ref() == Some(&pair) {
                    self.consecutive_rounds += 1;
                } else {
                    self.current_agent_pair = Some(pair.clone());
                    self.consecutive_rounds = 1;
                }

                self.max_observed_rounds = self.max_observed_rounds.max(self.consecutive_rounds);

                if self.consecutive_rounds > 2 {
                    self.stalls.push(CircuitStallEvent {
                        agent_pair: pair,
                        observed_rounds: self.consecutive_rounds,
                        turn_index: entry.turn_index,
                    });
                }
            }
        }
    }

    fn finalize(self) -> Self::Output {
        let tripped = self.max_observed_rounds > 2;
        CircuitBreakerReport { max_observed_rounds: self.max_observed_rounds, stalls: self.stalls, tripped }
    }
}
