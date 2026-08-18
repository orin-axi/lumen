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
}

impl Default for TrajectoryGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl TrajectoryGraph {
    pub fn new() -> Self {
        Self { graph: DiGraph::new(), last_node: None, total_mutations: 0, total_reads: 0 }
    }

    pub fn push_tool(&mut self, node: ToolNode) -> NodeIndex {
        if node.is_mutation {
            self.total_mutations += 1;
        } else {
            self.total_reads += 1;
        }

        let current = self.graph.add_node(node);
        if let Some(prev) = self.last_node {
            self.graph.add_edge(prev, current, ());
        }
        self.last_node = Some(current);
        current
    }

    pub fn detect_circular_loops(&self) -> Vec<CircularLoopAnomaly> {
        let mut anomalies = Vec::new();
        let sccs = tarjan_scc(&self.graph);

        for component in sccs {
            if component.len() >= 3 {
                let first_symbol = self.graph[component[0]].target_symbol.clone();
                let is_redundant = component
                    .iter()
                    .all(|idx| self.graph[*idx].target_symbol == first_symbol && !self.graph[*idx].is_mutation);

                if is_redundant {
                    if let Some(sym) = first_symbol {
                        anomalies.push(CircularLoopAnomaly { symbol: sym, cycle_depth: component.len() });
                    }
                }
            }
        }
        anomalies
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

/// Convenience helper to build `TrajectoryGraph` and calculate monotonicity score.
pub fn calculate_monotonicity(transcript: &CanonicalTranscript) -> f32 {
    let mut tg = TrajectoryGraph::new();
    for turn in &transcript.turns {
        for call in &turn.tool_calls {
            let is_mutation = matches!(call.intent, ToolIntent::FileEdit { .. } | ToolIntent::FileCreate { .. });
            tg.push_tool(ToolNode {
                turn_index: turn.turn_index,
                tool_name: call.tool_name.clone(),
                target_symbol: None,
                target_file: None,
                is_mutation,
                had_error: false,
            });
        }
    }
    tg.calculate_monotonicity()
}
