use compact_str::CompactString;
use lumen_model::{CanonicalTurn, ToolIntent};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::traits::EntryAccumulator;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrLinkMetrics {
    pub pr_urls: BTreeSet<CompactString>,
    pub first_pr_turn_index: Option<usize>,
    pub linked_via_vcs_tool: usize,
}

#[derive(Debug, Default, Clone)]
pub struct PrLinkAccumulator {
    pub pr_urls: BTreeSet<CompactString>,
    pub first_pr_turn_index: Option<usize>,
    pub linked_via_vcs_tool: usize,
}

/// Scans `text` for `github.com/{owner}/{repo}/pull/{digits}` substrings via manual
/// string splitting (no regex crate is available in this workspace).
fn find_pr_matches(text: &str) -> Vec<(CompactString, CompactString, CompactString)> {
    let mut matches = Vec::new();
    let marker = "github.com/";
    let mut search_from = 0;

    while let Some(rel_idx) = text[search_from..].find(marker) {
        let start = search_from + rel_idx + marker.len();
        let remainder = &text[start..];
        let mut segments = remainder.splitn(4, '/');

        let owner = segments.next().unwrap_or("");
        let repo = segments.next().unwrap_or("");
        let literal_pull = segments.next().unwrap_or("");
        let tail = segments.next().unwrap_or("");

        if !owner.is_empty() && !repo.is_empty() && literal_pull == "pull" {
            let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                matches.push((CompactString::new(owner), CompactString::new(repo), CompactString::new(digits)));
            }
        }

        search_from = start;
    }

    matches
}

impl PrLinkAccumulator {
    fn record(&mut self, turn_index: usize, owner: &str, repo: &str, digits: &str) {
        self.pr_urls.insert(CompactString::new(format!("{owner}/{repo}#{digits}")));
        if self.first_pr_turn_index.is_none() {
            self.first_pr_turn_index = Some(turn_index);
        }
    }
}

impl EntryAccumulator for PrLinkAccumulator {
    type Output = PrLinkMetrics;

    fn update(&mut self, entry: &CanonicalTurn) {
        let call_intents: BTreeMap<&CompactString, &ToolIntent> =
            entry.tool_calls.iter().map(|call| (&call.call_id, &call.intent)).collect();

        for result in &entry.tool_results {
            let Some(output) = &result.truncated_output else { continue };
            let is_vcs = matches!(call_intents.get(&result.call_id), Some(ToolIntent::VersionControl { .. }));

            for (owner, repo, digits) in find_pr_matches(output) {
                self.record(entry.turn_index, &owner, &repo, &digits);
                if is_vcs {
                    self.linked_via_vcs_tool += 1;
                }
            }
        }

        if let Some(text) = &entry.text {
            for (owner, repo, digits) in find_pr_matches(text) {
                self.record(entry.turn_index, &owner, &repo, &digits);
            }
        }
    }

    fn finalize(self) -> Self::Output {
        PrLinkMetrics {
            pr_urls: self.pr_urls,
            first_pr_turn_index: self.first_pr_turn_index,
            linked_via_vcs_tool: self.linked_via_vcs_tool,
        }
    }
}
