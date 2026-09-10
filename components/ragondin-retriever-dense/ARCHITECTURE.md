# ARCHITECTURE — ragondin-retriever-dense

**Status: a component, and therefore a leaf** of the dependency graph. Not an
API boundary: nothing in the workspace depends on it except a binary, which
constructs it and registers it on an `EngineContext`.

## What lives here

One implementation of [`Retriever`](../../core/ragondin-contracts/src/lib.rs):
the query is embedded, and the vectors nearest it are read out of a
`VectorStore`. Two calls and nothing between them — no model, no index, no
scoring of its own.

That is the whole crate, and it is deliberately that small. Dense retrieval is
*a model and an index*, and both of those already have a contract: keeping the
composition separate from either is what lets the M2 bench run one dense arm
over an exact in-memory store in plain CI and the same arm over a real backend
elsewhere, with the same code and the same configuration.

## Local invariants

- **It is a leaf ([INV-5](../../AGENTS.md)).** It depends on
  `ragondin-contracts`, `ragondin-types`, `async-trait` and `thiserror` — never
  on `ragondin-engine`, and never on the embedder or the store it is built from:
  those are `Box<dyn Embedder>` and `Box<dyn VectorStore>`, chosen by the
  composition root
  ([ADR-C5](../../docs/adr/ADR-C05-engine-depends-only-on-traits-components-are-leaves.md)).
  A dependency on `ragondin-embedder-onnx` or `ragondin-store-memory` would make
  one component depend on another, which is the shape this crate exists to
  avoid.
- **No privilege for being built-in (INV-7).** It implements the same
  `Retriever` trait as BM25 and passes the same suite
  ([ADR-C6](../../docs/adr/ADR-C06-identical-api-plus-conformance-suite.md)) —
  `tests/conformance.rs` is the call a contributor writes, verbatim.
- **The query is embedded under `EmbedRole::Query`.** The role is a per-call
  parameter because retrieval embedders are frequently asymmetric, and passing
  the wrong side raises no error anywhere — it only scores worse
  ([ADR-C17](../../docs/adr/ADR-C17-embedding-role-per-call-prefixes-in-the-constructor.md)).
  This crate is one of the two production call sites that ADR names, and it
  knows its side statically, so the role is a constant here rather than
  anything a caller can set.
- **A `top_k` of zero is refused here, not delegated.** It is this component's
  obligation under the `Retriever` contract; a store may answer a search of zero
  however it likes, so delegating the check would make conformance depend on
  which store the retriever was built over.
- **The store's ranking is passed through untouched.** Scores are not rescaled
  and the list is not re-sorted or re-truncated: `search` already returns at
  most `top_k` hits, descending and finite, and a second sort here would be a
  silent second opinion about a contract the store is already held to. It
  follows that the ranking contract this crate satisfies is exactly the one its
  store satisfies — which is what the conformance suite checks on both.
- **A width disagreement is the store's to report.** An embedder paired with an
  index built by a different model is a wiring mistake, and this crate cannot
  see it: a width is all it could compare, and two models frequently share one.
  The store rejects the search, which is where `ragondin-types` says a
  dimensionality disagreement is meaningful.
- **An embedder that answers one text with something other than one vector is a
  backend failure.** One vector per input, in order, is the `Embedder` contract;
  broken, it leaves nothing to search with. The error is boxed inside
  `ComponentError::Backend` and its type is private, because that variant's
  fidelity exists in-process and not over the wire.
- **The `dense` feature gates nothing heavy, on purpose.** The backends arrive
  as trait objects, so there is no dependency here to keep out of the default
  build. The feature exists so that every component crate is entered the same
  way, which is what makes the pattern guessable.
  [ADR-C14](../../docs/adr/ADR-C14-heavy-backends-feature-gated-lean-default-build.md)
  is the rule it follows.

## What is deliberately not here

**No corpus embedding.** This crate embeds the query and only the query. Filling
a store means embedding the corpus, which the harness and the binary drive at
bench time, ad hoc — not expressed as a pipeline. Whether indexing belongs in
the pipeline formalism at all is question 5 of `docs/OPEN_QUESTIONS.md`, and it
is open: nothing here answers it.

**No fallback and no hybrid.** Combining this arm with a sparse one is
`ragondin-fusion-rrf`'s job, and a dense-only run is exactly this retriever with
no fusion and no reranker — the baseline the M2 criterion compares hybrid
against. A store that fails a search fails the retrieval; retrying elsewhere
would make a run's number depend on which backend was reachable.
