# ARCHITECTURE — ragondin-harness

## What lives here

The **evaluation driver**: `evaluate`, which takes a validated
`LogicalPipeline`, a ready `EngineContext` and a loaded `Benchmark`, runs the
pipeline once per query through `ragondin-engine`, scores the rankings against
the benchmark's qrels, and assembles the `Run` that names itself by the
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
store holds, and files it under its query. Two consequences are deliberate:

- **The rendering is hand-written** (`src/trace.rs`) and `ExecutionTrace` is
  not serialized by a derive. The engine is internal and not an API boundary
  (INV-2), so its trace type is meant to move; a derive would make every stored
  run a hostage of that shape. The harness is the crate that knows both the
  engine and the run store, so the translation lives here.
- **A failed query carries its trace out with the error.**
  `HarnessError::Execute` holds the rendered trace of the run that failed,
  because that is the trace worth reading and an error that dropped it would
  discard exactly the evidence INV-10 exists to preserve.

## Indexing is ad hoc, and that is not a position on open question 5

`CorpusIndex::build` prepares the corpus **directly**: one chunk per document,
in corpus order. No indexing graph, no node kind, no `ValueKind` — because
question 5 of `docs/OPEN_QUESTIONS.md` (*does indexing share the IR
formalism?*) is deliberately unresolved, and this crate does not resolve it. When it is
settled, `src/corpus.rs` is what moves.

What that module does **not** do is build a backend index, and the reason is a
property of the contracts rather than a choice: a BM25 index is built inside
`Bm25Retriever::new`, from the chunks it is given, and a `VectorStore` instance
is not reachable from an `EngineContext` at all — a context holds constructors,
and its resolution half is crate-private to `ragondin-engine`. So the backing
index is built where components are constructed, which is the composition root,
out of exactly `CorpusIndex::chunks()` — the set `CorpusIndex::version()`
content-addresses. Making a component instance reachable from a driver would be
a change to the engine's public surface, which `AGENTS.md` § Rules of
engagement escalates; it is not done here.

That escalation is **open**, as decision issue
[#212](https://github.com/genesary/Ragondin/issues/212) — *how does a driver get
a corpus into the index it will retrieve from?* It is the decision this section
is waiting on, and the place its alternatives are argued: until it is settled,
what this crate does about a backend index is what is written above, and a
change here that presumes an answer is presuming one.

## Run identity

`run_id = hash(pipeline_config, dataset_version, index_version, model_hashes,
engine_version)` (`docs/system-architecture.md` §7.1). `ragondin-experiments`
defines the record and states that the digest is assembled by the harness;
`src/identity.rs` is where. Four decisions in that file are load-bearing:

- **The encoding is tagged and length-prefixed**, in the shape
  `core/ragondin-pipeline/src/hash.rs` uses, and fed to SHA-256 in one pass.
  Length prefixes make it injective; the domain separator keeps a dataset
  digest from colliding with an index digest over the same bytes. Nothing goes
  through `serde`: a serializer's output is documented as readable, not as
  stable, and run identity is not a thing to hang on that.
- **`dataset_version` is taken over the loaded benchmark**, not over the files
  it was parsed from: two snapshots that parse to the same corpus, queries and
  judgments are the same dataset, and a digest over bytes would make a
  re-download a different one.
- **`index_version` is taken over the chunk set**, not over a backend artifact:
  the chunks are what any index is built from, and an index file's bytes move
  with a library version that changed nothing about what is indexed.
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
2. **The metric set is nDCG@k, recall@k and MRR**, under the names `ndcg@{k}`,
   `recall@{k}` and `mrr`. Those are the three the issue names; `precision@k`
   and MAP@k exist in `ragondin-metrics` and are not computed, because a metric
   nothing asked for is a number someone has to maintain. MRR is the **uncut**
   `reciprocal_rank`, matching `trec_eval`'s `recip_rank`, which is why its
   name carries no `@k`.
3. **A query with no qrels line at all is executed and left unscored.**
   `ragondin-metrics` records that `trec_eval` evaluates a judged-and-empty
   query (counting its zero in the mean) and never evaluates an unjudged one;
   `Benchmark::iter` yields an empty map for a query the qrels never name, and
   that empty map is exactly the distinction. The query still runs and still
   leaves a trace — the mean is over the queries that were *scored*, and a
   benchmark in which that count is zero is refused
   (`HarnessError::NothingToScore`) rather than reported as `NaN`. On a
   BEIR-loaded benchmark the two denominators coincide, because the adapter
   already filters queries by qrels.
4. **A chunk ranking collapses to a document ranking by first occurrence.**
   Metrics score documents; a pipeline returns chunks. The output is sorted by
   descending score by the ranking contract, so the first chunk of a document
   is its best — which makes first-occurrence the max-score-per-document rule
   BEIR evaluations use, without a second sort.
5. **One failing query fails the whole run.** Averaging over the queries that
   happened to succeed would report a smaller benchmark as the whole one, under
   an id that claims to name the whole one.
6. **The pipeline arrives with its configuration text.** The harness never
   re-serializes a `LogicalPipeline` to fill `ConfigDocument`: the store keeps
   the text whose canonical logical form hashes to `RunInputs::pipeline`
   (INV-8), and a re-serialization would file a second spelling of it.

## Tests

`tests/harness_over_beir_mini.rs` drives the whole crate over the **miniature
BEIR fixture that belongs to `ragondin-benchmarks`**, reached by a relative path
from `CARGO_MANIFEST_DIR`. It is not copied here: one miniature dataset in the
repository is one set of expected numbers to keep true, and two would drift the
day one of them is edited.

The expected metrics in that file are derived by hand in a comment, from the
fixture's qrels and from what the stub components fabricate. That is the point
of stubs: the arithmetic is checkable by a reader, and **no number the test
asserts is a measurement of retrieval quality**. The pipeline fixture labels its
two legs with corpus document ids so that the fabricated ranking lands on judged
documents; that is a fixture trick, not a retrieval claim.
