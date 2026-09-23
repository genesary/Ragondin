---
id: ADR-C30
title: The generation evaluation regime — exact match and F1 after the SQuAD script, SQuAD v1.1 dev as the first QA benchmark, and the ranking retrieval metrics read once a pipeline ends in an answer
status: accepted
invariants: [INV-1]
supersedes: []
superseded_by: null
---

# ADR-C30: The generation evaluation regime — exact match and F1 after the SQuAD script, SQuAD v1.1 dev as the first QA benchmark, and the ranking retrieval metrics read once a pipeline ends in an answer

## Context

M3's exit criterion, as its milestone states it, is that "an end-to-end pipeline
is benchmarked against reference answers". ADR-C29 contracted the chain that produces an answer — a `ContextBuilder` and a
`Generator`, their nodes, their kinds and what the trace names — and left one
question to this decision by name: which node's output the retrieval metrics
read once the terminal node is a generator. Two more come with it. What a
reference answer looks like is unsettled: `Benchmark` in
`eval/ragondin-benchmarks/src/benchmark.rs` says "The fourth piece, reference
answers, belongs to a later milestone and is deliberately absent", and gives it
no shape — one answer, or several accepted spellings. And which metric scores
it is unsettled too. None of the three can be answered apart from the others. The benchmark constrains the metric, since a dataset's references
are written for the scorer it was published with, and both constrain the
harness, which has to know where a ranking is and what a reference is before it
can score either.

**The metric is a decision, not a free choice, and ADR-10 says so.** ADR-10
fixes `trec_eval` as "the reference implementation, not a source of
inspiration" for retrieval, and closes the door on improvisation: "A future
metric that has no `trec_eval` counterpart is a new decision, not a free
choice." No generation metric has a `trec_eval` counterpart. Exact match and
token-F1 are defined by the reading-comprehension literature, and in that
literature by one script — the official SQuAD evaluation script — whose choices
(what counts as punctuation, which articles are removed, how several references
combine) change the number a given set of answers scores. `docs/system-architecture.md` § 9.2
says the platform borrows metric definitions rather than reinventing them; RAGAS,
ARES and RAGChecker, the three it names, define neither metric.

**ADR-10's reproduction obligation is discharged, and does not recur per
benchmark.** ADR-10 earns credibility "by reproduction, not by implementation",
and calls that reproduction "a one-time validation of the pipeline". It has
happened. `bin/ragondin/ARCHITECTURE.md` § Calibration against a published
leaderboard records the dense-only SciFact run at nDCG@10 0.6450816521455768
against the 0.64508 MTEB publishes for the same model and revision, and
NFCorpus, the graded case, followed at 0.31667 against 0.31594; both are frozen
query by query under `eval/ragondin-metrics/tests/`. What remains to establish
for M3 is narrower: that the *new read path* — a ranking taken from inside a
pipeline that ends in an answer — lands on the same numbers as the old one, and
that the generation metrics agree with their reference implementation. Neither
needs a second published leaderboard, and this decision does not go looking for
one.

**A published generation figure cannot be reproduced the way a retrieval figure
can.** It depends on the model that produced the answers, and the platform
calls that model over the network and does not control it
(`docs/system-architecture.md` § 10). Sampling, batching and the serving stack
all move the output; ADR-15 says an LLM served with dynamic batching "is **not
deterministic even at a fixed seed**", and promises traceability and
statistical reproducibility instead. So the shape of ADR-10's guarantee —
reproduce a figure, freeze the run — transfers to generation only in part: the
metric can be frozen against its script exactly, and a run's answers can be
frozen as text, but a rerun's answers can only be held to a tolerance.

**The benchmark has to satisfy five constraints at once.** It must carry
reference answers and qrels both, so that ADR-8's regime with both pieces
exists; its corpus must fit an exact in-memory search (ADR-10 forbids an ANN
index in a calibration); its documents must fit the encoder, because the
workspace has no chunker — `eval/ragondin-harness/src/corpus.rs` § One chunk per
document makes each document one chunk carrying its whole text, and the M2
encoder truncates at 256 word pieces (`max_sequence_length: 256` in the
calibration configurations); its scorer must be deterministic, since the judge
is M4's instrument; and its licence must allow the data to be used and a frozen
fixture of it to be committed. A second, independent review checked the
candidates against their data rather than their papers, and its findings are
what § 2 of the Decision rests on.

**The ranking the retrieval metrics read has to be named, because the harness
only knows one place to look.** `evaluate` in
`eval/ragondin-harness/src/evaluate.rs` scores `ranked_documents(&output)`,
where `output` is the executor's return value — the terminal node's chunks. The
executor's module documentation says the M2 `Output` "is that node's
`Vec<ScoredChunk>`". Once the terminal node is a generator, the pipeline's
output is an answer, and nDCG needs a ranking from somewhere else in the trace.
ADR-C28 made every node's output entry name its chunks in rank order, so the
ranking is present; ADR-C29 gave the context builder a typed `Context` whose
`chunks` are a second candidate; which of them the metrics read changes what a
run's nDCG *means*, and ADR-C29 declined to answer it.

**Identity has to follow.** `dataset_version` in
`eval/ragondin-harness/src/identity.rs` digests the corpus, the query set and
every judgment, and nothing else. ADR-C29 records that the harness must add
reference answers to it, since two benchmarks differing only in their references
would otherwise share one identity; how it does so without moving the digests
`bin/ragondin/tests/calibration.rs` pins (`RECORDED_DATASET`,
`RECORDED_NFCORPUS_DATASET`) is part of this decision.

The first question is one ADR-10 reserves to a decision; the second decides a
fixture that enters the tree permanently; the third changes what a run's metrics
mean. Deciding any of them inside an implementation issue is the pattern ADR-10's
own Amendments section records as the thing not to repeat. Decided in #252.

## Decision

**Generation is scored by exact match and token-F1 as the official SQuAD v1.1
script defines them; SQuAD v1.1 dev, read as a retrieval corpus, is the first QA
benchmark; and once a pipeline ends in an answer, the retrieval metrics read the
ranking that fed its context builder, found by port position.**

### 1. The metrics and their reference implementation

**This ADR is the new decision ADR-10 requires for a metric with no `trec_eval`
counterpart.** It extends ADR-10 to generation the way ADR-10's 2026-09-05
amendment extended it to `trec_eval`, and it reopens nothing (process rule 4 of `docs/adr/README.md`):
retrieval keeps its reference, its conventions and its calibration. It is a
separate ADR, and never a second in-place edit of ADR-10, because ADR-10's
Amendments section says that if its added paragraphs "need to change again,
supersede this ADR rather than amend it a second time". This ADR changes none of
ADR-10's paragraphs; it adds a decision beside them, and so neither amends nor
supersedes it. ADR-10's reproduction of a published figure was "a one-time
validation of the pipeline", and it is **discharged** (see the Context): the
obligation is not re-triggered by a new benchmark.

**The reference implementation is the official SQuAD v1.1 evaluation script**,
pinned as:

- URL: `https://worksheets.codalab.org/rest/bundles/0xbcd57bee090b421c982906709c8c27e1/contents/blob/`
  — the bundle the SQuAD leaderboard's own v1.1 configuration names as
  `evaluate-v1.1.py`;
- SHA-256: `f5a673dbbd173e29e9ea38f1b2091d883583b77b3a4c17144b223fb0f2f9bd09`.

It plays for generation the role `trec_eval` plays for retrieval: **where a
definition admits alternatives, the script's binds**, and `ragondin-metrics`
implements what the script does, not what a paper describes. The script runs
under Python 3 as the reference, where its regular expression and its string
methods are Unicode-aware; that is the behaviour below. Concretely, from its
`normalize_answer`, `exact_match_score`, `f1_score` and
`metric_max_over_ground_truths`:

- **Normalisation**, applied to prediction and reference alike, in this order:
  lower-case (`str.lower`); remove every character of `string.punctuation` —
  the ASCII punctuation set and nothing else, so curly quotes, typographic
  dashes and other non-ASCII punctuation survive; replace the articles *a*,
  *an* and *the* by a space where they stand as whole words, the boundary being
  the regular expression `\b`, which is Unicode-aware; collapse whitespace by
  splitting with `str.split()` and joining on one space, which splits on the
  non-breaking space as on any other whitespace. Punctuation goes **before**
  articles, so a hyphen is removed rather than replaced — "Bankman-Fried"
  normalises to "bankmanfried", and "the-man" to "theman", article kept.
- **Exact match** of a prediction against one reference is the equality of the
  two normalised strings.
- **Token-F1** against one reference is the F1 over the multiset of the
  whitespace-separated tokens of the two normalised strings: the overlap is the
  multiset intersection, precision divides it by the prediction's token count,
  recall by the reference's, and an overlap of zero scores 0.
- **Over several references, each metric takes the maximum** of its per-reference
  scores, independently: the reference that maximises exact match need not be
  the one that maximises F1.
- **Where both sides normalise to empty, v1.1's behaviour binds: exact match 1,
  F1 0** — equal empty strings, and an overlap of zero. This is not a
  hypothetical: three questions of the dev set carry the reference ".", which
  normalises to empty. SQuAD v2.0's script scores the same case F1 1; it is
  rejected below.

**Scale: every value lies in [0, 1]**, as every metric in `ragondin-metrics`
does. The script reports its means ×100; the parity fixture divides by 100
before it compares.

**No extraction step.** The metric scores `Answer.text` (ADR-C29) exactly as the
generator returned it. Producing a short answer rather than a sentence is the
prompt template's job — constructor configuration of the generator under
ADR-C29 — and never the metric's: a metric that pulled a span out of an answer
before scoring it would be scoring its own extractor.

**A query with no reference answer is unjudged.** The script is undefined over
an empty reference list — `metric_max_over_ground_truths` takes `max` of an
empty list, which raises — so this ADR defines the case: such a query is
executed and not scored for exact match or F1. That is `trec_eval`'s rule for a
query with no qrels, which `evaluate` already applies to retrieval, applied to
each piece separately. An **empty prediction** is not a missing reference and is
scored like any other: against a reference that does not normalise to empty it
scores 0 on both metrics.

**The parity fixture** lives under `eval/ragondin-metrics/tests/fixtures/` and is
regenerated by a recorded script that calls the pinned SQuAD script, as
`pytrec_eval_parity.tsv` is regenerated by one that calls `pytrec_eval`. It
covers at least:

- ASCII punctuation beside non-ASCII punctuation (’ “ —);
- a leading article;
- an article directly before a non-ASCII quote ("the’s");
- a non-breaking space;
- a hyphenated name;
- a Greek capital sigma in final position, under lower-casing;
- an accented word adjacent to an article;
- an empty prediction;
- several references, where the maximum is taken;
- prediction and reference both normalising to empty.

Tolerance: **exact match exactly; F1 within 1e-12**.

**The metric names in `Run.metrics` are `exact_match` and `token_f1`** — not
`f1`, which beside precision and recall would be ambiguous. The normalisation is
English-specific, since its articles are English ones, and `ragondin-metrics`
says so where it defines the two functions.

### 2. The benchmark: SQuAD v1.1 dev as a retrieval corpus

**SQuAD v1.1 dev is read as a retrieval benchmark with reference answers**,
pinned as:

- URL: `https://rajpurkar.github.io/SQuAD-explorer/dataset/dev-v1.1.json`;
- SHA-256: `95aa6a52d5d6a735563366753ca50492a658031da74f301ac5238b03966972c9`
  (4 854 279 bytes, `"version": "1.1"`);
- counts, taken from that file: **48 articles, 2 067 paragraphs, 10 570
  questions**, every question id distinct, every article title distinct and
  none containing `#`; answers per question: 1 answer for 3 questions, 2 for
  136, 3 for 8 490, 4 for 759, 5 for 1 147 and 6 for 35 — so no question lacks
  one.

The four pieces of ADR-8's quadruple come out of it as follows:

- **The corpus is the paragraphs.** Each paragraph is one `Document`; its
  `DocId` is `<article title>#<paragraph index within the article>`, counted
  from 0 in file order; its text is the paragraph; its `metadata` carries the
  article title.
- **The queries are the questions**; a `QueryId` is the dataset's question id.
- **The qrels**: for each question, the paragraph it was asked over, at grade 1.
  Binary, one relevant document per question.
- **The reference answers** are the `answers[].text` strings of a question, in
  file order and with repeats kept, as a `Vec<String>` per query — the shape
  that lets the maximum over references be exercised, since most dev questions
  carry three.

**The adapter is named `--benchmark squad/<dir>`** and reads `dev-v1.1.json` in
that directory. The file it reads is selectable — the train file exists, is
about six times larger (30 288 272 bytes), and is not M3's — and the adapter applies no split
beyond the choice of file. **A question with no `answers` is an adapter error**,
a `BenchmarkError`, never a silently unjudged query: in this dataset every
question has one, so a missing list means a corrupt or wrong file.

**Why SQuAD dev, and why now.** Its paragraphs are short enough for the
encoder: median 111 words, and by a re-count with the M2 encoder's WordPiece
vocabulary about 92 % of paragraphs (1 899 of 2 067) fall within 256 word
pieces; only 1.5 % of questions have their first reference answer beginning
after word 180. The dense leg therefore mostly measures retrieval rather
than truncation. Its official script *is* the metric (§ 1), so nothing stands between
the benchmark's own scorer and ours. It carries several references per question,
the case the maximum exists for. Its qrels follow from the file, with no
string-matching heuristic to decide which document is evidence. It needs no
chunker, and it needs nothing from `Document.metadata` inside the embedded text.

Its costs, named plainly:

- **It is extractive.** Every reference is a span of its paragraph, so a
  generator that copies the right span is correct; it is a weaker test of
  generation than an abstractive benchmark, and a strong one of whether the
  right paragraph reached the prompt.
- **There is no published document-level retrieval figure to reproduce.** None
  is owed (§ 1): the reproduction obligation is discharged, and the new read
  path is checked against SciFact instead (§ 4).
- **Its licence is CC BY-SA 4.0**, as the SQuAD site states. The data stays
  outside the tree, as SciFact and NFCorpus do, under the directory
  `RAGONDIN_CALIBRATION_DATASETS` names; and **any frozen fixture that carries
  reference-answer text carries the licence notice beside it**, in the same
  directory.

**MultiHop-RAG is the benchmark that follows a chunker, not this one.** The
second review counted it at Hugging Face revision
`71ac0d0bd1f951d2d6b70311f7d2ae404e1ffa82` of `yixuantt/MultiHopRAG`, and what it
found rules it out for M3 on three independent grounds:

- **Its documents do not fit the encoder.** 609 articles with a median body of
  1 298 words, and 76 % of its 6 084 evidence facts begin after word 180 —
  beyond the M2 encoder's window, so a document-level dense leg never sees most
  of the evidence it is scored against.
- **Its published retrieval figures are chunk-level and not reproducible here
  even with a chunker.** They were produced with LlamaIndex's
  `SentenceSplitter`, with metadata prepended to the embedded text, with CLS
  pooling where this workspace mean-pools, with a hit decided by string
  containment, and with MAP@10 dividing by `min(|gold|, 10)` — the convention
  ADR-10 names and declines in favour of `trec_eval`'s.
- **Its answers do not score deterministically as written.** 94 % of its
  comparison and temporal answers are yes or no, with inconsistent casing and
  synonyms, so that a constant "Yes" scores 30.6 % exact match.

A chunker in the composition root would contradict
`eval/ragondin-harness/src/corpus.rs` § One chunk per document, which places a
real chunker in the pipeline, and it would change what `index_version` names.
That is a decision of its own, taken together with `docs/OPEN_QUESTIONS.md` #5,
and not this one.

**The other candidates are rejected** — CRAG, HotpotQA in its distractor
setting, Natural Questions and TriviaQA over Wikipedia — for the reasons under
*Alternatives rejected*.

### 3. Which ranking the retrieval metrics read once a pipeline ends in an answer

**The ranking is found by port position in the `LogicalPipeline`, never by
node name.** When the terminal node is a `Generator`:

1. its `inputs[1]` — the context port, `Fixed([Query, Context])` under ADR-C29 —
   names the node that produced its context, which must be a `ContextBuilder`;
2. that builder's `inputs[1]` — its chunks port, `Fixed([Query, Chunks])` —
   names the node that fed it chunks;
3. that node's **output entry in the trace**, a `ValueSummary::RankedChunks`
   under ADR-C28, is the ranking every retrieval metric of the run reads —
   nDCG, recall, precision, MRR and MAP alike, whichever of them the run
   reports — collapsed to documents as `ranked_documents` collapses a ranking
   today.

`NodeTrace.inputs` cannot serve in place of step 3: a node's input entry holds a
count, not a ranking (ADR-C28).

**A pipeline whose terminal node produces chunks keeps today's rule**: the
metrics read the terminal node's own output.

**The walk is unambiguous in M3**, and each fact that makes it so is in the tree
today: a plan has exactly one terminal node (`terminal_node` in
`engine/ragondin-engine/src/execute.rs` refuses zero or several); there is no
control flow — `LogicalNode` has no `Branch` or `Loop` variant; physical
planning refuses every `Extension` node (`PlanError::ExtensionUnsupported`, in
`engine/ragondin-engine/src/plan.rs`); and a node that fails stops the query,
which `evaluate` turns into `HarnessError::Execute`. Every node of a scored
query has therefore run, and the entry the walk looks for exists.

**When the walk fails, and the benchmark carries qrels, the harness returns a
typed `HarnessError` of its own, distinct from `NothingToScore`.** The walk
fails when the generator's context comes from a node that is not a
`ContextBuilder`, or when the builder's chunks port is fed by a node whose trace
entry is not a `RankedChunks`. It **never falls back to `Context.chunks`**: that
list is cut to the builder's budget, so an nDCG over it would mean one thing
under a builder that keeps three chunks and another under one that keeps
twenty. `Context.chunks` stays available as a second ranking — what entered the
prompt, beside what retrieval offered — for a future per-node comparison; it is
not the metrics' input. A benchmark with no qrels needs no ranking, and the
walk is not attempted.

**A ranking node that did not run** — possible once a `Branch` exists, in M5 —
is an open item, decided with `Branch`, not here.

### 4. What is reproduced, and what is frozen

**The new read path is reproduced against a published figure, cheaply.** SciFact
dense-only, run as `dense → context_builder → stub generator` over a benchmark
with qrels only, must land under § 3 on the dense-only nDCG@10
`bin/ragondin/tests/calibration.rs` records (`RECORDED_DENSE`,
0.6450816521455768, within `RECORDED_TOLERANCE`) and therefore on the published
0.64508 within `PUBLISHED_TOLERANCE` — the same number the retrieval-only
pipeline produces, because § 3 reads the same node. This is the first
assertion of the M3 calibration (#269), and it costs about the dense leg's two
minutes.

**The adapter is cross-checked against its file, not against a second
implementation.** The SQuAD adapter's tests (#263) check the qrels and the
reference answers it builds against `dev-v1.1.json` by count — the counts in
§ 2 — and by a sample of question ids.

**Frozen, and run in CI with neither the dataset nor a model**, under
`eval/ragondin-metrics/tests/` as the SciFact fixture is: the SQuAD calibration
run's per-query rankings, **exactly**; its answers, **exactly as text**; and
their exact match and F1, re-scored from those frozen answers and references,
**exactly**. The reference-answer text in that fixture carries the CC BY-SA 4.0
notice beside it.

**A rerun of the calibration is held to a tolerance, not to equality**, because
ADR-15 promises statistical reproducibility and never determinism: rerun with
the same model, temperature zero and a fixed seed, the mean `exact_match` and
the mean `token_f1` land within **0.01 absolute** of the recorded means. The
count of answers whose text changed is **recorded, not bounded**. The rerun's
retrieval metrics match the recorded ones **exactly**, because the retrieval
side is deterministic.

**The calibration's generation leg may run over a documented, deterministic
subset** of the dev questions — the first N in file order, N fixed and recorded
by #269 — because 10 570 generations through a real model are a budget the
retrieval leg does not have. The retrieval leg runs over the whole set.

**The M3 exit criterion (#268) reuses the M2 curated fixture**,
`bin/ragondin/tests/fixtures/exit-criterion/`, with reference answers written by
hand, and no SQuAD subset. The gap that criterion demonstrates is there by
construction, and nothing licensed enters the tree for it.

### 5. Identity and aggregation

**Reference answers enter `dataset_version` — only when present, behind a tag
of their own.** Without them two benchmarks differing only in references share
one identity, and their runs collide. With them digested unconditionally, every
qrels-only benchmark's digest moves, including the two
`bin/ragondin/tests/calibration.rs` pins as `RECORDED_DATASET` and
`RECORDED_NFCORPUS_DATASET`. So a benchmark in which no query carries a
reference answer digests exactly as today, byte for byte; one in which any
query does appends, after the qrels, a section that opens with its own tag and
then carries the references in the length-prefixed encoding the rest of the
digest already uses.

**Each metric averages over its own judged set**: exact match and F1 over the
queries that carry a reference answer; the retrieval metrics over the queries
that carry qrels. A query may be in one set, both or neither, and is scored in
each it belongs to and no other. The regime — which families a run reports — is
ADR-8's, read from the pieces the benchmark carries, and `Benchmark` reports it
(#263). A family the regime reports whose judged set is empty is the case
`HarnessError::NothingToScore` exists for: a mean over nothing, not a zero.

**`Query` gains nothing (INV-1).** Any per-question attribute a future benchmark
needs — a question type, for per-type reporting — lives beside the benchmark's
pieces, not on the value type.

## Alternatives rejected

- **SQuAD v2.0's script.** Its normalisation is v1.1's, and its additions serve
  unanswerable questions, which no M3 benchmark has: it drops references that
  normalise to empty and, when none is left, expects the empty string as the
  only correct answer. It also scores the both-empty case F1 1 where v1.1 scores
  0. Adopting it would change how three dev questions score an empty or
  punctuation-only answer, and import machinery whose only input the benchmark
  never supplies.

- **The `squad` metric of the Hugging Face `evaluate` library.** A re-host of
  the v1.1 script, reformatted, reporting on the same 0–100 scale. Pinning a
  copy pins a copy's history as well as the script's; the original is
  available, is what the leaderboard runs, and is what this ADR pins.

- **An extraction step in the metric** — pulling a short span out of a long
  answer before scoring it. It would make the metric score its extractor, and
  two extractors would give two exact matches for one answer. Answer length is
  the prompt template's to control, and that template is recorded in the
  generator's identity (ADR-C29).

- **MultiHop-RAG in M3**, with or without a chunker in the composition root. Its
  evidence lies mostly beyond the encoder's window, its published retrieval
  figures rest on choices this workspace does not make, and its yes/no answers
  make exact match reward a constant (§ 2). A composition-root chunker would
  contradict where `corpus.rs` places chunking and change what `index_version`
  names — a decision of its own, with `docs/OPEN_QUESTIONS.md` #5.

- **CRAG.** Its official scoring — perfect, acceptable, missing, incorrect —
  presumes a judge, which is M4's instrument and which ADR-10 keeps out of the
  foundation; and its mock-API design has no counterpart in the pipeline
  representation.

- **HotpotQA, distractor setting.** Its official script scores exact match and
  F1, but each question comes with its own ten paragraphs: a corpus per
  question, which `Benchmark` — one corpus for every query — cannot model. Its
  fullwiki setting is Wikipedia's problem, below.

- **Natural Questions or TriviaQA, open-domain over Wikipedia.** The canonical
  setting, with published retrieval figures — over twenty-one million passages,
  which an exact in-memory search cannot hold. A subset would invalidate the
  figure that is the only reason to choose them.

- **Reading the ranking from `Context.chunks`.** It is what the answer was
  built from, which is its appeal; but it is cut to the builder's budget, so the
  same retrieval scores differently under two builders, and nDCG@10 over a
  context of three chunks is not the nDCG@10 of any leaderboard.

- **Reading the ranking from the last `Chunks`-kind node in execution order.**
  Simpler to find, and the same node as § 3 finds in every pipeline M3 builds — and
  a different one as soon as a graph has two retrieval branches: which of two
  ready branches runs last is decided by the canonical order of their ids, not
  by which of them fed the prompt.

- **Scoring generation only once a pipeline ends in an answer.** It loses the
  regime with both pieces that ADR-8 promises, and with it the comparison of
  retrieval quality against answer quality query by query, which is what M3
  exists to show.

- **Reproducing a published generation figure with the same open-weights
  model.** Sampling, batching and the serving stack all move it, and ADR-15
  says no seed pins them. A tolerance wide enough to pass across stacks is a
  tolerance that checks nothing.

- **Digesting reference answers into `dataset_version` unconditionally.**
  Simpler, and it changes every M2 digest the calibration pins, turning a
  benchmark that did not change into a different dataset.

## Consequences

- **The M3 evaluation issues are unblocked, and each owes this ADR something
  specific.** #263: the SQuAD adapter under `--benchmark squad/<dir>`, reference
  answers as a `Vec<String>` per query, the `BenchmarkError` for a question
  with no `answers`, the regime `Benchmark` reports, construction extended
  additively, as `Benchmark`'s documentation already provides for, and the count
  and sample cross-check. #264: `exact_match` and `token_f1` in
  `ragondin-metrics`, on the [0, 1] scale, with the parity fixture and every
  edge case § 1 lists, regenerated by a recorded script calling the pinned
  SQuAD script. #265: regime selection in the harness, the port walk of § 3 and
  its typed error, a separate denominator per metric family, and the
  `dataset_version` tag of § 5. #268: the M2 exit-criterion fixture with
  hand-written reference answers. #269: the SciFact assertion through the new
  read path, the SQuAD calibration, the subset rule, the tolerances of § 4, and
  the frozen fixture with its licence notice. None of them reopens this
  decision; each implements a named part of it.

- **The meaning of a run's retrieval metrics is fixed for pipelines that end in
  an answer**: they measure what retrieval handed the context builder, not what
  the builder kept. A reader comparing the nDCG@10 of a generating pipeline with
  a retrieval-only one compares the same thing.

- **English-specific normalisation is now part of the platform's contract**, and
  a benchmark in another language needs its own decision rather than a knob on
  this metric.

- **The calibration's data stays outside the tree**, as SciFact's does, and the
  licence travels with every committed fixture that quotes it.

- **What stays open.** The chunker, and with it MultiHop-RAG, together with
  `docs/OPEN_QUESTIONS.md` #5. Per-question-type reporting, which needs no
  `Query` field and takes a benchmark-side attribute when a benchmark needs it.
  `Branch`, and what § 3 does with a ranking node that did not run, in M5.
  Each is named so that the next reader can see it was weighed rather than
  missed.

- **The invariants.** INV-1 is untouched: no type in `ragondin-types`,
  `ragondin-pipeline` or `ragondin-contracts` changes for this decision —
  reference answers belong to `ragondin-benchmarks`, and `Query` stays as it is.
  ADR-8 is applied, not changed: the regime follows from the pieces present.
  ADR-10 is extended, not reopened. ADR-15 is applied to the rerun tolerance.
  ADR-C28 and ADR-C29 are what make § 3 a lookup rather than a computation:
  the trace names each node's ranking, and the two generation nodes fix which
  port is which.

- No entry in `docs/OPEN_QUESTIONS.md` is opened, closed, or changed.

## Status

Accepted.
