# Lumen Architecture (`ARCHITECTURE.md`)

This document defines the crate architecture, dataflow pipeline, memory layout, and mathematical invariants of Lumen.

---

## 1. Crate Hierarchy & Licensing Boundaries

```mermaid
flowchart TD
    classDef layer4 fill:#ecfdf5,stroke:#10b981,stroke-width:2px,color:#064e3b,rx:8px,ry:8px;
    classDef layer2 fill:#f5f3ff,stroke:#8b5cf6,stroke-width:2px,color:#4c1d95,rx:8px,ry:8px;
    classDef layer15 fill:#eef2ff,stroke:#6366f1,stroke-width:2px,color:#1e1b4b,rx:8px,ry:8px;
    classDef layer1 fill:#f8fafc,stroke:#64748b,stroke-width:2px,color:#0f172a,rx:8px,ry:8px;

    subgraph L4 [" LAYER 4: CLI BINARY "]
        C["<b>lumen-cli</b><br/><code>FSL-1.1-MIT</code> • Standalone CLI & TUI"]:::layer4
    end

    subgraph L2 [" LAYER 2: STATEFUL ANALYTICS & GRAPH "]
        A["<b>lumen-analysis</b><br/><code>FSL-1.1-MIT</code> • 22 Single-Pass Accumulators"]:::layer2
        P["<b>lumen-pattern</b><br/><code>FSL-1.1-MIT</code> • Petgraph Trajectory DAG"]:::layer2
    end

    subgraph L15 [" LAYER 1.5: MULTI-ORCHESTRATOR INGESTION "]
        S["<b>lumen-session</b><br/><code>MIT / Apache-2.0</code> • Streaming SIMD Parsers"]:::layer15
    end

    subgraph L1 [" LAYER 1: CANONICAL IR & PRICING PRIMITIVES "]
        M["<b>lumen-model</b><br/><code>MIT / Apache-2.0</code> • Canonical IR & Pricing Matrix"]:::layer1
    end

    C -->|Invokes analysis| A
    C -->|Invokes graph DAG| P
    A -->|Consumes transcripts| S
    P -->|Extracts tool transitions| S
    S -->|Constructs domain models| M

    style L4 fill:#fafafa,stroke:#cbd5e1,stroke-width:1.5px,stroke-dasharray: 4 4,rx:10px,ry:10px
    style L2 fill:#fafafa,stroke:#cbd5e1,stroke-width:1.5px,stroke-dasharray: 4 4,rx:10px,ry:10px
    style L15 fill:#fafafa,stroke:#cbd5e1,stroke-width:1.5px,stroke-dasharray: 4 4,rx:10px,ry:10px
    style L1 fill:#fafafa,stroke:#cbd5e1,stroke-width:1.5px,stroke-dasharray: 4 4,rx:10px,ry:10px
```

---

## 2. Ingestion Pipeline & Auto-Fingerprinting

```mermaid
flowchart TD
    classDef input fill:#eef2ff,stroke:#6366f1,stroke-width:2px,color:#1e1b4b,rx:8px,ry:8px;
    classDef router fill:#fffbeb,stroke:#f59e0b,stroke-width:2px,color:#78350f,rx:8px,ry:8px;
    classDef parser fill:#f8fafc,stroke:#64748b,stroke-width:2px,color:#0f172a,rx:8px,ry:8px;
    classDef model fill:#f8fafc,stroke:#64748b,stroke-width:2px,color:#0f172a,rx:8px,ry:8px;
    classDef engine fill:#f5f3ff,stroke:#8b5cf6,stroke-width:2px,color:#4c1d95,rx:8px,ry:8px;

    Raw[("<b>Raw Byte Stream</b><br/>First 2048 Bytes")]:::parser
    Detect(["<b>detect_orchestrator()</b><br/>Heuristic Sniffer"]):::router

    Raw -->|< 1µs, measured| Detect

    Detect -->|Contains 'sessionId'| Ad1["<b>ClaudeCodeAdapter</b><br/>Anthropic Format"]:::input
    Detect -->|Contains 'step_index'| Ad2["<b>AgyAdapter</b><br/>Antigravity Format"]:::input
    Detect -->|Contains 'prompt_tokens'| Ad3["<b>CodexAdapter</b><br/>OpenAI Format"]:::input
    Detect -->|Contains 'action: run'| Ad4["<b>OpenCodeAdapter</b><br/>OpenHands Format"]:::input

    Stream[("<b>SIMD Streaming Parser</b><br/><code>simd-json + memmap2</code>")]:::parser

    Ad1 --> Stream
    Ad2 --> Stream
    Ad3 --> Stream
    Ad4 --> Stream

    IR[("<b>CanonicalTranscript</b><br/>Normalized IR")]:::model
    Stream -->|Zero-copy parse| IR

    An["<b>lumen-analysis</b><br/>22 Single-Pass Accumulators"]:::engine
    Pt["<b>lumen-pattern</b><br/>Trajectory DAG & Tarjan SCC"]:::engine

    IR -->|Linear pass| An
    IR -->|Tool nodes| Pt
```

---

## 3. Mathematical Formulations & Prompt Cache Economics

### A. Turn Cost Formulation:
$$\text{Cost}(t) = \frac{I_{\text{uncached}}(t) \cdot P_{\text{in}} + I_{\text{write}}(t) \cdot P_{\text{write}} + I_{\text{read}}(t) \cdot P_{\text{read}} + O(t) \cdot P_{\text{out}}}{1,000,000}$$

### B. Prompt Cache Hit Ratio ($H$):
$$H = \frac{\sum_{t=1}^N I_{\text{read}}(t)}{\sum_{t=1}^N \left( I_{\text{uncached}}(t) + I_{\text{write}}(t) + I_{\text{read}}(t) \right)} \times 100\%$$

### C. Financial Efficiency Multiplier ($\eta$):
$$\eta = \frac{\text{Baseline Cost (No Cache)}}{\text{Actual Cost With Prompt Caching}}$$

---

## 4. Tarjan SCC Cycle Detection

Tool transitions are represented as directed graph $G = (V, E)$. Tarjan's Strongly Connected Components algorithm identifies non-trivial components with cycle depth $\ge 3$ where all nodes share identical target symbols and zero state mutations, flagging anomalous circular exploration loops.
