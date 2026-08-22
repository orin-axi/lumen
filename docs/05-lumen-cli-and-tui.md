# Lumen Architecture: Standalone Observability CLI & TUI (`05-lumen-cli-and-tui.md`)

This document defines the user-facing CLI surface in `crates/lumen-cli`. No TUI exists yet
despite this file's name; see SPEC-LUMEN-008-CLI's non_goals for what's deferred (a daemon-backed
architecture, async, a TUI, TOON output) versus what's actually built below.

---

## 1. CLI Commands

- `lumen trace <session_path>`: Renders ASCII tool trajectory DAG for one session file (no SQLite store access).
- `lumen audit <session_path>`: Computes token economics, cache hit %, and USD costs for one session file (no SQLite store access). A model with no seeded pricing row reports cost as an explicit `unknown`, never a fabricated figure.
- `lumen ingest <path>`: Parses every real session found at a file, directory, or OpenCode SQLite database, and persists each to the SQLite store (`--db`, default `~/.lumen/lumen.db`).
- `lumen sessions [--provider <name>] [--limit <n>]`: Lists sessions previously ingested into the store.
- `lumen session <provider> <id>`: Shows one stored session's full detail.

`lumen insights` and a `scan` subcommand do not exist -- corrected 2026-08-21 after a real-data
audit found this list didn't match the actual built CLI at all (previously `trace`/`audit`/`scan`
with `scan` never wired to any persistence layer).

---

## 2. Output Formatting

- Terminal mode: Uses `comfy-table` with UTF-8 borders.
- `--json` mode: Emits `serde_json`-formatted output for every command.
- `anstream`/`anstyle`/`indicatif`/`rayon`/`tracing` are declared workspace dependencies not
  currently wired into any command -- do not assume progress bars, ANSI-aware piping detection,
  parallelism, or logging exist until they're actually used in `crates/lumen-cli/src/main.rs`.
