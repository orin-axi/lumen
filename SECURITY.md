# Security Policy

## Reporting a Vulnerability

Email **security@orin-dx.com** — don't open a public issue for anything that could be exploited before a fix ships.

Include:
- Which crate is affected (`lumen-model`, `lumen-session`, `lumen-analysis`, `lumen-pattern`, `lumen-store`, `lumen-cli`)
- The concrete failure scenario — what an attacker could do, and how
- A sample transcript file or repro steps, if you have them

Expect an acknowledgment within 5 business days. We'll keep you posted as a fix moves through triage, and credit you in the release notes unless you'd rather stay anonymous.

## Scope

Lumen parses AI coding agent transcript logs (Claude Code, Antigravity, Codex, OpenCode) and stores derived data locally. The relevant threat model:

- **Untrusted transcript parsing** — `lumen-session`'s SIMD streaming parser (`simd-json` + `memmap2`) reads transcript files that may come from an untrusted source (a shared repo, a downloaded session, a malicious agent run). A crafted transcript that crashes the parser, corrupts memory, or triggers unbounded resource use is a real finding here.
- **Local data at rest** — `lumen-store` persists parsed session data (via `rusqlite`) on disk. If transcript content includes sensitive material (file paths, code snippets, credentials an agent handled), how that's stored and who can read it matters.
- **Supply chain** — `Cargo.toml`'s dependency tree, especially `unsafe`-adjacent crates like `memmap2` and `rusqlite`'s bundled SQLite.
- **CLI argument and env handling** — `lumen-cli`'s `clap`-based surface (`--json` output, file path arguments).

Narrower than it could be: the workspace enforces `unsafe_code = "forbid"` at the lint level (`Cargo.toml`'s `[workspace.lints.rust]`), so memory-safety bugs from `unsafe` blocks are not in scope for the crates that ship with that lint — a report showing the forbid was bypassed or a memory-safety bug exists despite it is a legitimate, high-priority finding.

Out of scope: issues in Claude Code, Antigravity, Codex, or OpenCode themselves — report those to the respective platform, not here.

## Supported Versions

Lumen is pre-1.0 (`0.1.0`). Security fixes land on `main` only — there is no version matrix to backport across yet.
