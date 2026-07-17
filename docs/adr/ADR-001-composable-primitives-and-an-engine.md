# ADR-1: Composable primitives and an engine; techniques are configuration

## Context

"State-of-the-art RAG techniques" — query transformation (HyDE, multi-query, step-back), fusion and scoring (Reciprocal Rank Fusion, cross-encoder reranking, ColBERT late interaction), corrective and agentic variants (corrective RAG, self-RAG, adaptive RAG), and multi-stage indexing (GraphRAG, RAPTOR) — are scattered across dozens of frameworks and papers and are rarely comparable to one another.

The founding observation is that the vast majority of these are **not irreducible algorithms**. They are compositions or variations of a small number of primitives: query transformation, fusion and scoring, control flow (branch, bounded loop) around a pipeline, and multi-stage indexing pipelines producing derived artifacts. Trying to implement "every technique" would be a maintenance burden that grows faster than any team can absorb — and it would still not make the techniques comparable, which is the actual goal.

## Decision

Provide a **small core of orthogonal, composable primitives, executed by a fast engine, from which most named RAG techniques emerge as configuration** — a value of an `impl:` field or a control-flow node — accompanied by a curated set of ready-made recipes. The engineering goal is to identify the *right* primitives and make everything else expressible by composition.

## Alternatives rejected

- **Hardcode each published technique.** A maintenance burden that misses the target entirely: the set of techniques grows without bound, the code never catches up, and the techniques still are not comparable to one another because each is implemented in isolation.

## Consequences

The thesis yields a measurable indicator of design quality, usable as a compass throughout the project's life: **the fraction of RAG techniques expressible without modifying the engine** — through configuration alone, or through a new component behind an existing contract. The higher that fraction, the better the abstraction.

Any technique that *forces* a change to the engine is a signal that the pipeline representation or the contracts are incomplete, and warrants reopening the design rather than patching the core.

## Status

Accepted.
