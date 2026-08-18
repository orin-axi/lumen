use compact_str::CompactString;
use lumen_model::{CanonicalTurn, SchemaCitation};

use crate::traits::EntryAccumulator;

#[derive(Debug, Default, Clone)]
pub struct SchemaExtractorAccumulator {
    pub citations: Vec<SchemaCitation>,
}

impl EntryAccumulator for SchemaExtractorAccumulator {
    type Output = Vec<SchemaCitation>;

    fn update(&mut self, entry: &CanonicalTurn) {
        if let Some(text) = &entry.text {
            // Scan for schema identifiers: spec@1, plan@1, changeset@1, eval-report@1
            let schemas = ["spec@1", "plan@1", "changeset@1", "eval-report@1", "finding-report@1"];

            for s in schemas {
                if text.contains(s) {
                    let has_json_block = text.contains("```json") || text.contains("{\"$schema\"");
                    self.citations.push(SchemaCitation {
                        schema_id: CompactString::new(s),
                        turn_index: entry.turn_index,
                        is_valid: has_json_block,
                        summary: None,
                    });
                }
            }
        }
    }

    fn finalize(self) -> Self::Output {
        self.citations
    }
}
