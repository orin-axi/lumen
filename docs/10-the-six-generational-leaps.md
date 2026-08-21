# The Six Generational Leaps in Session Intelligence (`docs/10`)

> **Status: Vision / Roadmap.** This document describes a target architecture and product direction, not the current implementation — components and crates referenced here (e.g. `lumen-daemon`, `lumen-mac`, `lumen-cloud`, `lumen-store`, `lumen-insights`) may not yet exist in the workspace or may differ materially once built. Treat claims of behavior, licensing, or performance in this document as aspirational unless independently verified against the current codebase; docs 01-06 describe the implemented system and take precedence wherever the two conflict.

This document defines the 6 breakthrough capabilities that elevate Lumen from a passive telemetry counter into an active developer flight controller and self-evolving knowledge engine.

---

## 1. Real-Time "In-Flight" Circuit Breaking & Loop Prevention

* **Problem**: Existing tools only tell you an agent wasted $5 after the session ends.
* **Lumen Solution**: `lumen-daemon` inspects turn transitions in `< 5ms`. When Tarjan SCC detects a 3rd identical read cycle, Lumen injects a recovery directive into the next turn:
  ```text
  [LUMEN FLIGHT CONTROLLER]: You have executed `view_file` on "scanner.md" 3 consecutive
  times without state mutation. Pivot your search strategy or execute a reproduction test.
  ```

---

## 2. Golden Trace Distillation (`lumen distill`)

* **Problem**: Masterclass developer sessions are lost when the terminal closes.
* **Lumen Solution**: `lumen distill <session-id> --name my-skill` strips workspace noise, compiles the sequence into an Orin DX Skill (`SKILL.md` + 4-part prompt), and verifies it in a Prism sandbox.

---

## 3. Visual KV-Cache Heatmap & Prefill Pinpointing

* **Problem**: Developers cannot see where prompt cache invalidations occur.
* **Lumen Solution**: `lumen cache inspect <session-id>` highlights the exact byte/line in the prompt that triggered cold cache creation ($1.25\times$) vs warm cache reads ($0.10\times$), with recommendations to move dynamic tokens to the end.

---

## 4. Zero-Overhead Local MCP Memory (`lumen-mcp`)

* **Problem**: Agents start fresh every session with zero memory of past bug fixes.
* **Lumen Solution**: Native Model Context Protocol (MCP) server executing direct SQLite queries in `< 1ms` to recall past solutions and team architectural patterns.

---

## 5. Multi-Agent Subagent Tree Forensics

* **Problem**: Flat logs break when analyzing recursive subagent hierarchies.
* **Lumen Solution**: Models the complete recursive subagent swarm topology (Parent Coordinator $\to$ Recon $\to$ Scanner $\to$ Adversary $\to$ Exit Gate), attributing token burn and latencies to each branch independently.

---

## 6. Time-Travel Sandbox Replay (`lumen replay` $\longleftrightarrow$ Prism)

* **Problem**: Debugging agent hallucinations requires manual state recreation.
* **Lumen Solution**: `lumen replay <session-id>` uses Prism to spin up an ephemeral Reflink/APFS CoW Git sandbox in `/tmp/`, check out the initial commit, and replay every tool call and diff step-by-step.
