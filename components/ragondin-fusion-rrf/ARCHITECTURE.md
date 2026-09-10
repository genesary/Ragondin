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
  `k + rank >= 1` for every configuration, and `k` is **clamped to `MAX_K`**
  (`2^32`) at construction, so the divisor cannot leave the range where
  consecutive ranks still separate — see below. The division has no edge case
  and this component has no failure mode of its own. Every `fuse` call succeeds.
- **A repeated id within one leg is counted once for that leg**, at its first
  position, and the ranks after it are unaffected. Counting both occurrences
  roughly doubles that chunk's fused score and floats it to the top — invisibly,
  since the fused output still holds distinct ids and passes every downstream
  check, so the only symptom is a wrong number.

  The earlier reading was that no guard is needed because a well-behaved
  retriever never emits a duplicate. The contract does not require that of one:
  `check_no_duplicate_ids` is called from the `Fusion` and `Reranker` suites
  only, never from `Retriever` or `VectorStore`, so a fully conformant leg may
  repeat an id. Across legs it is different — a chunk seen in several legs is
  accumulated and returned once, which is the whole point of fusing.
- **`k` is clamped to `2^32`, and the bound is load-bearing.** A rank's
  contribution is `1 / (k + rank + 1)`, accumulated in `f64`. Consecutive
  divisors around `k` give reciprocals differing by roughly `1/k` in relative
  terms, so as `k` grows they converge; once they round together, every rank in
  a leg scores identically, the ascending-id tiebreak decides an order the ranks
  were supposed to, and **a single leg comes back sorted by id instead of in the
  order it arrived** — the one property `check_fusion_conformance` states a
  fusion must preserve. Setting the bound where they *begin* to collide (`2^53`)
  is not enough: the gap there is a single ULP and neighbouring ranks round
  together unpredictably. `2^32` leaves about two million ULPs. For scale, the
  conventional `k` is 60.
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
