# ARCHITECTURE — ragondin-stub

**Status: a component, and therefore a leaf** of the dependency graph. Not an
API boundary: nothing in the workspace depends on it except a binary, which
constructs it and registers it on an `EngineContext`.

## What lives here

Two deterministic implementations of contracts in
[`ragondin-contracts`](../../core/ragondin-contracts/src/lib.rs):

- **`StubRetriever`** — a `Retriever` with no corpus. It returns exactly
  `top_k` chunks, with ids `<label>-0`, `<label>-1`, … and score
  `1 / (rank + 1)`. The query text reaches the chunk text and changes nothing
  else, so two different questions get the same answer.
- **`StubFusion`** — a `Fusion` that interleaves: rank 0 of every leg, then
  rank 1 of every leg, in the order the pipeline wires them, skipping a chunk
  already taken. It re-scores by output position, for the reason RRF re-scores:
  two legs score on incomparable scales.

## What it is for

It is the **fixture the end-to-end paths are wired with** — the vertical slice
in [`bin/ragondin/tests/`](../../bin/ragondin/tests/vertical_slice.rs), and the
evaluation harness and `ragondin bench` that build on it. A pipeline made of
these components needs no corpus, no index and no model, so it runs anywhere,
and it runs the same way twice. What it makes observable is the **wiring**: a
configuration file on disk, a registration on an `EngineContext`, a plan, and an
`ExecutionTrace` with one entry per node.

## Local invariants

- **It is a leaf ([INV-5](../../AGENTS.md)).** It depends on
  `ragondin-contracts`, `ragondin-types` and `async-trait` — never on
  `ragondin-engine`, never on another component. The engine knows only traits
  ([ADR-C5](../../docs/adr/ADR-C05-engine-depends-only-on-traits-components-are-leaves.md)),
  so the arrow points from a binary to this crate and never the reverse. Holding
  a stub to the same edges as a real component is what makes it a usable
  rehearsal of one: a fixture that reached further would rehearse a wiring
  nothing in production could have.
- **No privilege for being built-in (INV-7).** It registers the way a
  third-party component registers, and it passes the same suite
  ([ADR-C6](../../docs/adr/ADR-C06-identical-api-plus-conformance-suite.md)) —
  `tests/conformance.rs` is the call a contributor writes, verbatim. Conformance
  matters more here than elsewhere: a fixture that quietly broke the contract
  would make every pipeline built on it prove the wrong thing.
- **Nothing here is a measurement.** A stub retriever answers every query
  identically and its scores encode rank alone, so no number taken over these
  components says anything about retrieval quality. Conformance is a floor, not
  an endorsement.
- **Determinism is the product.** No clock, no randomness, no I/O, no
  interior state. Two runs of one pipeline return the same chunks, in the same
  order, with the same scores — which is what lets a test assert on an output
  and on a `LogicalPipeline`'s content hash at the same time.
- **The `stub` feature gates nothing heavy, on purpose.** There is no heavy
  dependency here to confine; the feature exists so that every component crate
  names its implementation the same way, following
  [ADR-C14](../../docs/adr/ADR-C14-heavy-backends-feature-gated-lean-default-build.md)
  as `ragondin-fusion-rrf` and `ragondin-store-memory` do — a feature gating a
  heavy dependency is off by default, one gating nothing is on by default.

## Two open questions this crate does not answer

- **`components/` granularity** (`docs/OPEN_QUESTIONS.md` #4) — one crate per
  family, or one per implementation. This crate holds two components of two
  different families, and that is a fixture kept in one place, **not** a
  position on how production components should be split. Do not cite it as a
  precedent.
- **Registry ergonomics** (`docs/OPEN_QUESTIONS.md` #1) — the composition root
  registers these stubs explicitly and verbosely, which is the valid interim.
  This crate reaches no registry of its own.

## What is deliberately not here

No corpus, no index, no randomness, and no stub for a family the fixture
pipeline does not use. A vector store is not stubbed: `ragondin-store-memory`
is a real component that needs no external service, so a pipeline that needs one
uses it. Fewer fakes on the path is better.
