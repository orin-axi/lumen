use lumen_model::{CanonicalTurn, TurnRole};
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionMetrics {
    pub auto_accepted_actions: usize,
    pub manual_approval_prompts: usize,
    pub auto_accept_rate: f32,
}

#[derive(Debug, Default, Clone)]
pub struct PermissionModeAccumulator {
    pub auto_accepted_actions: usize,
    pub manual_approval_prompts: usize,
}

impl EntryAccumulator for PermissionModeAccumulator {
    type Output = PermissionMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        if entry.role == TurnRole::User {
            if let Some(text) = &entry.text {
                let trimmed = text.trim();
                if trimmed == "y" || trimmed == "yes" || trimmed == "ok" || trimmed == "proceed" {
                    self.manual_approval_prompts += 1;
                }
            }
        } else if !entry.tool_calls.is_empty() {
            self.auto_accepted_actions += entry.tool_calls.len();
        }
    }

    fn finalize(self) -> Self::Output {
        let total = self.auto_accepted_actions + self.manual_approval_prompts;
        let auto_accept_rate = if total > 0 { self.auto_accepted_actions as f32 / total as f32 } else { 1.0 };

        PermissionMetrics {
            auto_accepted_actions: self.auto_accepted_actions,
            manual_approval_prompts: self.manual_approval_prompts,
            auto_accept_rate,
        }
    }
}
