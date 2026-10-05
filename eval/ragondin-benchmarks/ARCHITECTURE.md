# ARCHITECTURE — ragondin-benchmarks

## What lives here

The `BenchmarkAdapter` trait, the internal benchmark structure it produces
(`Benchmark` = corpus + queries + `Qrels` + `ReferenceAnswers`), and one
adapter per external dataset format: `BeirAdapter` and `SquadAdapter`.

And, in `src/identity.rs`, the **identity of a dataset and of the chunk set
derived from it**: `dataset_version` over a loaded `Benchmark`, `CorpusIndex`
— the one-chunk-per-document derivation — and `index_version` over a chunk
set, with the length-prefixed `Encoder` all three digests are written through.
§ *The identity of a dataset and of a derived chunk set* below says why they
are here.

And the **datasets directory**: in `src/manifest.rs`, the manifest of the
datasets this build can obtain, each pinned by digest; in `src/datasets.rs`,
the digest-verified download that puts one on disk, the verification of what
a directory holds against the identity a run carries, and the import of a
local corpus. § *The benchmark manifest* and § *Putting a dataset on disk*
below say how.

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
- **ADR-C30 § 5's refusal is defined on the enum, once.**
  `CarriedPieces::scores_answers` says whether reference answers are carried,
  and `CarriedPieces::scorable(ends_in_answer)` holds the conjunction — a
  benchmark carrying them needs a pipeline ending in an answer. The harness
  calls it for its `NoAnswer` refusal and `ragondin-api` for its submission
  check; the API may not reach the harness (INV-12), and both already depend
  on this crate. It takes a `bool` rather than a `ValueKind` so that this
  crate stays off `ragondin-pipeline`: each caller answers "ends in an
  answer" from what it holds — a query's trace, a cut node's produced kind.
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
  `BenchmarkError::NoReferenceAnswer`, naming the file and the question id,
  with no line: a JSON document read whole has none to give.
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
- **A line with an empty `answers` list is `NoReferenceAnswer`**, naming the
  file, the line and the id — our choice: a query with nothing to say has no
  line, so a line that lists nothing is a corrupt one, the same strictness
  `SquadAdapter` applies. A line with no `answers` key is `MalformedJson`.
  The variant's line is an `Option`, not a second variant: the fault is the
  same on both paths, and only the file's shape decides whether a line exists.
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
it discards a real judgment *and*, because `judged_in_split` filters queries by
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
state: judgments loaded, every query filtered out by `judged_in_split`, and the
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

## The identity of a dataset and of a derived chunk set

Two of the five pieces of `run_id` (`docs/system-architecture.md` §7.1) are
defined here, together with the derivation the second is taken over:

- **`identity::dataset_version(&Benchmark)`** — a digest over the loaded
  corpus, queries, qrels, and the reference answers when the benchmark carries
  them, under the domain `ragondin/dataset-version/v1`. The reference answers
  are a tagged section present only when carried (ADR-C30 § 5), so a
  qrels-only benchmark digests as it did before they existed.
- **`identity::CorpusIndex::build(&[Document])`** — the chunk set a run
  retrieves over: one chunk per document, carrying its whole text under the
  document's id, in corpus order. Ad hoc and deliberately not a pipeline:
  question 5 of `docs/OPEN_QUESTIONS.md` is unresolved, and this derivation
  takes no position on it.
- **`identity::index_version(&[Chunk])`** — a digest over that chunk set,
  under the domain `ragondin/index-version/v1`; `CorpusIndex::version` is it.

**Why here.** ADR-C36 § 4 lets replay show a passage's text only when the
dataset on disk digests to the run's `dataset_version` and the chunk set
derived from it to the run's `index_version`, and it allows **one definition**
of each digest and of the chunk derivation, shared by the writer and the
reader. The writer is `ragondin-harness`, which reaches the engine; the reader
is the experiment plane's API, which ADR-C36 § 3 forbids the engine. This
crate is the one both reach: it already holds the `Benchmark` the dataset
digest is taken over, and its closure holds no engine and no component. A
second implementation of either digest, or of the derivation, anywhere else is
not allowed.

**The encoding is frozen by the stored runs.** Every byte of it enters
`run_id`, so a change here changes the id of every run already recorded
(INV-8). `tests/identity_golden.rs` pins the digests of this crate's three
fixtures as literals — the values the harness computed before the definitions
moved — and the recorded SciFact and NFCorpus snapshots still digest to the
`dataset_version`s `bin/ragondin/tests/calibration.rs` pins. What is digested,
in what order and under which domain tag is not this crate's to change in
passing.

**ADR-C26, read against this crate.** ADR-C26 describes `ragondin-harness` as
the place the chunk set is prepared and `CorpusIndex` as a harness type. Its
decision is unchanged — the composition root builds the `CorpusIndex`,
constructs its components from `CorpusIndex::chunks`, and hands the same value
to the harness — but the type and its derivation are now defined here, and
`ragondin-harness` re-exports `CorpusIndex` so the composition root's path to
it did not move. The ADR is immutable, so this note is the correction. The
same holds for ADR-C29, ADR-C30 and ADR-C31, which cite
`eval/ragondin-harness/src/corpus.rs` — its § One chunk per document, and
`CorpusIndex::version`'s caveat that it addresses *a* chunk set, not provably
the one retrieved from: that file no longer exists, both texts are now the
`CorpusIndex` documentation in `src/identity.rs`, and what they say is
unchanged.

Choices made here, inside this crate (`AGENTS.md` § Rules of engagement):

- **`Encoder` is public.** `run_id` stays in the harness — it digests
  `RunInputs`, the experiment plane's type, and only the harness holds all
  five pieces — but it is written through the same length-prefixed encoder
  under its own domain. Moving the encoder with the digests and exporting it
  keeps one definition of how a string and a count are written; a copy left in
  the harness would be a second encoder the three digests could drift apart
  through. It exposes `new`, `field`, `count` and `finish`, nothing more.
- **`CorpusIndex` moved whole**, not only its body. The derivation is its
  constructor, and a harness type wrapping a benchmarks function would leave
  the harness defining the derivation's entry point. A reader verifying a run
  calls `CorpusIndex::build(benchmark.corpus()).version()`, exactly what the
  composition root calls before the run.
- **`sha2` is a dependency**, already in `[workspace.dependencies]` and already
  used by the harness for the same digests; it moved with them.

## The benchmark manifest

`manifest::manifest()` lists every dataset this build can obtain — the list
the Setup screen shows as *available* (the design document § 6). It is Rust,
versioned with the crate, so a manifest and the binary that carries it cannot
disagree. Each `ManifestEntry` holds:

- `name`, the benchmark selector `<format>/<dir>` — `beir/scifact` — that
  `ragondin bench --benchmark` takes, and `format`, a `manifest::Format`
  naming the same three selectors (`beir`, `beir-qa`, `squad`);
- `files`: each file of the snapshot, with the path it takes under
  `<datasets>/<dir>`, its URL **at a fixed revision** (a commit, never a
  branch — a URL naming a branch reads whatever the branch holds today, a
  moving source), its SHA-256 and its size;
- `licence`, SPDX identifiers where the upstream states licences that have
  them, and `licence_url`, where the upstream states it, at a fixed revision;
- `dataset_version`: what the loaded snapshot digests to through
  `identity::dataset_version`, recorded when this repository loaded it. A
  download is checked against it, so "verified" means the identity a run over
  the dataset will carry, not only the bytes received.

`tests/manifest.rs` holds every entry complete, every name and directory
unique, every format one `bench` accepts, and every URL pinned to a commit.

**The entries, and why only these.** The manifest holds two, each checked for
a licence that permits the copy a user makes by downloading:

| Entry | Snapshot | Licence | `dataset_version` |
|---|---|---|---|
| `beir/scifact` | `mteb/scifact` on the Hugging Face hub at commit `cf10ab68…`: `corpus.jsonl`, `queries.jsonl`, `qrels/test.tsv`, `qrels/train.tsv` | `CC-BY-4.0 AND ODC-By-1.0` — the claims under CC BY 4.0, the S2ORC abstracts under ODC-By 1.0 (`allenai/scifact`'s `LICENSE.md`) | `9a07f80c…` |
| `squad/dev` | `dev-v1.1.json` from `rajpurkar/SQuAD-explorer` at commit `240e165a…` | `CC-BY-SA-4.0`, as the SQuAD site states it | `e4e3b760…` |

The SciFact snapshot digests to `9a07f80c…`, the `dataset_version` of the
original BEIR `scifact.zip` that `bin/ragondin/tests/calibration.rs` pins: its
JSON is re-serialized — the file digests differ from the zip's — but the
loaded benchmark is the same one, so a run over a downloaded SciFact is
comparable with the calibration.

**`mteb/scifact` is a third-party mirror**, maintained by the MTEB project,
not by SciFact's authors. Its card states no licence (`unknown`); the licence
recorded is the one `allenai/scifact` states for the data, which a mirror
inherits and cannot change. What makes the mirror trustworthy is not who
hosts it but the manifest: every file is pinned by SHA-256 and the loaded
snapshot by `dataset_version`, so a mirror that changed a byte would be
refused, not believed.

Left out, and why — each a choice made here, recorded so a later reader can
disagree with it:

- **`beir/nfcorpus`**: its terms of use say it is "free to use for academic
  purposes" and send any other use to the NutritionFacts.org author; they say
  nothing about redistribution. Unclear, so it is not offered.
- **`beir/fiqa`**: no licence is stated upstream, and the hub mirrors mark it
  `unknown`. Unclear.
- **`beir/trec-covid`**: its corpus is CORD-19, whose papers each carry their
  own licence, `no-cc` and `unk` among them. Unclear for the corpus as a whole.
- **Every `beir-qa/` entry**: no `answers.jsonl` snapshot is published for any
  BEIR dataset. The only reference answers in this repository are the
  hand-written ones of the M2 exit-criterion fixture
  (`bin/ragondin/tests/exit_criterion_generation.rs`), which are a fixture, not
  a published snapshot. None is invented here.

**A snapshot is a list of files, not one archive.** The design document § 6
gives an entry one URL and one SHA-256, and BEIR publishes each dataset as a
zip. That zip is served from an unversioned bucket — its URL carries no
revision, which is why the calibration records its hash — and extracting a zip
needs an archive reader, a new `[workspace.dependencies]` entry that ADR-C36
§ 6 says is a new decision. A dataset repository at a fixed commit gives what
the zip does not: a versioned URL, and files already in the layout the
adapters read, so nothing is extracted and nothing new enters the workspace.
The manifest pins each file's digest instead of an archive's; the design's
"URL, sha256" holds per file.

## Putting a dataset on disk

`src/datasets.rs`, over a datasets directory in which `<format>/<dir>` lives
at `<datasets>/<dir>`. Every verdict is a statement about digests, through
`identity::dataset_version` and no other computation.

- **`download(entry, datasets, fetcher, controls)`** fetches each file of the
  entry through `fetcher` into a staging directory of its own, and refuses the
  file unless it is exactly the manifest's size and SHA-256 — before anything
  is loaded or placed. It then loads the staged snapshot with the entry's
  format and refuses it unless it digests to the manifest's
  `dataset_version`. Only then is the staging directory renamed to
  `<datasets>/<dir>`. Each refusal is a `DownloadError` naming the entry and,
  for a size or a digest, the expected and the found value. `controls` carries
  the progress callback (bytes received of the snapshot's total, after every
  chunk), the cancellation flag and the deadline.
- **`verify(dir, format, expected)`** loads what a directory holds and
  compares its `dataset_version` with `expected`: `Verified` (with the
  `CarriedPieces` it carries), `Differs { expected, found, carries }`,
  `Unreadable { error }` with the adapter's own error, or `Absent`. It loads
  the dataset whole — adequate for the datasets in the manifest, which are
  held in memory anyway (§ *Local constraints*) — and caches nothing.
- **`import(datasets, name, source, manifest)`** registers a corpus that
  already carries its ground truth: a directory is read as BEIR (`beir-qa`
  when it holds `answers.jsonl`), a file as SQuAD v1.1. It loads the source and
  refuses it with the adapter's error when it does not load; copies the files
  the adapter read into a staging directory — the SQuAD file under the name
  `SquadAdapter::new` reads; loads the copy and refuses it unless it digests
  as the source did (`ImportError::Changed`, a source that changed while it
  was read); and publishes it as `<datasets>/<name>` beside
  `ragondin-local.json` (`LOCAL_MARKER`), which records the format and the
  `dataset_version`. `local_entries` lists them, one `Result` per directory:
  a marker this build cannot read is that directory's `MarkerError`, not the
  listing's failure. Generating questions or judgments for a bare corpus is
  not import.

**The transport is handed in.** This crate does not speak HTTP: `download`
takes a `Fetcher`, which fetches one URL and feeds a `Body` — `announce` for a
length known up front, `write` per chunk — and the body applies every rule.
The experiment plane's API supplies the HTTP fetcher (`reqwest`, in
`ragondin-api`), and the tests an in-memory one. The reason is the lean build
(ADR-C14): `ragondin-harness` and the binary depend on this crate and never
download, and an HTTP client here would put an HTTP and TLS stack —
`rustls`, `ring` and its C build — into both. With the transport outside,
their dependency closures are what they were before the download existed.

Choices made here, inside this crate:

- **A failure leaves nothing behind, however many attempts overlap.** A
  download or an import is assembled in a staging directory named
  `.<dir>.download-<process>-<attempt>` (`.import-` for an import), unique to
  the attempt, and renamed into place only once every check passed; a drop
  guard removes it on any error. Two downloads of one entry therefore never
  share a directory: both verify their own copy, the first rename wins, and
  the second — a rename onto a directory that exists — is
  `DownloadError::Occupied`, leaving the winner's verified copy in place. So a
  dataset's directory exists only when what it holds verified (P4), and
  `local_entries` skips `.`-named entries. `sweep_staging` removes what an
  interrupted attempt left; it is called at startup, not per attempt, since a
  running attempt's staging directory has the same shape.
- **A download never overwrites.** A destination that exists is
  `DownloadError::Occupied`: what is there may be a user's, and removing it on
  a failed download would lose it.
- **The manifest's size is a cap.** A length announced beyond it is refused
  before a byte is written (`TooLarge`); a body that runs past it is cut off
  at the chunk that passes it (`TooLarge`); a body that ends short is
  `Truncated`. A server cannot fill the disk past what the manifest pins, and
  a transport that ignores the refusal and reports success is still refused:
  the body records why it stopped, and that reason wins.
- **Cancellation and a deadline.** `Controls::cancelled` is an `AtomicBool`,
  checked after every chunk and before each file — the shape the harness's
  cancellation took — and a cancelled download is `Cancelled` with nothing
  left. `Controls::deadline` bounds the whole download from its start;
  `Controls::deadline_for` gives a minute's grace plus the entry's size at
  32 KiB/s, a minimum mean throughput, so a server that trickles bytes fails
  at the deadline (`DeadlineExceeded`) rather than holding the download. Both
  are checked when a chunk arrives: a server that sends nothing at all is the
  transport's to time out, and the HTTP fetcher does, per read.
- **Synchronous.** `download` blocks on the fetcher, as `BenchmarkAdapter::load`
  blocks on the disk, and the caller decides where the blocking happens — the
  experiment plane's API runs it on a blocking thread.
- **Import names are an allow-list**: `[A-Za-z0-9_-][A-Za-z0-9._-]*`, at most
  64 bytes, not ending in `.` and not a Windows device name (`CON`, `PRN`,
  `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, in any case, with or without an
  extension) — one directory name on every platform, never a staging
  directory's, and never a byte such as NUL that a filesystem call would
  reject after the name was accepted. A name is also refused when a dataset
  already has it, or when the manifest the caller passes names an entry with
  that directory.

## Local constraints

- **I/O here is correct.** INV-3 (value types only, no I/O) names
  `ragondin-types` and `ragondin-pipeline`; this crate is not covered by it.
  Reading dataset files from disk is this crate's job. The `Benchmark` it
  produces is still plain data.
- **Keep it light (INV-4 in spirit).** A JSON reader (`serde_json`), a TSV
  reader (`csv`) and the SHA-256 the identity digests and the download check
  need (`sha2`) are the whole toolkit. No heavy backend, no vector store, and
  no HTTP client: the download's transport is handed in (§ *Putting a dataset
  on disk*).
- **No fetch during evaluation; a pinned, verified download to put a snapshot
  on disk.** A published score is attached to a specific snapshot, and
  benchmarking against a live source is not reproducible
  (`docs/system-architecture.md` § 9.1). So an adapter reads only a path on
  disk, and nothing that loads a benchmark for a run touches the network. The
  one download this crate governs is `datasets::download`, which takes a
  snapshot the manifest pins by digest, refuses bytes whose size or digest
  differs, and leaves on disk only a snapshot that verified — the way a frozen
  snapshot gets onto the disk, not a way around it.
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

- CRAG, MultiHop-RAG and any adapter needing a judge. ADR-C30 rejects CRAG
  as the first QA benchmark and leaves MultiHop-RAG to follow a chunker.
  Reference answers here are labels, and ADR-10 puts label-based metrics
  beneath any judge.
- Metric computation (`ragondin-metrics`) and engine execution
  (`ragondin-harness`).
