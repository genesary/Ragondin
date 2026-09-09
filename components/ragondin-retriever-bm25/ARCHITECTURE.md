# ARCHITECTURE — ragondin-retriever-bm25

**Status: a component — a leaf of the dependency graph, and internal.** Nothing
here is an API boundary (INV-1 protects `core/`, not this crate). The stable
surface a caller depends on is the `Retriever` trait in `ragondin-contracts`;
this crate is one implementation of it and may be refactored freely.

## What lives here

In-process **BM25 sparse retrieval** over an in-memory `tantivy` index:
`Bm25Retriever`, constructed over a `Vec<Chunk>`, implementing `Retriever`.

Sparse retrieval **without an Elasticsearch sidecar** is one of the reasons the
data plane is written in Rust (`docs/system-architecture.md` §10). This is the
first crate to make that concrete.

**Not here, deliberately:** fetching or chunking a corpus (the harness and the
`Chunker` family), dense retrieval (`ragondin-retriever-dense`), and registration
on an `EngineContext` — the registry lives on the engine, and the binary is the
composition root that puts the two together (`docs/code-architecture.md` §8.1).

## Local invariants

- **A component is a leaf (INV-5, CI-enforced).** The dependencies are
  `ragondin-contracts`, `ragondin-types`, `async-trait`, `thiserror` and the heavy
  backend — never `ragondin-engine`, never a sibling component. The engine knows
  only traits, and the check walks the dependency graph to prove it.
- **The heavy backend is confined and feature-gated (ADR-C14).** `tantivy` is an
  optional dependency behind `bm25`, and **`bm25` is not a default feature**:
  `cargo build --workspace` compiles none of it. With the feature off the crate
  exports nothing — a BM25 retriever without tantivy would be a different
  component, not a degraded one — so the tests are behind the same `cfg` and
  `just test-bm25` is what runs them. `just check-features` proves the gated code
  still compiles.
- **No privilege for built-ins (INV-7).** This crate is reachable only through
  the `Retriever` trait and its own public constructor. It has no entry point
  into the engine that a third-party crate could not also call, and adding one
  would be the two-tier system ADR-C6 exists to prevent.
- **The ranking contract is honoured, and the ties are too.** `Retriever`
  requires descending, finite scores. tantivy leaves equally scoring documents
  ordered by document address — stable for one index, but meaningless to a
  caller — so this crate sorts on `(score descending, chunk id ascending)`.
  Without that, two chunks with equal BM25 scores could rank two ways over one
  corpus, and the only symptom would be a benchmark number that moves.
- **The query is analyzed, never parsed.** A retriever is handed
  natural-language questions. tantivy's `QueryParser` would read `?`, `:` or a
  quote in one as syntax — failing the call, or silently changing what was
  asked — so the query is run through the text field's own analyzer and turned
  into a `Should` disjunction of `TermQuery`s. That also guarantees the query is
  tokenized exactly as the corpus was.
- **The index is immutable.** It is built once, in RAM, at construction.
  Re-indexing means constructing another retriever. That is what makes a run
  reproducible: the same corpus yields the same index yields the same ranking.
- **Typed errors (ADR-C13).** `IndexError` for construction, `ComponentError`
  for the call. A library never imposes `anyhow` on its consumers.

## What is not configurable, and why

`k1` and `b` are named in `docs/code-architecture.md` §6.3 as the example of
constructor configuration. **They are not exposed here**, because tantivy holds
them as private constants and offers no way to set them: a knob on this
constructor would be a claim the backend cannot honour. It arrives when the
backend supports it, not before.

## Conformance

`tests/conformance.rs` runs `ragondin_conformance::assert_retriever_conformance`
against the same public constructor a third-party caller uses — which is what
makes built-in/third-party equivalence real rather than asserted (ADR-C6). The
rest of that file is what conformance deliberately does not check: the suite
knows nothing of the corpus behind a retriever, so *ranking* — which chunk comes
first, and why — is checked here, against a corpus small enough to read.
