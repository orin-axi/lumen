use compact_str::CompactString;
use lumen_model::{AttributionSource, CanonicalTurn};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AttributionMetrics {
    pub by_plugin: BTreeMap<CompactString, u64>,
    pub by_skill: BTreeMap<CompactString, u64>,
    pub unattributed_tokens: u64,
}

#[derive(Debug, Default, Clone)]
pub struct AttributionAccumulator {
    pub by_plugin: BTreeMap<CompactString, u64>,
    pub by_skill: BTreeMap<CompactString, u64>,
    pub unattributed_tokens: u64,
}

impl EntryAccumulator for AttributionAccumulator {
    type Output = AttributionMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        let tokens = entry.usage.map(|usage| usage.total_tokens()).unwrap_or(0);

        match &entry.attribution {
            Some(AttributionSource::Plugin { name }) => {
                *self.by_plugin.entry(name.clone()).or_insert(0) += tokens;
            }
            Some(AttributionSource::Skill { name, .. }) => {
                *self.by_skill.entry(name.clone()).or_insert(0) += tokens;
            }
            Some(AttributionSource::Root) | None => {
                self.unattributed_tokens += tokens;
            }
        }
    }

    fn finalize(self) -> Self::Output {
        AttributionMetrics {
            by_plugin: self.by_plugin,
            by_skill: self.by_skill,
            unattributed_tokens: self.unattributed_tokens,
        }
    }
}
