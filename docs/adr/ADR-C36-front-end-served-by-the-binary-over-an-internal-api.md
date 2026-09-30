---
id: ADR-C36
title: The front end is served by `ragondin ui` from an internal crate that reaches no engine and no component, over a versioned JSON API that is not yet promised; replay resolves passages at read time against a verified dataset; the UI is a TypeScript application under `ui/`, governed like the workspace
status: accepted
invariants: [INV-1, INV-2, INV-4, INV-5, INV-6, INV-8, INV-9, INV-10, INV-11, INV-12]
supersedes: []
superseded_by: null
---

# ADR-C36: The front end is served by `ragondin ui` from an internal crate that reaches no engine and no component, over a versioned JSON API that is not yet promised; replay resolves passages at read time against a verified dataset; the UI is a TypeScript application under `ui/`, governed like the workspace

## Context

The architecture promises a front end with two load-bearing views — run
comparison and per-node execution replay — and calls the second the platform's
differentiating capability (ADR-12, ADR-16, `docs/system-architecture.md` § 6.5
and § 11.1). ADR-12 places it in the experiment plane and forbids it the data
plane; ADR-16 makes it composition, benchmarking and replay in one canvas, over
pipeline files that stay the source of truth. No document says how it is built,
and decision issue #327 asked the four coupled questions that must be answered
before its first issue can name the crates it touches:

1. where the front end runs and how it reaches a run;
2. what it consumes, and whether that becomes an API boundary;
3. what replay shows for a produced ranking;
4. which toolchain the repository takes on, and how it is governed.

Each escalates under `AGENTS.md` § Rules of engagement: an HTTP server would be a
new `[workspace.dependencies]` entry used by a crate outside `components/`; the
trace document is "what the trace carries"; a chunk's text in the record is an
accepted ADR (ADR-C28); and a second toolchain has no gate in a repository whose
every gate is Rust.

The repository owner has also set the front end's place: it is the platform's
main human entry point — composition included — and will be the human entry
point of the cluster deployment too, while the command line, the pipeline
documents and, later, the custom resources remain the machine entry points.

### What the tree has today, checked for this decision

- **The run store has no trait.** `runtime/ragondin-experiments` exposes
  `FileSystemRunStore` with `save`, `load` and `compare` and nothing behind it.
  Its `ARCHITECTURE.md` already says the directory layout is internal and freely
  replaceable, and that anything listing the store root must skip entries whose
  name begins with `.`.
- **The trace document is hand-rendered and unversioned.** The harness renders
  an `ExecutionTrace` into JSON by hand (`eval/ragondin-harness/src/trace.rs`),
  deliberately, so that the engine's internal shape never becomes a file format
  (INV-2). The store wraps it as `TraceDocument` and treats it as opaque. The
  configuration document beside it is versioned: `compare` peeks its schema
  version and reports a version this build cannot read. The trace carries no
  version at all.
- **What a produced ranking holds.** For every node that produced chunks, the
  trace names each one by chunk id, document id and score, in rank order, and
  deliberately not by its text (ADR-C28). A produced context carries its chunks
  and its full text, and a produced answer its full text (ADR-C31 § 5).
- **How chunks come from a corpus.** No chunker exists: the composition root
  builds a `CorpusIndex` with one chunk per document, carrying the document's
  whole text under the document's id (ADR-C26). `dataset_version` is a content
  digest over the parsed corpus, queries, judgments and reference answers, and
  `index_version` a digest over the chunk set; both are computed in
  `ragondin-harness`, which depends on `ragondin-engine`.
- **Where each piece the front end needs lives, and what it reaches.**
  `ragondin-experiments` depends on `ragondin-pipeline` and `ragondin-types`;
  `ragondin-config` on `ragondin-pipeline` and `ragondin-proto`;
  `ragondin-benchmarks` and `ragondin-metrics` on `ragondin-types` (with `serde`,
  `csv` and `serde_json` for the adapters). None reaches `ragondin-engine` or a
  component. `ragondin-harness` and `ragondin-server` both depend on
  `ragondin-engine`. `ragondin validate` needs no `EngineContext`: parsing,
  validation — the edge-kind check included (ADR-C16) — and the canonical hash all
  live in `ragondin-pipeline` and `ragondin-config`.
- **The HTTP server stack is already in the build.** `[workspace.dependencies]`
  names no HTTP server, but `tonic` 0.12, which `ragondin-proto` depends on
  unconditionally with its default `transport` feature, resolves `axum` 0.7,
  `hyper` 1, `tower-http`, `futures-util` and `tokio-stream` in `Cargo.lock`
  today. `hyper` is on the INV-4 deny-list.
- **`docs/code-architecture.md` § 4.1** describes `ragondin-experiments` as the
  crate holding "the API the UI consumes"; no crate holds one, and no document
  names an HTTP API.

The repository owner decided #327 on 2026-09-30, on the front-end design that
accompanies it. Decided in #327.

## Decision

**The front end is a browser application under `ui/`, served — assets and a JSON
API — by a `ui` subcommand of the one binary. The API lives in a new internal
crate, `runtime/ragondin-api`, which consumes the experiment plane through traits
and reaches no engine and no component (INV-12). Its `/api/v1` is built as a
contract — versioned, generated, golden-tested — and is not a stable API
boundary: its only consumer is the UI embedded in the same binary. Replay shows
what the record holds and resolves a passage's text at read time, only against a
dataset that digests to the run's own. The UI's toolchain is TypeScript, and it
carries the governance the Rust workspace already has, transposed.**

### 1. Where the front end runs: `ragondin ui`

- **A fifth subcommand, `ragondin ui`, behind a `ui` feature of `bin/ragondin`**
  (ADR-C15, ADR-C14). It serves the UI's static assets, embedded in the binary at
  build time, at `/`, and the JSON API under `/api/`. The browser reads no file:
  every run, pipeline, dataset and setting reaches it through the API. A build
  without the feature answers `ragondin ui` by saying it does not carry the UI.
- **The graph the canvas draws is lowered on the server**, by `ragondin-config`
  and `ragondin-pipeline`, from the stored or edited pipeline document. There is
  one implementation of the pipeline grammar, and it is Rust.
- **The binary is the composition root for the UI as for everything else.** It
  constructs the API's backends and passes them in; nothing in the API is a
  static or a global (INV-6's reasoning, applied to the backends). The binary
  implements the one trait through which the UI can cause execution, `Launcher`,
  by reusing `bench`'s path, and it answers through that same trait everything
  that needs the registry or a component: the build's capabilities (which
  implementations each family has, whether `remote` is on), a `Remote`
  service's identity probe, and a run's identity at submission.
- **A run's identity is announced at submission and decided at execution.** The
  launcher computes the `run_id` when a job is submitted, so an existing run is
  refused and an unreachable service fails at once. The id the store receives is
  the one the harness computes from what actually ran; if it differs from the
  one announced, the job reports the difference and never files a run under an
  id computed from something other than what ran (P4).
- **The server is local by default and defends its origin.** It listens on a
  loopback address unless the operator names another, and binding elsewhere is an
  explicit act the command announces as unauthenticated. It answers only requests
  whose `Host` names the address it serves, and refuses a state-changing request
  whose `Origin` is not its own — a local server that writes files and launches
  runs is otherwise reachable from any page the user's browser opens. It sends a
  `Content-Security-Policy` whose default source is `'self'`, so the browser
  itself refuses any other origin (§ 5).
- **The UI and the API it talks to are the same build.** The API reports the
  build's identity, and the UI compares it with its own at load and whenever its
  event stream reconnects; a UI that finds a different build reloads instead of
  continuing.

### 2. What it consumes: `ragondin-api`, and whether `/api/v1` is a boundary

- **A new crate, `runtime/ragondin-api`**: the router, the handlers, the request
  and response types, and the traits the service consumes — `PipelineSource`
  (the workspace's pipeline documents and their layouts), `Registry` (benchmarks:
  present, available from a manifest pinned by digest, importable),
  `WorkspaceSettings` (deployment data such as `Remote` bindings, never hashed)
  and `Launcher` (the job queue and everything above that needs the composition
  root). **`RunStore`** is extracted from `FileSystemRunStore` into
  `ragondin-experiments`, its home, and `FileSystemRunStore` becomes its first
  implementation. The file implementations of the other traits live in
  `ragondin-api`. A trait that is async uses `async_trait` (frozen decision).
- **Its types are its own.** Every request and response type is defined in
  `ragondin-api`, derives `serde` and `schemars`, and is converted to from the
  experiment plane's and the core's types — never a core type serialized
  directly, for the reason INV-9 gives for the wire format: the in-memory
  representation must stay free to move behind the surface a reader sees. Errors
  are typed in the crate (ADR-C13) and rendered as `application/problem+json`
  with a stable code, a message, a hint naming the action and, for a validation
  failure, the node and edge it concerns.
- **Built as a contract.** Every path is under `/api/v1`; the API's description
  (an OpenAPI document assembled from the `schemars` schemas) is generated and
  kept as a golden file, so a change to the API is a reviewed diff; the UI's
  TypeScript types are generated from it and a check fails when they are stale.
  No type on either side is written by hand where it can be generated.
- **Not a stable API boundary, today.** ADR-C21's test asks who would be broken
  by a change, and whether we compile them. The only consumer of `/api/v1` is the
  UI under `ui/`: built by this repository's single gate from the same commit,
  embedded by `cargo` into the binary that serves it, and refused by § 1's build
  check when it meets another build. Neither form ADR-C21 names is present — no
  code compiled elsewhere depends on the API, and no artifact that outlives its
  build is written in its shape. The Rust crate itself is consumed only by the
  binary. So the API changes with the UI, in the same pull request, like
  everything in-workspace, and owes no deprecation. This applies ADR-C21's test;
  it does not amend it. INV-1's list is unchanged.
- **Publishing it is a later decision, not a migration.** The day a client other
  than the embedded UI is supported — a script, another tool, a UI served apart
  from the binary — the API is a boundary under ADR-C21's own test, and that step
  is taken by a decision issue and its ADR. Because the API was built as a
  contract, that decision costs a promise, not a rewrite. Until then the command
  line and the pipeline documents are the scriptable contract.
- **The trace document needs no version for the UI's sake**, because no code
  outside the workspace reads it. Three rules make that true rather than hoped:
  the shape has **one definition in Rust**, in `ragondin-experiments` beside
  `TraceDocument` — the one crate both its writer (the harness) and its readers
  (`ragondin-api`) depend on — so the writer and the reader cannot drift inside
  one build; a stored trace the reader cannot parse is **reported, never repaired
  or guessed**; and the first change to the trace's shape that a stored trace
  could not satisfy **adds a version to the document in the same change** — a
  change that already escalates as "what the trace carries". The store continues
  to hold the document opaquely (ADR-C28).

### 3. INV-12: the API crate reaches no engine and no component

**`ragondin-api` must not depend, directly or through any other crate, on any
crate under `engine/` or `components/`, nor on `wire/ragondin-remote`.** This is
ADR-12 made mechanical: the crate that answers the browser cannot execute a
pipeline, construct a component, or call one — in process or over the wire. The
only path from the UI to the data plane is the `Launcher` trait, implemented by
the binary. It is **CI-enforced by closure**, the way INV-5 is: the check walks
the `--all-features` dependency graph, so `ragondin-harness` and
`ragondin-server`, which reach the engine, are refused through it.

It is a numbered invariant rather than a consequence of this ADR because a rule
agents read in `AGENTS.md` § Invariants and CI checks is the only form that
survives the locally reasonable change it exists to stop — here, depending on the
harness to reuse `bench`'s code. Its row, its check in
`scripts/check-invariants.py` and that check's test land in the pull request that
creates `runtime/ragondin-api`, so that the rule never describes a check that
does not exist. The check has one known blind spot, which that row must name:
`ragondin-proto` is reachable through `ragondin-config`, so a hand-built client
over its generated stubs is left to review.

Nothing in the API crate knows whether it runs on a laptop or in a cluster: the
binary picks each trait's backend, and a cluster deployment is a set of new
backends behind the same traits, never a rewrite of the crate.

### 4. What replay shows for a produced ranking

- **What the record holds**, unchanged: for every node that produced a ranking,
  the chunk id, document id and score in rank order (ADR-C28); the produced
  context's chunks and text, and the answer's text (ADR-C31 § 5). **ADR-C28
  stands, and nothing is added to the trace.**
- **A passage's text is resolved at read time, and only against the run's own
  data.** The experiment plane reads the run's benchmark from the datasets
  directory and shows a chunk's text only when that dataset digests to the run's
  `dataset_version` and the chunk set derived from it digests to the run's
  `index_version`. Otherwise it shows the ids alone and says why, distinguishing
  a dataset that is absent from one that differs. It never shows text from a
  corpus the run was not evaluated on. The digests and the chunk derivation have
  **one definition**, reachable by `ragondin-api` under INV-12 and used by the
  writer and the reader alike; a second implementation of either is not allowed.
- **This holds while chunks are derived outside the pipeline.** The day chunks
  are produced by a component inside a pipeline, a chunk's text can no longer be
  derived without executing that component, which INV-12 forbids the API crate.
  What replay shows for such chunks is then a new decision — recorded here so
  that it is met deliberately, not discovered.

### 5. The toolchain, and its governance

- **A TypeScript application under a top-level `ui/`**, outside Cargo: Vite,
  React, React Flow (xyflow) for the canvas, a typed client-side router for the
  URL state every view carries, CSS built from the design system's tokens, and no
  component kit.
- **The same gates, transposed.** A pinned Node version and a committed lockfile,
  installed with `npm ci` only. A `ui` CI job — lint, typecheck, unit tests,
  build — and `just check` grows a `check-ui`, so the single gate covers both
  worlds. A licence and advisory audit whose licence allow list is `deny.toml`'s
  and whose advisory exceptions carry a date, a reason and the issue that removes
  them, as `deny.toml`'s do. `ui/DEPENDENCIES.md` lists every runtime dependency
  with its role and its reason — the `[workspace.dependencies]` rule transposed:
  adding one is named in the pull request under its own heading, and one that
  duplicates a role already filled (a second graph library, router or state
  store), or a component kit, escalates as a duplicated utility role does in
  `AGENTS.md` § Rules of engagement. `ui/ARCHITECTURE.md` exists from the first
  pull request that creates `ui/` — the load-bearing-crate rule applied to a
  directory that is not a crate but is load-bearing in the sense that matters.
- **ADR-12, mechanical on the browser side.** Two layers, and the first is the
  one that holds: the server's content security policy makes the browser refuse
  any origin but the UI's own — for the UI's code and for every dependency it
  bundles — so fonts and every other asset are served by the binary, never
  fetched from elsewhere. The second is a lint confining every network primitive
  (`fetch`, `XMLHttpRequest`, `WebSocket`, `EventSource`) to `ui/src/api/`, with a
  test that the module's base address is relative. The lint is a best-effort
  scan and is documented as one.
- **The Rust build does not need Node.** A build with the `ui` feature whose
  assets have not been built compiles and embeds a page saying so; the job that
  produces a shipped binary builds the assets first, and a test asserts that the
  binary carries them. `cargo build --all-features` therefore stays a Rust-only
  command, and every recipe of `just check` that resolves `--all-features` keeps
  working without the UI built.

### 6. The dependencies this ADR admits

- **`axum`**, on the 0.7 line `tonic` 0.12 already resolves, so `Cargo.lock`
  gains no second server stack; used by `ragondin-api` only. It is Tower-based:
  its stack is the UI server's network envelope, the same place ADR-C10 puts
  Tower for serving, and no handler, trait or component is a `tower::Service`
  (INV-11). Moving it to another line moves with `tonic`, and escalates.
- **`schemars`**, used by `ragondin-api` only.
- **`rust-embed`**, used by `bin/ragondin` only, behind its `ui` feature.
- **`tokio`'s `net`, `sync` and `time` features**, added to the existing
  workspace entry, because `ragondin-api` names the listener, the job-state
  channels and timers in its own code rather than borrowing them from another
  crate's features.
- **One stream utility for the event stream** — `futures-util` or
  `tokio-stream`, both already in `Cargo.lock` through `tonic` — chosen by the
  implementing pull request and named there.

Any other entry the implementation finds necessary is a new decision.
`hyper` is on the INV-4 deny-list, so the core already cannot reach `axum`.
Each entry's version, feature list and `default-features` setting are the
implementing pull request's to choose within this list, and are argued in the
entry's comment in the root `Cargo.toml`.

### 7. What this ADR does not reach

The exact paths and shapes of the API beyond what the sections above require;
the client router library; the layout and stage-pairing file formats; the
benchmark manifest's entries, their URLs and licences; the file backends' module
layout; the job queue's internals — one worker, and the concurrency degree
recorded as provenance the day it is not one; the harness's per-query progress
observer and cancellation token, which stay inside the harness, cancel only
between queries, leave nothing in the store for a run that did not finish, and
change neither the trace nor the order in which nodes run (otherwise they
escalate as what a driver observes of its execution). Prefix runs — "run up to
this node" — are ordinary runs of ordinary pipelines with their own identity; if
producing one needs any change to `ragondin-pipeline`'s public API, that change
escalates under INV-1 as any other would. Authentication, several users, a
remote store and cost accounting are outside this ADR. What backs each trait in a
cluster is the cloud-native milestone's to decide, and nothing here touches the
controller or `docs/OPEN_QUESTIONS.md` § 2.

## Alternatives rejected

**1. Where the front end runs**

- **A static page that reads the store's files itself.** No new Rust dependency.
  Rejected because the page would have to parse and lower pipeline documents —
  a second implementation of `ragondin-pipeline`'s grammar in another language,
  drifting with every schema version — and every file in a run directory would
  become a public format read by code the workspace does not compile, the
  layout `ragondin-experiments` records as deliberately internal among them.
- **A separate process or binary for the UI.** Isolates the UI's dependencies.
  Rejected because it reopens ADR-C15, which makes `ragondin` the user-facing
  surface, and it still needs the same HTTP server — it buys the isolation INV-12
  already gives, at the price of a second thing to run.
- **A terminal UI.** No browser and no second toolchain. Rejected because replay
  is a rendered graph with execution overlaid and ADR-16 makes the same canvas
  editable; a terminal draws neither well, so it would be a front end to replace,
  not to extend.
- **The API in `ragondin-experiments`**, where `docs/code-architecture.md` § 4.1
  placed "the API the UI consumes". Rejected because `ragondin-harness` depends on
  `ragondin-experiments` to write runs: an HTTP server there would enter the
  evaluation driver's closure — the UI's machinery inside the data plane's
  dependency graph, ADR-12 in reverse. A new crate is the one place that keeps
  both INV-12 and the harness's closure clean. It is a new crate, not a split of
  an existing one; the frozen rule on crate granularity is not reopened.

**2. What the front end consumes**

- **Version the trace document and let the UI read it** (#327's 2a). The smallest
  change. Rejected because it turns the harness's hand rendering into a public
  format, so the UI's needs would start to shape the record; and the UI needs the
  graph, the trace, the metrics and the dataset joined, which a set of files
  cannot give it without a second implementation of the joins.
- **The UI in-workspace, reading the files, with no API layer** (#327's 2c on its
  own). No boundary and no version. Rejected because it still needs the static
  page's second grammar, and a run stored by one build and read by a later UI is
  a cross-version read in a language the workspace does not compile. Its stance —
  what is in-workspace owes no promise — is kept, and applied to the API instead.
- **Declaring `/api/v1` a stable boundary now.** Rejected because nobody outside
  the embedded UI depends on it, so a promise would buy nothing and would price
  every change the UI needs as a breaking one while the UI is being built.
- **Hand-written API types on either side.** Rejected because the Rust and the
  TypeScript types would drift without a failing check; generation plus a
  freshness check makes the drift a red build.

**3. What replay shows for a ranking**

- **Ids, documents and scores only** (#327's 3a). No moving part. Rejected
  because a retriever's or reranker's output would be readable only through the
  final context, and replay's point — seeing what each stage returned — would be
  lost for every stage but the last.
- **The chunk text stored in the run** (#327's 3c). Self-contained replay.
  Rejected because it supersedes ADR-C28 for a record that would grow with the
  corpus, `top_k × nodes × queries × chunk size`, the growth that ADR rejected;
  and the text is already recoverable, verifiably, from the dataset the run's
  identity names.
- **Resolving text whenever a dataset is present, unverified** (#327's 3b as
  written). Rejected because a re-downloaded or edited dataset under the same
  path would put another corpus's text under this run's ids with nothing to say
  so. The digests are already in the run; checking them is what makes the text
  the run's.

**4. Toolchain**

- **Rust compiled to WebAssembly.** One language, and `cargo` audits it.
  Rejected because no Rust library matches React Flow for an editable node
  canvas, it adds a large framework and a `wasm32` target to CI, and a browser
  build depending on `ragondin-pipeline` would be a new, unplanned consumer of an
  INV-1 boundary compiled elsewhere.
- **Server-rendered HTML from the binary.** No second toolchain. Rejected because
  replay and the editor are interactive canvases; it would cap at the tables and
  force a rewrite at the first view that matters.
- **A server-side JavaScript framework (Next.js).** Rejected because its value —
  server rendering, API routes, a Node process — is irrelevant to a local tool or
  duplicates the Rust API, and a second process to run and secure goes against
  ADR-C15.

**5. INV-12**

- **A consequence of this ADR rather than a numbered invariant.** Rejected
  because a consequence is read once and a CI row is checked on every build, and
  the change it stops is locally reasonable every time it is made.
- **Denying `engine/` and `components/` only.** Rejected because
  `ragondin-remote` is the adapter through which a component is called over the
  wire; an API crate that could reach it could execute a component without the
  engine.

## Consequences

- **The front-end milestone's issues can name their crates.** The API and its
  traits land in `runtime/ragondin-api`; `RunStore` in `ragondin-experiments`;
  the progress observer and cancellation token in `ragondin-harness`; the
  subcommand, the `Launcher` implementation and the embedding in `bin/ragondin`;
  the manifest in `ragondin-benchmarks`; the application in `ui/`. The
  two-crate rule per issue holds throughout.
- **The pull request that creates `runtime/ragondin-api` carries**, by
  `AGENTS.md`'s documentation rule: the crate's `ARCHITECTURE.md`, opening with
  "not an API boundary" and stating § 2's status of `/api/v1`; INV-12's row in
  `AGENTS.md` § Invariants, its check and that check's test; the invariant count
  wherever prose states it (`CONTEXT.md`, `CONTRIBUTING.md`,
  `docs/code-architecture.md` § 5 and the `architecture-invariants` skill); the
  new member in `docs/code-architecture.md` § 4.1 and § 4.3; and the new entries
  in `[workspace.dependencies]` with their comments.
- **The pull request that creates `ui/` carries** `ui/ARCHITECTURE.md`,
  `ui/DEPENDENCIES.md`, the CI job, the audit and its policy, `check-ui` in
  `just check`, and the `ui/` rules in `AGENTS.md`.
- **`just check` needs Node from then on.** The Rust build does not (§ 5), but
  the single gate does, because it covers both worlds; `CONTRIBUTING.md` says so
  in the same pull request.
- **`docs/code-architecture.md` § 4.1 is corrected in this change**: it no longer
  says `ragondin-experiments` holds the API the UI consumes.
- **INV-1 and INV-2 are unchanged.** `ragondin-api` is internal by ADR-C21's
  test; the engine's internals stay out of every surface the UI sees, since the
  API crate cannot reach them (INV-12) and serializes only its own types.
- **INV-4, INV-5, INV-8, INV-9, INV-10 and INV-11 are untouched.** The core
  gains no dependency; the engine none; the canvas's output hashes over the
  canonical logical form and goes through the wire schema (ADR-16); the trace
  remains the executor's return value and the UI reads it from the store, never
  from telemetry; no component or trait becomes a `tower::Service`.
- **ADR-C28 and ADR-C31 § 5 stand.** The trace gains nothing; the harness's
  rendering moves to, or is tested against, the one definition § 2 places in
  `ragondin-experiments`, with a golden test proving the stored bytes unchanged.
- **The serving driver's HTTP face, when it is built, meets this server.** A
  second HTTP server in `[workspace.dependencies]` would duplicate a utility role
  and escalate; using `axum` there too needs no new entry.
- **The front-end design is bound by this ADR where they differ**: its
  `dataset_missing` case becomes two — absent and differs; the server's origin
  defences, the build-identity check and the content security policy are
  requirements, not options; fonts are served by the binary.
- **No entry in `docs/OPEN_QUESTIONS.md` is opened, closed or changed by this
  ADR** (ADR-16 narrows § 7), and no frozen decision is reopened.

## Status

Accepted.
