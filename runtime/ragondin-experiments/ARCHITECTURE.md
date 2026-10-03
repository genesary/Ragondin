# ARCHITECTURE — ragondin-experiments

**Status: not an API boundary.** INV-1 names the three crates that are, and this
is not among them. Refactor it freely — including the storage layout below,
which is deliberately not a boundary either.

## What lives here

The experiment plane's state (`docs/system-architecture.md` §6.2): the **run
store** — the history of runs, their metrics and their traces — and the **run
comparison** that store exists to serve, which §6.5 names as the load-bearing
view of the product.

| Piece | Role |
|---|---|
| `RunId` | The content address of a run: a 32-byte digest, rendered and parsed as 64 lowercase hex digits |
| `Run` | One execution: its identity tuple's components, its metrics, the configuration document, the per-query traces |
| `RunStore` | The trait a run store backend implements: `save`, `load` by id, `ids` — every stored run's id |
| `FileSystemRunStore` | `RunStore`'s first implementation, one directory per run; also `compare` by two ids |
| `Trace` | The stored trace document's one typed definition, converted to and from `TraceDocument`; a query's latency, `Trace::latency_nanos` |
| `lower_median` | The one median of durations: of a run's query latencies, and of one node's durations over a run's queries |
| `terminal`, `ranking_node` | The walk (ADR-C30 § 3): which node's ranking a pipeline's retrieval metrics are read from, with `WalkError` naming where it stops |
| `conformance` | Behind the `conformance` feature: the suite every `RunStore` backend passes |
| `compare`, `compare_runs` | The diff behind `ragondin compare`: metric by metric, and the configuration parameters the two runs differ in; and the same over a baseline and further runs of one benchmark, behind `POST /compare` |

**Deliberately absent**, and each for its own reason: the **export adapters**
(MLflow, OpenTelemetry) — additive to the plane and not what a local benchmark
needs; the **registry** of benchmarks, datasets and indexes — a later addition;
the **user interface** — post-M2 (§6.5); **metric computation** —
`ragondin-metrics`; and **execution** of anything at all — the engine and the
harness.

## Local invariants

- **The run store is native (ADR-13), and stays native.** A run here is a
  content-addressed tuple whose configuration is a *graph* and whose central
  artifact is a *structured per-node trace*; a conventional experiment tracker
  models a run as scalar parameters, metrics and opaque blobs, and storing
  these as blobs would keep the storage and lose the link that matters. No
  experiment-tracker client is a dependency of this crate, now or later. The
  export trait ADR-13 also calls for writes *to* such a tracker from the
  outside; it is not a backend, and adding one does not make this store one.
- **A run is keyed by its content address (P4).** Identical inputs map to one
  id, and the id is the only key the store has. That is what lets a caller ask
  *is this run already stored* instead of executing it — the unification loop of
  §7 — so nothing here may key a run by anything else: not a timestamp, not an
  insertion counter, not a name a user chose.
- **`RunId` assembly belongs to the harness, not here.** `run_id =
  hash(pipeline_config, dataset_version, index_version, model_hashes,
  engine_version)` (§7.1), and the harness is the one place where all five are
  in hand: it holds the `LogicalPipeline` (hence `PipelineHash`), the benchmark
  it is iterating, the index it bound, the models it called and the engine it
  ran. A store that also decided identity would be two things at once, and the
  fold would then live where four of its five inputs are not. So this crate
  takes a `RunId` and never computes one. `RunId::from_digest` is the whole of
  the door for a computed digest; `FromStr` is the other one, for an id a person
  typed or a directory name carries.
- **`RunId` is a digest, not a string.** One run has one spelling — the parse
  refuses uppercase rather than folding it — and a value that can only be 64
  hex digits cannot name a path outside the store's root. Path safety is a
  property of the type here, not of a check in the store that someone must
  remember to keep.
- **The filesystem is a v0 choice internal to this plane, not an architecture
  boundary.** A directory per run buys no database, no schema migration, `ls`
  and `cat`, and a store that copies with `cp -r`. It answers one question —
  *give me this run* — and nothing about *which runs match*. When a query or
  index need appears, replacing it (an embedded database, say) is a change
  **inside this crate**: callers name a run by its `RunId` and never by a path,
  no invariant mentions the layout, and no ADR is owed for the swap. Whoever
  does it should not treat it as an architectural act, and should not be made
  to argue for it as one.
- **A run directory appears whole or not at all.** `save` assembles the run in
  a staging directory and renames it into place. A crash therefore leaves an
  inert `.partial` directory and no run, so *a directory under an id that `save`
  accepted* is *a run that loads* — which is exactly the question the harness
  asks before deciding to execute. A run already stored is **left alone** rather
  than rewritten: its id is the digest of its inputs, so a rewrite could only
  replace it with itself, and `fs::write` truncates, so a crash mid-rewrite
  would destroy a run that was complete.
- **The staging name is per writer, not per process.** `.<id>.<pid>-<n>.partial`
  — the process id separates two programs, a counter separates two calls within
  one, *including two threads*: the store is `Clone` and `Sync`, so concurrent
  saves are ordinary. It is unique among *live* writers rather than for all
  time — a recycled pid restarts the counter at zero and may name what a crashed
  process left — which is harmless, because creating the directory is
  idempotent, every file of the run is written before the rename, and no other
  name is ever written there. Nothing clears a staging directory on the way in, and
  nothing may: a name no live writer shares has nothing to clear, and clearing a
  shared one reaches into a directory another live writer owns. Whoever renames
  first wins, and the others find the run already there and report success.
- **A leading dot under the store root means "not a run".** A failing `save`
  asks for its `.partial` directory to be removed, and the removal is best
  effort: the fault that broke the write can be the one that denies the removal
  (a directory that cannot be written to usually cannot be emptied either), and
  a process that dies inside a `save` asks for nothing. Nothing sweeps what is
  left, so they **accumulate** until someone deletes them. Anything that lists
  the root — a future `ragondin runs`,
  the comparison view — must skip entries whose name begins with `.`: a run id
  is 64 hex digits, so parsing one of these as an id fails rather than
  misreading, but only if the lister expects the convention.
- **A torn directory is reported, never repaired.** `save` checks that a
  destination already under the id holds the four files every run has, and reports
  `Incomplete` if it does not, rather than answering `Ok(())` for something
  `load` cannot read. It does not delete and rewrite: a run's metrics and traces
  are **not** determined by its id — a judge's scores are not reproducible — so
  discarding a torn directory could destroy the only copy of something. Whoever
  can decide that is a person, and the error names the file. Present is as far
  as the check goes: a file corrupted in place is a fault `load` finds.
- **What that does *not* guarantee.** The rename is atomic **within one
  filesystem** — a store root that spans one, which a directory tree does by
  construction, but not a layout someone points across a mount. Nothing is
  `fsync`ed, so the guarantee is against a crashing *process*, not against a
  crashing *machine*: after a power loss the metadata may be behind. And an
  incomplete directory can still be *made* — by a hand deleting a file under
  the store root, or by a writer that predates this scheme — which is why it
  has a name of its own, `RunStoreError::Incomplete`, kept distinct from the
  `Io` a permission failure produces.
- **A run records its `Remote` bindings, outside its identity (ADR-C32 § 2).**
  `Run::bindings` is a list of `RunBinding` — family, name and URI, plain
  strings as the composition root wrote them, in the order given. It is not a
  field of `RunInputs` and the run id does not digest it: where a service
  listened is not an input of the experiment, so two runs with one id and
  different bindings are one experiment run twice, and `save` keeps whichever
  record it had first, as it does for every rerun. Choices recorded here, since
  the ADR leaves the shape to the implementation:
  - *A file of its own, `bindings.json`*, written for every run, `[]` when
    nothing was bound. The store keeps each other field in a file of its own,
    so this one follows, and a person reads it with `cat` beside the rest.
  - *Additive on disk.* A run stored before the field existed has no
    `bindings.json`; `load` reads it as bound to nothing, and `save`'s
    completeness check still requires only the four original files, so such a
    run is complete rather than torn. `tests/run_store.rs` stores a run, deletes
    its `bindings.json`, and reads it back.
  - *Filled by the composition root, not the harness.* `ragondin-harness`
    assembles the run with no bindings, since it never sees the command line,
    and the binary sets the field before saving. `compare` does not show
    bindings.
- **A run records how it was launched, outside its identity (ADR-C39).**
  `Run::provenance` is an `Option<RunProvenance>`: `name`, the workspace
  pipeline name the run was launched as, and `prefix_of`, a `PrefixOf` —
  `up_to`, the node a prefix run stops at, and `parent_pipeline_hash`, the
  canonical hash (`PipelineHash`, the type `RunInputs::pipeline` holds) of
  the parent's version it was cut from. Both optional, both plain values.
  These are the final names ADR-C39 left to this file: the type
  `RunProvenance`, the file `provenance.json`, the keys `name`, `prefix_of`,
  `up_to` and `parent_pipeline_hash`.
  - *Outside identity.* Not a field of `RunInputs`, and the run id does not
    digest it (INV-8): a run with a record and the same run without one are
    one run. `tests/run_store.rs` saves both and lists one.
  - *A file of its own, written once, with the run, only when recorded.*
    `provenance.json` is staged with the rest of the run and renamed into
    place with it, beside `bindings.json` and `times.json`. Like
    `times.json`, it is absent when `provenance` is `None`, and an absent file
    is what *no record* means: `load` reads it as `None`, and a file that does
    not parse as `Malformed`. The completeness check still requires only the
    four original files, so every run stored before the file existed is
    complete. A key is written only for a field that is set.
  - *`{}` is a valid empty record, and differs from no file.* `Some` of an
    empty record is written as `{}` and reads back as `Some`; `None` writes
    nothing. The CLI never writes an empty one (`bench` records a name or no
    record), but the store does not refuse one, since it is a fact a caller
    stated.
  - *The first record wins*, as for every rerun under a stored id: a second
    `save` with another record, or with none, leaves the first.
  - *With `prefix_of`, `name` names the parent*: the run is a prefix of
    `name` at `parent_pipeline_hash`, cut at `up_to`, and never an earlier
    version of `name` (ADR-C39 § 2).
  - *The constructors require a name whenever `prefix_of` is set, and the
    reader stays tolerant.* The fields are private: `RunProvenance::named`
    and `RunProvenance::prefix` both take the name, and `Default` is the empty
    record, so no caller builds a prefix record without its parent's name.
    A file holding `prefix_of` and no `name` still reads, because the
    Pipeline matrix's cell rule (ADR-C39 § 6) reads only
    `parent_pipeline_hash`, and refusing the record would lose that.
  - *The reader tolerates unknown fields, and every backend owes it.* There
    is no `deny_unknown_fields`, so a field a later build adds is read past
    without a version bump; the concurrency degree
    (`docs/design/2026-09-29-front-end-design.md` § 7) is a future field of
    this record, not a third file. The conformance suite cannot put unknown
    bytes into a backend through the trait, so each backend proves this in
    its own tests: the file backend's is `an_unknown_provenance_field_is_ignored`.
  - *Stamped by the composition root.* `ragondin-harness` assembles the run
    with `provenance: None`, and `ragondin bench` sets it before saving — the
    only stamper in the tree today; ADR-C39 § 3 names the UI's launcher as
    the other one, and `ragondin-api` as never one. The conformance suite holds every
    backend to the round trip with a name and with a prefix, to inventing no
    record, to the first record winning, and to an older run reading none.
- **A run records when it ran, outside its identity.** `Run::times` is
  an `Option<RunTimes>` — `started` and `finished`, each a `UnixMillis`, whole
  milliseconds since the epoch. It is not a field of `RunInputs` and the run id
  does not digest it (INV-8): a run with times and the same run without them
  are one run. A leaf choice of this crate, recorded here:
  - *A file of its own, `times.json`*, holding `{"started_ms", "finished_ms"}`,
    staged with the rest of the run and renamed into place with it.
  - *Written only when known.* Unlike `bindings.json`, which is written for
    every run, `times.json` is absent when `times` is `None`: an absent file
    is what *unknown* means, and writing `null` would give one fact two
    spellings. `load` reads an absent file as `None` — never an estimate — and
    a file that does not parse as `Malformed`. The completeness check still
    requires only the four original files, so a run stored before the file
    existed is complete.
  - *Never the backend's clock.* No modification time, birth time or
    object-store `LastModified` is read for it, not even as a fallback: such a
    time says when a file was written rather than when the run ran, and it is
    not portable across `RunStore` backends. A time before the epoch is
    unknown too — `UnixMillis::from_system_time` answers `None`, never `0`.
  - *The first record wins, times included*, as for every rerun under a stored
    id; a rerun saved without times does not erase the stored ones.
  - *For display and ordering only.* A `finished` earlier than `started` is
    stored and read back as written; nothing validates or reorders it.
  - *Stamped by the composition root.* `ragondin-harness` assembles the run
    with `times: None`; the binary's execution path reads the clock before
    preparation and again when evaluation returns, and sets the field before
    saving. The conformance suite holds every backend to the round trip, to
    inventing no time, to the first record winning and to the reversed pair.
- **The configuration is kept verbatim, and the traces are opaque to the
  store.** The store writes the configuration document as it was handed in —
  the text whose canonical logical form hashes to the `pipeline` digest beside
  it — and never re-serializes one out of an in-memory pipeline type, which
  would put a second, drifting spelling of the configuration in the store. A
  trace is the harness's rendering of `ragondin-engine`'s `ExecutionTrace`
  (INV-10), held as a JSON document, `TraceDocument`, which `save` and `load`
  move without parsing (ADR-C28) — the conformance suite round-trips a
  document that is not a valid `Trace` to hold every backend to it. This crate
  does not depend on the engine; the document's shape is defined here, in
  `Trace` (§ The trace has one typed definition).
- **Metrics are recorded here, never computed here.** `ragondin-metrics` scores
  one query and the harness averages over a query set; a `Metrics` value is the
  figure a comparison puts side by side. The map takes any name — quality,
  cost and latency all land in it (§6.5) — and a stored name is kept whatever
  it is. `ragondin-metrics`' closed catalogue names the metrics the harness
  writes, with their family and direction; a stored name it does not know is
  still a metric here, with no direction.
- **A metric JSON cannot write is refused on the way in.** `serde_json` writes a
  non-finite float as `null`, and `null` does not read back as an `f64`, so a
  run stored with one would be unreadable for good under an id whose existence
  says it is done. `save` refuses it (`RunStoreError::NotFinite`) before it
  creates anything. It is not the per-query metrics that produce one —
  `ragondin-metrics` guards each of its zero denominators, `ndcg_at_k` returning
  `0.0` when nothing is relevant — but an aggregate has denominators of its own:
  a mean over an empty query set, or a cost-per-query where the count is zero,
  is `0.0 / 0.0`.

## `RunStore` is the seam; `FileSystemRunStore` is one backend

ADR-C36 § 2 extracts the trait here, the store's home, so that a reader —
`ragondin-api` — holds a backend it did not choose, and a cluster deployment
supplies another. What the trait promises is written on it, method by method,
and the conformance suite checks it; everything above about directories,
staging names and leading dots is the file backend's way of keeping that
promise, not part of it.

- **Synchronous, and `Send + Sync`.** The file backend is synchronous and its
  callers are blocking paths; the trait follows it. A backend that needs
  `async` is its own escalation (the async-trait decision is frozen), not a
  change this trait anticipates.
- **`save`, `load`, `ids`, and nothing else.** `compare` stays a function of two
  `Run`s, since a comparison does not care where its runs were kept;
  `FileSystemRunStore::compare`, loading two ids and comparing them, is kept as
  the file backend's convenience because `ragondin compare` calls it. There is
  no `exists`: nothing asks it yet, and `load` answers it.
- **The file backend's `save` and `load` stay inherent**, and its trait
  implementation calls them, so that a caller holding a `FileSystemRunStore`
  needs no trait in scope — the binary did not change when the trait arrived.
  `ids` is on the trait alone.
- **`ids` lists in ascending order of the hex rendering**, a choice made here:
  a listing wants a stable order, and this one means nothing, as a digest
  order should — which is why `RunId` still has no `Ord`. The file backend
  lists every directory whose name parses as a `RunId`, skipping staging
  directories, files, symbolic links (the store writes none) and other names,
  and reports an entry whose type cannot be read rather than skipping it; a
  root never created lists nothing.
  A torn directory is listed: its id names it, and `load` reports it
  `Incomplete`.
- **The conformance suite is a module behind a feature**, not a crate: its
  subject is this crate's trait and its fixtures are this crate's types, and a
  crate of its own would be a split the frozen crate granularity does not
  allow. `assert_run_store_conformance` takes three closures, a choice made
  here: `fresh` builds an empty store for each case, so a backend with
  external state makes one per case; `tear` damages a stored run, because the
  trait offers no way to and every backend can reach a torn run, so the suite
  checks that it is *reported* and leaves how it happens to the backend;
  `before` returns a store holding a run the backend kept before the launch
  record existed, and its id — the file backend's is the committed fixture —
  because only the backend knows how an older build of it laid a run out, and
  a run saved today without a record is a different case. It panics
  on the first failure, naming the case. `FileSystemRunStore` runs it in
  `tests/run_store_conformance.rs`, built only under the feature
  (`required-features`), which `just test-features` turns on.

## The trace has one typed definition

`Trace` (`src/trace.rs`) is the shape of a stored trace document: its nodes,
each with its id, input summaries, output summary, `duration_nanos` and
`error`, and a summary in each of the seven shapes the harness renders.
`From<Trace> for TraceDocument` is the rendering, and `TryFrom<&TraceDocument>
for Trace` reads it back. The harness writes through it and a reader parses
through it, so within one build the two cannot drift. ADR-C36 § 2 gives it
three rules, which this crate keeps:

- **One definition in Rust**, here, beside `TraceDocument` — the one crate the
  writer and every reader depend on. No engine type appears in it: the
  harness maps the engine's `ExecutionTrace` into it field by field (INV-2).
- **Reported, never repaired or guessed.** `Trace::try_from` refuses a missing
  field, a field the shape does not have, a summary in neither of its kind's
  two shapes, a value of the wrong type — an integer score included, since
  the rendering writes every score as a float — and a named chunk list whose
  `count` disagrees with its chunks, with a `TraceError` naming the node and
  the field. Strictness is what makes the conversion exact: every document it
  accepts renders back to the same JSON value, so a reader never shows a
  trace other than the one stored. (The same *value*, not always the same
  text: a number spelled unusually — `0.50` — reads as `0.5`, which is how
  the store writes it anyway.)
- **No version now; the first incompatible change adds one**, in the same
  change — which is a change to what the trace carries and escalates
  (`AGENTS.md` § Rules of engagement).

**A query's latency has one definition**, beside the trace it is read
from: `Trace::latency_nanos`, the sum of its nodes' `duration_nanos` — read
from the trace's values (INV-10), never from a log. The sum is checked, not
saturating: durations past `u64` nanoseconds, some 584 years, are a malformed
trace rather than a slow query, so the answer is `None` and no reader ranks a
clamped figure. Its median is `lower_median`, the lower of the two middle
values over an even count, so the figure reported is one that occurred. Both
are this crate's because both readers of a run's durations reach it —
`ragondin-api`'s listing and its comparison — and a definition each kept its
own copy of could drift between the two. Neither is written onto the `Run`:
latency is derived, and the store writes nothing derived.

Two choices made here. A chunk's score is an `f64` where the engine records
an `f32`: the harness widens it losslessly, as the hand-built renderer did, and
an `f64` reads any stored number back exactly. Counts and byte lengths are
`u64`, so the shape does not depend on the reader's pointer width.
`tests/trace.rs` loads a run the hand-built renderer stored
(`tests/fixtures/stored-before-typed-trace/`), parses every trace, and checks
that the typed shape renders the bytes on disk.

The store itself still never parses a trace: `Trace` is for the code on
either side of it.

## The walk has one definition

`src/walk.rs` holds ADR-C30 § 3's walk over a `LogicalPipeline`: `terminal`,
the one node no other node consumes, and `ranking_node`, the node whose output
is the ranking the retrieval metrics read — a terminal generator's context port
names a context builder, whose chunks port names it; a terminal builder is
entered at its own chunks port; any other terminal node is its own ranking.
Both go by port position, never by name.

It is here for the reason `Trace` is (ADR-C36 § 2, applied to a rule rather
than a shape): the harness walks to the ranking it scores when it writes a
run's metrics, and `ragondin-api` walks to the same node when it reads the
stored traces back, and INV-12 keeps `ragondin-api` off the harness. This
crate is the one both already depend on for the trace and the lowering. It is
not in `ragondin-pipeline`, whose public API is an INV-1 boundary: the walk is
written against that crate's existing public surface (`nodes`, `inputs`, `id`,
the `LogicalNode` variants) and adds nothing to it. Its companion rule, the
chunk-to-document fold, is `ragondin-metrics`' `documents_by_first_occurrence`,
which is where a metric's input is shaped.

Choices made here (`AGENTS.md` § Rules of engagement):

- **`WalkError` is this crate's own error, with the three stops of the
  pipeline's shape**: no single terminal node, a missing port, and a
  generator whose context port names no context builder. Whether the node
  found holds a ranking for a query is a question about a trace, and each
  caller asks it of its own trace type — the harness of the engine's
  `ExecutionTrace`, `ragondin-api` of the stored `Trace` — so it is not a
  variant here. The harness's `RankingWalkError` maps each stop to its
  variant of the same name and message, and keeps `NoRankedChunks` for the
  trace question; `ragondin-api` reads any stop as "no output ranking".
- **`ranking_node` borrows from the pipeline** (`Result<&NodeId, _>`): both
  callers hold the pipeline for the whole run, and the one that keeps the id
  clones it.
- **`terminal` is public too**, because the answer is read at the terminal
  node, and both callers read it there.

`src/walk.rs`'s tests are the walk's tests, moved from the harness when the
walk moved: a chunk-producing terminal, a generator, a terminal builder, a
generator fed by something other than a builder, a missing port, and several
terminals.

## The comparison lowers the stored configurations

A choice made in this crate (`AGENTS.md` § Rules of engagement), recorded here.

`compare` says which node-level parameters and `impl:` names two runs differ
in. It takes that difference between the two stored configuration documents
**lowered to `LogicalPipeline`**, never between their texts — two spellings of
one configuration are one configuration (the spirit of INV-8) — and it lowers
them from the kept text through `ragondin-config`'s `parse_document`, the one
definition of a document's load, which `LocalFile` runs over a file. INV-9 is
kept there: the text lands in `ragondin-pipeline`'s hand-maintained wire
schema and reaches the in-memory model only through the pass.

Why here and not elsewhere:

- **Through `ragondin-config`'s text entry point, not its `LocalFile`.** A
  stored run is text, and its path is this crate's internal layout, which no
  caller may name; `parse_document` takes the text and is synchronous. The
  edge from this crate to `ragondin-config` is drawn in
  `docs/code-architecture.md` § 4.3's graph, and that crate's closure is
  `ragondin-pipeline`, the YAML parser and two macro crates — no RPC or HTTP
  stack.
- **Not in the binary.** The binary would lower the same text the same way,
  and the comparison is this crate's (§ What lives here): the binary's
  `compare` is a packaging of it (ADR-C15 makes the one binary a packaging
  decision) and renders it.
- **A stored document that no longer lowers** is reported as
  `ConfigurationComparison::Unavailable`, naming the side and the reason —
  `parse_document` peeks the schema version first, so a run stored under a
  version this build cannot read says exactly that — and the metrics are
  compared regardless. The store itself still never parses a document.
- **The YAML format has one reader.** `ragondin-config`'s `parse_document`
  reads a configuration file's text for `LocalFile` and the verbatim copy a
  run kept for this crate; what is this crate's is only the sentence around
  its verdict.
- **The lowering is one public function, `lower_configuration`.** A reader of
  a stored run needs the same graph `compare` computes — `ragondin-api` draws
  a run's graph from it — so it calls this function rather than writing a
  third reader. Its `Err` is the reason in words, as `Unavailable` carries it.

What is compared, and what is not: every node's component family, its `impl:`
name (an extension node's `kind`, which is where its `impl:` lands on lowering)
and every key under its `params:`, identified by node id and key, sorted in
that order. The family is a key of its own, a choice made here: the canonical
form hashes a node's variant, and without it a node moved from `retriever` to
`extension` with the same `impl:` would differ in nothing listed. A node only
one run has shows each of its keys with the other side absent. The wiring — a
node's `inputs`, the pipeline's declared inputs — is not listed parameter by
parameter; whether the two canonical forms hash equal
(`LogicalPipeline::content_hash`) is carried beside the list, so two
configurations that differ only there are never reported identical.

## Several runs against a baseline

`compare_runs(baseline, others)` is the comparison the front end's Compare
screen reads, through `ragondin-api` (the design document § 3): the metric
table — every metric any run recorded, one value per run, the best of each
row and each run's delta to the baseline — and the configuration matrix —
every parameter, family and `impl:` included, whose value is not the same
in every run, absence included. The columns are the baseline, then the
others in the order given.

- **`compare` is its two-run case.** Both are built from one metric table
  and one configuration matrix, so the pairwise diff `ragondin compare`
  prints cannot drift from the comparison the UI shows. `tests/compare_runs.rs`
  holds the two to the same answer over two runs, and the binary's output did
  not change.
- **Runs of different benchmarks are refused**, by `NotComparable` naming
  the first such run and both `dataset_version`s: a metric is what a
  benchmark's ground truth allows, so two benchmarks' figures side by side
  compare the benchmarks. `compare` keeps answering for any two stored runs,
  as `ragondin compare` always has.
- **The ceiling is not here.** A baseline and at most four runs is the design
  system's — four run inks — and `ragondin-api` refuses a sixth; this
  function compares however many it is given.

Choices made here (`AGENTS.md` § Rules of engagement):

- **Which way a metric improves is `ragondin-metrics`' catalogue's**
  (`Direction::of`, re-exported here as `ragondin_experiments::Direction`
  rather than mapped onto a second enum, so a comparison and the catalogue
  name one type): every metric the harness records is better higher, and a
  name the catalogue does not know — a latency percentile among them — has
  no direction, `None`. The direction is part of each row, so a reader sees
  which one was used, and a row with none has no best (§ Local invariants:
  a stored name is kept whatever it is).
- **`best` and `deltas` are methods of `MetricRow`**, computed from its
  values rather than stored beside them: a row cannot hold a best value its
  values contradict. A tie names every run holding the value; a run that did
  not record the metric is never the best, and its delta is absent, never
  zero.
- **`ConfigurationMatrix::Unavailable` names the run and its column**, the
  first whose configuration does not lower: the column is what lets
  `compare` say *left* or *right* even when one run is compared with
  itself.
