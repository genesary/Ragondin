# CONTEXT.md

The vocabulary this repository is written in.

`AGENTS.md` states the **rules**. The two documents in `docs/` state the
**reasoning**. Both are written in a domain vocabulary that is defined only inside
them — *canonical logical form*, *two-faced contract*, *the judge*, *driver* — and
scattered across 1,400 lines. This file collects that vocabulary in one place so
that reading a rule does not require first reading an architecture document to
find out what its nouns mean.

**Every entry here is extracted, not authored.** Each one names the document or
ADR it comes from, which is where the full treatment lives; the line here is a
pointer, not a replacement. **Nothing here is a rule.** No invariant, no frozen
decision, no process — this file says what a word *means*, never what you must
*do*. For obligations, read `AGENTS.md`; it is the only place they are binding.

---

## The pipeline representation

| Term | Meaning | Grounded in |
|---|---|---|
| **Pipeline representation** | The graph-with-control-flow data structure describing a RAG pipeline, independent of syntax and execution. The central abstraction. | [`docs/system-architecture.md`](docs/system-architecture.md) §14 · [ADR-2](docs/adr/ADR-002-pipeline-representation-is-a-graph-with-control-flow.md) |
| **Node** | One stage in that graph. A node names, in its `inputs`, the ids of the nodes whose output it consumes; `inputs` is positional and order-significant. | [`core/ragondin-pipeline/src/node.rs`](core/ragondin-pipeline/src/node.rs) · [ADR-C16](docs/adr/ADR-C16-erased-edge-values-checked-before-execution.md) |
| **Control flow** | Branch and bounded-loop nodes in the representation. What makes corrective and agentic RAG expressible as configuration rather than as code. | [`docs/system-architecture.md`](docs/system-architecture.md) §14 · [ADR-2](docs/adr/ADR-002-pipeline-representation-is-a-graph-with-control-flow.md) |
| **Raw form** (`RawPipeline`) | The permissive deserialization target a configuration file lands in: strings and unresolved references. It may be malformed, and it is never executed. | [`docs/code-architecture.md`](docs/code-architecture.md) §6.1 · [ADR-C2](docs/adr/ADR-C02-three-level-pipeline-representation.md) |
| **Logical form** (`LogicalPipeline`) | The validated, canonical level: it *names* each implementation and resolves none. A value type, and the level that is content-addressed. `impl: qdrant_dense` is part of the logical form, so two backends are two different logical forms. | [`docs/code-architecture.md`](docs/code-architecture.md) §6.1 · [ADR-C2](docs/adr/ADR-C02-three-level-pipeline-representation.md) |
| **Canonical logical form** | The normalized form of a `LogicalPipeline`: two semantically equivalent configurations, formatted differently, reduce to the same one. It is the form the content hash is computed over. | [`docs/code-architecture.md`](docs/code-architecture.md) §6.1 · [`core/ragondin-pipeline/ARCHITECTURE.md`](core/ragondin-pipeline/ARCHITECTURE.md) |
| **Physical form** (`PhysicalPipeline`) | The level with implementations **resolved** to trait objects and defaults applied, ready to execute. It holds `Box<dyn>`, so it is not serializable and cannot be hashed. *(The documents usually call this the "physical level"; "physical form" is the same thing said in parallel with "logical form".)* | [`docs/code-architecture.md`](docs/code-architecture.md) §6.1 · [ADR-C2](docs/adr/ADR-C02-three-level-pipeline-representation.md) |
| **`Extension` variant** | The open variant of the node enum, carrying a node defined outside the core — how a genuinely new node *kind* is added without modifying the core. | [`docs/code-architecture.md`](docs/code-architecture.md) §6.2 · [ADR-C3](docs/adr/ADR-C03-closed-enum-plus-open-extension-variant.md) |
| **Wire schema** | The hand-maintained, independently versioned serialized shape of a configuration, carrying its own `SchemaVersion`. Structurally distinct from the in-memory model. | [`core/ragondin-pipeline/ARCHITECTURE.md`](core/ragondin-pipeline/ARCHITECTURE.md) · [ADR-C11](docs/adr/ADR-C11-wire-format-separate-and-versioned.md) |

## Components and the engine

| Term | Meaning | Grounded in |
|---|---|---|
| **Component** | A pipeline stage — chunker, embedder, retriever, fusion, reranker, context builder, generator, grader — satisfying the two-faced contract. | [`docs/system-architecture.md`](docs/system-architecture.md) §14 |
| **Two-faced contract** | The component contract, in two mirrored faces: a **Rust trait** (in `ragondin-contracts`) and a **protobuf service** (in `ragondin-proto`). One contract, two ways to satisfy it. | [`docs/system-architecture.md`](docs/system-architecture.md) §5.2 · [ADR-3](docs/adr/ADR-003-two-faced-component-contract-local-remote.md) |
| **`Local` / `Remote`** | The two implementation natures of a component: in-process Rust (the hot path), or a gRPC service in any language. The engine cannot tell them apart — a `Remote` is reached through the generic `Remote<T>` adapter, and both arrive as `Box<dyn Reranker>`. | [`docs/code-architecture.md`](docs/code-architecture.md) §7 · [ADR-3](docs/adr/ADR-003-two-faced-component-contract-local-remote.md) |
| **Conformance suite** (`ragondin-conformance`) | The behavioural test suite every implementation of a contract must pass, whatever its nature. It is what makes `Local`/`Remote` and built-in/third-party equivalence real rather than asserted. | [`docs/code-architecture.md`](docs/code-architecture.md) §7.4 · [ADR-C6](docs/adr/ADR-C06-identical-api-plus-conformance-suite.md) |
| **Registry** | The table mapping an implementation name to a component constructor, consulted during physical planning. | [`docs/code-architecture.md`](docs/code-architecture.md) §8.1, §17 |
| **`EngineContext`** | The object that carries the registry, passed explicitly as a parameter. Several distinct contexts can exist in one process — which is what lets the evaluation harness compare two configurations side by side. | [`docs/code-architecture.md`](docs/code-architecture.md) §8.1 · [ADR-C4](docs/adr/ADR-C04-engine-as-embeddable-library-with-explicit-context.md) |
| **Composition root** | The single place where the engine and the concrete components are assembled: the binary. *(§8.1 also calls `EngineContext` "the composition root" — it is the object the binary assembles into.)* | [`docs/code-architecture.md`](docs/code-architecture.md) §17, §4.3 |
| **Driver** | A thin wrapper over the one engine. There are two: `ragondin-server` (the Tower network envelope) and `ragondin-harness` (the benchmark iterator). The engine is mode-agnostic; only the drivers differ. | [`docs/code-architecture.md`](docs/code-architecture.md) §9 · [ADR-4](docs/adr/ADR-004-one-engine-two-drivers.md) |
| **`ConfigSource`** | The data plane's configuration-source abstraction: `LocalFile` or `Stream`. | [`docs/system-architecture.md`](docs/system-architecture.md) §14 · [ADR-7](docs/adr/ADR-007-custom-resource-is-serialization-of-the-representation.md) |

## Evaluation

| Term | Meaning | Grounded in |
|---|---|---|
| **Benchmark** | Structurally a quadruple: **corpus + queries + qrels + reference answers**. Which pieces are present mechanically determines which metrics are computable. | [`docs/system-architecture.md`](docs/system-architecture.md) §5.3 · [ADR-8](docs/adr/ADR-008-benchmark-contract-typed-by-ground-truth.md) |
| **qrels** | Query relevance judgments — query-to-document relevance labels. The basis of the deterministic retrieval metrics. | [`docs/system-architecture.md`](docs/system-architecture.md) §14 |
| **Benchmark adapter** | A component normalizing an external benchmark format (BEIR, CRAG) into that internal quadruple. | [`docs/system-architecture.md`](docs/system-architecture.md) §14 |
| **The judge** | The LLM that scores generated answers. It is a **component of the representation** like any other, carrying its model hash, prompt, temperature and seed — so it is itself an experiment variable, and an evaluation run that uses one is itself a pipeline. | [`docs/system-architecture.md`](docs/system-architecture.md) §9.3 · [ADR-9](docs/adr/ADR-009-the-judge-is-a-component.md) |
| **Self-preference** | The bias of a judge scoring its own outputs favourably. Detectable mechanically here, by comparing the judge's model hash against the generator's. | [`docs/system-architecture.md`](docs/system-architecture.md) §9.6, §14 |
| **Meta-benchmark** | A dataset of `(answer, context, human label)` triples, used to calibrate a judge against human agreement. | [`docs/system-architecture.md`](docs/system-architecture.md) §14 |

## Runs and identity

| Term | Meaning | Grounded in |
|---|---|---|
| **Content addressing** | Identifying an entity by the hash of its content. The foundation of reproducibility here. | [`docs/system-architecture.md`](docs/system-architecture.md) §14 |
| **Run** | One execution of a pipeline over a benchmark, together with its metrics and its traces. The **unit of recomputation** in this system. | [`docs/system-architecture.md`](docs/system-architecture.md) §7.1 · [`docs/code-architecture.md`](docs/code-architecture.md) §12 |
| **Run identity** (`run_id`) | The content-addressed tuple that names a run: `hash(pipeline_config, dataset_version, index_version, model_hashes, engine_version)`. `model_hashes` includes the judge, on the same footing as the generator and the embedder. | [`docs/system-architecture.md`](docs/system-architecture.md) §7.1 |
| **Run cache** | The content-addressed cache keyed on that tuple: if the configuration, dataset, index, models and engine are unchanged, the run is not re-executed. | [`docs/code-architecture.md`](docs/code-architecture.md) §12 |
| **Execution trace** (`ExecutionTrace`) | The executor's structured, per-node return value: for each node its input, output, duration, and the branch taken. It is what per-node replay in the interface is built from. | [`docs/code-architecture.md`](docs/code-architecture.md) §17 · [ADR-C9](docs/adr/ADR-C09-traces-are-the-executors-return-value.md) |
| **Index / `index_version`** | An index is a **derived artifact** — produced by applying an indexing configuration to a corpus — not merely a table of vectors. It is immutable, versioned and pinned; a serving deployment binds to a specific version. | [`docs/system-architecture.md`](docs/system-architecture.md) §7.1 |
| **Run store** | The experiment plane's native store of runs, their metrics and their traces, with export adapters to third-party trackers. | [`docs/system-architecture.md`](docs/system-architecture.md) §6.4 · [ADR-13](docs/adr/ADR-013-native-run-store-with-export-adapters.md) |

## The planes

| Term | Meaning | Grounded in |
|---|---|---|
| **Data plane** | The headless, pure-compute plane with no durable state, which executes the pipeline representation through both drivers. The user interface never touches it. | [`docs/system-architecture.md`](docs/system-architecture.md) §6.1 · [ADR-5](docs/adr/ADR-005-pure-compute-data-plane.md) |
| **Experiment plane** | The plane holding all product-level state: the run store, the registry of benchmarks and datasets and indexes, the metrics catalogue, run comparison, the user interface, the export adapters, and the controller. | [`docs/system-architecture.md`](docs/system-architecture.md) §6.2 · [ADR-12](docs/adr/ADR-012-ui-in-experiment-plane-not-data-plane.md) |
| **External stores** | The third plane, out of scope and behind traits: the vector store, the artifact store, the LLM inference server. | [`docs/system-architecture.md`](docs/system-architecture.md) §6.3 |
| **Unification loop** | The content-addressed loop binding the planes together, which is what makes this one product rather than a data plane and a benchmarking tool glued together. | [`docs/system-architecture.md`](docs/system-architecture.md) §7 |
| **Evaluation/serving skew** | Divergence between the evaluation code path and the serving code path, which makes benchmarks unrepresentative of production. Eliminated structurally here, by there being one engine. | [`docs/system-architecture.md`](docs/system-architecture.md) §14 · [ADR-4](docs/adr/ADR-004-one-engine-two-drivers.md) |
