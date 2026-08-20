use compact_str::CompactString;
use lumen_model::{CanonicalTurn, ToolIntent};
use serde::{Deserialize, Serialize};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolNode {
    pub call_id: CompactString,
    pub tool_name: CompactString,
    pub target_file: Option<CompactString>,
    pub target_symbol: Option<CompactString>,
    pub is_mutation: bool,
    pub had_error: bool,
}

#[derive(Debug, Default, Clone)]
pub struct TrajectoryDagAccumulator {
    pub nodes: Vec<ToolNode>,
}

impl EntryAccumulator for TrajectoryDagAccumulator {
    type Output = Vec<ToolNode>;

    fn update(&mut self, entry: &CanonicalTurn) {
        for call in &entry.tool_calls {
            let (target_file, target_symbol, is_mutation) = match &call.intent {
                ToolIntent::FileRead { path, .. } => (Some(path.clone()), None, false),
                ToolIntent::FileEdit { path, .. } => (Some(path.clone()), None, true),
                ToolIntent::FileCreate { path } => (Some(path.clone()), None, true),
                ToolIntent::CodeSearch { query, .. } => (None, Some(query.clone()), false),
                ToolIntent::VersionControl { action } => (None, None, action == "commit" || action == "push"),
                ToolIntent::FileDiscovery { .. }
                | ToolIntent::TestExecution { .. }
                | ToolIntent::SubagentSpawn { .. }
                | ToolIntent::McpCall { .. }
                | ToolIntent::Other { .. } => (None, None, false),
            };

            let had_error = entry
                .tool_results
                .iter()
                .find(|result| result.call_id == call.call_id)
                .map(|result| result.is_error)
                .unwrap_or(false);

            self.nodes.push(ToolNode {
                call_id: call.call_id.clone(),
                tool_name: call.tool_name.clone(),
                target_file,
                target_symbol,
                is_mutation,
                had_error,
            });
        }
    }

    fn finalize(self) -> Self::Output {
        self.nodes
    }
}
