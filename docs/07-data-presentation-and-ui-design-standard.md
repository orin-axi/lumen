# Data Presentation & UI/CLI Design Standard (`docs/07`)

This document defines the quantitative visualization standards, color palettes, typography, and human-computer interaction (HCI) rules governing `lumen-cli` and `Lumen for Mac`.

---

## 1. Foundational Design Principles

Lumen adheres to the rigorous visual display principles established by **Edward Tufte**, **Stephen Few**, **Ben Shneiderman**, and **Colin Ware**:

```text
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                        DATA PRESENTATION DESIGN SYSTEM                                 │
├────────────────────────────────────────────────────────────────────────────────────────┤
│  1. SHNEIDERMAN'S MANTRA: Overview First ➔ Zoom & Filter ➔ Details-on-Demand           │
│  2. TUFTE'S DATA-INK MAXIMIZATION: Eliminate chartjunk; use inline sparklines          │
│  3. FEW'S DENSE BULLET GRAPHS: Replace round gauges with compact threshold bars        │
│  4. PREATTENTIVE COLOR ENCODING (Ware): Semantic color only for anomalies & state      │
│  5. ZERO-MENTAL-MATH NUMBER FORMATTING: Always pair raw counts with financial context  │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Preattentive Semantic Palette

Human visual perception decodes contrast and color in **`< 200ms`**. To minimize cognitive fatigue:
* **Muted Neutral Slate Chrome (90% of screen)**: Charcoal, slate, and off-white for borders, headers, labels, and standard turn rows.
* **Saturated Semantic Accents (10% of screen — ONLY for state & anomalies)**:
  * 🟢 **Emerald Green (`#10b981`)**: Test Pass, Net Financial Savings ($\Delta -40\%$), Warm Cache Hits ($\ge 90\%$).
  * 🔴 **Crimson Red (`#ef4444`)**: Tarjan SCC Cyclic Loops, Circuit Breaker Trips ($>2$ rounds), Test Failures.
  * 🟡 **Amber Yellow (`#f59e0b`)**: Cache Invalidation Drops ($<75\%$), Legacy CLI Warnings (RTK coaching).
  * 🔵 **Cobalt Blue (`#3b82f6`)**: Active in-flight running sessions.

---

## 3. Inline Unicode Sparklines (` ▂▃▄▅▆▇█`)

Instead of rendering vertical chart junk, embed high-density Unicode sparklines directly in text and tables:

```text
Repository      Sessions   Latency Trend      Cache Hit Trend    Total Spend
callisto        24 sess    ▂▃▅▃▂  (p95 1.8s)  ▇█████  (94.2%)    $4.18 (+$12.40 saved)
agent-plugins   16 sess    ▃▅█▅▂  (p95 3.2s)  ▅▆▇███  (88.1%)    $2.64 (+$6.80 saved)
prism            8 sess    ▂▂▃▂▂  (p95 1.1s)  ██████  (98.0%)    $1.60 (+$5.10 saved)
```

---

## 4. Stephen Few’s Dense Bullet Graphs

Replace speedometer needle gauges and pie charts with dense horizontal bullet bars:

```text
PROMPT CACHE HEALTH:
[█████████████████████████████████░░░░░░] 91.4%  (Target: | 85.0% • Status: EXCELLENT)
 0%        Poor        50%    Good   85%|   100%
```

---

## 5. Zero-Mental-Math Formatting Standard

Never force an engineer to compute unit conversions:

| Bad / Raw Presentation | Lumen Standard Presentation |
| :--- | :--- |
| `148201 tokens, 1850 out` | **`148k tokens ($0.04) • 90.5% Cache Hit • Saved $0.40`** |
| `active_ms: 18400` | **`18.4s active (32.1s wall)`** |
| `cost_usd: 0.204128` | **`$0.204 (Saved $0.400 / 66.2%)`** |

---

## 6. Token-Optimized Object Notation (TOON) for Agents

When CLI output is consumed by AI subagents or MCP tools, use `--toon` via `michi-toon` to cut token overhead by 60%:

```text
$ lumen sessions --limit 3 --toon
sessions[3]{id,repo,cost,turns,loops}:
  sess_084,agent-plugins,0.204,5,1
  sess_083,callisto,0.158,4,0
  sess_082,prism,0.082,2,0
```
