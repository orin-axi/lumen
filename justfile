# Default recipe: run full CI pipeline via moon & just
default: ci

# ── Build ──────────────────────────────────────────────────────────────────
build:
    moon run :build

build-release:
    cargo build --release --workspace

# ── Test ───────────────────────────────────────────────────────────────────
# nextest runs the whole workspace as one process (proper parallelism, no
# per-crate build-lock contention); falls back to moon's per-project fan-out
# if nextest isn't installed. Same pattern callisto validated in its own justfile.
test:
    cargo nextest run --workspace || moon run :test
    cargo test --doc

# ── Lint & Format ───────────────────────────────────────────────────────────
check: fmt-check lint

# Single workspace invocation. Moon's per-project `cargo clippy -p $project` tasks
# (.moon/tasks/rust.yml) all lock the same shared target/ dir, so running them
# one-per-project serializes on Cargo's own build lock instead of parallelizing --
# a single --workspace invocation lets Cargo's internal job scheduler parallelize
# across crates instead. Same fix callisto already applied to its own justfile.
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Lint only projects affected by changes since the base branch, still via one
# cargo invocation (not moon's per-project fan-out) to avoid the same build-lock
# serialization while skipping unaffected crates entirely.
lint-affected:
    #!/usr/bin/env bash
    set -euo pipefail
    projects=$(moon query projects --affected 2>/dev/null | jq -r '.projects[].id')
    if [ -z "$projects" ]; then
        echo "No projects affected — skipping lint."
        exit 0
    fi
    args=()
    for p in $projects; do args+=(-p "$p"); done
    cargo clippy "${args[@]}" --all-targets -- -D warnings

# ── Licensing / Dependency Policy ────────────────────────────────────────────
deny:
    moon run :deny

# Known-vulnerable dependency check (RUSTSEC advisories). Separate from `deny`
# (license/layer-boundary bans) -- both are cargo-deny checks but against
# different databases, and neither subsumes the other.
audit:
    moon run :audit

# Unused-dependency check. Directly closes the gap that let 6 unused deps across
# 3 crates (rayon/memmap2, smallvec/tracing, simd-json/memmap2) go unnoticed
# until a manual audit found them.
machete:
    cargo machete

# ── API Surface ───────────────────────────────────────────────────────────
# Verifies public API changes against the last-published crates.io version.
# NOT wired into `ci:` yet: cargo-semver-checks needs a published baseline to
# diff against, and no Lumen crate has been published yet -- running this
# before a first publish exists just fails with no baseline. Wire into `ci:`
# once the first crate is on crates.io; until then, run manually pre-release.
check-api:
    cargo semver-checks check-release

# Documentation build check (warnings treated as errors) -- catches broken
# intra-doc links for free.
doc-check:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace

# ── Pricing Data (CRIT-LUMEN-170) ────────────────────────────────────────────
# Refreshes the vendored LiteLLM pricing snapshot lumen-model's PricingTable::seed() loads at
# compile time. Never fetched live -- Lumen prices historical sessions and needs point-in-time-
# stable rates, so this is a periodic, explicit re-vendor step, not a per-request fetch. Review
# the diff (especially any rate change for a currently-seeded model) before committing.
update-pricing:
    curl -sSL https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json \
        -o crates/lumen-model/data/litellm_model_prices.json

fmt:
    moon run :format

format: fmt

fmt-check:
    moon run :format-check

# ── Continuous Development & Clean ──────────────────────────────────────────
clean:
    moon clean 2>/dev/null || true
    cargo clean

# ── Git Hooks & Pre-push ───────────────────────────────────────────────────
pre-commit: fmt-check

pre-push: fmt-check lint-affected

hooks:
    @echo '#!/bin/sh\njust pre-commit' > .git/hooks/pre-commit
    @chmod +x .git/hooks/pre-commit
    @echo '#!/bin/sh\njust pre-push' > .git/hooks/pre-push
    @chmod +x .git/hooks/pre-push
    @echo "Git pre-commit and pre-push hooks installed successfully."

# ── CI Verification Pipeline ───────────────────────────────────────────────
ci: fmt-check lint deny audit machete doc-check test
