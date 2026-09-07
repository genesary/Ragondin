# Task runner. `just check` is the single command to run before declaring any
# work complete (AGENTS.md: Definition of done).

# List available recipes.
default:
    @just --list

# Compile everything, including tests, examples and binaries.
build:
    cargo build --all-targets

# Run the whole test suite.
test:
    cargo test --workspace

# Lint with warnings promoted to errors.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Verify formatting (does not modify files).
fmt:
    cargo fmt --check

# Compile the feature-gated code paths. The default build is lean, so it never
# sees them: without this, a broken `#[cfg(feature = "...")]` block ships
# unnoticed. `check` rather than `build` — this proves it compiles, cheaply.
check-features:
    cargo check --workspace --all-features --all-targets

# Enforce the CI-guarded architecture invariants (INV-4, INV-5).
check-invariants:
    python3 scripts/check-invariants.py

# Verify that every ADR citation in the documentation resolves to a real file.
# A broken citation reads as a missing decision, not as a typo, so it is checked
# rather than trusted.
check-doc-links:
    python3 scripts/check-doc-links.py

# Regenerate the ADR index in docs/adr/README.md from each ADR's front-matter.
gen-adr-index:
    python3 scripts/gen-adr-index.py

# Verify the ADR index is current. A generated index that is allowed to go stale
# is a hand-maintained index with extra steps.
check-adr-index:
    python3 scripts/gen-adr-index.py --check

# Everything CI runs, in one command. Run this before declaring work done.
check: fmt build test clippy check-features check-invariants check-doc-links check-adr-index
