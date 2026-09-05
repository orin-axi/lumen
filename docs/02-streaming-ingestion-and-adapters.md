# Lumen Architecture: Streaming Ingestion & Orchestrator Adapters (`02-streaming-ingestion-and-adapters.md`)

This document defines the zero-copy streaming ingestion mechanics for `crates/lumen-session`.

---

## 1. Multi-Orchestrator Adapters

```rust
pub trait SessionAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn matches_fingerprint(&self, sample: &str) -> bool;
    fn capabilities(&self) -> AdapterCapabilities;
    fn parse_stream<'a>(&self, reader: Box<dyn BufRead + 'a>) -> Result<CanonicalTranscript, IngestionError>;
}
```

### Supported Ingestion Targets:
1. **`ClaudeCodeAdapter`**: Parses `~/.claude/projects/<slug>/<session>.jsonl`, extracts `tool_use` blocks, multi-tier `usage`, and traverses `subagents/*.jsonl`.
2. **`AgyAdapter`**: Parses `<appDataDir>/brain/<id>/.system_generated/logs/transcript.jsonl`, extracts `thinking` blocks, and step-indexed `tool_calls`.
3. **`CodexAdapter`**: Parses OpenAI assistant thread runs and CLI streams.
4. **`OpenCodeAdapter`**: Parses OpenHands event streams.

### Non-Transcript Ingestion

| Source | Shape | Defined in |
| :--- | :--- | :--- |
| **Wisp / Monokl telemetry** | Sanitized `tracing` events annotating a session another adapter produced, correlated by session id — not a transcript, so not a `SessionAdapter` | [`11-wisp-and-monokl-telemetry.md`](./11-wisp-and-monokl-telemetry.md) |

---

## 2. PreCompact Snapshot Merging Algorithm

When Claude Code sessions exceed context limits and compact older turns:
- Snapshots are cumulative totals to date.
- **Merge Rule**: Take $\max(\text{Snapshot}, \text{Final Parse})$ over token counts to prevent double-counting.
