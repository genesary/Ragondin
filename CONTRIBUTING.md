# Contributing

Thank you for considering a contribution. This document covers how to build and
test, and the two paths for contributing a component.

Before writing code, read [`AGENTS.md`](AGENTS.md) — the binding operating rules
(invariants, frozen decisions, definition of done). It applies to human and AI
contributors alike.

## Build and test

```bash
cargo build                 # lean default build (no heavy backend)
cargo test --workspace      # run all tests
just check                  # everything CI runs, in one command
```

`just check` runs build, tests, `clippy` (warnings are errors), `cargo fmt
--check`, `cargo doc` (rustdoc warnings are errors), the architecture invariant
checks, the documentation link check, the ADR index staleness check, and the
dependency audit. **A change is not done
until `just check` passes.** The toolchain is pinned to stable
(`rust-toolchain.toml`); never rely on nightly.

The dependency audit (`just check-deny`) is the one check that needs a tool the
toolchain does not carry: install it once with `cargo install cargo-deny
--locked --version 0.20.2`. Pin that version — it is the one the CI action
bundles, and an unpinned install eventually disagrees with CI about a policy
file neither of you changed. Its policy — allowed licences, advisory allowances,
duplicate versions, source registries — is [`deny.toml`](deny.toml) at the
workspace root, with each section's reasoning in the file.

Unlike every other check here, this one reads an advisory database that is
updated daily, so `check-deny` can go red on a branch whose diff caused nothing;
treat that as a finding about the dependency, not about the branch.

### The architecture invariant checks

Five invariants are enforced as blocking CI checks, not just documented:

- **INV-4 — the core stays light.** `ragondin-types` and `ragondin-contracts` carry no
  heavy dependency (tantivy, tonic, prost, ort, candle, vector-store clients,
  HTTP clients). If you add one, the build fails and tells you why.
- **INV-5 — the engine knows only traits.** `ragondin-engine` depends on no crate
  under `components/`. Add such a dependency and the build fails.
- **INV-3 — value types only.** `ragondin-types` and `ragondin-pipeline` carry no
  I/O dependency (async runtime, transport, store client). The "no interner" and
  "no global context" clauses of INV-3 name no crate and stay review-enforced.
- **INV-6 — no global state.** No crate in this repository declares `inventory`
  or `linkme` as a **direct** dependency. Reaching one transitively, through the
  internals of a third-party crate, is not a violation — `tantivy` does exactly
  that through `typetag`, and it is neither a component registry nor an obstacle
  to two `EngineContext`s in one process — decided in #146. "Or equivalent" is a
  judgment a reviewer makes; these two are the part a build can decide.
- **INV-11 — Tower governs the network envelope only.** No `impl tower::Service`
  under `components/`.

Run them directly with `just check-invariants` (implemented in
`scripts/check-invariants.py`). If a check blocks you, it is the architecture
speaking — do not route around it. If you believe it is wrong, open a `decision`
issue rather than weakening the check.

### The documentation link check

`just check-doc-links` (`scripts/check-doc-links.py`) verifies that every
`ADR-<n>` citation in the repository's tracked Markdown **and Rust** sources
resolves to a file under `docs/adr/`, and that ADR filenames follow the convention
documented in [`docs/adr/README.md`](docs/adr/README.md). A citation that does not
resolve does not read as a typo — it reads as a missing decision, and whoever was
sent to read it derives their own instead.

Rust is read because that is where citations live: five of the six wrong `ADR-C3`
citations corrected in #100 were in `core/ragondin-contracts/src/lib.rs`, which the
check could not see. The scan is line-based, so a citation counts wherever in a
`.rs` file it is written. The **relative-link** half of the check stays
Markdown-only — a `](path)` link has no meaning in a doc comment, and rustdoc's
intra-doc links are `cargo doc`'s business.

It resolves a citation; it cannot judge whether the citation is **apt**. `ADR-C3`
cited for `ADR-3`'s decision passes this check. Open an ADR before citing it.

`just test-check-doc-links` (`scripts/test-check-doc-links.py`, also a CI step)
tests that check. Its failure mode is silence — a scan narrowed back to Markdown,
or a filename convention no longer enforced, keeps printing `All documentation
links resolve.` while resolving less — so each decision it makes is pinned by a
case, the Markdown-only relative-link rule included. The fixtures are built at run
time in a throwaway repository under the system temporary directory: the checker
selects tracked files, so a fixture committed here would be scanned by the real
check. If you change `check-doc-links.py`, change these cases in the same PR.

### The ADR index

The decision table in [`docs/adr/README.md`](docs/adr/README.md) is **generated**
from each ADR's YAML front-matter by `scripts/gen-adr-index.py`. Do not edit the
table by hand: change the front-matter and run `just gen-adr-index`.
`just check-adr-index` (also a CI step) fails when the committed table no longer
matches the ADRs.

Front-matter is metadata about a decision, never part of one. Adding or correcting
it is allowed; editing an ADR's Context, Decision, Alternatives rejected,
Consequences or Status prose is governed by the process rules in
[`docs/adr/README.md`](docs/adr/README.md) and is not something a PR does in
passing.

## Contributing a component

A component (a `Retriever`, `Reranker`, `Generator`, …) can be contributed two
ways. **Both are benchmarked identically, through the same code path, with no
privilege for built-in components (INV-7).**

### Local — a Rust crate

A new crate under `components/` that implements a trait from `ragondin-contracts`.

- It depends only on `ragondin-contracts` and `ragondin-types` — a component is a **leaf**
  of the dependency graph. It never depends on `ragondin-engine` or on another
  component. You compile only the contracts crate and the value types, not the
  whole engine.
- Its heavy dependency (the retrieval engine, the ML runtime, the store client)
  is confined to the crate and **feature-gated**, so the default workspace build
  stays lean.
- It registers on an `EngineContext` through exactly the same mechanism a
  third-party component would use.
- Naming: `ragondin-<role>-<implementation>`, e.g. `ragondin-reranker-onnx`,
  `ragondin-store-qdrant`.

See [`components/README.md`](components/README.md).

### Remote — a gRPC service in any language

A service (commonly Python) implementing the corresponding protobuf service from
`ragondin-proto`. It runs outside this repository, in any language, and is named by
URL in the configuration; the engine reaches it through a generic `Remote<T>`
adapter and cannot tell it apart from a `Local` component.

A `Remote` component that wins a benchmark can later be ported to `Local`
(Rust) with no configuration change for any user.

### Conformance

Whichever path you take, your component must pass the conformance suite
(`ragondin-conformance`) — the behavioural suite every implementation passes, `Local`
or `Remote`. That is what makes the two paths genuinely equivalent rather than
equivalent by assertion.

## Conventions

- **Language:** English everywhere — code, comments, docs, commits, PRs.
- **Commits:** Conventional Commits, scoped by crate where useful, e.g.
  `feat(ragondin-pipeline): add canonical hashing`.
- **Branches:** `<type>/<issue-number>-<slug>`.
- **Closing an issue from a PR:** write the keyword **bare** — `Closes #123`, not
  `Closes issue #123`. GitHub matches `<keyword> #<number>` with nothing in
  between; an intervening word makes the reference an ordinary mention, and the
  issue stays open when the PR merges. A task-list item is fine — the nesting is
  not the problem, the extra word is.
- **Errors:** `thiserror` (typed) in libraries; `anyhow` in binaries only.

### Stacked pull requests

Two changes that touch the same files are sometimes better reviewed as a stack —
PR B based on PR A's branch rather than on `main` — so each diff shows only its own
work. That is a real gain in reviewability, and it carries a cost worth knowing
**before** you choose it rather than after:

- **A stacked PR closes nothing.** GitHub creates closing references only for pull
  requests targeting the default branch, so a child PR's `Closes #123` is inert
  while its base is another branch — whatever the wording, and with no warning on
  the PR.
- **Merge a stack bottom-up.** GitHub retargets each child to `main` as its base
  lands. That part is automatic — but it is **not** enough on its own, see the next
  point.
- **After a child retargets, edit its body once.** GitHub evaluates closing
  references when the body is written, against the base *at that moment*, and does
  **not** re-evaluate them on retarget. A child whose body was written while its
  base was a feature branch keeps an inert `Closes #123` forever. Any edit to the
  body — even re-saving the same text — makes the reference register. Verify it
  did: the issue should appear under **Development** in the sidebar.
- **A squash-merged parent leaves the child conflicting.** The parent lands on
  `main` as one new commit, while the child's branch still carries its own copy of
  the same work under a different SHA, so Git sees both sides editing the same
  lines. Resolve it with a rebase that drops the duplicate, never a merge:

  ```bash
  git fetch origin
  git rebase --onto origin/main <parent-branch-tip> <child-branch>
  git push --force-with-lease origin <child-branch>
  ```

- **Out of order, issues close silently wrong or not at all.** After merging any
  stacked PR, check that its issue actually closed, and close it by hand if not.
- **Merging the parent closes the child.** On 2026-09-10, merging #124 deleted the
  base branch of #126. GitHub retargeted #126 to `main` at `07:35:47Z` and closed
  it **in the same second** — the timeline records
  `automatic_base_change_succeeded` and `closed` one after the other. Withholding
  `--delete-branch` is not the remedy: this repository merges with
  `delete_branch_on_merge` set (`gh api repos/genesary/Ragondin --jq
  '.delete_branch_on_merge'` returns `true`), so the base branch dies on merge
  whatever the client passed. **Retarget the child yourself before merging the
  parent** — `gh pr edit <child> --base main`, then merge. A child already based on
  `main` cannot be caught by its base being deleted. Failing that, expect to
  reopen the child straight away.
- **Before merging any pull request, confirm nothing is based on its branch.**
  `gh pr list --base <branch> --state open` must come back empty. The branch is
  deleted on merge, so a non-empty result is the list of pull requests you are
  about to close. That is the whole precaution, and it takes a second.
- **A `mergeable` of `UNKNOWN` that never resolves means the PR is closed**, not
  that GitHub is lagging: it does not compute mergeability for a closed pull
  request, so `gh pr view <n> --json mergeable` will sit there forever. Query
  `state` before you conclude anything from `mergeable`.
- **Never force-push a branch whose pull request is closed.** Reopening is refused
  permanently afterwards:

  ```
  gh pr reopen 126
    GraphQL: Could not open the pull request. (reopenPullRequest)

  gh api -X PATCH repos/genesary/Ragondin/pulls/126 -f state=open
    HTTP 422: state cannot be changed. The ci/44-cargo-deny branch was force-pushed or recreated.
  ```

  The branch and the work on it are intact; the pull request number is not
  recoverable. #126's work was re-filed as #127.

A stack is still the right shape when two PRs genuinely build on each other. Just
budget for the rebase and the body touch.
