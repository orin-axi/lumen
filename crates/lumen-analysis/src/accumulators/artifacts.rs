use compact_str::CompactString;
use lumen_model::{CanonicalTurn, ToolIntent};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtifactMetrics {
    pub files_read: BTreeSet<CompactString>,
    pub files_created: BTreeSet<CompactString>,
    pub files_edited: BTreeSet<CompactString>,
    pub total_unique_files: usize,
}

#[derive(Debug, Default, Clone)]
pub struct ArtifactsAccumulator {
    pub files_read: BTreeSet<CompactString>,
    pub files_created: BTreeSet<CompactString>,
    pub files_edited: BTreeSet<CompactString>,
}

impl EntryAccumulator for ArtifactsAccumulator {
    type Output = ArtifactMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        for call in &entry.tool_calls {
            match &call.intent {
                ToolIntent::FileRead { path, .. } if !path.is_empty() => {
                    self.files_read.insert(path.clone());
                }
                ToolIntent::FileCreate { path } if !path.is_empty() => {
                    self.files_created.insert(path.clone());
                }
                ToolIntent::FileEdit { path, .. } if !path.is_empty() => {
                    self.files_edited.insert(path.clone());
                }
                _ => {}
            }
        }
    }

    fn finalize(self) -> Self::Output {
        let mut all_files = BTreeSet::new();
        all_files.extend(self.files_read.iter().cloned());
        all_files.extend(self.files_created.iter().cloned());
        all_files.extend(self.files_edited.iter().cloned());

        ArtifactMetrics {
            total_unique_files: all_files.len(),
            files_read: self.files_read,
            files_created: self.files_created,
            files_edited: self.files_edited,
        }
    }
}
