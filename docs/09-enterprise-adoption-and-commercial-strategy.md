# Enterprise Adoption, Privacy & Commercial Licensing Strategy (`docs/09`)

> **Status: Vision / Roadmap.** This document describes a target architecture and product direction, not the current implementation — components and crates referenced here (e.g. `lumen-daemon`, `lumen-mac`, `lumen-cloud`, `lumen-store`, `lumen-insights`) may not yet exist in the workspace or may differ materially once built. Treat claims of behavior, licensing, or performance in this document as aspirational unless independently verified against the current codebase; docs 01-06 describe the implemented system and take precedence wherever the two conflict.

This document defines the 3-tier customer roadmap, enterprise data security standards, OpenTelemetry fleet export architecture, and the Functional Source License (FSL-1.1-MIT) commercial model.

---

## 1. The 3-Tier Adoption Hierarchy

```text
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                        CUSTOMER ADOPTION ROADMAP                                       │
├───────────────────────┬─────────────────────────┬──────────────────────────────────────┤
│ CUSTOMER TIER         │ CORE VALUE FOCUS        │ PRIMARY CAPABILITIES                 │
├───────────────────────┼─────────────────────────┼──────────────────────────────────────┤
│ 1. Solo Developers    │ Speed, Privacy, Zero-   │ • Single-binary install (<5ms CLI)   │
│    (Priority 1)       │ Config, Spend Control   │ • 100% on-device SQLite WAL          │
│                       │                         │ • Personal RTK CLI coaching          │
├───────────────────────┼─────────────────────────┼──────────────────────────────────────┤
│ 2. Small Teams        │ Skill Consistency, Team │ • Shared `prism.toml` test suites    │
│    (5-20 Engineers)   │ Offloading & Economics  │ • Model offloading scorecards        │
│                       │                         │ • Git PR and branch attribution      │
├───────────────────────┼─────────────────────────┼──────────────────────────────────────┤
│ 3. Enterprises        │ Fleet Observability,    │ • OpenTelemetry OTLP v1.28+ export   │
│    (100+ Engineers)   │ Compliance, PII Defense │ • Local-first air-gapped security    │
│                       │                         │ • Machine-salted PII redaction       │
└───────────────────────┴─────────────────────────┴──────────────────────────────────────┘
```

---

## 2. Enterprise Privacy & Redaction by Default

* **Zero Prompt / Tool Output Storage**: Raw user prompts and tool outputs are never persisted in SQLite fact tables.
* **Command Argument Redaction**: Shell commands are sanitized to store only binary names and parameter shapes (e.g. `git checkout [REDACTED_BRANCH]`).
* **Machine-Salted Identity**: User usernames and machine hostnames are hashed with a local machine salt before fleet aggregation.

---

## 3. OpenTelemetry (OTel OTLP v1.28+) Fleet Export

For enterprise fleet monitoring in Datadog, Dynatrace, Honeycomb, or AWS CloudWatch, `lumen-daemon` can be configured to stream anonymized `gen_ai.*` spans:

```toml
# ~/.lumen/config.toml
[telemetry.otlp]
enabled = true
endpoint = "https://otlp.internal.company.com:4317"
protocol = "grpc"
batch_size = 50
flush_interval_seconds = 30
```

---

## 4. The Functional Source License (FSL-1.1-MIT) Architecture

```text
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                        WORKSPACE LICENSING STRUCTURE                                   │
├───────────────────────┬─────────────────────────┬──────────────────────────────────────┤
│ CRATE / ARTIFACT      │ LICENSE                 │ STATUS                               │
├───────────────────────┼─────────────────────────┼──────────────────────────────────────┤
│ `lumen-model`         │ `MIT OR Apache-2.0`     │ Exists                               │
│ `lumen-session`       │ `MIT OR Apache-2.0`     │ Exists                               │
│ `lumen-analysis`      │ `FSL-1.1-MIT`           │ Exists                               │
│ `lumen-pattern`       │ `FSL-1.1-MIT`           │ Exists                               │
│ `lumen-cli`           │ `FSL-1.1-MIT`           │ Exists                               │
│ `lumen-store`         │ Planned, not licensed   │ Does not exist yet                   │
│ `lumen-daemon`        │ Planned, not licensed   │ Does not exist yet                   │
│ `lumen-insights`      │ Planned, not licensed   │ Does not exist yet                   │
│ `apps/lumen-mac`      │ Planned, not licensed   │ Does not exist yet                   │
│ `lumen-cloud`         │ Planned, not licensed   │ Does not exist yet                   │
└───────────────────────┴─────────────────────────┴──────────────────────────────────────┘
```

> Note: the FSL-1.1-MIT crates above share a single Excluded Purpose clause and a single
> Change Date, both defined once in the repository's `LICENSE` file — not a separate clause
> per crate. The Excluded Purpose is "providing the functionality of the Software to third
> parties as a competing commercial product or service"; the Change Date is 2 years from
> each version's release, after which that version relicenses to MIT.
