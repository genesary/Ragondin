# ARCHITECTURE — ragondin-benchmarks

## What lives here

The `BenchmarkAdapter` trait, the internal benchmark structure it produces
(`Benchmark` = corpus + queries + `Qrels`), and one adapter per external
dataset format. `BeirAdapter` is the first.

## The internal structure is the stable thing

This crate is the **data-side mirror of the component contract**: one stable
interface, N implementations. `Benchmark` is that interface. Adding a dataset
format means adding an implementor of `BenchmarkAdapter`; it must never mean
changing `Benchmark`, because everything downstream — the harness, the metrics
call sites, the run store — is written against `Benchmark` and not against any
one dataset.

`Benchmark`'s fields are private and it is built through `Benchmark::new`.
That is deliberate: §5.3 models a benchmark as a **quadruple** — corpus +
queries + qrels + **reference answers** — and only the first three are in
scope while there is no LLM judge. When reference answers arrive, they arrive
as an additional constructor or setter, leaving `new` and every existing
accessor untouched. Do not add the field before the milestone that reads it.

## This crate owns qrels; `ragondin-metrics` owns none of it

`Qrels` is defined here because judgments are part of the quadruple and this is
the crate that produces them. `ragondin-metrics` defines no qrels type at all:
its functions take `ranked: &[DocId]` and `relevance: &BTreeMap<DocId, u8>` —
the judgments of *one* query, which is exactly what `Qrels::for_query` returns.

**Neither crate depends on the other, in either direction.** That is not an
accident to be tidied up later; it is what keeps `ragondin-metrics` a pure leaf
with no I/O, and it is what let the two crates be built in parallel. If a
change appears to need an edge between them, that is a design change: stop and
open a decision issue.

The contract both sides must honour, and that nothing in the type system
checks: **a grade is a `u8` and `0` means judged-and-not-relevant**. A
grade-`0` entry is *not* the same as an absent one — the first was assessed and
found irrelevant, the second was never assessed — and both count for nothing in
a metric, but dropping grade-`0` rows while parsing would still be wrong, since
it discards the distinction the dataset went to the trouble of recording.

## The title-concatenation rule — do not silently undo this

`ragondin_types::Document` is `{ id, text, metadata }` and has no title field.
BEIR datasets have one, and:

> `Document.text` is set to **title, one space, then text**, omitting both the
> space and the title when the title is empty or absent. The raw title is
> additionally kept under `metadata["title"]`.

BEIR's own evaluation code indexes the concatenation, and **every published
BEIR leaderboard number is computed that way**. Indexing the text alone changes
nDCG@10 on most BEIR datasets, which would make the M2 exit criterion —
"hybrid retrieval beats dense-only on BEIR, reproducibly" (#33) — irreproducible
against the published figures. This is not a matter of taste, and it is the
decision in this crate most likely to be undone by a well-meaning
simplification. `combined_text` is the single place it is implemented, and
`tests/beir_fixture.rs` pins it, including the empty-title and absent-title
cases.

`metadata["title"]` is written only when the title is non-empty: an empty title
is the absence of a title, and storing `""` would make the two
indistinguishable while preserving nothing.

## BEIR's own `metadata` object is deliberately dropped

Each BEIR corpus line may carry a `metadata` JSON object of its own (for
example `{"url": "..."}` on a MED-10-style line), separate from `title`. It is
**not** mapped onto `Document.metadata` — only `title` is preserved there, as
above. `Document.metadata` is `BTreeMap<String, String>`, while BEIR's
`metadata` is arbitrary JSON; carrying it over would mean inventing a
JSON-to-string encoding that nothing in scope consumes. This was a deliberate
choice, not an oversight, and it is recorded here so it is not "fixed" later
by someone assuming it was missed.

## Local constraints

- **I/O here is correct.** INV-3 (value types only, no I/O) names
  `ragondin-types` and `ragondin-pipeline`; this crate is not covered by it.
  Reading dataset files from disk is this crate's job. The `Benchmark` it
  produces is still plain data.
- **Keep it light (INV-4 in spirit).** A JSON reader (`serde_json`) and a TSV
  reader (`csv`) are the whole toolkit. No heavy backend, no vector store, no
  HTTP client — and in particular **no network fetch**: a dataset path comes
  from configuration and the snapshot is frozen on disk, because a published
  score is attached to a specific snapshot and benchmarking against a live
  source is not reproducible (§9.1).
- **Ids are opaque strings, never parsed as numbers.** BEIR ids look like
  `MED-10` and `4983`; leading zeros are significant. They map straight onto
  `DocId` / `QueryId`.
- **`BenchmarkAdapter::load` is synchronous.** `async_trait` is the frozen
  decision for *component* traits, which sit on the request hot path. A dataset
  is read once from local files before a run starts; an async signature would
  force a runtime into this crate and buy nothing.
- **The adapter chooses the query set.** BEIR's `queries.jsonl` spans every
  split, so `BeirAdapter` keeps only the queries judged in the split it loaded.
  `Benchmark::iter` itself imposes no such rule — it yields an empty relevance
  map for an unjudged query — because that filtering is a per-format decision,
  not a property of the structure.
- **The corpus is fully materialized in memory.** `Benchmark` holds its corpus
  as a `Vec<Document>` and exposes only `corpus() -> &[Document]`; there is no
  streaming path. This is adequate for the datasets M2 targets, which are
  small enough to hold in memory whole. It would not be adequate for a corpus
  the size of MS MARCO, which runs to many gigabytes. Adding a streaming
  ingestion path later would change the shape of `Benchmark` and the contract
  the harness is written against — that is a decision issue when it is
  needed, not a change to make quietly inside this crate.

## What is deliberately not here

- CRAG, MultiHop-RAG and any end-to-end adapter carrying reference answers:
  they need generation and a judge, and belong to M3+ (ADR-10).
- Metric computation (`ragondin-metrics`) and engine execution
  (`ragondin-harness`).
