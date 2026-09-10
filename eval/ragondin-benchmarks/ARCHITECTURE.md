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

## The qrels header is detected, not assumed — and ids are trimmed everywhere

Two rules in `read_qrels` look like fussy defensiveness and are not. Both were
written after the plain version was shown to lose data silently.

**The header row is identified by its content, not by its position.** A BEIR
qrels TSV normally opens with `query-id  corpus-id  score`, and reading that
as data invents a judgment for a query called `query-id` that no run will ever
answer, quietly lowering every mean. The obvious guard — always skip line one —
fails the other way round on the qrels files that ship without a header, where
it discards a real judgment *and*, because `read_queries` filters queries by
qrels, removes that query from the run entirely. So the first record is a
header, and is skipped, **only when its score field both fails to parse as a
`u8` and spells BEIR's own `score`**. Do not replace this with
`has_headers(true)`.

Both halves of that condition are load-bearing. Failing to parse is not on its
own evidence of a header: a judgment whose score is *corrupt* fails identically,
and treating it as a header dropped a real row without a word — and, via the
qrels filter, the query with it — while the same corruption on any later line
was a hard `MalformedRecord`. Corruption was tolerated on line 1 and nowhere
else, which is the "judgment lost, query filtered out, `Ok` returned" state the
guard below exists for, reached past the guard.

The spelling therefore decides whether the row is a header, while the three
columns are still read **by position** — across BEIR the order is fixed and only
the spelling varies. The cost is that a qrels file whose third column is spelled
something other than `score` is now a typed error rather than a silent skip.
That is the right direction to fail in: an operator can see and correct an
error, whereas the drop it replaces could only be found by noticing that a
published metric was wrong.

Detecting the header by content is why a **headerless three-column qrels file
loads while a four-column TREC-style one (`query-id iteration corpus-id score`)
is rejected**, which looks inconsistent and is not: the three-column file is
the BEIR shape with a row this reader can identify by inspection, whereas a
fourth column means the columns are *positional in a different order*, and
this reader takes columns by position. Reading it as BEIR would file the
iteration number as the corpus id and score every judgment against a document
that does not exist. Rejecting it names the mismatch; tolerating it would
produce a plausible benchmark computed from the wrong columns.

**Ids are trimmed on every side — the qrels TSV, `corpus.jsonl` and
`queries.jsonl`.** Trimming is all-or-nothing here. Trimming *one* side is
strictly worse than trimming none: a dataset whose ids carry the same
surrounding whitespace in every file matches itself when nothing is trimmed,
but with only the qrels side trimmed, `q1` no longer equals `q1 ` and every
affected query is filtered away. If you ever remove a `.trim()` here, remove
all of them.

## The repeated symptom has its own guard

Three separate parsing bugs on this reader — a swallowed headerless first row,
a one-sided trim, a byte welded onto the first id — all ended in the same
state: judgments loaded, every query filtered out by `read_queries`, and the
benchmark returned `Ok` with an empty query set. Nothing failed; the mismatch
between the two files surfaced later as a metric of zero, reported as a
number.

So `BeirAdapter::load` names that state directly. When the qrels are non-empty
and the query set comes back empty, it returns
`BenchmarkError::NoJudgedQuery { path, judged }`, whose message gives the qrels
path and the judgment count. The guard diagnoses *the symptom*, not any one of
its causes, which is the point: it is the fourth cause it exists for. Empty
qrels stays a normal load — a split with nothing judged is useless, not
corrupt — and a partial mismatch, where some queries still match, is invisible
to it. The guard is the last line, never the fix.

**The same guard exists on the document side**, and it is the harder of the two
to do without. `BenchmarkError::NoJudgedDocument` fires when the qrels are
non-empty and not one judgment names a document `corpus.jsonl` defines — the
shape a `qrels/` directory paired with a corpus from another snapshot produces,
or a mirror that prefixes or re-cases its ids. Without it the query set is
*full*, every query runs, and the report carries nDCG@10 = 0 over the whole
benchmark with nothing looking empty anywhere, which is strictly harder to
diagnose than the empty query set the first guard names. Both guards fire only
on a **total** miss: a benchmark may legitimately judge a document its corpus
does not hold, and `trec_eval` counts such a judgment in the denominator.
