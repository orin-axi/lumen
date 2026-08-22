# Lumen AI Agent Guidelines (`AGENTS.md`)

This guide provides instructions, architectural rules, engineering invariants, and task runner workflows for AI coding agents (Antigravity, Claude, Cursor, Copilot, Codex) working on the Lumen codebase.

---

## 1. Repository Architecture & Crate Layers

Lumen is a multi-orchestrator telemetry, session intelligence, and token economics engine written in Rust. It is structured into strict architectural layers:

```text
┌────────────────────────────────────────────────────────────────────────┐
│                          LUMEN CRATE LAYERS                            │
├───────────────────────────────────┬────────────────────────────────────┤
│ LAYER & CRATE                     │ LICENSE & PURPOSE                  │
├───────────────────────────────────┼────────────────────────────────────┤
│ Layer 1: lumen-model              │ MIT / Apache-2.0                   │
│                                   │ Canonical IR & Pricing Matrix      │
├───────────────────────────────────┼────────────────────────────────────┤
│ Layer 1.5: lumen-session          │ MIT / Apache-2.0                   │
│                                   │ Streaming Ingestion & Adapters     │
├───────────────────────────────────┼────────────────────────────────────┤
│ Layer 2: lumen-analysis           │ FSL-1.1-MIT                        │
│          lumen-pattern            │ 22 Accumulators, Trajectory DAG    │
├───────────────────────────────────┼────────────────────────────────────┤
│ Layer 2.5: lumen-store            │ FSL-1.1-MIT                        │
│            lumen-fixtures         │ SQLite Persistence & Test Doubles  │
├───────────────────────────────────┼────────────────────────────────────┤
│ Layer 4: lumen-cli                │ FSL-1.1-MIT                        │
│                                   │ Standalone CLI & Telemetry Tools   │
└───────────────────────────────────┴────────────────────────────────────┘
```

> **CRITICAL RULE (Layer Licensing Boundaries)**: Layer 1 (`lumen-model`) and Layer 1.5 (`lumen-session`) MUST NOT depend, as a runtime `[dependencies]` entry, on Layer 2/2.5/4 crates (`lumen-analysis`, `lumen-pattern`, `lumen-store`, `lumen-fixtures`, `lumen-cli`). Layer 1/1.5 crates must remain permissive (`MIT OR Apache-2.0`) at the package-license level.
>
> **Known gap, not yet resolved (audit finding, 2026-08-22):** `lumen-session`'s own `[dev-dependencies]` include `lumen-fixtures` (for adapter test fixtures), which itself depends on `lumen-store` (both FSL-1.1-MIT) — so `cargo test -p lumen-session` currently pulls FSL-licensed code into a crate documented as "standalone." `lumen-model` has no such dependency. This rule was written without an explicit dev-dependency carve-out; either add one here (dev/test-only FSL dependencies are permitted, since they never ship in the published crate) or move the fixtures that need `lumen-store` into a separate test-only crate so `lumen-session`'s full build graph — not just its published one — stays MIT/Apache-2.0. Not resolved as of 2026-08-22; pick one before treating this rule as satisfied again.

---

## 2. Primary Task Runners & Verification Pipelines

Always use `just` or `moon` task runners for building, testing, linting, and formatting.

| Action | Primary Task Runner Command | Moon Engine Command |
| :--- | :--- | :--- |
| **Run Full Verification CI** | `just ci` | `moon run :format-check && moon run :lint && moon run :test` |
| **Run Test Suite** | `just test` | `moon run :test` |
| **Check Clippy Lints** | `just lint` | `moon run :lint` |
| **Check Formatting** | `just fmt-check` | `moon run :format-check` |
| **Format Code** | `just fmt` | `moon run :format` |

---

## 3. Strict Engineering Invariants

1. **Safe Rust Only (`unsafe_code = "forbid"`)**: `unsafe` code blocks are strictly forbidden across all workspace crates.
2. **Deterministic Token Accounting**: Token calculations must account for uncached input, 5m ephemeral write, 1h ephemeral write, and output tokens without division by zero. Cache-read and cache-write discount ratios are per-model, versioned rates looked up from `PricingTable` (e.g. Anthropic's confirmed pattern: 5m write = 1.25x input, 1h write = 2x input, cache read = 0.10x input — but this varies by provider, there is no single universal ratio). Real Claude Code session data publishes the 5m/1h split directly (`usage.cache_creation.{ephemeral_5m_input_tokens,ephemeral_1h_input_tokens}`); `TurnTokenUsage.cache_creation_1h_tokens` and `TokenRateKind::CacheWrite1h` carry it through pricing.
3. **Linear Streaming Pass $O(N)$**: Telemetry accumulators must operate in a single linear pass over the canonical turns. Avoiding nested heap allocations per turn is a design goal enforced by code review, not a mechanically-verified invariant — a true allocation-counting harness would require an `unsafe impl GlobalAlloc`, which invariant 1 forbids.
4. **Tarjan SCC Cycle Detection**: Cycle depth $\ge 3$ on identical target symbols with zero mutation must be flagged as circular loops.
5. **Rich Diagnostic Cards (`miette`)**: User-facing CLI errors must derive `miette::Diagnostic`.
6. **No Emoji Directive in Code & Comments**: Code comments and docs must remain technical, clean, and scannable.
