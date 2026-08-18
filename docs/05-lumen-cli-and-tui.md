# Lumen Architecture: Standalone Observability CLI & TUI (`05-lumen-cli-and-tui.md`)

This document defines the user-facing CLI surface in `crates/lumen-cli`.

---

## 1. CLI Commands

- `lumen trace <session_path>`: Renders ASCII tool trajectory DAG and latency waterfall.
- `lumen audit <session_path>`: Computes exact token economics, cache hit %, and USD costs.
- `lumen scan [dir]`: Parallel multi-session directory scanner using `rayon` and `memmap2`.
- `lumen insights`: Displays detected anomalies, circular loops, and actionable cues.

---

## 2. Output Formatting

- Terminal mode: Uses `comfy-table` with UTF-8 borders, `anstream` ANSI styling, and `indicatif` progress spinners.
- Non-interactive / piped mode: Automatically suppresses colors and spinner animations.
- `--json` mode: Emits deterministic, un-truncated, snapshot-testable JSON.
