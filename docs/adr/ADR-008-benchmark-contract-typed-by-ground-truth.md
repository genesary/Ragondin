# ADR-8: Benchmark contract typed by the presence of qrels and reference answers

## Context

A benchmark is structurally a **quadruple**: corpus (documents) + queries + qrels (query-to-document relevance judgments) + reference answers (optional). Different public benchmarks carry different pieces, and the pieces present determine which metrics can legitimately be computed — and whether an LLM judge is required at all.

## Decision

The benchmark contract is **typed by the presence or absence of each piece**, which mechanically determines the computable metrics:

| Pieces present | Computable metrics | LLM judge required? |
|---|---|---|
| corpus + queries + **qrels** | Retrieval: recall@k, MRR, nDCG@k | **No** — fully deterministic |
| corpus + queries + **reference answers** | Generation: comparison against reference | No, or partially |
| corpus + **queries only** | Generation: faithfulness, relevance | **Yes** — the noisiest regime |

A `BenchmarkAdapter` normalizes each external format (BEIR, CRAG, …) into this internal structure.

## Alternatives rejected

- **A single flat dataset format.** Cannot express which metrics are legitimately computable for a given benchmark. It would either force a judge everywhere (even where deterministic qrels exist) or forbid it everywhere.

## Consequences

The available ground truth determines the evaluation regime automatically, rather than by a hand-configured choice that could be wrong. The contract is the **data-side mirror of the component contract**: one stable interface, N implementations. Public benchmarks become supplied adapters; a custom benchmark means populating the same structure.

## Status

Accepted.
