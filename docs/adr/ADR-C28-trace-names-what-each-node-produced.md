---
id: ADR-C28
title: The execution trace names what each node produced, in rank order
status: accepted
invariants: [INV-2, INV-10]
supersedes: []
superseded_by: null
---

# ADR-C28: The execution trace names what each node produced, in rank order

## Context

ADR-C9 made the execution trace the executor's return value (INV-10) so that
per-node replay — the differentiating capability of the single front end
(ADR-14) — has structured business data to render. It lists what a node's entry
carries, "input, output, duration, branch taken", and stops there: it does not
say how much of an output the entry holds. The engine answered by construction.
`NodeTrace.output` is a `ValueSummary`, and for a list of chunks a summary is a
**count**. A stored run today says that, for each of its queries, `lexical`
produced 50 chunks, `fused` 98 and `reranked` 10 — and nothing else. The
ranking itself is built by `ragondin-harness` to score the metrics, handed to
the running sums, and dropped.

Four things need that ranking and cannot be built without it:

- **ADR-10's permanent regression fixture.** Once the harness reproduces a
  published figure, ADR-10 freezes "a run, its qrels, and the expected scores
  checked against `pytrec_eval`" as a permanent CI test. A metric fixture takes
  a ranking as its input; from the store one can freeze only means, and a
  metric defect that leaves the mean intact is caught by nothing. The SciFact
  calibration (`bin/ragondin/tests/calibration.rs`) pins aggregates and digests
  for exactly this reason, and says so.
- **Per-node replay.** A count is not a replay: a view cannot show what a
  reranker reordered without knowing what it received and what it returned.
- **The graded-relevance calibration and the exact-versus-approximate recall
  comparison** of `docs/system-architecture.md` § 9.8 Calibrating the harness
  against a published leaderboard, both of which compare result sets query by
  query.
- **Comparison at the query level.** Which queries a hybrid pipeline won
  against a dense one is the question a researcher asks of two runs, and
  `compare` can only answer it over means.

The choice could not be made inside an implementation issue. What the trace
carries is on the escalation list of `AGENTS.md` § Rules of engagement, and the
shape of `Run` is the experiment plane's public API and its on-disk format.
Decided in #239.

## Decision

**The execution trace names what each node produced.** For a list of chunks,
a node's output entry carries the chunks in the order the node returned them —
each as its chunk id, its document id and its score — rather than their count.
Id, document and score are what a `ScoredChunk` already holds; nothing is
added to `ragondin-contracts` or `ragondin-types`.

**A node's inputs stay summaries.** An input is the output of the node that
produced it, already named under that node; writing it twice would double the
trace for no information. A count remains the summary of an input.

**The trace, not the run, is the record.** `Run` gains no per-query field. The
harness renders the trace into the `TraceDocument` a run stores, as it does
today, and that rendering carries the new fields; the store's file layout does
not change, because the document is opaque to it. Whatever needs a per-query
ranking — the ADR-10 fixture, a graded-relevance calibration, a recall
comparison, a per-query `compare` — reads it from the stored trace of the node
whose output is the pipeline's.

**No cap and no detail knob.** The trace is bounded by the plan's `top_k`
values, which are the user's configuration, never by the corpus. The day a run
is too large to keep as it is, that is a compression question for the store,
not a shape question for the trace.

## Alternatives rejected

- **A per-query ranking on `Run`, beside the metrics.** Exactly what the freeze
  needs and nothing more — and that is the objection. It duplicates the last
  node of the trace in a second record, changes the experiment plane's public
  type and on-disk format, and serves none of per-node replay. Two records of
  one fact drift.
- **Aggregates only, no per-query record anywhere.** Nothing changes on any
  shared surface today, and ADR-10's regression fixture, the graded-relevance
  calibration and per-node replay all stay impossible. Each of them returns the
  day it is wanted; deferring the question does not answer it.
- **Ids without scores.** Smaller, and enough for a metric that reads rank
  alone. But replay wants to show why a chunk moved, and a score is the only
  reason a reranker gives; and `pytrec_eval`, the cross-check ADR-10 names,
  takes scores.
- **Naming inputs as well as outputs.** Complete per node in isolation, and
  twice the size for nothing: an input is a producer's output, already named.
- **A trace-detail knob, counts or ids per run.** A switch whose off position
  reproduces today's gap. Nothing today has a run too large to keep, and when
  one appears a switch on the trace's shape is the wrong tool.

## Consequences

- `ragondin-engine`'s `ValueSummary` names the ordered chunks of an output;
  INV-2 lets the exact representation move. The executor still returns the
  trace: INV-10 is untouched, and so is the signature ADR-C9 fixed.
- `ragondin-harness` renders the new fields, and may derive the ranking it
  scores from the trace rather than beside it. It is the one crate that touches
  both the engine and the store, and it changes in the same issue as the
  engine.
- `ragondin-experiments`, `ragondin-contracts`, `ragondin-types` and the
  store's file layout do not change.
- A stored trace grows to a few megabytes for a run of a few hundred queries at
  `top_k` 50 per leg — bounded by the plan, not the corpus.
- The trace is not part of a run's identity, so scores in it do not touch the
  run id (P4). Two machines may differ in the last bits of a score; a
  per-query fixture compares ranks, and scores under a tolerance, as the
  metric parity fixtures already do.
- What follows, each its own issue and none reopening this decision: the
  ADR-10 per-query fixture under `eval/ragondin-metrics`, then the
  graded-relevance (NFCorpus) calibration, then a per-query `compare`.

## Status

Accepted.
