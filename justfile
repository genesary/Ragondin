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

# Enforce the CI-guarded architecture invariants (INV-4, INV-5).
check-invariants:
    python3 scripts/check-invariants.py

# Everything CI runs, in one command. Run this before declaring work done.
check: fmt build test clippy check-invariants
