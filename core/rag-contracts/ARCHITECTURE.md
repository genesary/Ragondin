# ARCHITECTURE — rag-contracts

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
map (`rag-pipeline`); each trait here takes a typed params struct. Physical
planning bridges the two (`docs/code-architecture.md` §6.3): it resolves an
`impl:` name into a *constructed* component, so implementation-specific
configuration — BM25's `k1`/`b`, a model path — goes to the constructor, and
the params structs carry only what varies per call. §5.1's reference pipeline
shows the split: `top_k` on a retriever and a reranker, nothing on the fusion.

**This is the crate an external contributor implements.** A `Local` component is
a crate under `components/` that implements one of these traits; a `Remote`
component is a gRPC service honouring the mirror protobuf in `rag-proto`.

## Local invariants

- **The core stays light (INV-4, CI-enforced).** Someone implementing a
  `Reranker` should compile only this crate and `rag-types` — not the engine,
  not `tantivy`, not `tonic`. **No heavy dependency may appear here.** If one
  seems necessary, the abstraction is leaking through the contract: stop and
  raise it, do not add the dependency.
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
- **Public structs here are `#[non_exhaustive]` with constructors.** This is
  the crate every contributor compiles against, and the params structs will
  gain knobs; a caller who wrote a struct literal should not break. Note this
  is the *opposite* of `rag-pipeline`'s recorded choice, deliberately: there,
  an exhaustive `match` that stops compiling is the intended signal that a new
  node kind needs handling, whereas here breaking a caller signals nothing to
  anyone.

## Why it is a boundary

If this contract were unstable, every component in and out of the repository
would break on each change, and the contribution model would collapse. Stability
here is the foundation of the whole extension story.
