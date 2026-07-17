# ADR-4: One engine, serving and evaluation drivers

## Context

A RAG pipeline is the **same computation** whether it serves one live query or evaluates ten thousand from a dataset. If the evaluation harness reimplemented the pipeline differently from the serving path, the platform would be benchmarking a system it never deploys — the benchmarks would be lies. This is the RAG analogue of train/serve skew, and it is the failure mode this platform exists to prevent.

## Decision

There is **one engine**. Only the feed differs, through thin **drivers**:

- a **serving driver** — the engine receives live traffic;
- an **evaluation driver** — the same engine, fed from a dataset, capturing metrics and traces.

## Alternatives rejected

- **A separate evaluation harness that reimplements the pipeline.** Reintroduces evaluation/serving skew — the exact divergence between what is measured and what is deployed that the platform is built to eliminate.

## Consequences

The benchmark travels **exactly the same code path** as production, so skew is structurally impossible rather than merely discouraged. The configuration that wins a benchmark is exactly the configuration promoted to serving; the same artifact travels from laptop to production without rewriting.

This principle *mandates* a single engine; it does not merely recommend one. Every driver is a thin wrapper over the same engine.

## Status

Accepted.
