use compact_str::CompactString;
use lumen_model::CanonicalTurn;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolInventoryMetrics {
    pub distinct_tools_count: usize,
    pub total_invocations: usize,
    pub invocations_by_tool: BTreeMap<CompactString, usize>,
    pub errors_by_tool: BTreeMap<CompactString, usize>,
}

#[derive(Debug, Default, Clone)]
pub struct ToolInventoryAccumulator {
    pub invocations_by_tool: BTreeMap<CompactString, usize>,
    pub errors_by_tool: BTreeMap<CompactString, usize>,
    pub last_tool_name: Option<CompactString>,
}

impl EntryAccumulator for ToolInventoryAccumulator {
    type Output = ToolInventoryMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        for call in &entry.tool_calls {
            *self.invocations_by_tool.entry(call.tool_name.clone()).or_insert(0) += 1;
            self.last_tool_name = Some(call.tool_name.clone());
        }

        for result in &entry.tool_results {
            if result.is_error {
                if let Some(tool) = &self.last_tool_name {
                    *self.errors_by_tool.entry(tool.clone()).or_insert(0) += 1;
                }
            }
        }
    }

    fn finalize(self) -> Self::Output {
        let total_invocations: usize = self.invocations_by_tool.values().sum();
        let distinct_tools_count = self.invocations_by_tool.len();

        ToolInventoryMetrics {
            distinct_tools_count,
            total_invocations,
            invocations_by_tool: self.invocations_by_tool,
            errors_by_tool: self.errors_by_tool,
        }
    }
}
