# ADR-10: Deterministic retrieval metrics first, judge later

## Context

RAG benchmarks come in two families of fundamentally different natures. Retrieval benchmarks (BEIR, now the retrieval portion of MTEB) provide public, objective, reproducible leaderboards with metrics — nDCG@10, MRR@k, recall@k, precision@k, MAP@k — computed from qrels prepared in advance. These require **no LLM judge**: the computation is deterministic and unnoised. End-to-end RAG benchmarks (CRAG, MultiHop-RAG, …) evaluate the full retrieval-to-generation chain and require either a reference answer or a judge, introducing noise, cost and variance.

Recent literature shows deterministic metrics still dominate practice, and the research community is itself wary of the judge.

## Decision

**Begin the bench with the retrieval half.** Deliver a scientifically credible bench — hybrid versus dense versus sparse versus reranked — using deterministic retrieval metrics, **without ever invoking a judge**. Treat label-based metrics as the foundation and the judge as a separate, calibrated, optional instrument introduced only later.

**Credibility is earned by reproduction, not by implementation.** Writing the metric functions is not sufficient: the harness is trusted only once it reproduces a published leaderboard score (SciFact first, as the small binary-relevance calibration case; NFCorpus next, for graded relevance) to within 0.5 point, using an exact brute-force search — never an ANN index — so that any discrepancy is attributable to the metric or the encoding, not to approximation. This reproduction is a one-time validation of the pipeline, and the fixtures it freezes (a run, its qrels, and the expected scores checked against `pytrec_eval`) become permanent CI regression tests.

## Alternatives rejected

- **Full end-to-end evaluation from v0.** Makes the platform's first credibility depend on the noisiest, most expensive, least-trusted instrument (the judge) — when the entire LLM-as-judge problem simply does not arise if the bench starts with retrieval.

## Consequences

The most defensible possible v0: credibility without depending on a noisy instrument. When the judge is later introduced, it enters as a calibrated instrument (seeds, confidence intervals, a human meta-benchmark), never as a foundation. This is also the primary guard against scope inflation for the first deliverable.

## Status

Accepted.
