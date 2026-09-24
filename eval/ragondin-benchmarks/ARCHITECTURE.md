# ARCHITECTURE — ragondin-benchmarks

## What lives here

The `BenchmarkAdapter` trait, the internal benchmark structure it produces
(`Benchmark` = corpus + queries + `Qrels` + `ReferenceAnswers`), and one
adapter per external dataset format: `BeirAdapter` and `SquadAdapter`.

## The internal structure is the stable thing

This crate is the **data-side mirror of the component contract**: one stable
interface, N implementations. `Benchmark` is that interface. Adding a dataset
format means adding an implementor of `BenchmarkAdapter`; it must never mean
changing `Benchmark`, because everything downstream — the harness, the metrics
call sites, the run store — is written against `Benchmark` and not against any
one dataset.

`Benchmark`'s fields are private and it is built through `Benchmark::new`.
That is deliberate: §5.3 models a benchmark as a **quadruple** — corpus +
queries + qrels + **reference answers** — and the fourth piece was added after
the first three without touching a construction site. `Benchmark::new` still
takes the retrieval triple and builds a benchmark with no reference answer;
`Benchmark::with_reference_answers` is the builder step that adds the fourth
piece. Every accessor and `Benchmark::iter` are unchanged;
`Benchmark::iter_with_references` is the loop that also yields each query's
references, as an empty slice for a query that has none.

## The regime is read off the value: `CarriedPieces`

ADR-8 says the pieces present determine the computable metrics, and ADR-C30
§ 5 defines *present* as **carried**: a piece is carried when at least one of
the benchmark's queries has a non-empty list for it — one judgment, grade `0`
included, for qrels; one string for reference answers. `Benchmark::carries`
returns that as a `CarriedPieces` value — `Neither`, `QrelsOnly`,
`ReferenceAnswersOnly`, `QrelsAndReferenceAnswers` — which the harness matches
on to pick the metric families. Choices made here, inside this crate:

- **An enum of the four combinations, not two booleans or a bit set.** A
  `match` over it is exhaustive, so a harness that forgets the generation-only
  case does not compile. It is deliberately not `#[non_exhaustive]`: the
  quadruple has two optional pieces, and a fifth case would be a new regime a
  caller must not absorb through a wildcard arm.
- **Judged over `Benchmark::queries`, not over the pieces as sets.** A
  judgment or a reference naming a query the benchmark does not hold is
  nothing a run can score, and the ADR's wording is "at least one of its
  queries".
- **An empty reference list is not stored.** `ReferenceAnswers::insert` with
  an empty `Vec` removes the query's entry, so `ReferenceAnswers::is_empty`,
  its counts and `carries` never disagree about a set holding only empty
  lists. Otherwise it replaces, last-wins, like `Qrels::insert`.

`ReferenceAnswers` holds a `Vec<String>` per query in dataset order, repeats
kept (ADR-C30 § 2): the metric takes the maximum over references, so order
changes no score, but a frozen fixture compares files byte for byte.

## The SQuAD adapter

`SquadAdapter` reads SQuAD v1.1 as ADR-C30 § 2 maps it: each paragraph a
`Document` with `DocId` `<article title>#<paragraph index from 0>`, its text
the `context`, `metadata["title"]` the article title; each question a `Query`
under its own id; the source paragraph as the one grade-1 judgment; and
`answers[].text` as the references. `SquadAdapter::new(root)` reads
`dev-v1.1.json`; `SquadAdapter::with_file(root, name)` reads another file,
shaped as `BeirAdapter::with_split` is. Choices made here:

- **Absent, `null` and empty `answers` are one error**,
  `BenchmarkError::NoReferenceAnswer`, naming the file and the question id.
  The ADR makes a question with no `answers` an adapter error; a key present
  with nothing in it says the same thing, and treating it as an unjudged
  query would shrink the generation family's judged set in silence.
- **A repeated question id, or a repeated derived `DocId`, is
  `BenchmarkError::DuplicateRecord`**, naming the file and the id — the
  single-document counterpart of `DuplicateId`, which carries a line a JSON
  document read whole cannot give. Two articles sharing a title collide on
  their paragraphs' ids, and a judgment would then name two documents.
- **The file is read whole, and everything is kept verbatim.** The format has
  no line structure to stream, and the dev file is under 5 MB. A malformed
  file is `MalformedJson` with the line `serde_json` reports. No id is trimmed:
  BEIR trims because one id must match itself across several files, and every
  SQuAD id is the file's own or derived inside it.
- `version` and `answer_start` are not read: no rule depends on the first, and
  an answer is scored as text, never located in the paragraph.

## BEIR's reference-answer path

`BeirAdapter::with_reference_answers()` selects a second reading path that
**requires** `answers.jsonl` in the dataset root — one line per query,
`{"_id": <query id>, "answers": [<strings>]}` — and loads it as the reference
answers (ADR-C30 § 2). Without it the file is never opened, so every M2 load,
and the `beir-mini` fixture, is unchanged. On that path:

- A line whose `_id` names no query of `queries.jsonl` is
  `BenchmarkError::UnknownQuery`, and an `_id` on two lines is `DuplicateId`;
  both name the file, the line and the id (the ADR's two errors).
- **A line with an empty `answers` list is `NoReferenceAnswer`** — our choice:
  a query with nothing to say has no line, so a line that lists nothing is a
  corrupt one, the same strictness `SquadAdapter` applies. A line with no
  `answers` key is `MalformedJson`.
- **`_id`s are checked against every query of `queries.jsonl`**, which spans
  all splits, and the query set is still the split's judged queries. A line
  about a query of another split names a real query and is accepted; only the
  references of the queries the benchmark holds are kept, so its pieces
  describe its own query set. `_id`s are trimmed, as on every other side of
  this reader.

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

> `Document.text` is **title, one space, then text, with the whole result
> trimmed** — so an empty, absent or whitespace-only title contributes neither
> itself nor a separator. The title is additionally kept under
> `metadata["title"]`, trimmed.

`combined_text` **mirrors the reference implementation's expression rather than
paraphrasing it**. BEIR concatenates in
`beir/retrieval/models/util.py::extract_corpus_sentences`:

```python
(doc["title"] + sep + doc["text"]).strip()   # sep defaults to " "
```

Join with one space unconditionally, then trim the whole thing. Do not
special-case the empty title and do not trim the parts: the separator the join
always contributes is exactly what the trim then removes, which is why no
branch is needed.

The difference is not cosmetic even though no tokenizer can see it. Trimming
the *title* first — the obvious reading of the rule as prose — collapses an
interior whitespace run that BEIR preserves, so `Document.text` would stop being
byte-identical to the string BEIR indexes and become merely equivalent.
`document_text_matches_the_string_beir_would_index` pins the padded case for
that reason. What compares against published figures is the leaderboard
calibration —
`bin/ragondin/ARCHITECTURE.md` § Calibration against a published leaderboard
— and a property that can be checked is worth more there than one that has to be
argued from tokenizer behaviour.

BEIR's own evaluation code indexes the concatenation, and **every published
BEIR leaderboard number is computed that way**. Indexing the text alone changes
nDCG@10 on most BEIR datasets, which would put that calibration out of reach,
and with it the M2 exit criterion — "hybrid retrieval beats dense-only on BEIR,
reproducibly" — as `bin/ragondin/tests/calibration.rs` measures it on the same
real corpus; `bin/ragondin/tests/exit_criterion.rs` asserts that criterion over
a fixture. This is not a matter of taste,
and it is the decision in this crate most likely to be undone by a well-meaning
simplification. `combined_text` is the single place it is implemented, and
`tests/beir_fixture.rs` pins it, including the empty-title and absent-title
cases.

`metadata["title"]` is written only when the title is non-empty: an empty title
is the absence of a title, and storing `""` would make the two
indistinguishable while preserving nothing. Emptiness is judged **after
trimming**, for the same reason ids are trimmed everywhere below: a title of
nothing but whitespace is semantically absent. This is our own decision, not a
mirrored one — BEIR has no notion of per-document metadata to preserve a title
in — which is why the raw title goes into the concatenation while the trimmed
one goes here.

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

## One `_id` names one record

A repeated `_id` in `corpus.jsonl` or `queries.jsonl` is
`BenchmarkError::DuplicateId`, naming the file, the id, and the line of the
second occurrence. Two records under one id disagree about what that id *is*,
so either choice is a guess — which is why this rejects rather than
deduplicating.

Note that this is the opposite resolution from a repeated `(query, document)`
pair in the qrels, which `Qrels::insert` resolves last-wins. The two are not
inconsistent: a repeated judgment *restates* a fact about a pair that exists
either way, while a repeated id asserts two different records. The cost of
getting the query side wrong is specific and doubled — a duplicated query id
both double-weights that query in a macro-average and inflates
`Benchmark::queries().len()`, which this crate documents as the correct
denominator of a mean over a run — so one duplicate moves the headline number
twice, in the same direction, in silence.

The same asymmetry principle governs the physical line numbers in
`BenchmarkError`: they are counted while reading rather than derived from the
`csv` reader's record positions, which cannot be mapped back to file lines once
blank lines are skipped or the file is CRLF. Three separate attempts at that
arithmetic each fixed one file shape and broke another.

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

- CRAG, MultiHop-RAG and any adapter needing a judge: ADR-C30 rejects CRAG
  as the first QA benchmark and leaves MultiHop-RAG to follow a chunker, and
  reference answers here are labels, and ADR-10 puts label-based metrics
  beneath any judge.
- Metric computation (`ragondin-metrics`) and engine execution
  (`ragondin-harness`).
