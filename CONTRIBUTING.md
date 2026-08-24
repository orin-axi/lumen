# Contributing to Lumen

Contributions to Lumen are welcome. This guide outlines the development environment setup, architectural invariants, and testing standards. By participating, you agree to abide by the [Code of Conduct](./CODE_OF_CONDUCT.md); found a security issue? See [SECURITY.md](./SECURITY.md) instead of opening a public issue.

---

## 1. Development Environment

### Prerequisites
- **Rust Toolchain**: Rust 1.80+ (`stable`)
- **Cargo Tools**: `cargo-clippy`, `cargo-fmt`
- **Preferred CLI Tools**: `eza`, `bat`, `ripgrep`, `fd`

### Build and Test

```bash
# Build the workspace
cargo build --workspace

# Run all unit and integration tests
cargo test --workspace
```

### Before Opening a PR

CI runs `just ci`, which is more than `build`/`test`/`fmt`/`clippy` — it also checks for
unused dependencies (`cargo machete`), broken doc links (`cargo doc -D warnings`), and
license/dependency policy (`cargo deny`). Run the same pipeline locally before pushing:

```bash
just ci
```

Individual checks (`just check`, `just lint`, `just fmt-check`, `just machete`,
`just doc-check`, `just deny`, `just audit`) are defined in the [`justfile`](./justfile) if you
want to run one in isolation.

---

## 2. Engineering Invariants

Contributors must maintain the following architectural rules:

1. **Safe Rust Only (`unsafe_code = "forbid"`)**: Memory and concurrency safety are guaranteed by safe Rust abstractions.
2. **Single-Pass $O(N)$ Processing**: Accumulators in `lumen-analysis` must operate in a single linear pass over the message stream with zero heap allocations in inner loops.
3. **Cumulative Snapshot Merge Invariant**: Context compaction snapshots must always be merged using component-wise $\max()$, never `sum()`.
4. **Permissive Layer 1 Isolation**: Layer 1 crates (`lumen-model`, `lumen-session`) must remain permissive (`MIT OR Apache-2.0`) and must not depend on Layer 2 or Layer 4 crates.
5. **No Emojis or Filler Phrases**: Documentation, commit messages, and diagnostic output must remain technical, concise, and scannable.

---

## 3. Pull Request Workflow

1. **Create a Branch**: Use a descriptive branch name (e.g. `feat/new-accumulator`, `fix/agy-parser`).
2. **Write Tests First**: Add unit or integration tests verifying the change.
3. **Verify CI**: Run `just ci` locally — it's the same pipeline CI runs.
4. **Conventional Commits**: Format commit messages using conventional prefixes (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`).
