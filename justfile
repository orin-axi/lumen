# Default recipe: run full CI pipeline via moon & just
default: ci

# ── Build ──────────────────────────────────────────────────────────────────
build:
    moon run :build

build-release:
    cargo build --release --workspace

# ── Test ───────────────────────────────────────────────────────────────────
test:
    moon run :test

# ── Lint & Format ───────────────────────────────────────────────────────────
check: fmt-check lint

lint:
    moon run :lint

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

pre-push: fmt-check lint

hooks:
    @echo '#!/bin/sh\njust pre-commit' > .git/hooks/pre-commit
    @chmod +x .git/hooks/pre-commit
    @echo '#!/bin/sh\njust pre-push' > .git/hooks/pre-push
    @chmod +x .git/hooks/pre-push
    @echo "Git pre-commit and pre-push hooks installed successfully."

# ── CI Verification Pipeline ───────────────────────────────────────────────
ci: fmt-check lint test
