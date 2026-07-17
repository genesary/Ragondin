# ADR-C14: Heavy backends feature-gated; lean default build

## Context

Standalone-first means a researcher benchmarking retrieval on a laptop should not have to compile the world. The heavy backends — `tantivy`, ONNX Runtime, `candle`, a vector-store client, the remote adapter — are expensive to compile.

## Decision

The **default build is minimal** — the core plus a light retriever — and compiles fast. **Every heavy backend sits behind a feature** (`bm25`, `onnx`, `candle`, `qdrant`, `remote`). Any heavy dependency of a component is **confined to its crate and feature-gated**. Dependency versions are centralized in `[workspace.dependencies]`.

## Alternatives rejected

- **Everything compiled by default.** Punishes the common case — a lean local retrieval bench — with the compile cost of every backend the platform can reach.

## Consequences

Standalone-first is real, and compile times stay low. Fine crate granularity additionally buys compilation parallelism and independent per-crate testing. A heavy dependency never leaks out of the component that needs it.

## Status

Accepted.
