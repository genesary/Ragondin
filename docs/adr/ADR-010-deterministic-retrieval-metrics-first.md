# ADR-10: Deterministic retrieval metrics first, judge later

## Context

RAG benchmarks come in two families of fundamentally different natures. Retrieval benchmarks (BEIR, now the retrieval portion of MTEB) provide public, objective, reproducible leaderboards with metrics — nDCG@10, MRR@k, recall@k, precision@k, MAP@k — computed from qrels prepared in advance. These require **no LLM judge**: the computation is deterministic and unnoised. End-to-end RAG benchmarks (CRAG, MultiHop-RAG, …) evaluate the full retrieval-to-generation chain and require either a reference answer or a judge, introducing noise, cost and variance.

Recent literature shows deterministic metrics still dominate practice, and the research community is itself wary of the judge.

## Decision

**Begin the bench with the retrieval half.** Deliver a scientifically credible bench — hybrid versus dense versus sparse versus reranked — using deterministic retrieval metrics, **without ever invoking a judge**. Treat label-based metrics as the foundation and the judge as a separate, calibrated, optional instrument introduced only later.

**`trec_eval` is the reference implementation, not a source of inspiration.** Where a metric admits more than one defensible definition, `ragondin-metrics` implements the one `trec_eval` implements — because the objective is to replay a *published* MTEB/BEIR evaluation and land on the same number, and every published figure was produced by `trec_eval` or by `pytrec_eval`, its Python binding. Three such choices are already load-bearing and are documented in the crate: linear rather than exponential nDCG gain; precision@k dividing by `k` even for a short run; and **MAP@k dividing by the total number of relevant documents rather than by `min(k, R)`** — the latter is the recommender-systems convention, is a perfectly good metric, and is *not* the one on the leaderboard. A future metric that has no `trec_eval` counterpart is a new decision, not a free choice.

**Credibility is earned by reproduction, not by implementation.** Writing the metric functions is not sufficient: the harness is trusted only once it reproduces a published leaderboard score (SciFact first, as the small binary-relevance calibration case; NFCorpus next, for graded relevance) to within 0.5 point, using an exact brute-force search — never an ANN index — so that any discrepancy is attributable to the metric or the encoding, not to approximation. This reproduction is a one-time validation of the pipeline, and the fixtures it freezes (a run, its qrels, and the expected scores checked against `pytrec_eval`) become permanent CI regression tests.

## Alternatives rejected

- **Full end-to-end evaluation from v0.** Makes the platform's first credibility depend on the noisiest, most expensive, least-trusted instrument (the judge) — when the entire LLM-as-judge problem simply does not arise if the bench starts with retrieval.

## Consequences

The most defensible possible v0: credibility without depending on a noisy instrument. When the judge is later introduced, it enters as a calibrated instrument (seeds, confidence intervals, a human meta-benchmark), never as a foundation. This is also the primary guard against scope inflation for the first deliverable.

## Amendments

**2026-09-05 — the Decision section was *extended*, not retracted, on the repository owner's decision, in the PR implementing #25.**

This entry does not record a retraction. Nothing above was found false and nothing was removed: the decision as originally accepted — begin with the retrieval half, no judge — stands verbatim as paragraph 1. What happened is that two further normative paragraphs were added to **Decision**: `trec_eval` as the reference implementation rather than a source of inspiration, and credibility earned by reproduction against a frozen `pytrec_eval` fixture.

That is outside the mechanism process rule 2 provides. Rule 2 covers retracting a false claim from the *reasoning*, and is confined to Context, Alternatives rejected and Consequences — "**The Decision section changes only by supersession.**" Adding binding rules to Decision inside an implementation PR is what rule 4 exists to prevent. It was done knowingly and with sign-off rather than by oversight, because the conventions had to be written down somewhere the moment `ragondin-metrics` had to choose between two defensible MAP@k denominators, and no superseding ADR existed yet.

It is recorded here because the one guarantee still available is that the change not be silent. Two consequences for a future reader: the added paragraphs have had materially less scrutiny than the decision they extend — they were written and reviewed inside a feature PR, not in a `decision` issue — and they now constrain #26, #29 and #33. If they need to change again, supersede this ADR rather than amend it a second time.

## Status

Accepted (amended 2026-09-05, see above).
