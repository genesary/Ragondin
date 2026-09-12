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
| `tests/cli.rs` | `validate` and the rest of the command line, exercised as a process |
| `tests/compare.rs` | `compare`, exercised as a process, against runs written straight into a store |
| `tests/vertical_slice.rs` | The composition root assembled for real, end to end |

**Four subcommands are declared; two are implemented.** `validate` loads a
configuration and prints its content hash. `compare` reads two runs already in
a run store and prints their diff. `bench` and `serve` parse their arguments
and refuse. Declaring all four is deliberate rather than premature: ADR-C15
makes the set of subcommands the product's surface, and a surface discovered
one subcommand at a time is one a user has to rediscover at each release.

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
- **Heavy backends arrive optional and feature-gated (ADR-C14).** The
  `[features]` table is empty today and is the place a backend is switched on
  when the component that needs it lands. The default build stays lean.
- **`compare` reads; it never executes (ADR-C15).** It loads two runs by
  `run_id` from `ragondin-experiments`' `FileSystemRunStore` and hands them to
  that crate's own `compare()`; the diff it prints is that function's result,
  rendered. No metric is computed here and the engine is never touched.
- **`compare --store` is a required flag, not a default path.** No default run
  store location is settled anywhere in `docs/` yet — `bench` and `serve` do
  not exist to need one either — so this crate does not invent one. This is
  the kind of choice `AGENTS.md` § Rules of engagement leaves to the crate
  ("how a knob is exposed"): recorded here rather than escalated, and open to
  revisiting once a subcommand that writes to the store exists and a shared
  default becomes worth settling.

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

## Not here

- **No `EngineContext`, and no component registration.** The registration the
  binary will do lives, today, in `tests/vertical_slice.rs` — the composition
  root assembled for real, with stub components, through the ordinary public
  `register_*` API. It arrives in `main` with the subcommand that first needs to
  execute a pipeline.
- **No serving.** `ragondin-server` is a declared dependency and an unbuilt
  driver; `serve` refuses. The Tower envelope is out of M0–M2 entirely.
- **No metric, no benchmark adapter, no run store.** Those live in `eval/` and
  `runtime/`, and a subcommand reaches them rather than restating them.
