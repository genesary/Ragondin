# ARCHITECTURE — rag-contracts

**Status: stable API boundary (INV-1).** Versioned; breaking its public API is a
deliberate act.

## What lives here

The **component contract** — the domain traits a contributor implements:
`Chunker`, `Embedder`, `Indexer`, `Retriever`, `Fusion`, `Reranker`,
`ContextBuilder`, `Generator`, `Grader`, `VectorStore`. Plus the shared
`ComponentError` each boundary returns.

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

## Why it is a boundary

If this contract were unstable, every component in and out of the repository
would break on each change, and the contribution model would collapse. Stability
here is the foundation of the whole extension story.
