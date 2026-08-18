# Lumen Architecture: The 22 Single-Pass Streaming Accumulators (`03-22-accumulators-and-analytics.md`)

This document defines the 22 single-pass streaming accumulators in `crates/lumen-analysis`.

---

## 1. Accumulator Lifecycle

- **`RawMessageAccumulator`**: Observes low-level JSON envelopes (`update_raw(&Value)`).
- **`EntryAccumulator`**: Observes normalized semantic turns (`update(&CanonicalTurn)`).
- **`finalize(self)`**: Consumes the accumulator and produces an owned, immutable summary struct.

---

## 2. Complete Accumulator Inventory

1. `token_usage`: Exact 5m/1h ephemeral cache tiers & per-model totals.
2. `otel_correlation`: Links `sessionId` to `requestIds` arrays.
3. `span_mapping`: Maps `tool_use_id` to OTel spans.
4. `stats`: Top tools, MCP counts, user/agent totals.
5. `timeline`: Groups assistant runs, flags >5m idle intervals.
6. `artifacts`: Created/edited files, commits, PRs.
7. `circuit_breaker`: Measures Drafter ↔ Auditor consensus rounds ($\le 2$).
8. `mcp_affinity`: Ratio of structured MCP tools vs shell fallbacks.
9. `flow`: Autonomy streaks, permission blocks.
10. `turn_duration`: p50, p95, avg turn latency distribution.
11. `tool_inventory`: Installed vs used MCP tools.
12. `context_growth`: Compaction events and token growth rate.
13. `permission_mode`: Tracks auto vs default modes.
14. `hook_activity`: Hook latency and block rate.
15. `api_health`: Buckets 429 rate limits & 5xx retries.
16. `pr_link`: Maps session to GitHub PR URLs.
17. `fuzzy_tools`: Levenshtein clustering for MCP typos.
18. `attribution`: Plugin ➔ Skill ➔ Agent execution windows.
19. `self_correction`: Detects tool_retry and approach_pivot.
20. `autonomy`: Classifies autonomous vs corrected outcomes.
21. `schema_extractor`: Validates embedded spec@1, plan@1 JSON.
22. `trajectory_dag`: Graph node sequence feeder.
