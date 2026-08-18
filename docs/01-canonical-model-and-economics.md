# Lumen Architecture: Canonical Domain Model & Economic Token Pricing (`01-canonical-model-and-economics.md`)

This document defines the deep semantic modeling and mathematical formulations for `crates/lumen-model`.

---

## 1. Domain Types & Memory Layout

```rust
pub struct CanonicalTranscript {
    pub session_id: CompactString,
    pub parent_session_id: Option<CompactString>,
    pub orchestrator: OrchestratorKind,
    pub model_family: CompactString,
    pub timing: ExecutionTiming,
    pub economics: TokenEconomics,
    pub turns: Vec<CanonicalTurn>,
    pub subagents: Vec<CanonicalTranscript>,
    pub extracted_schemas: SmallVec<[SchemaCitation; 4]>,
    pub detected_anomalies: SmallVec<[TrajectoryAnomaly; 4]>,
}
```

### Memory Allocation Budget:
- **`CompactString`**: 24-byte inlined strings on the stack for IDs, tool names, and hashes. Zero heap allocations for strings $\le 24$ bytes.
- **`SmallVec<[T; 2]>`**: Stores tool calls and results on the stack for typical single-tool or dual-tool turns.

---

## 2. Token Pricing & Prompt Caching Mathematics

### A. Turn Cost Formula:
$$\text{Cost}(t) = \frac{I_{\text{uncached}}(t) \cdot P_{\text{in}} + I_{\text{write}}(t) \cdot P_{\text{write}} + I_{\text{read}}(t) \cdot P_{\text{read}} + O(t) \cdot P_{\text{out}}}{1,000,000}$$

### B. Prompt Cache Hit Ratio ($H$):
$$H = \frac{\sum_{t=1}^N I_{\text{read}}(t)}{\sum_{t=1}^N \left( I_{\text{uncached}}(t) + I_{\text{write}}(t) + I_{\text{read}}(t) \right)} \times 100\%$$

### C. Official Rates Matrix:
- **Claude 3.5 Sonnet**: $P_{\text{in}} = \$3.00$, $P_{\text{write}} = \$3.75$ (1.25x), $P_{\text{read}} = \$0.30$ (0.10x), $P_{\text{out}} = \$15.00$.
- **Claude 3.5 Haiku**: $P_{\text{in}} = \$0.80$, $P_{\text{write}} = \$1.00$ (1.25x), $P_{\text{read}} = \$0.08$ (0.10x), $P_{\text{out}} = \$4.00$.
- **Claude Opus**: $P_{\text{in}} = \$15.00$, $P_{\text{write}} = \$18.75$ (1.25x), $P_{\text{read}} = \$1.50$ (0.10x), $P_{\text{out}} = \$75.00$.
