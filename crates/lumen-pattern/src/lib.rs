use compact_str::CompactString;
use lumen_model::{CanonicalTranscript, ToolIntent};
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolNode {
    pub turn_index: usize,
    pub tool_name: CompactString,
    pub target_symbol: Option<CompactString>,
    pub target_file: Option<CompactString>,
    pub is_mutation: bool,
    pub had_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CircularLoopAnomaly {
    pub symbol: CompactString,
    pub cycle_depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryMetrics {
    pub grounding_score: f32,
    pub recovery_index: f32,
    pub monotonicity: f32,
    pub trajectory_efficiency: f32,
    pub circular_loops: Vec<CircularLoopAnomaly>,
}

pub struct TrajectoryGraph {
    pub graph: DiGraph<ToolNode, ()>,
    pub last_node: Option<NodeIndex>,
    pub total_mutations: usize,
    pub total_reads: usize,
    /// Most recent node index seen for a given grounded target (symbol, falling back to
    /// file). Used to close a back-edge when the same target is revisited, so repeated
    /// access to the same symbol/file forms an actual graph cycle for Tarjan SCC to find --
    /// without this, `push_tool` only ever produces a linear chain and no cycle is
    /// topologically possible regardless of how `target_symbol`/`target_file` are populated.
    target_last_seen: std::collections::HashMap<CompactString, NodeIndex>,
}

impl Default for TrajectoryGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl TrajectoryGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            last_node: None,
            total_mutations: 0,
            total_reads: 0,
            target_last_seen: std::collections::HashMap::new(),
        }
    }

    pub fn push_tool(&mut self, node: ToolNode) -> NodeIndex {
        if node.is_mutation {
            self.total_mutations += 1;
        } else {
            self.total_reads += 1;
        }

        let target_key = Self::grounded_key(&node);

        let current = self.graph.add_node(node);
        if let Some(prev) = self.last_node {
            self.graph.add_edge(prev, current, ());
        }

        if let Some(key) = target_key {
            if let Some(&prior) = self.target_last_seen.get(&key) {
                // Close the loop: prior --(forward chain)--> current --(back-edge)--> prior
                self.graph.add_edge(current, prior, ());
            }
            self.target_last_seen.insert(key, current);
        }

        self.last_node = Some(current);
        current
    }

    pub fn detect_circular_loops(&self) -> Vec<CircularLoopAnomaly> {
        let mut anomalies = Vec::new();
        let sccs = tarjan_scc(&self.graph);

        for component in sccs {
            if component.len() >= 3 {
                let first_key = Self::grounded_key(&self.graph[component[0]]);
                let is_redundant = component
                    .iter()
                    .all(|idx| Self::grounded_key(&self.graph[*idx]) == first_key && !self.graph[*idx].is_mutation);

                if is_redundant {
                    if let Some(key) = first_key {
                        anomalies.push(CircularLoopAnomaly { symbol: key, cycle_depth: component.len() });
                    }
                }
            }
        }
        anomalies
    }

    /// Grounded key for cycle detection: the node's target symbol, falling back to its
    /// target file when no symbol is present. Shared between `push_tool` (which uses it
    /// to close back-edges) and `detect_circular_loops` (which uses it to decide whether
    /// an SCC is a genuine redundant-target cycle) so the two can never drift out of sync
    /// -- a back-edge closed on one key must be checked against that same key.
    fn grounded_key(node: &ToolNode) -> Option<CompactString> {
        node.target_symbol.clone().or_else(|| node.target_file.clone())
    }

    pub fn calculate_efficiency(&self) -> f32 {
        let total = self.total_mutations + self.total_reads;
        if total == 0 {
            return 1.0;
        }
        (self.total_mutations as f32 + (self.total_reads as f32 * 0.5)) / (total as f32)
    }

    pub fn calculate_monotonicity(&self) -> f32 {
        let total_nodes = self.graph.node_count();
        if total_nodes == 0 {
            return 1.0;
        }
        let anomalies = self.detect_circular_loops();
        let loop_nodes: usize = anomalies.iter().map(|a| a.cycle_depth).sum();
        let loop_penalty = (loop_nodes as f32 / total_nodes as f32).min(1.0);

        (1.0 - loop_penalty).max(0.0)
    }
}

/// Computes Argument Grounding Score G = (Grounded Tool Args) / (Total Invocations).
pub fn compute_grounding_score(transcript: &CanonicalTranscript) -> f32 {
    let mut total_tools = 0;
    let mut grounded_tools = 0;

    for turn in &transcript.turns {
        for call in &turn.tool_calls {
            total_tools += 1;
            match &call.intent {
                ToolIntent::FileRead { path, .. } | ToolIntent::FileEdit { path, .. } => {
                    if !path.is_empty() {
                        grounded_tools += 1;
                    }
                }
                ToolIntent::CodeSearch { query, .. } => {
                    if !query.is_empty() {
                        grounded_tools += 1;
                    }
                }
                _ => {
                    grounded_tools += 1;
                }
            }
        }
    }

    if total_tools == 0 {
        1.0
    } else {
        grounded_tools as f32 / total_tools as f32
    }
}

/// Computes Error Recovery Index R = (Adaptive Error Pivots) / (Total Error Events).
pub fn compute_recovery_index(transcript: &CanonicalTranscript) -> f32 {
    let mut error_events = 0;
    let mut adaptive_pivots = 0;

    for i in 0..transcript.turns.len() {
        let turn = &transcript.turns[i];
        let has_error = turn.tool_results.iter().any(|r| r.is_error);

        if has_error {
            error_events += 1;
            if i + 1 < transcript.turns.len() {
                let next_turn = &transcript.turns[i + 1];
                let next_has_error = next_turn.tool_results.iter().any(|r| r.is_error);
                if !next_has_error {
                    adaptive_pivots += 1;
                }
            }
        }
    }

    if error_events == 0 {
        1.0
    } else {
        adaptive_pivots as f32 / error_events as f32
    }
}

/// Derives (target_file, target_symbol, is_mutation) from a tool call's intent, matching
/// the grounding convention established by CRIT-LUMEN-145 (trajectory_dag accumulator):
/// FileRead/FileEdit/FileCreate ground on their path; CodeSearch grounds on its query;
/// mutation is true for FileEdit, FileCreate, and VersionControl whose second
/// whitespace-separated word (the git subcommand, e.g. "commit" in "git commit -m ...")
/// is "commit" or "push" -- `action` holds the full raw shell command, not a bare verb;
/// everything else is ungrounded and non-mutating.
fn ground_tool_intent(intent: &ToolIntent) -> (Option<CompactString>, Option<CompactString>, bool) {
    match intent {
        ToolIntent::FileRead { path, .. } => (Some(path.clone()), None, false),
        ToolIntent::FileEdit { path, .. } => (Some(path.clone()), None, true),
        ToolIntent::FileCreate { path } => (Some(path.clone()), None, true),
        ToolIntent::CodeSearch { query, .. } => (None, Some(query.clone()), false),
        ToolIntent::VersionControl { action } => {
            let subcommand = action.split_whitespace().nth(1);
            (None, None, subcommand == Some("commit") || subcommand == Some("push"))
        }
        _ => (None, None, false),
    }
}

/// Convenience helper to build `TrajectoryGraph` and calculate monotonicity score.
pub fn calculate_monotonicity(transcript: &CanonicalTranscript) -> f32 {
    build_trajectory_graph(transcript).calculate_monotonicity()
}

/// Convenience helper to build `TrajectoryGraph` and detect circular tool-call loops --
/// CRIT-LUMEN-179. Mirrors `calculate_monotonicity`'s construction so both share the same
/// grounding logic and can never disagree about what counts as a cycle.
pub fn detect_circular_loops(transcript: &CanonicalTranscript) -> Vec<CircularLoopAnomaly> {
    build_trajectory_graph(transcript).detect_circular_loops()
}

fn build_trajectory_graph(transcript: &CanonicalTranscript) -> TrajectoryGraph {
    let mut tg = TrajectoryGraph::new();
    for turn in &transcript.turns {
        for call in &turn.tool_calls {
            let (target_file, target_symbol, is_mutation) = ground_tool_intent(&call.intent);
            tg.push_tool(ToolNode {
                turn_index: turn.turn_index,
                tool_name: call.tool_name.clone(),
                target_symbol,
                target_file,
                is_mutation,
                had_error: false,
            });
        }
    }
    tg
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for the VersionControl mutation-detection bug: `action` on
    /// `ToolIntent::VersionControl` holds the FULL raw shell command (confirmed by
    /// reading the real construction sites in lumen-session's claude.rs and
    /// opencode.rs adapters, e.g. `ToolIntent::VersionControl { action:
    /// CompactString::new(cmd) }` where `cmd` is `"git commit -m 'fix bug'"`), not
    /// the bare word "commit"/"push". An exact-equality check against "commit" or
    /// "push" can never match a real command string, so `is_mutation` was always
    /// false for real git commit/push tool calls.
    #[test]
    fn ground_tool_intent_detects_mutation_for_real_git_commit_command() {
        let intent = ToolIntent::VersionControl { action: CompactString::new("git commit -m 'fix bug'") };
        let (_, _, is_mutation) = ground_tool_intent(&intent);
        assert!(is_mutation, "expected 'git commit -m ...' to be detected as a mutation");
    }

    #[test]
    fn ground_tool_intent_detects_mutation_for_real_git_push_command() {
        let intent = ToolIntent::VersionControl { action: CompactString::new("git push origin main") };
        let (_, _, is_mutation) = ground_tool_intent(&intent);
        assert!(is_mutation, "expected 'git push origin main' to be detected as a mutation");
    }

    #[test]
    fn ground_tool_intent_does_not_flag_non_mutating_git_commands_as_mutations() {
        let status = ToolIntent::VersionControl { action: CompactString::new("git status") };
        let (_, _, status_is_mutation) = ground_tool_intent(&status);
        assert!(!status_is_mutation, "expected 'git status' to remain non-mutating");

        let log = ToolIntent::VersionControl { action: CompactString::new("git log") };
        let (_, _, log_is_mutation) = ground_tool_intent(&log);
        assert!(!log_is_mutation, "expected 'git log' to remain non-mutating");
    }
}
