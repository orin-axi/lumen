use compact_str::CompactString;
use lumen_model::CanonicalTurn;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ToolCluster {
    pub canonical: CompactString,
    pub variants: Vec<(CompactString, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FuzzyToolsMetrics {
    pub clusters: Vec<ToolCluster>,
    pub typo_call_count: usize,
    pub typo_rate: f64,
    pub total_tool_calls: usize,
}

#[derive(Debug, Default, Clone)]
pub struct FuzzyToolsAccumulator {
    pub counts: BTreeMap<CompactString, usize>,
    pub total_tool_calls: usize,
}

impl EntryAccumulator for FuzzyToolsAccumulator {
    type Output = FuzzyToolsMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        for call in &entry.tool_calls {
            *self.counts.entry(call.tool_name.clone()).or_insert(0) += 1;
        }
        self.total_tool_calls += entry.tool_calls.len();
    }

    fn finalize(self) -> Self::Output {
        let mut sorted: Vec<(CompactString, usize)> = self.counts.into_iter().collect();
        sorted.sort_by(|a, b| Reverse(a.1).cmp(&Reverse(b.1)).then_with(|| a.0.cmp(&b.0)));

        let mut visited: BTreeSet<CompactString> = BTreeSet::new();
        let mut clusters: Vec<ToolCluster> = Vec::new();

        for i in 0..sorted.len() {
            let canonical = sorted[i].0.clone();
            if visited.contains(&canonical) {
                continue;
            }
            visited.insert(canonical.clone());

            let mut variants: Vec<(CompactString, usize)> = Vec::new();
            for (candidate, count) in sorted.iter().skip(i + 1) {
                if visited.contains(candidate) {
                    continue;
                }
                let threshold = if candidate.len() < 5 { 1 } else { 2 };
                let distance = levenshtein(&canonical, candidate);
                if distance > 0 && distance <= threshold {
                    visited.insert(candidate.clone());
                    variants.push((candidate.clone(), *count));
                }
            }

            clusters.push(ToolCluster { canonical, variants });
        }

        let typo_call_count: usize =
            clusters.iter().map(|c| c.variants.iter().map(|(_, count)| count).sum::<usize>()).sum();

        let typo_rate =
            if self.total_tool_calls == 0 { 0.0 } else { typo_call_count as f64 / self.total_tool_calls as f64 };

        FuzzyToolsMetrics { clusters, typo_call_count, typo_rate, total_tool_calls: self.total_tool_calls }
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());

    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate() {
        *cell = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1).min(dp[i][j - 1] + 1).min(dp[i - 1][j - 1] + cost);
        }
    }

    dp[m][n]
}
