use lumen_model::{CanonicalTurn, ToolIntent};
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpAffinityMetrics {
    pub structured_mcp_count: u32,
    pub raw_shell_count: u32,
    pub mcp_adoption_ratio: f32,
}

#[derive(Debug, Default, Clone)]
pub struct McpAffinityAccumulator {
    pub structured_mcp_count: u32,
    pub raw_shell_count: u32,
}

impl EntryAccumulator for McpAffinityAccumulator {
    type Output = McpAffinityMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        for call in &entry.tool_calls {
            match &call.intent {
                ToolIntent::McpCall { .. } => self.structured_mcp_count += 1,
                ToolIntent::Other { raw_name } if raw_name.contains("mcp") => self.structured_mcp_count += 1,
                ToolIntent::Other { raw_name } if raw_name == "run_command" || raw_name == "Bash" => {
                    self.raw_shell_count += 1;
                }
                _ => {}
            }
        }
    }

    fn finalize(self) -> Self::Output {
        let total = self.structured_mcp_count + self.raw_shell_count;
        let mcp_adoption_ratio = if total > 0 { self.structured_mcp_count as f32 / total as f32 } else { 1.0 };

        McpAffinityMetrics {
            structured_mcp_count: self.structured_mcp_count,
            raw_shell_count: self.raw_shell_count,
            mcp_adoption_ratio,
        }
    }
}
