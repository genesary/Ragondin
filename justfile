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

# Enforce the CI-guarded architecture invariants (INV-3, INV-4, INV-5, INV-6,
# INV-11).
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

# Audit the dependency graph: RustSec advisories, licences, duplicate versions
# and source registries. The policy is deny.toml at the workspace root, not
# cargo-deny's defaults. Unlike the checks above, this one needs a tool that is
# not in the toolchain, so it says how to get it rather than failing as an
# unrecognized cargo subcommand.
check-deny:
    @cargo deny --version >/dev/null 2>&1 || { echo "error: cargo-deny is not installed. Install the version CI runs with:"; echo "    cargo install cargo-deny --locked --version 0.20.2"; exit 1; }
    cargo deny --all-features check

# The repository's cross-reference map. `just map <entity>` prints one entity's
# neighbourhood; `just map --conflicts` lists every claim the code contradicts;
# `just map --view` writes the interactive viewer under target/map/.
#
# Deliberately NOT part of `just check` (#102). Every edge carries the tier it was
# learned from -- closure is complete, scan is best-effort, claim is unverified
# prose -- and a diagnostic that blocks a build on an unverified tier would be
# asserting more than it knows. Promoting a query to a check is a separate act.
map *ARGS:
    python3 scripts/gen-map.py {{ARGS}}

# Everything CI runs, in one command. Run this before declaring work done.
check: fmt build test clippy check-features check-invariants check-doc-links check-adr-index check-deny
