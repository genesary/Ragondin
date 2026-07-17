# ADR-3: Two-faced component contract (Rust trait + protobuf), Local / Remote

## Context

A component is a pipeline stage: chunker, embedder, indexer, retriever, fusion, reranker, context builder, generator, grader (the LLM judge), vector store. Its contract is the interface that every component of a given kind satisfies, regardless of implementation.

Two forces pull in opposite directions. In-process implementations need to be fast — they are on the retrieval hot path. External contributors need to be able to write in any language: researchers write Python, and if contributing a component required performant Rust, the contribution funnel would be a trickle — and a benchmarking platform with no contributed techniques has nothing to benchmark.

## Decision

The component contract has **two faces that mirror each other exactly**:

- **Face 1 — a Rust trait**, implemented by in-process `Local` components (the hot path).
- **Face 2 — a protobuf service**, implemented by `Remote` components over gRPC (any language, any process).

The engine calls the trait (e.g. `reranker.rerank(...)`) without knowing whether the implementation is native Rust or a remote service several network hops away.

## Alternatives rejected

- **Rust-only contribution.** Narrows the contribution funnel to those who write performant Rust — a trickle, for a platform whose value depends on contributed techniques.
- **Dynamic plugin loading (`dlopen`, WebAssembly).** An exotic mechanism not needed in v0. Extension happens through a recompiled `Local` crate or a `Remote` endpoint; a sandboxed WASM nature is a much-later option the contract is designed to accommodate but does not require now.

## Consequences

A researcher with a new component writes no Rust at all: they implement the gRPC service, and their component is benchmarked exactly like a native one — same code path, same metrics, same run identity. There is a **non-breaking optimization path**: a `Remote` component that wins the benchmark can later be ported to `Local` Rust, with no change to any user's configuration.

Two consequences follow for the code: dynamic dispatch becomes mandatory (Local and Remote are indistinguishable at compile time), and a **conformance suite** is required so that Local/Remote equivalence is verified rather than merely asserted.

## Status

Accepted.
