# ARCHITECTURE — ragondin-contracts

**Status: stable API boundary (INV-1).** Versioned; breaking its public API is a
deliberate act.

## What lives here

The **component contract** — the domain traits a contributor implements, plus
the shared `ComponentError` each boundary returns and the per-call params
structs each trait takes.

Defined today, the families the M2 retrieval bench exercises: `Retriever`,
`Fusion`, `Reranker`, `Embedder`, `VectorStore`. Arriving with M3/M4:
`Chunker`, `Indexer`, `ContextBuilder`, `Generator`, `Grader`. Adding a trait
is additive on this boundary, so defining one before anything implements it
would be dead API.

**Where parameters come from.** A pipeline node carries an untyped parameter
map (`ragondin-pipeline`); each trait here takes a typed params struct. Physical
planning bridges the two (`docs/code-architecture.md` §6.3): it resolves an
`impl:` name into a *constructed* component, so implementation-specific
configuration — BM25's `k1`/`b`, a model path — goes to the constructor, and
the params structs carry only what varies per call. §5.1's reference pipeline
shows the split: `top_k` on a retriever and a reranker, nothing on the fusion.

**This is the crate an external contributor implements.** A `Local` component is
a crate under `components/` that implements one of these traits; a `Remote`
component is a gRPC service honouring the mirror protobuf in `ragondin-proto`.

## Local invariants

- **The core stays light (INV-4, CI-enforced).** Someone implementing a
  `Reranker` compiles this crate, `ragondin-types`, `async-trait` and `thiserror` —
  not the engine, not `tantivy`, not `tonic`. **No heavy dependency may appear
  here.** If one seems necessary, the abstraction is leaking through the
  contract: stop and raise it, do not add the dependency.

  This crate also *declares* `ragondin-pipeline` and never references it, which drags
  `sha2` and its eight transitive crates into a contributor's build. The
  documented dependency graph sanctions the edge (`engine → contracts → ir →
  types`), so it is pre-declared rather than stray, and CI stays silent because
  none of those crates is heavy. It is still weight nobody asked for; tracked
  separately rather than removed in passing.
- **Async traits use `async_trait`** (frozen decision): `dyn`-compatible async,
  not RPITIT. Dynamic dispatch is mandatory — the engine cannot tell `Local`
  from `Remote` at compile time.
- **No privilege for built-ins (INV-7).** These traits are the *only* API. A
  first-party component and a third-party one implement exactly the same thing.
  There is no faster, more privileged contract for built-ins — and never will
  be.
- **Every trait is `dyn`-compatible and `Send + Sync`, and it is tested.** The
  engine holds components across await points and shares them between
  concurrent queries, and it cannot tell `Local` from `Remote`. Each trait
  therefore has a test coercing a stub to `Box<dyn _>` and calling **through
  the vtable** — constructing one is not enough to prove the property.
- **Every trait method takes a params struct, and every params struct is
  `#[non_exhaustive]` with a constructor.** One rule, no exceptions — including
  the structs that are empty today (`FusionParams`, `EmbedParams`). Adding a
  *field* is additive; changing a method's *arity* breaks every implementation
  in and out of the repository, third-party `Remote` services included, which
  is the contribution funnel ADR-C3 exists to protect. The uniformity is the
  point: an exception is where the next knob will land.

  This is the *opposite* of `ragondin-pipeline`'s recorded choice, deliberately.
  There, an exhaustive `match` that stops compiling is the intended signal that
  a new node kind needs handling. Here, breaking a contributor who wrote a
  struct literal signals nothing to anyone.

  `EmbeddedChunk` is **not** `#[non_exhaustive]`: it is a plain data carrier
  that `ragondin-remote` (#13) must construct by literal in its round-trip tests.
- **Open question — does an [`Embedder`] need to know its role?** Asymmetric
  models (E5, BGE, GTE) prefix a query differently from a passage, and getting
  it wrong costs retrieval quality with no error anywhere. Either that role is a
  field on `EmbedParams`, or it is constructor configuration and one model
  registers twice under two `impl:` names. Not settled here; `EmbedParams`
  exists so the answer is a field either way.

## Why it is a boundary

If this contract were unstable, every component in and out of the repository
would break on each change, and the contribution model would collapse. Stability
here is the foundation of the whole extension story.
