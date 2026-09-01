# ARCHITECTURE — rag-types

**Status: stable API boundary (INV-1).** This crate is versioned. Breaking its
public API is a deliberate, costly act — never a side effect of another change.

## What lives here

The platform's core value types: `Document`, `Chunk`, `Query`, `Embedding`,
`ScoredChunk`, `Context`, `Generation`. Everything downstream depends on these;
they depend on almost nothing. `rag-types` is the ultimate leaf of the
dependency graph.

## Local invariants

- **Value types only (INV-3).** No global context, no interner, no I/O. A value
  is fully determined by its content. Two equal values are indistinguishable.
- **The core stays light (INV-4, CI-enforced).** No heavy dependency —
  no `tantivy`, `tonic`, `prost`, `ort`, `candle`, vector-store client, or HTTP
  client. `serde` is the only dependency permitted. A heavy dependency reaching
  this crate is an abstraction leak: stop and raise it, do not work around it.
- **No workspace dependency.** This crate depends on no other crate in the
  workspace. If a type here seems to need one, the type is in the wrong crate.

## Why it is a boundary

The domain types are the **source of truth** for the whole system (the protobuf
wire types in `rag-proto` are generated to mirror them). Stability here is what
lets a contributor implement a component against `rag-contracts` + `rag-types`
alone, without compiling the engine.
