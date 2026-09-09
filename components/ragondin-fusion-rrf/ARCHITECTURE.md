# ARCHITECTURE — ragondin-fusion-rrf

**Status: a component, and therefore a leaf** of the dependency graph. Not an
API boundary: nothing in the workspace depends on it except a binary, which
constructs it and registers it on an `EngineContext`.

## What lives here

One implementation of [`Fusion`](../../core/ragondin-contracts/src/lib.rs):
**Reciprocal Rank Fusion**. A chunk's fused score is `sum over the legs that
ranked it of 1 / (k + rank)`, with `rank` 1-based and `k` defaulting to 60.

The incoming scores are **not read**. That is the property RRF is chosen for:
a BM25 score and a cosine similarity are on incomparable scales, and reading
only the position lets a hybrid pipeline combine them without calibrating
either.

## Local invariants

- **It is a leaf ([INV-5](../../AGENTS.md)).** It depends on
  `ragondin-contracts`, `ragondin-types` and `async-trait` — never on
  `ragondin-engine`, never on another component. The engine knows only traits
  ([ADR-C5](../../docs/adr/ADR-C05-engine-depends-only-on-traits-components-are-leaves.md)),
  so the arrow points from a binary to this crate and never the reverse.
- **No privilege for being built-in (INV-7).** It registers the way a
  third-party component registers, and it passes the same suite
  ([ADR-C6](../../docs/adr/ADR-C06-identical-api-plus-conformance-suite.md)) —
  `tests/conformance.rs` is the call a contributor writes, verbatim.
- **`k` is constructor configuration, not a per-call parameter.**
  `ragondin-contracts` draws that line: a params struct carries only what varies
  per call, and `FusionParams` is empty. `k` is a `usize`, which makes
  `k + rank >= 1` for every configuration, and the sum saturates rather than
  wrapping, which keeps it inside `1..=usize::MAX` at the other end — so the
  division has no edge case and this component has no failure mode of its own.
  Every `fuse` call succeeds.
- **A repeated id within one leg contributes once per occurrence.** The sum is
  over positions, by construction, and a well-behaved retriever never emits the
  same id twice in one list, so nothing guards against it: the conformance
  suite's "no duplicate ids" check reads a `Fusion`'s or a `Reranker`'s output,
  not a retriever's. Across legs it is different — a chunk seen in several legs
  is accumulated and returned once.
- **The result is deterministic, including its ties.** Equal fused scores are
  the common case rather than a rarity, so the order among them is pinned: by
  chunk id, ascending. A chunk offered by several legs keeps the copy of the
  first leg that offered it. Neither choice is arbitrary in effect — a run that
  reorders between executions is not reproducible, which is what M2 is for.
- **The `rrf` feature gates nothing heavy, on purpose.** There is no heavy
  dependency here to confine; the feature exists so that every component crate
  names its implementation the same way — one feature per backend, named after
  it — which is what makes the pattern guessable.
  [ADR-C14](../../docs/adr/ADR-C14-heavy-backends-feature-gated-lean-default-build.md)
  is the rule it follows. The uniformity is in the **naming, not the
  invocation**: the rule `components/` follows is that a feature gating a heavy
  dependency is off by default while a feature gating nothing is on by default,
  so `rrf` is in `default` where a `bm25` is not, and only the heavy one has to
  be asked for.

## What is deliberately not here

No retrieval, no scoring, no corpus. A fusion is handed ranked lists and
returns one; where they came from is the pipeline's business.
