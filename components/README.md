# components/

**Empty by design, for now.** One crate per component implementation is added
here in later issues (`rag-retriever-bm25`, `rag-embedder-onnx`,
`rag-store-qdrant`, …). This directory holds a `.gitkeep` until the first one
lands.

## What a component crate is

Each component is its own crate and a **leaf** of the dependency graph:

- It depends on **`rag-contracts`** (the trait it implements) and **`rag-types`**
  (the value types) — and on **nothing else in the workspace**. Never on
  `rag-engine`, never on another component.
- Its **heavy dependency** (tantivy, candle, ort, a vector-store client, …) is
  **confined to it** and **feature-gated**. The default workspace build stays
  lean and compiles fast.
- It registers on an `EngineContext` through **exactly the same mechanism** as a
  third-party component. There is **no privilege for built-ins** (INV-7) — no
  shortcut, no fast path, no special case. A built-in and a third-party
  component are indistinguishable to the engine.

## Why the leaf constraint matters

The engine depends on `rag-contracts`, not on any crate here (INV-5, CI-enforced).
Because components are leaves and the engine knows only traits, `Local` (Rust,
in-process) and `Remote` (gRPC, any language) components share exactly one API,
and the two-tier system that kills contribution-driven projects is made
structurally impossible.

The naming convention is `rag-<role>-<implementation>`, e.g. `rag-reranker-onnx`,
`rag-store-qdrant` — guessable rather than memorized.

See `CONTRIBUTING.md` for the two contribution paths (`Local` and `Remote`).
