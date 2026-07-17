# ADR-10: Deterministic retrieval metrics first, judge later

## Context

RAG benchmarks come in two families of fundamentally different natures. Retrieval benchmarks (BEIR, now the retrieval portion of MTEB) provide public, objective, reproducible leaderboards with metrics — nDCG@10, MRR@k — computed from qrels prepared in advance. These require **no LLM judge**: the computation is deterministic and unnoised. End-to-end RAG benchmarks (CRAG, MultiHop-RAG, …) evaluate the full retrieval-to-generation chain and require either a reference answer or a judge, introducing noise, cost and variance.

Recent literature shows deterministic metrics still dominate practice, and the research community is itself wary of the judge.

## Decision

**Begin the bench with the retrieval half.** Deliver a scientifically credible bench — hybrid versus dense versus sparse versus reranked — using deterministic retrieval metrics, **without ever invoking a judge**. Treat label-based metrics as the foundation and the judge as a separate, calibrated, optional instrument introduced only later.

## Alternatives rejected

- **Full end-to-end evaluation from v0.** Makes the platform's first credibility depend on the noisiest, most expensive, least-trusted instrument (the judge) — when the entire LLM-as-judge problem simply does not arise if the bench starts with retrieval.

## Consequences

The most defensible possible v0: credibility without depending on a noisy instrument. When the judge is later introduced, it enters as a calibrated instrument (seeds, confidence intervals, a human meta-benchmark), never as a foundation. This is also the primary guard against scope inflation for the first deliverable.

## Status

Accepted.
