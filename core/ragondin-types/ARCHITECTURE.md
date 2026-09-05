# ARCHITECTURE — ragondin-types

**Status: stable API boundary (INV-1).** This crate is versioned. Breaking its
public API is a deliberate, costly act — never a side effect of another change.

## What lives here

The platform's core value types: `Document`, `Chunk`, `Query`, `Embedding`,
`ScoredChunk`, and the `DocId` / `ChunkId` / `QueryId` newtypes that name them.
Everything downstream depends on these; they depend on almost nothing.
`ragondin-types` is the ultimate leaf of the dependency graph.

The generation-side types (`Context`, `Generation`) belong here too, but arrive
with the milestone that consumes them — adding them earlier would be dead code
on a stable boundary.

## Local invariants

- **Value types only (INV-3).** No global context, no interner, no I/O. A value
  is fully determined by its content. Two equal values are indistinguishable.
- **The core stays light (INV-4, CI-enforced).** No heavy dependency —
  no `tantivy`, `tonic`, `prost`, `ort`, `candle`, vector-store client, or HTTP
  client. `serde` is the only dependency permitted **that ships to consumers**;
  a test-only dev-dependency sits outside INV-4 by design, because it does not
  reach anyone implementing a component (`scripts/check-invariants.py` excludes
  dev edges deliberately). A heavy dependency reaching this crate is an
  abstraction leak: stop and raise it, do not work around it.
- **The types are not `#[non_exhaustive]`, deliberately.** Adding a field to one
  of them is therefore a breaking change — which is what INV-1 asks for: a
  deliberate, versioned act. The alternative would force every construction site
  through a builder to buy an additive path the crate does not need while it is
  unpublished at `0.0.0`. Revisit this at the first published version, not
  before; note that adding `#[non_exhaustive]` later is itself breaking.
- **No workspace dependency.** This crate depends on no other crate in the
  workspace. If a type here seems to need one, the type is in the wrong crate.

## Why it is a boundary

The domain types are the **source of truth** for the whole system (the protobuf
wire types in `ragondin-proto` are generated to mirror them). Stability here is what
lets a contributor implement a component against `ragondin-contracts` + `ragondin-types`
alone, without compiling the engine.
