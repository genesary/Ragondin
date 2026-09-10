# ARCHITECTURE — ragondin-store-memory

**Status: a component, and therefore a leaf** of the dependency graph. Not an
API boundary: nothing in the workspace depends on it except a binary, which
constructs it and registers it on an `EngineContext`.

## What lives here

One implementation of [`VectorStore`](../../core/ragondin-contracts/src/lib.rs):
vectors held in RAM, searched by **scanning all of them** and scoring by cosine
similarity. No index is built, so nothing about a result depends on how one was
built.

That is the whole reason this crate exists rather than only
`ragondin-store-qdrant`. The M2 exit criterion is a number that has to be
**reproducible in plain CI**, and an ANN index reached through a service is two
sources of run-to-run difference — approximation, and a backend that has to be
standing. Exact brute force has neither. The trade is a search linear in the
corpus, which is affordable at the sizes M2 measures (BEIR/scifact is some five
thousand documents) and would not be at production scale; Qdrant is the store
for that, and the one that proves the trait against a real service.

## Local invariants

- **It is a leaf ([INV-5](../../AGENTS.md)).** It depends on
  `ragondin-contracts`, `ragondin-types` and `async-trait` — never on
  `ragondin-engine`, never on another component, and never on an embedder: this
  crate stores vectors it is handed and computes none. The engine knows only
  traits
  ([ADR-C5](../../docs/adr/ADR-C05-engine-depends-only-on-traits-components-are-leaves.md)),
  so the arrow points from a binary to this crate and never the reverse.
- **No privilege for being built-in (INV-7).** It registers the way a
  third-party store registers, and it passes the same suite
  ([ADR-C6](../../docs/adr/ADR-C06-identical-api-plus-conformance-suite.md)) —
  `tests/conformance.rs` is the call a contributor writes, verbatim.
- **Cosine, not dot product.** Both are exact and both are cheap; they differ
  on whether a long vector pointing elsewhere may outrank a short one pointing
  at the query. Under dot product it may, and a corpus of unnormalised
  embeddings then ranks by document magnitude as much as by relevance. Cosine
  removes the magnitude, which also makes an inserted vector its own nearest
  neighbour whatever else the store holds — the property the conformance suite
  names as the one that makes a vector store a vector store.
- **The result is deterministic, including its ties.** Ties are the common case
  rather than a rarity — every vector orthogonal to the query scores exactly
  zero — so the order among them is pinned: by chunk id, ascending. Insertion
  order is therefore not observable in a result, which is what makes a run
  reproducible after a corpus is re-indexed in a different order.
- **A store holds exactly one width.** The first batch fixes it and every later
  vector, stored or queried, must match; a disagreement is an
  `InvalidRequest`, and a batch that disagrees is rejected whole so a caller
  never has to guess how much of it landed. `ragondin-types` says where this
  belongs: an empty embedding is representable, and "a dimensionality
  disagreement is caught where it is meaningful — by the vector store being
  searched." **That width is never zero**: an embedding with no components is
  refused on `upsert`, because a width-zero opening batch would otherwise fix
  the store at a width no later vector and no query can match — every search
  over it an `InvalidRequest`, with no way left to correct it.
- **A vector with no direction is answered differently on the two sides,
  deliberately.** Cosine is undefined against it — that is the zero vector, and
  also any vector carrying a component that is not a finite number. A **query**
  with no direction is an `InvalidRequest`: the caller is holding it right now
  and can fix it. A **stored** vector with no direction scores zero instead: it
  is already indexed, and failing every later search over the corpus that
  contains it would be a worse answer than ranking it last.
- **No score is ever `NaN`, and that is arranged rather than hoped for.**
  `NaN` breaks the ranking contract silently, which is the failure
  `ragondin-contracts` names when it requires finite scores. Two things make it
  unreachable. First, the norms and the dot product accumulate in `f64` even
  though a component is an `f32`: squaring is where an ordinary embedding leaves
  the `f32` range — `1e30` is a good `f32` and its square is not — and an `f32`
  accumulator turns `inf / inf` into `NaN` on the one search whose answer is
  least in doubt, a vector against itself. `f64` has room for the square of
  every `f32`, so a norm computed there is non-finite only if an input component
  already was. Second, `cosine` returns `None` when either norm is zero **or**
  non-finite, which is the single point where both cases above are decided.
- **The lock is an `RwLock`, and a poisoned one is recovered rather than
  propagated.** `upsert` takes `&self`, so the interior mutability is this
  component's to provide; `RwLock` rather than `Mutex` because a store is read
  far more than written and searches should not serialise against each other.
  A poisoned lock means a writer panicked mid-batch; the write section inserts
  and replaces whole entries, so what a reader then sees is some prefix of a
  batch rather than a torn value, and `into_inner` serves it instead of
  poisoning every later call. No lock is held across an `await`: every critical
  section is plain arithmetic.
- **`upsert` is linear in the corpus, per entry.** It finds an existing chunk id
  by scanning what is held, so a batch of *m* entries into a store of *n* costs
  *O(mn)* — the same brute-force trade the search side makes, and affordable for
  the same reason.
- **Nothing is filtered.** A chunk that scores zero comes back scored zero
  rather than dropped. "Few results" and "poor results" are different facts, and
  only the caller knows which one it wants; a store that hides the difference
  makes a low-recall run look like a small corpus.
- **The `memory` feature gates nothing heavy, on purpose.** Brute-force cosine
  over a `Vec` is `std` arithmetic. The feature exists so that every component
  crate is entered the same way, which is what makes the pattern guessable.
  [ADR-C14](../../docs/adr/ADR-C14-heavy-backends-feature-gated-lean-default-build.md)
  is the rule it follows, and uniformity is the reason it is followed where it
  buys no compile time.

## What is deliberately not here

No ANN index and no external service (#24 is the store for that). No embedding:
this crate never turns text into a vector — that is `ragondin-embedder-onnx`
(#20), and keeping the two apart is what lets the composition root pair either
embedder with either store.

An **empty `upsert`** is a no-op that succeeds. That is a local choice and not
an answer to #91, which asks whether the *contract* calls it valid; this crate
follows the neighbouring families, which treat an empty input as success.

Refusing a **zero-width embedding** is the same kind of local choice, and it
does not settle #90 either. #90 asks what an `Embedder` may legitimately
*return*; this decides only what this store will *hold*, and it would still be
this store's answer whichever way #90 goes.
