# ARCHITECTURE — ragondin-harness

## What lives here

The **evaluation driver**: `evaluate`, which takes a validated
`LogicalPipeline`, a ready `EngineContext` and a loaded `Benchmark`, runs the
pipeline once per query through `ragondin-engine`, scores every metric family
the benchmark's pieces allow — rankings against its qrels, answers against its
reference answers — and assembles the `Run` that names itself by the
content-addressed identity tuple.

One of the **two drivers** over the one engine (`docs/code-architecture.md` §9,
[ADR-4](../../docs/adr/ADR-004-one-engine-two-drivers.md)); the other is
`ragondin-server`. Both depend on the same `ragondin-engine`, which is what
makes evaluation/serving skew structurally impossible (P1) — and it is a
property of the *dependency graph*, not of anyone's discipline, so the rule
that protects it is simple: **no execution logic is written here**. Planning is
`plan_physical`; execution is `Engine::execute`. A retrieval, a ranking or a
merge performed in this crate would be a second execution path wearing the
harness's name, and would make every figure it reports unfaithful to
production.

## The crate receives a context; it never builds one

`evaluate` is handed an `EngineContext` that is already populated. It registers
nothing, constructs no component, and depends on no crate under `components/`
— the composition root is the binary (§8.1), and a driver that reached for a
concrete component would give built-ins a path a third-party crate has no
equivalent of (INV-7). `ragondin-stub` appears in `[dev-dependencies]` for the
same reason a test needs a fixture, and the end-to-end test wires it exactly as
a composition root would.

## The trace is a return value, and this crate is where it is rendered

`Engine::execute` returns `(Result<Output, ExecError>, ExecutionTrace)` — the
trace is the executor's return value, never a log (INV-10,
[ADR-C9](../../docs/adr/ADR-C09-traces-are-the-executors-return-value.md)). The
harness keeps every one of them, renders it into the `TraceDocument` the run
store holds, and files it under its query. Four consequences are deliberate:

- **The translation is hand-written** (`src/trace.rs`) and `ExecutionTrace` is
  not serialized by a derive. The engine is internal and not an API boundary
  (INV-2), so its trace type is meant to move; a derive would make every stored
  run a hostage of that shape. The harness is the crate that knows both the
  engine and the run store, so the translation lives here — field by field,
  into `ragondin-experiments`' `Trace`, the stored document's one typed
  definition (ADR-C36 § 2), whose `From<Trace> for TraceDocument` writes the
  JSON. The harness no longer spells the document itself: the JSON shapes
  below are `Trace`'s, and a reader parses them back through the same type.
  The unit tests in `src/trace.rs` keep asserting the JSON the hand-built
  renderer wrote, so a run stored before the typed shape reads back unchanged.
- **An output names its chunks, an input counts them** (ADR-C28). For each
  node, the rendered document carries the chunks the node produced in the order
  it produced them — chunk id, document id, score — and for each input port a
  count alone. That ordered list is where anything needing a per-query ranking
  reads it out of a stored run; `Run` gains no per-query field, and the store's
  file layout is untouched, because the document is opaque to it.
- **A context and an answer are named as outputs and sized as inputs**
  ([ADR-C31](../../docs/adr/ADR-C31-generation-contracts-template-and-served-model-per-call.md)
  § 5). A context a node produced renders as
  `{"context": {"chunks": [{"chunk", "document", "score"}, ...], "text": ...}}`
  — its chunks in the builder's order, each rendered as a ranked chunk is —
  and an answer a node produced as `{"answer": {"text": ...}}`. On an input
  port, a context renders as `{"context": {"count": ..., "text_bytes": ...}}`
  and an answer as `{"answer": {"text_bytes": ...}}`: its chunk count and the
  byte length of its text, the sizes the engine's trace records there.
- **A query that stops the run carries its trace out with the error**, whether
  it failed or was refused. `HarnessError::Execute` holds the rendered trace of
  the run that failed, and `HarnessError::NoAnswer` and
  `HarnessError::NoRanking` the trace of the run whose output they refused —
  the only record of what it produced — because that is the trace worth
  reading and an error that dropped it would discard exactly the evidence
  INV-10 exists to preserve.

### The observer is the second reader of a trace

`evaluate_observed` hands every rendered `TraceDocument` to a caller-supplied
observer, as a `QueryProgress` that also names the query, its position from 1,
the benchmark's total, and the time `Engine::execute` took around the call.
The run store is the first reader of a trace, once the run completes; the
observer is the second, while it runs. What it receives is the document the
run files under that query — rendered once, the same value — never a log line
and never a `tracing` event: progress is a return value in the same sense the
trace is (INV-10), and no `tracing` macro stands in for it here. The trace
itself carries nothing it did not carry before.

The loop's contract, which a caller that tracks jobs builds on:

- **The observer is called once per executed query, in benchmark order,
  immediately after the trace is rendered** — before the query is scored and
  before any error can return. A query whose execution fails, or whose output
  is refused (`NoAnswer`, `NoRanking`), reaches the observer before the error
  does, so the traces the observer holds are every trace the run produced,
  however the run stops. `HarnessError::Execute` still carries its trace: a
  caller with no observer keeps the evidence.
- **The observer is synchronous.** It runs on the loop, between one query and
  the next, so whatever it does is added to every query's turn: a caller that
  writes files or pushes progress from inside it holds the executor for that
  long. Do cheap work there, or send the `QueryProgress`'s contents over a
  channel to a consumer that does the rest.
- **The cancellation signal is read at the top of each iteration and nowhere
  else**: between two queries, never inside one. A query in flight runs to its
  end — a `Remote` call finishes or times out under its own rules — and is
  observed; a signal read set returns `HarnessError::Cancelled` naming how many
  queries ran, which is not a failure of the pipeline. A signal set before the
  first query runs nothing and reports 0; one set after the last query finds
  no boundary left and the run completes. A benchmark with no query never
  enters the loop, so it never reads the signal: it ends as it did before,
  in `HarnessError::NothingToScore`.
- **Queries stay sequential.** One worker is the caller's rule, and the loop
  does not grow a second.

`evaluate` is `evaluate_observed` with an observer that does nothing and a
signal nobody sets, so a caller that wants neither — `ragondin bench` — calls
it unchanged.

## The regime, and where each family reads its input

Which metric families a run reports follows the pieces the benchmark carries
(ADR-8, [ADR-C30](../../docs/adr/ADR-C30-generation-evaluation-regime-squad-metrics-and-benchmark.md)
§ 5), read once from `Benchmark::carries`: qrels score the retrieval metrics,
reference answers score `exact_match` and `token_f1`, and a benchmark carrying
both scores both. Each family averages over its own judged set — a query with
at least one judgment for the retrieval metrics, a query with a non-empty
reference list for the generation ones — so a query may be scored in one
family, both, or neither. A family the benchmark does not carry is absent from
`Run.metrics`, not zero.

- **The ranking** is read out of the trace, never out of the executor's
  output, by port position (ADR-C30 § 3): a terminal generator's context port
  names its context builder, the builder's chunks port names the node whose
  `RankedChunks` output entry is the ranking, and a terminal builder enters the
  walk at its own chunks port. A terminal node that produces chunks is its own
  ranking, which is today's rule and today's numbers. The walk never falls back
  to the context's own chunks, which are cut to the builder's budget; when it
  finds no ranking over a benchmark that carries qrels, the query is refused
  with `HarnessError::NoRanking`, naming where the walk stopped
  (`RankingWalkError`). **The walk and the fold are not defined here.** The
  walk is `ragondin-experiments`' `terminal` and `ranking_node`, and the
  chunk-to-document fold is `ragondin-metrics`' `documents_by_first_occurrence`:
  `ragondin-api` recomputes per-query and per-node figures from the stored
  traces and may not depend on this crate (INV-12), so the two rules live where
  both reach them, and both call them (ADR-C36's "one definition, used by the
  writer and the reader alike"). What stays here is reading a node's entry out
  of the engine's `ExecutionTrace` (`documents_at`), and `RankingWalkError`,
  which maps every stop of the shared `WalkError` to the variant of the same
  name and message and adds `NoRankedChunks`, a question about the trace
  rather than the pipeline's shape.
- **The answer** is read from the terminal node's output entry in the trace
  (ADR-C31 § 5), the `{"answer": {"text": ...}}` a stored run's `traces.json`
  holds — the place the per-query fixture reads it, so the text scored here and
  the text a re-scorer reads are one value. The text is scored exactly as the
  generator returned it; there is no extraction step (ADR-C30 § 1).
- **A benchmark carrying reference answers, run through a pipeline that
  produces no answer**, is refused with `HarnessError::NoAnswer`, never
  reported on its retrieval metrics alone. `NothingToScore` keeps its meaning:
  no family scored any query, which only a benchmark carrying neither piece
  reaches.

## Indexing is ad hoc, and that is not a position on open question 5

`CorpusIndex::build` prepares the corpus **directly**: one chunk per document,
in corpus order. No indexing graph, no node kind, no `ValueKind` — because
question 5 of `docs/OPEN_QUESTIONS.md` (*does indexing share the IR
formalism?*) is deliberately unresolved, and this crate does not resolve it.

**`CorpusIndex` is defined in `ragondin-benchmarks`, not here**
(`ragondin_benchmarks::identity`), and this crate re-exports it so that
`ragondin_harness::CorpusIndex` still names it. The derivation and the
`index_version` it names moved there with `dataset_version`, because a stored
run is verified against them by a reader that may not reach the engine, and
ADR-C36 § 4 allows one definition of each —
`eval/ragondin-benchmarks/ARCHITECTURE.md` § The identity of a dataset and of
a derived chunk set. ADR-C26 still describes this crate as the place the chunk
set is prepared; its decision (below) is untouched, and that crate's
`ARCHITECTURE.md` carries the correction, since the ADR is immutable.

What the derivation does **not** do is build a backend index, and the reason is a
property of the contracts rather than a choice: a BM25 index is built inside
`Bm25Retriever::new`, from the chunks it is given, and a `VectorStore` instance
is not reachable from an `EngineContext` at all — a context holds constructors,
and its resolution half is crate-private to `ragondin-engine`. So the backing
index is built where components are constructed, which is the composition root,
out of exactly `CorpusIndex::chunks()` — the set `CorpusIndex::version()`
content-addresses. Making a component instance reachable from a driver would be
a change to the engine's public surface, which `AGENTS.md` § Rules of
engagement escalates; it is not done here.

That escalation is **settled**, and it settled this way: **corpus ingestion is
the composition root's job** (ADR-C26). A driver was deliberately not given a
way to reach a constructed component, and no trait grew a post-construction
ingest method — both alternatives are argued and rejected there. What the
decision adds to what is written above is the other half of the exchange: the
composition root hands this crate the `CorpusIndex` it built its components
from, rather than the crate deriving one of its own, so that one value travels
where two could disagree. `Evaluation::index` is that parameter: `evaluate`
records its `index_version` and builds no `CorpusIndex` of its own. The caveat
`CorpusIndex::version` states still applies — the guarantee is structural, not
enforced, and nothing here stops a caller from passing an index that does not
match the components it registered.

## Run identity

`run_id = hash(pipeline_config, dataset_version, index_version, model_hashes,
engine_version)` (`docs/system-architecture.md` §7.1). `ragondin-experiments`
defines the record and states that the digest is assembled by the harness;
`src/identity.rs` is where `run_id` is taken. Two of its inputs are defined
elsewhere: `dataset_version`, `index_version` and the chunk derivation live in
`ragondin_benchmarks::identity` (ADR-C36 § 4), and so does the `Encoder` all
three digests are written through — `run_id` imports it back and writes its
tuple under its own domain, `ragondin/run-id/v1`. The encoder is a small
public helper of `ragondin-benchmarks`, exported for this one use; where it
lives is a leaf choice argued in `eval/ragondin-benchmarks/ARCHITECTURE.md`:
one encoder keeps the three digests from drifting apart in how a string or a
count is written. A literal-digest test in `src/identity.rs` pins the `run_id`
of fixed inputs, and `ragondin-benchmarks`' `tests/identity_golden.rs` pins the
other two, so a change in either crate that moves any stored run's id fails by
name.

What the encoding is (tagged, length-prefixed, never through `serde`), what
`dataset_version` is taken over (the loaded benchmark, reference answers as a
tagged section only when carried, ADR-C30 § 5) and what `index_version` is
taken over (the chunk set, not a backend artifact) are decided in
`ragondin_benchmarks::identity` and argued there — in its module
documentation and in `eval/ragondin-benchmarks/ARCHITECTURE.md` § The identity
of a dataset and of a derived chunk set — and are not restated here, so the two
files cannot drift; `run_id` inherits all three. The answer text a run
produces is in its trace, and the trace is not part of identity. One decision
is this crate's own:

- **`engine_version` is this crate's own `CARGO_PKG_VERSION`.** Every member
  takes its version from `[workspace.package]`, so the harness's version *is*
  the version of the engine it was compiled against. If the workspace ever
  versions its crates separately, this is the line that becomes wrong.

`model_hashes` is supplied by the caller. A model file belongs to a component,
and a component reaches this crate only inside a context that was assembled
elsewhere; an empty map is the honest value for a pipeline that reads no model.

## Design choices made here

Recorded under `AGENTS.md` § Rules of engagement: each stayed inside this
crate, so each was the implementer's to make — and is written down so the next
reader can disagree with it.

1. **The harness returns the `Run`; it does not write it.** Scope — IN of the
   issue says the harness "writes a `Run`", and what it assembles is exactly
   that record, named by its digest. Persisting it is one
   `FileSystemRunStore::save` at the caller, and it is left there because the
   store root is a caller's concern (a CLI flag in `ragondin bench`), because a
   driver that owned a store would be the natural place to grow the run *cache*
   that question 6 of `docs/OPEN_QUESTIONS.md` leaves unresolved, and because a
   returned record is testable without a filesystem. The end-to-end test performs the
   save, so the record is proved storable here rather than two crates away.
2. **The retrieval metric set is nDCG@k, recall@k and MRR**, under the names
   `ndcg@{k}`, `recall@{k}` and `mrr`; the generation set is ADR-C30's,
   `exact_match` and `token_f1`, and is not this crate's choice. The retrieval
   three are the ones the issue that built this driver named; `precision@k`
   and MAP@k exist in `ragondin-metrics` and are not computed, because a metric
   nothing asked for is a number someone has to maintain. MRR is the **uncut**
   `reciprocal_rank`, matching `trec_eval`'s `recip_rank`, which is why its
   name carries no `@k`.
3. **A query with no qrels line at all is executed and left unscored** by the
   retrieval family, as a query with no reference answer is by the generation
   one (ADR-C30 § 1 applies the same rule to each piece).
   `ragondin-metrics` records that `trec_eval` evaluates a judged-and-empty
   query (counting its zero in the mean) and never evaluates an unjudged one;
   `Benchmark::iter` yields an empty map for a query the qrels never name, and
   that empty map is exactly the distinction. The query still runs and still
   leaves a trace — each mean is over the queries that family *scored*, and a
   benchmark in which no family scored any query is refused
   (`HarnessError::NothingToScore`) rather than reported as `NaN`. On a
   BEIR-loaded benchmark the two denominators coincide, because the adapter
   already filters queries by qrels.
4. **A chunk ranking collapses to a document ranking by first occurrence.**
   Metrics score documents; a pipeline ranks chunks. The node's output entry
   names them in the order the node returned them, which the ranking contract
   makes descending score order, so the first chunk of a document
   is its best — which makes first-occurrence the max-score-per-document rule
   BEIR evaluations use, without a second sort. The fold itself is
   `ragondin_metrics::documents_by_first_occurrence`, shared with
   `ragondin-api`: the rule was chosen here, and is defined there.
5. **A refusal is checked for every query once the benchmark carries the
   piece**, judged or not. `NoAnswer` is returned for the first query whose
   pipeline produced no answer over a benchmark carrying reference answers,
   and `NoRanking` for the first whose ranking the walk cannot find over a
   benchmark carrying qrels, even when that query itself carries no reference
   or no judgment: a refusal that depended on which queries happen to be
   judged would accept a pipeline on one benchmark and refuse it on a subset of
   the same one. The walk's own failure modes are ADR-C30's; naming each of
   them as a `RankingWalkError` variant is this crate's; finding the terminal
   node as the one node no other node consumes is the shared walk's
   (`ragondin_experiments::terminal`), which this crate's reading became.
6. **One failing query fails the whole run.** Averaging over the queries that
   happened to succeed would report a smaller benchmark as the whole one, under
   an id that claims to name the whole one.
7. **The pipeline arrives with its configuration text.** The harness never
   re-serializes a `LogicalPipeline` to fill `ConfigDocument`: the store keeps
   the text whose canonical logical form hashes to `RunInputs::pipeline`
   (INV-8), and a re-serialization would file a second spelling of it.

8. **The observer is a closure; the signal is a borrowed `AtomicBool`.**
   `evaluate_observed` takes `O: FnMut(QueryProgress<'_>) + Send` by value
   (a caller that keeps its observer passes `&mut observer`) and `&AtomicBool`. A
   one-method trait would add a name and an impl for what a closure already
   is; a crate-owned token type would wrap one `AtomicBool` and gain nothing
   the standard library does not give — a caller on another thread holds an
   `Arc<AtomicBool>` and passes `&*arc`. A cancellation-token crate would be a
   new `[workspace.dependencies]` entry, which escalates, for a job the
   standard library already does. `SeqCst` on both sides: one load per query
   costs nothing worth reasoning about a weaker ordering for. The observer is
   `Send` so the returned future is, across the loop's await points.
9. **Every executed query is observed, refused ones included.** The observer
   call sits right after the trace is rendered, ahead of the execution error
   and both refusals, rather than beside the `traces.insert` of a query that
   passed: a partial record that dropped the query which stopped the run
   would drop the one trace worth replaying.

## Tests

`tests/harness_over_beir_mini.rs` drives the whole crate over the **miniature
BEIR fixture that belongs to `ragondin-benchmarks`**, reached by a relative path
from `CARGO_MANIFEST_DIR`. It is not copied here: one miniature dataset in the
repository is one set of expected numbers to keep true, and two would drift the
day one of them is edited.

`tests/generation_regime.rs` drives the generation regime over a benchmark
written in the file and `retriever → fusion → context builder → generator`
stubs (`tests/fixtures/stub-generation.yaml`, and `stub-context.yaml` without
its generator). Its context builder keeps one chunk out of a two-document
ranking, so a query judging the second document scores differently over the
ranking and over the context — which is what pins the ranking ADR-C30 § 3 names
as the one read. One of its tests recomputes the run's `ndcg@10` as
`ragondin-api` does — from the stored traces, at the node the shared walk
names, through the shared fold — and asserts it equals the recorded figure bit
for bit, on both pipelines. With one definition of each rule, a change to it
cannot reach one of the two crates and not the other; the test is the record
that the writer's reading and the reader's agree.

`tests/progress_and_cancellation.rs` drives `evaluate_observed` over the BEIR
fixture, with a retriever that wraps the stub's and counts its calls — the
proof that no query ran past a cancellation — and, for the signal set while a
query is in flight, sets it from inside that query's first retrieval leg.

The expected metrics in the first two files are derived by hand in a
comment, from the fixture's qrels and from what the stub components fabricate;
the third reuses the first file's to pin that the new entry point scores what
`evaluate` scored before it was rerouted. That is the point of stubs: the
arithmetic is checkable by a reader, and **no number the test asserts is a
measurement of retrieval quality**. The pipeline fixture labels its
two legs with corpus document ids so that the fabricated ranking lands on judged
documents; that is a fixture trick, not a retrieval claim.
