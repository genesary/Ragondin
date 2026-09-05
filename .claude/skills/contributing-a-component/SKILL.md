---
name: contributing-a-component
description: Load this whenever you are adding or modifying a pipeline component in this RAG platform — a retriever, embedder, reranker, fusion, chunker, indexer, context builder, generator, grader (the LLM judge), or vector store — whether in-process Rust (Local) or an external gRPC service (Remote). It gives the exact procedure for both natures, the rule that built-in and third-party components share one API with no privilege for built-ins (INV-7), and the requirement that every implementation pass the shared conformance suite. Reach for it even if you're "just wrapping an existing library" — a component still has to register on the EngineContext and confine its heavy dependency behind a feature flag.
---

# Contributing a component

A component is a pipeline stage satisfying a **two-faced contract**: a Rust trait (for in-process implementations) and a mirror protobuf service (for remote ones). The engine calls the trait without knowing which nature is behind it — that indistinguishability is the foundation of the whole contribution model, so the procedure below exists to keep it real rather than aspirational.

There are exactly two ways to add a component, plus one escape hatch for a genuinely new *kind* of node.

## Local — an in-process Rust component

Use this for the performance-critical path (BM25, an ONNX reranker, a dense retriever over a vector store).

1. **Create a new crate under `components/`**, named `ragondin-<role>-<implementation>` — e.g. `ragondin-reranker-onnx`, `ragondin-store-qdrant`. The pattern is meant to be guessable, not memorized.
2. **Implement the relevant trait from `ragondin-contracts`** (`Retriever`, `Reranker`, `Generator`, `Grader`, `VectorStore`, …).
3. **Depend on `ragondin-contracts` and `ragondin-types` and nothing else in the workspace.** A component is a *leaf* of the dependency graph. In particular it must never depend on `ragondin-engine` — the engine knows only traits (INV-5), and the arrow points from binaries to components, never the reverse.
4. **Confine your heavy dependency to this crate and feature-gate it.** `tantivy`, `ort`, `candle`, a vector-store client — each stays inside its component crate and behind a feature, so the default build stays lean and a researcher benchmarking on a laptop does not compile the world (INV-4 keeps that weight out of the core).
5. **Register on the `EngineContext`.** The binary (the composition root) constructs and registers the component explicitly. Never register through a static global (INV-6).

## Remote — a gRPC service in any language

Use this so contributors who write Python (or anything else) are first-class. This is deliberate: if contributing required performant Rust, the contribution funnel would be a trickle.

1. **Implement the corresponding protobuf service from `ragondin-proto`** — the mirror of the Rust trait. Your service can be written in any language and can live entirely outside this repository.
2. **It is reached through the generic `Remote<T>` adapter** (in `ragondin-remote`), which implements the domain trait by speaking protobuf over gRPC. The engine perceives no difference between it and a `Local` implementation.
3. **Name it by URL in the pipeline configuration.** Physical planning resolves that reference to a `Remote<T>`.

**The non-breaking optimization path:** a `Remote` component (say, Python) that wins the benchmark can later be ported to `Local` Rust — same contract, no configuration change for any user. That path is why `Remote` is a first-class citizen, not a fallback.

## A genuinely new *kind* of node

If you are not implementing an existing contract but inventing a new node *type*, you go through the `Extension` variant of the pipeline node enum — **without modifying the core**. Repeated use of `Extension` for the same shape is the signal to later promote it into a primitive node. If you find yourself wanting to change the core enum instead, stop: that is an architectural decision — open a `decision` issue.

## Both natures must pass the conformance suite

Whether `Local` or `Remote`, every implementation must pass the shared conformance suite — a set of behavioural tests every implementation of a contract must satisfy, whatever its nature. This is what operationally enforces **INV-7: no privilege for built-in components.** A built-in registers, and is tested, through *exactly* the same mechanism as a third-party one. Without the suite, "no privilege for built-ins" is only a slogan; with it, `Local`/`Remote` equivalence is verified rather than asserted.

> The suite lives under `testkit/`. Its crate is named `ragondin-conformance` in the code-architecture document; the issue that commissioned this skill referred to it as `ragondin-contract-tests`. The two names are being reconciled — check the actual workspace layout for the current path rather than trusting either name blindly.

> The source of truth for the component contract is `docs/` (the component contract sections) and `ragondin-contracts` itself. This skill summarizes the procedure; when a detail here and the code disagree, the code and `docs/` win.
