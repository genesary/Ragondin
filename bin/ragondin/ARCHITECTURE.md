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
| `tests/bench.rs` | `bench`, exercised as a process, over a miniature BEIR fixture |
| `tests/vertical_slice.rs` | The composition root assembled for real, end to end |

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
