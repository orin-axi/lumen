# Lumen Architecture: Trajectory DAG & Cycle Detection (`04-trajectory-dag-and-loop-detection.md`)

This document defines the graph modeling and anomaly detection engine in `crates/lumen-pattern`.

---

## 1. Graph Formulation

- Directed graph $G = (V, E)$ where $V = \text{ToolNode}$ and $E = \text{Sequential Transitions}$.
- Uses `petgraph::graph::DiGraph<ToolNode, ()>`.

---

## 2. Tarjan SCC Cycle Detection

- Tarjan's Strongly Connected Components algorithm runs in $O(V + E)$ time.
- Any strongly connected component with depth $\ge 3$ where all nodes share identical target symbols and zero state mutations is emitted as a `CircularLoopAnomaly`.

---

## 3. The 6 Trajectory Metrics

1. **Argument Grounding ($\mathcal{G}$)**: Ratio of tool arguments grounded in prior turn observations.
2. **Error Recovery ($\mathcal{R}$)**: Ratio of adaptive error pivots vs blind retries.
3. **Plan Monotonicity ($M$)**: Proportion of productive transitions penalizing detected graph cycles.
4. **Trajectory Efficiency ($E$)**: Ratio of state mutations and productive reads to total tool calls.
5. **Economic Efficiency ($\mathcal{E}$)**: Prompt cache hit ratio scaled by turn limits.
6. **Task Completion ($S$)**: Binary verification of Red-to-Green state transition.
