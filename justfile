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

# Lint with warnings promoted to errors, in both feature configurations.
#
# `--all-features` is load-bearing, not thoroughness for its own sake: a heavy
# backend is never a default feature, so without it clippy is `cfg`'d out of
# every component crate's real code and the gate silently covers nothing where
# it matters most.
#
# The default run is kept alongside it rather than replaced by it. The two cover
# different code: an item reachable only under `#[cfg(not(feature = ...))]` is
# compiled away by the all-features run, and no other gate here denies warnings
# — neither `build` nor `test` does — so dropping this one would leave the
# default configuration ungated entirely.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Verify formatting (does not modify files).
fmt:
    cargo fmt --check

# Compile the feature-gated code paths. The default build is lean, so it never
# sees them: without this, a broken `#[cfg(feature = "...")]` block ships
# unnoticed. `check` rather than `build` — this proves it compiles, cheaply.
check-features:
    cargo check --workspace --all-features --all-targets

# Run the tests that live behind a feature. `just test` builds the workspace
# with default features, which by rule enable no heavy backend, so it compiles
# those tests away rather than running them; `check-features` proves they
# compile but runs nothing. This is where they actually execute.
#
# Whole-workspace and feature-blind on purpose. Naming a crate and a feature
# here would mean every future component has to remember to add itself, and
# forgetting is silent — the tests simply never run, which is the failure this
# recipe exists to prevent. Not `--all-targets`: that would switch doctests off.
test-features:
    cargo test --workspace --all-features

# Enforce the CI-guarded architecture invariants (INV-3, INV-4, INV-5, INV-6,
# INV-11). Resolves the `--all-features` graph, as clippy, the tests and
# deny.toml do: every heavy backend sits behind a feature by rule, so the lean
# graph contains no component crate's real dependencies at all.
check-invariants:
    python3 scripts/check-invariants.py

# Test that check itself. It is the gate CI calls the most important one, and
# its failure mode is silence: a check resolving a narrower graph, or no longer
# looking at the edge it is supposed to look at, keeps printing `All
# architecture invariants hold.` while holding less. That is not hypothetical —
# it is how the check came to walk a graph with no component crate in it.
#
# The fixtures are throwaway cargo workspaces built at run time under the system
# temporary directory. Their stand-ins for third-party crates are path
# dependencies outside the fixture workspace, so nothing is fetched and nothing
# is committed.
test-check-invariants:
    python3 scripts/test-check-invariants.py

# Verify that every ADR citation in the documentation resolves to a real file.
# A broken citation reads as a missing decision, not as a typo, so it is checked
# rather than trusted.
check-doc-links:
    python3 scripts/check-doc-links.py

# Test the documentation link check itself. That check is a blocking gate whose
# failure mode is silence: a scan reverted to Markdown-only, or a filename
# convention no longer enforced, would keep printing `All documentation links
# resolve.` while resolving less. This pins the decisions it makes.
#
# The fixtures are built at run time in a throwaway git repository under the
# system temporary directory, never committed — the checker selects tracked
# files, so a committed fixture would be scanned by the real check and a
# deliberately broken citation would fail it for real.
test-check-doc-links:
    python3 scripts/test-check-doc-links.py

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
# Advisory, and deliberately NOT part of `just check` — the reason is in
# AGENTS.md, § What you write about the code is checked against the code.
# Promoting a query to a check is a separate act.
map *ARGS:
    python3 scripts/gen-map.py {{ARGS}}

# Everything CI runs, in one command. Run this before declaring work done.
check: fmt build test test-features clippy check-features test-check-invariants check-invariants test-check-doc-links check-doc-links check-adr-index check-deny
