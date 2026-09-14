# ARCHITECTURE — ragondin

**Status: not an API boundary, and not a library.** This crate has no `lib`
target and exports nothing. Nothing compiles against it, so its internals are
free to move; what it owes stability to is its **command line**, which is the
entire user-facing surface of the product (ADR-C15).

## What lives here

The command line, and the composition root.

`docs/code-architecture.md` §4.2 gives the surface: one binary, four
subcommands. §4.3 gives this crate its position — the only one allowed to know
both the engine and the concrete components. Those two facts are the whole
charter.

| Piece | Role |
|---|---|
| `src/main.rs` | The `clap` definition of the four subcommands, and the dispatch |
| `src/validate.rs` | Loads a configuration and prints its content hash |
| `src/compare.rs` | Reads two stored runs and prints their metric-by-metric diff |
| `src/bench.rs` | Evaluates a configuration against a benchmark and records the run |
| `src/wiring.rs` | A node's `Params` on one side, a constructed component on the other |
| `tests/cli.rs` | `validate` and the rest of the command line, exercised as a process |
| `tests/compare.rs` | `compare`, exercised as a process, against runs written straight into a store |
| `tests/calibration.rs` | The harness against a published SciFact figure, and the exit criterion on real data — ignored by default, run by `just calibrate` |
| `tests/bench.rs` | `bench`, exercised as a process, over a miniature BEIR fixture |
| `tests/vertical_slice.rs` | The composition root assembled for real, end to end |
| `tests/exit_criterion.rs` | The M2 exit criterion: hybrid retrieval with reranking beats dense-only, reproducibly, and `compare` says so |

**Four subcommands are declared; three are implemented.** `validate` loads a
configuration and prints its content hash. `compare` reads two runs already in
a run store and prints their diff. `bench` evaluates a configuration against a
benchmark, records the run and prints what it scored. `serve` parses its
arguments and refuses. Declaring all four is deliberate rather than premature:
ADR-C15 makes the set of subcommands the product's surface, and a surface
discovered one subcommand at a time is one a user has to rediscover at each
release.

## Local invariants

- **`anyhow` here, and only here (ADR-C13).** Every crate this binary calls
  returns a typed error; the aggregation into one report happens at the top of
  the chain, which is this crate. Nothing in this crate is ever imported by a
  library, so that choice reaches no consumer.
- **Subcommands are packaging, not architecture (ADR-C15).** The serving driver
  and the evaluation harness stay distinct crates over one engine; this binary
  puts one door in front of them. A handler that grew logic of its own would be
  merging them through the back.
- **Handlers are thin by rule.** A subcommand parses arguments, calls into the
  crate that owns the work, and renders the result. No business logic lands
  here. The concrete test: a handler body that a driver crate could not have
  contained is in the wrong place.
- **`validate` executes nothing and resolves nothing.** `ConfigSource::load`
  stops at `LogicalPipeline`, which names each implementation and resolves none
  (ADR-C2), so the command needs no `EngineContext` and no registry. A
  configuration naming an implementation this build does not carry still
  validates, and still hashes.
- **The kind check is surfaced here, never implemented here.** ADR-C16 places
  the check of an edge's value kinds in `ragondin-pipeline`'s validation pass.
  This crate matches on its `ValidationError::KindMismatch` to render a report —
  the edge, the kind the port expects, the kind that arrives — and re-derives
  none of the check's reasoning. An `extension` node's ports are unknown to the
  core, so an edge at one is not kind-checked — though an edge arriving at a
  position where the consuming node declares no port at all is refused whatever
  produced it, extension included. `validate --help` states both, rather than
  implying full coverage. The report names the producer first and the error
  type's own `Display` names the consumer first; the divergence is deliberate —
  the report follows the direction the value travels — and is argued where the
  renderer is defined.
- **A bad configuration is a diagnosis, never a crash.** Every load-path failure
  reaches the user as an exit status and a message naming the file. There is no
  `unwrap` on the load path, and `tests/cli.rs` asserts the absence of a panic
  rather than trusting it.
- **Heavy backends arrive optional and feature-gated (ADR-C14).** Two features
  carry them: `bm25` (tantivy) and `onnx` (ONNX Runtime and `tokenizers`, and
  the dense retriever and in-memory store that compose with it). The default
  build enables neither and stays lean — and it still loads, validates and
  hashes a configuration naming a component it does not carry. What refuses it
  depends on what the node names: a `bm25` node reaches planning, which
  reports the unknown `impl:` against the node; a `dense` or `cross_encoder`
  node names a model file, which is digested before anything is planned, so a
  missing file is what refuses it first, and the unknown `impl:` only once the
  file exists. `tests/bench.rs` asserts the `bm25` refusal in the lean build,
  so "lean" is a tested claim rather than an intention. RRF is a normal
  dependency rather than a feature: it is
  rank arithmetic with no backend behind it, so gating it would buy no compile
  time.
- **`compare` reads; it never executes (ADR-C15).** It loads two runs by
  `run_id` from `ragondin-experiments`' `FileSystemRunStore` and hands them to
  that crate's own `compare()`; the diff it prints is that function's result,
  rendered. No metric is computed here and the engine is never touched.
- **`--store` and `--datasets` are required flags, not default paths.** No
  default run store location is settled anywhere in `docs/` yet, and no dataset
  location either, so this crate invents neither: `compare` names its store and
  `bench` names both. This is the kind of choice `AGENTS.md` § Rules of
  engagement leaves to the crate ("how a knob is exposed"): recorded here
  rather than escalated, and open to revisiting once a shared default becomes
  worth settling. `bench` now writes to a store, which is the condition that
  makes settling one worth doing — it is still not this issue's to settle.
- **`bench` registers; nothing else does.** It is the subcommand that needs an
  `EngineContext`, so it is where the components this build carries are
  registered — through the ordinary `register_*` call, one per component, with
  no shortcut for a first-party one (INV-7). The engine depends on no component
  crate and this one depends on all of them, which is §4.3's rule made
  mechanical: break it and the arrow in `Cargo.toml` is what a reviewer sees.
- **The corpus is prepared here, and the components are constructed from it**
  (ADR-C26). `bench` builds one `CorpusIndex`, constructs every component from
  its chunks, and hands the harness that same value. A second `CorpusIndex` in
  this crate, or a component built from a separately loaded benchmark, would
  make the `index_version` the run records name a set nothing searched.
- **The asynchrony a constructor cannot do happens before construction.** A
  `ComponentCtor` is synchronous; `Embedder::embed` and `VectorStore::upsert`
  are not. So `bench` embeds the corpus in `main` and the store reaches its
  constructor already holding the vectors, through
  `MemoryVectorStore::seeded` — a synchronous door that shares `upsert`'s
  validation, recorded in that crate's own `ARCHITECTURE.md`. The alternative
  weighed was a shared store behind an `Arc`, wrapped here in a `VectorStore`
  of this crate's own: that puts a contract implementation in the composition
  root, and a second one beside it for the embedder. ADR-C26 names the
  constraint and deliberately picks neither, so the choice is recorded rather
  than silent. The seam has a cost, recorded here so nobody rediscovers it as
  a bug: the embedder is constructed twice per run — once in `prepare`, to
  embed the corpus, and once inside the `dense` constructor, for the queries —
  because `DenseRetriever::new` takes a `Box<dyn Embedder>` it owns, and a
  constructor can capture what was prepared but not await it. Two session
  loads of one file, in sequence. Sharing one is a change to that leaf's
  constructor signature, not to this crate.
- **One model per role, and one embedder per pipeline, in v0.** A run records
  its model hashes by the role each model played (§7.1), and the corpus is
  embedded once — so two nodes on one role naming different models, or two
  `dense` nodes configured differently, are refused with a message saying to
  evaluate them as two pipelines. Embedding twice instead would leave one
  `index_version` naming neither index.
- **The metrics' cutoff is a constant, not a flag.** `10` is what every BEIR
  leaderboard reports, and the milestone's claim is a comparison against
  published numbers; a flag would only offer a way to produce an incomparable
  one. It becomes a flag the day a benchmark reports at another cutoff.
- **The M2 exit criterion is a test in this crate, over a fixture built for
  it.** `tests/exit_criterion.rs` drives `bench` and `compare` as processes over
  `tests/fixtures/exit-criterion/` and asserts the milestone's three claims:
  hybrid retrieval with reranking beats dense-only on nDCG@10, the same
  configuration evaluated twice is one run (P4), and `compare` reports the
  win. It is an instance of the end-to-end row of `docs/code-architecture.md`
  § 11.4 Testing strategy. Three choices are recorded here:
  - *The dataset is curated, and the gap is by construction.* Fifteen
    documents, five queries and two models, designed together so that each
    query defeats a different stage: the embedder ranks a distractor above the
    answer, or crowds the answer out of the dense leg's `top_k`; BM25's length
    normalization prefers a short distractor, or cuts the answer from its
    `top_k` so that only the dense leg surfaces it; and the fused list ties on
    two queries, where only the cross-encoder separates answer from distractor
    on content rather than by chunk id. `models/generate.py` walks through
    each. A fourth test removes each leg and the reranker in turn — four
    ablation configurations in `ablations/` — and asserts each removal costs
    a query, so the criterion cannot be met by a pipeline in which a stage
    does nothing; what the fusion contributes is the union of the legs, which
    the two leg ablations show, and not an order the reranker keeps. A real
    subset under real
    models would make the gap a fact about two trained models, and neither is
    fast, offline or deterministic. What stays real is the path and the
    numbers: the same engine, planner, executor and harness, over the real
    components, scoring what each pipeline returned. So the test proves that
    the composition comes out the right way and that each leg and the
    reranker matter, and it is not the quality claim — the leaderboard
    calibration in the same table is, and it is not a test.
  - *The models are committed, with the source that reproduces them beside
    them.* `models/generate.py` writes the embedder, the cross-encoder and the
    tokenizer, byte for byte — the convention `ragondin-embedder-onnx`'s
    fixtures set. `ragondin-reranker-onnx` chose the other one, emitting its
    model inside the test binary, because its tests hold the model in-process;
    here the model must be a file the spawned binary reads, and a committed
    file with its generator is the form a reader can inspect without running
    anything. The cross-encoder graph is that crate's lexical-overlap graph
    written once more: a crate's test fixture is not a library another crate
    can import, and making it one would be a shared surface this test does
    not own.
  - *The configurations are committed with relative model paths, and the test
    runs the binary from the fixture directory.* A path in a configuration
    resolves against the working directory; the alternative — writing the file
    at test time around an absolute path, as `tests/bench.rs` does for its
    hybrid case — leaves nothing reviewable in the tree. Relative paths also
    keep the pipeline hash, and with it the run id, the same on every machine.
- **A model file is hashed here.** Only the composition root sees every node's
  configuration at once, so it is what can record which model a run read. A
  digest is over the file's bytes: not its path, which moves between machines,
  and not its timestamp, which a checkout resets. It is taken before the
  benchmark is loaded, so a missing file is found before the corpus is
  embedded; and it is keyed on the `impl:` names that read a model (`dense`,
  `cross_encoder`), so a component registered later that reads one must be
  added to `wiring::model_hashes` in the same change — a run over it would
  otherwise record no hash, and two runs over two models would content-address
  alike.

## Dependency choices made here

Two third-party crates enter `[workspace.dependencies]` with this crate, and
both are reachable only from it.

- **`clap` (derive), as the CLI parser.** ADR-C15 makes a subcommanded binary
  the product's whole surface, so the parser is load-bearing: it owns the
  subcommand set, the help text, and the exit status on a bad invocation.
  `clap`'s derive keeps that set declared in one enum beside the handlers, which
  is what lets `bench` and `compare` be declared now and filled in later without
  the declaration drifting from the dispatch. The alternative considered was
  hand-rolled argument parsing — cheaper as a dependency, and it would have put
  the help text, the subcommand table and the arity checks in three places that
  drift. `clap` is used in this crate and nowhere else; no library takes it.
- **`assert_cmd`, as the CLI test harness.** `validate`'s contract is an exit
  status and what lands on stdout and stderr. None of that is observable from a
  test inside the crate — a function returning `Result` proves nothing about the
  process's exit code — so the tests spawn the built binary. `assert_cmd`
  resolves the binary cargo just built, which is the part of that job worth not
  hand-rolling. It is a dev-dependency, so it is absent from every shipped
  build.

Neither duplicates a role `[workspace.dependencies]` already fills: the table
held no CLI parser and no CLI test harness before this crate needed one.

Two more entries are *used* here without being added by it, so neither is a new
utility role and neither escalates: **`sha2`**, the crate the canonical
logical-form hash and run identity already use, because `bench` digests the
model files a run read; and **`serde_yaml`** as a dev-dependency, because
`src/wiring.rs` reads a node's parameters out of a validated pipeline and its
tests need pipelines built the way the product builds them — the door
`ragondin-config` puts in front of that lowering takes a path and a runtime,
which a unit test wants neither of, so the tests parse the same YAML into the
same `RawPipeline` and run the same `validate`.

Two workspace crates also become normal dependencies of the binary with
`bench`: **`ragondin-contracts`**, because the constructors `src/wiring.rs`
registers return its traits and move its `EmbeddedChunk`; and
**`ragondin-types`**, for the `Chunk` those constructors are built from. Both
are what the composition root already is — the one crate that names the traits
and the concrete components together (§4.3) — so neither is a new arrow
anywhere else in the graph.

## Not here

- **No component logic.** Every constructor in `src/wiring.rs` reads a node's
  parameters and calls the component crate's own constructor; none of them
  computes anything. The concrete test is the one the handlers are held to: a
  body that a component crate could have contained is in the wrong place.
- **No serving.** `ragondin-server` is a declared dependency and an unbuilt
  driver; `serve` refuses. The Tower envelope is out of M0–M2 entirely.
- **No metric, no benchmark adapter, no run store.** Those live in `eval/` and
  `runtime/`, and a subcommand reaches them rather than restating them. `bench`
  computes no metric of its own: it hands the harness a context and a prepared
  index, and prints what comes back.
- **No judge, no generation, no control flow.** M2 evaluates retrieval, and
  today those would arrive as an `extension` node (ADR-C3), so `bench` refuses
  one by name. It is the whole of that check, and it grows a case the day one
  of them becomes a primitive.

## Calibration against a published leaderboard

ADR-10 trusts the harness only once it reproduces a published leaderboard score
to within half a point through an exact search, so that a discrepancy is the
metric's or the encoding's and never approximation's;
`docs/system-architecture.md` § 9.8 Calibrating the harness against a published
leaderboard gives the procedure and the diagnostic table, and names the two
cases it asks for: SciFact, whose qrels are binary, and then NFCorpus, whose
graded qrels "alone can expose a linear-versus-exponential gain bug".
`tests/calibration.rs` is both reproductions, through `bench`, one test each,
and beside the SciFact one the M2 exit criterion — hybrid retrieval with
reranking against dense-only — on that real corpus. They are `#[ignore]` and
run by `just calibrate`: what they need never enters the tree and is never
fetched by it (ADR-C27 downloads the runtime, not a model), and together they
cost the better part of half an hour of CPU. Two environment variables name the
material:
`RAGONDIN_CALIBRATION_DATASETS`, a directory holding `scifact/` and `nfcorpus/`
in the layout the BEIR adapter reads, and `RAGONDIN_CALIBRATION_MODELS`, a
directory holding `all-MiniLM-L6-v2/` and `ms-marco-MiniLM-L6-v2/`, each with a
`model.onnx` and its `tokenizer.json`. The configurations in
`tests/fixtures/calibration/` name the models by those relative paths and the
tests run the binary from the models directory, so their content hashes — and
the run ids — are the same on every machine.

### SciFact, the binary case

**The reference, so that the reproduction can be redone from this section.**

- *Dataset:* BEIR SciFact, the original `scifact.zip` from the BEIR datasets
  bucket, SHA-256
  `536e14446a0ba56ed1398ab1055f39fe852686ecad24a6306c80c490fa8e0165`; the
  archive carries no revision, hence the hash. Unpacked as is: `corpus.jsonl`,
  `queries.jsonl`, `qrels/test.tsv`; 5 183 documents, 300 judged queries on
  `test`. The adapter's `dataset_version` for it is
  `9a07f80c0d4f1e9e74912d033a8d1fbd52c54b758dafcaa85c19abacfdee5f29`.
- *Embedder:* `sentence-transformers/all-MiniLM-L6-v2` at revision
  `8b3219a92973c328a8e22fadcfa821b5dc75636a` — the revision MTEB records against
  the published figure, not `main`; its weights and tokenizer are byte-identical
  to `main`'s, but the figure belongs to the pinned one. Mean pooling, L2, no
  instruction prefix, 256 word pieces: what the card specifies, and what
  `ragondin-embedder-onnx` applies with no knob. That is what made a
  mean-pooling model the only choice — a CLS-pooling reference would need a
  pooling option that crate deliberately does not have.
- *Reranker:* `cross-encoder/ms-marco-MiniLM-L6-v2` at revision
  `233902d25c440f23af6f7d6e94d2946bac0bee0a`, single-logit head, 512 word
  pieces. The repository was renamed from `ms-marco-MiniLM-L-6-v2`; the older
  name redirects.
- *Export:* each repository pinned on disk with `huggingface_hub`'s
  `snapshot_download(repo_id, revision)`, then
  `optimum-cli export onnx --library-name transformers --task feature-extraction`
  for the embedder and `--task text-classification` for the reranker.
  `--library-name transformers` is load-bearing: a `sentence_transformers`
  export pools inside the graph and returns `[batch, hidden]`, which the
  embedder crate refuses. The exported files digest to
  `9348202758f11c56c329d947ae359fea54be1a3d905bfcac4a3521a1eafc0414` (embedder)
  and `8b0fe5bc3c5ddc752524552d8e081baa7726e389b1d23396e56ad31d69b88d52`
  (reranker); the test pins both.
- *Published figure:* SciFact, `test`, nDCG@10 **0.64508** on the MTEB
  leaderboard for that model and revision, read at reproduction time.
- *Configurations:* dense-only at `top_k: 10`; hybrid at `top_k: 50` per leg,
  RRF `k: 60`, reranker `top_k: 10` — a fused list of at most a hundred, the
  depth BEIR's own reranking baseline reranks.

**What the recorded run scored.**

| | nDCG@10 | recall@10 | MRR |
|---|---|---|---|
| Published (MTEB) | 0.64508 | — | — |
| dense-only, `bench` | 0.6450816521455768 | 0.7833333333333333 | 0.6047248677248677 |
| hybrid + rerank, `bench` | 0.6886092429213343 | 0.8122222222222222 | 0.6579272486772487 |
| `pytrec_eval` over the dense-only run | 0.6450816521455776 | 0.7833333333333333 | — |

The gap to the published figure is 0.0002 of a point against a tolerance of
0.5; the gap to `pytrec_eval` is the last two bits of an `f64`, the summation
order `ragondin-metrics`' parity fixture already documents. The hybrid gain is
4.35 points of nDCG@10 and holds on recall and MRR too. Run ids, over the
committed configurations: `e9f178018e9974f216d6cf81ebd71bd5a7273a281e47e48d016fb1cd265382e7`
(dense-only) and
`9b0e2d9419a1b5d84ed384f50ce4a100a983c0525b93636f749ac50928456238`
(hybrid + rerank); dense-only takes about 105 s on a laptop CPU, the hybrid
run about 24 minutes, dominated by the cross-encoder over the fused list.

The reproduction was first done by hand, before a line of this test existed,
and missed by 53 points: `ragondin-embedder-onnx` built its attention mask from
the id count and pooled every `[PAD]` of a self-padding tokenizer into the mean.
Replaying the crate's documented procedure in Python landed on the published
figure and replaying the mask it actually built landed on the miss, which is
the diagnostic § 9.8 asks for, and it became #237. The number now travels
through the BEIR adapter, the store's cosine search and `ragondin-metrics`, and
all three agree with `pytrec_eval` over 300 real queries: that is P1 doing its
job, and it is what makes this a calibration rather than a second opinion.

**What is frozen, and what is not.** The test pins the aggregates — every
metric of both runs to the values above, within a tolerance for another
machine's floating-point summation — and the dataset and model digests, so a run
over the wrong revision fails by name; and it evaluates the dense configuration
twice into two stores and requires one run id and equal metrics (P4). The
per-query freeze ADR-10 asks for — each query's ranking, checked against
`pytrec_eval`, as a permanent fixture — is not produced here, but the ranking it
needs now exists: ADR-C28, deciding #239, has each node's output entry in the
execution trace name the chunks it produced in rank order, so a stored run's
`traces.json` holds the ranking of every query. The fixture that freezes them —
both runs above, query by query, against `pytrec_eval` — is
`eval/ragondin-metrics/tests/scifact_calibration_fixture.rs`, beside the metric
it guards.

**What this calibration does not claim.** The exit criterion it confirms is the
one M2 states — hybrid with reranking against dense alone. On SciFact the
lexical leg is strong, and nothing here says the hybrid beats its best single
leg. And the equality of run ids holds on every machine by construction, while
the metrics may move in their last bits across platforms, which is what the
recorded tolerance is for.

### NFCorpus, the graded case

SciFact's qrels are 0/1, and on binary judgments `rel` and `2^rel - 1` are the
same number — so no SciFact fixture, however many queries it freezes, can tell
the two nDCG gain formulas apart. That is why § 9.8 asks for a second case.
NFCorpus's qrels grade 1 and 2, and this is the reproduction over them.
Dense-only only: the M2 exit criterion is SciFact's, and a hybrid case here
would confirm nothing the one above has not already confirmed.

**The reference, so that this reproduction can be redone from this section.**

- *Dataset:* BEIR NFCorpus, the original `nfcorpus.zip` from the same BEIR
  datasets bucket, SHA-256
  `efe5be03f8c5b86a5870102d0599d227c8c6e2484328e68c6522560385671b0b`; the
  archive carries no revision, hence the hash. Unpacked as is: `corpus.jsonl`,
  `queries.jsonl`, `qrels/test.tsv`; 3 633 documents, 323 judged queries on
  `test`, and 12 334 judgments of which 576 are grade 2. The adapter's
  `dataset_version` for it is
  `8046025011c86dcbac3c15f9f52e5cf0ebc534282944b50fe72884cfcb6a112b`.
- *Embedder:* the same one, at the same revision, exported the same way and
  digesting to the same
  `9348202758f11c56c329d947ae359fea54be1a3d905bfcac4a3521a1eafc0414`. Holding
  the encoder fixed is what makes this a second measurement of the harness
  rather than a second experiment.
- *Published figure:* NFCorpus, `test`, nDCG@10 **0.31594** for that model at
  that revision, read on 2026-09-14 from the MTEB results repository —
  `https://raw.githubusercontent.com/embeddings-benchmark/results/main/results/sentence-transformers__all-MiniLM-L6-v2/8b3219a92973c328a8e22fadcfa821b5dc75636a/NFCorpus.json`,
  field `ndcg_at_10` of the `test` split, over MTEB's dataset revision
  `ec0fa4fe99da2ff19ca1214b7966684033a58814`. The same file publishes
  `recall_at_10` 0.15499, which the run below reproduces to five decimals and
  which is therefore a second, independent check on the same encoding.
- *Configuration:* `tests/fixtures/calibration/nfcorpus-dense-only.yaml`,
  dense-only at `top_k: 10` and 256 word pieces — SciFact's dense configuration
  in every respect, committed separately so that a change made for one
  dataset's sake cannot move the other's run id.

**What the recorded run scored.**

| | nDCG@10 | recall@10 | MRR |
|---|---|---|---|
| Published (MTEB) | 0.31594 | 0.15499 | — |
| dense-only, `bench` | 0.31667312754717813 | 0.15498797328057862 | 0.5076539387684899 |
| `pytrec_eval` over that run | 0.31667312754717813 | 0.15498797328057862 | 0.5076539387684899 |

The gap to the published nDCG@10 is 0.073 of a point against a tolerance of
0.5, and recall@10 agrees with the published figure to the precision it is
published at. `pytrec_eval` agrees bit for bit here — not merely within a
tolerance, as on SciFact — over all 323 queries and all three metrics. Run id,
over the committed configuration:
`5df02792921fe418538358a0c8710bfb683b1b852fecf808c666429388d0fe21`; the two
evaluations the P4 check requires take about 120 s each on a laptop CPU, four
minutes for the test.

MRR is not comparable to anything MTEB publishes: `trec_eval`'s `recip_rank`,
which the harness reports, is uncut, and MTEB reports `mrr_at_10`. It is
recorded because the fixture freezes it, not because it corroborates anything.

**What is frozen, and what is not.** As above: the aggregates and the digests,
and the P4 pair of evaluations into two stores. The per-query freeze is
`eval/ragondin-metrics/tests/nfcorpus_calibration_fixture.rs`, and on this
dataset it carries weight the SciFact one cannot. Swapping the linear gain for
the exponential one moves 80 of the 323 queries — the worst by 12.4 points of
nDCG@10 — but moves the **mean** only from 0.31667 to 0.31727, six hundredths
of a point. That is inside the half-point tolerance this reproduction is held
to, and inside the 1e-4 the test allows an aggregate. So the reproduction alone
would report success with the wrong gain function, and the per-query fixture is
what turns that into a named failing query. This is the reason § 9.8 asks for
both, stated as a number rather than as a principle.

**What this calibration does not claim.** Nothing about hybrid retrieval, which
was not run here. And nothing about NFCorpus being an easy corpus: an nDCG@10
of 0.32 with a recall@10 of 0.15 is what the leaderboard reports for this model
on this dataset, and reproducing a modest figure is the same evidence as
reproducing a strong one.
