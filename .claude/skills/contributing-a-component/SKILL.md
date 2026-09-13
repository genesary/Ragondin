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
3. **Keep the crate a leaf.** `AGENTS.md` § Crate dependency graph names the workspace crates a component may depend on and bars the rest; **INV-5** (§ Invariants) is the same boundary seen from the engine's side. The temptation this step exists to catch is a helper that already lives in `ragondin-engine`: copy it or do without it, because an arrow pointing back at the engine is what makes a built-in component a privileged one.
4. **Confine your heavy dependency to this crate and put it behind a feature.** *Which* backend you gate is yours to pick; that it is gated is not — the **Feature flags** row of `AGENTS.md` § Frozen decisions and **INV-4** (§ Invariants, which names the dependencies it bars from the core) settle that between them. The reason it bites at this step rather than at review: the default build is what a researcher benchmarking on a laptop compiles, and an ungated heavy dependency makes them compile your backend to run someone else's.
5. **Register on the `EngineContext`.** The binary — the composition root — constructs your component and registers it explicitly, so registration is something a caller does holding a context, never something your crate arranges for itself at load time. **INV-6** (`AGENTS.md` § Invariants) states what that rules out and how much of it CI decides; the **Registry** row of § Frozen decisions says why it was chosen.

### Which choices are yours

Adding a component always meets a choice the documents do not settle — a tokenizer, how `k1`/`b` are exposed, which of two backends to gate. `AGENTS.md` § Rules of engagement decides who makes it by where it lands: it lists the shared surfaces that escalate to a `decision` issue (load `opening-a-decision-issue` for those), and states the recording obligation for a choice that does not.

## Remote — a gRPC service in any language

Use this so contributors who write Python (or anything else) are first-class. This is deliberate: if contributing required performant Rust, the contribution funnel would be a trickle.

1. **Implement the corresponding protobuf service from `ragondin-proto`** — the mirror of the Rust trait. Your service can be written in any language and can live entirely outside this repository.
2. **It is reached through the generic `Remote<T>` adapter** (in `ragondin-remote`), which implements the domain trait by speaking protobuf over gRPC. The engine perceives no difference between it and a `Local` implementation.
3. **Name it by URL in the pipeline configuration.** Physical planning resolves that reference to a `Remote<T>`.

**The non-breaking optimization path:** a `Remote` component (say, Python) that wins the benchmark can later be ported to `Local` Rust — same contract, no configuration change for any user. That path is why `Remote` is a first-class citizen, not a fallback.

## A genuinely new *kind* of node

If you are not implementing an existing contract but inventing a new node *type*, you go through the `Extension` variant of the pipeline node enum — **without modifying the core**. Repeated use of `Extension` for the same shape is the signal to later promote it into a primitive node. If you find yourself wanting to change the core enum instead, stop: that is an architectural decision — open a `decision` issue.

## Both natures must pass the conformance suite

Whether `Local` or `Remote`, every implementation must pass the shared conformance suite — a set of behavioural tests every implementation of a contract must satisfy, whatever its nature. The suite is what operationally enforces **INV-7** (`AGENTS.md` § Invariants, which states the rule and the sign it leaves in a diff): it is review-enforced, so without the suite no privilege for built-ins is a slogan a reviewer has to take on trust, and with it `Local`/`Remote` equivalence is verified rather than asserted.

> The suite lives at `testkit/ragondin-conformance`, a workspace member declared in the root `Cargo.toml`. The workspace layout remains the authority on the path.

> The source of truth for the component contract is `docs/` (the component contract sections) and `ragondin-contracts` itself. This skill summarizes the procedure; when a detail here and the code disagree, the code and `docs/` win.
