# la-plateforme

An open-source, cloud-native platform — written in Rust — for **building,
serving, and evaluating Retrieval-Augmented Generation (RAG) systems**. You
compose a RAG pipeline from configurable primitives, run it, and rigorously
benchmark it, so you can answer *"which RAG configuration performs best on my
documents, at what cost and latency?"* with **reproducible numbers**.

## The design thesis

> **A small core of composable primitives, executed by a fast engine, from which
> most named RAG techniques emerge as configuration rather than code.**

Chunkers, embedders, retrievers, fusion, rerankers, context builders,
generators, graders — a handful of primitives wired into a pipeline graph.
"Hybrid retrieval", "corrective RAG", "RAG-fusion" and most other named
techniques are then *configurations* of that graph, not new code. A genuinely
new node type is added through an open `Extension` variant without touching the
core.

## Standalone first

The platform is **usable from a single binary and a configuration file — no
Kubernetes required**. The default build is lean and compiles fast; heavy
backends (a sparse index, an ONNX embedder, a vector store) are optional,
feature-gated, and added only when you want them. Cloud-native operation is a
later milestone layered on top, never a prerequisite.

One binary, four subcommands:

```text
rag bench <config> --benchmark beir/scifact   # evaluate a pipeline against a benchmark
rag compare <run-a> <run-b>                    # compare two runs
rag serve <config>                             # serve the pipeline
rag validate <config>                          # validate a configuration
```

## Architecture in one breath

The load-bearing boundaries of the system are **not documented conventions —
they are crate boundaries the compiler refuses to violate.** The engine depends
only on traits; concrete components are leaves; the binary is the single
composition root. Evaluation and serving are two thin drivers over **one**
engine, so they cannot drift apart.

- **Why the system is designed this way** → [`docs/system-architecture.md`](docs/system-architecture.md)
- **Why the code is organized this way** → [`docs/code-architecture.md`](docs/code-architecture.md)
- **Individual decisions, each citable** → [`docs/adr/`](docs/adr/)
- **What is deliberately undecided** → [`docs/OPEN_QUESTIONS.md`](docs/OPEN_QUESTIONS.md)
- **Rules for contributors and agents** → [`AGENTS.md`](AGENTS.md) · [`CONTRIBUTING.md`](CONTRIBUTING.md)

## Build and test

```bash
cargo build            # lean default build
just check             # build + test + clippy + fmt + architecture invariants
```

`just check` is what CI runs; it must pass before any change merges.

## Status

**Pre-alpha (milestone M0 — Foundations).** This is the workspace scaffold: the
crate skeleton, the conventions, and the CI checks that make the architecture's
two most load-bearing constraints impossible to violate. Functionality — the IR,
the engine, the components, the metrics — arrives in later milestones.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
