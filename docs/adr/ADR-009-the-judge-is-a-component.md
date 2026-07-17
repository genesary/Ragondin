# ADR-9: The LLM judge is a component of the representation

## Context

A portion of generation metrics rests on LLM-as-judge: non-deterministic, expensive, high-variance. The judge could be hardcoded into the evaluation harness as a special case — but doing so would make it impossible to version, vary and benchmark the judge, and would duplicate the machinery of the generator (both are "LLM call" components).

A natural objection is infinite regress: if the judge is evaluated by the engine, who evaluates the judge that evaluates the judge?

## Decision

The judge is **not a special case; it is a component of the pipeline representation**, carrying its model hash, prompt, temperature and seed — on exactly the same footing as the generator and the embedder. An evaluation run that uses a judge is itself a pipeline, executed by the engine, versioned and content-addressed like everything else.

## Alternatives rejected

- **A judge hardcoded in the evaluation harness.** Makes the judge un-versioned and un-benchmarkable, duplicates the generator's "LLM call" machinery, and forfeits mechanical self-preference detection.

## Consequences

The judge becomes an **experiment variable** — which judge model? which prompt? which temperature? — and can be benchmarked in turn. Because judge and generator both carry a `model_hash`, **self-preference bias** (a model scoring its own outputs favourably) becomes mechanically detectable: the engine compares the two hashes and warns when judge and generator are the same model.

The regress objection dissolves because the recursion has **exactly one level of depth and bottoms out on human labels**: calibrating a judge uses a meta-benchmark of `(answer, context, human quality label)` triples — itself merely one more registry entry of the "with reference answers" type. At the bottom, a human annotated. The judge is not a foundation; it is a calibrated proxy for a small human bedrock.

## Status

Accepted.
