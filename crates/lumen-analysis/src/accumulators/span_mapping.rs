use compact_str::CompactString;
use lumen_model::CanonicalTurn;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::traits::EntryAccumulator;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanMappingAccumulator {
    pub mapped: BTreeMap<CompactString, CompactString>,
    pub unmapped_tool_use_count: usize,
}

impl EntryAccumulator for SpanMappingAccumulator {
    type Output = Self;

    fn update(&mut self, entry: &CanonicalTurn) {
        for result in &entry.tool_results {
            if let Some(span_id) = &result.otel_span_id {
                self.mapped.insert(result.call_id.clone(), span_id.clone());
            } else {
                self.unmapped_tool_use_count += 1;
            }
        }
    }

    fn finalize(self) -> Self::Output {
        self
    }
}
