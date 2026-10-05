# The front end — design

**Status: design, approved on 2026-09-29; decided on 2026-09-30; its exit criterion (§ 2) mechanised on 2026-10-05**, by `bin/ragondin/tests/ui_parity.rs` for parity, the journeys under `ui/e2e/` (`just test-ui-e2e`) on the real binary over the fixture workspace of § 9, and the ignored `bin/ragondin/tests/journey_scifact.rs` (`just journey-scifact`) over SciFact itself. This document records what the front end is, how it is built and why. Decision issue #327 resolved into ADR-C36, which decides the answers of § 10 and the invariant INV-12; ADR-016 supersedes ADR-014; the roadmap change is governance and has no ADR. Where this document and an ADR differ, the ADR wins — the amendments the decision brought are folded in below. `AGENTS.md` § Rules of engagement applies: an agent implementing from this document does not get to reopen its choices, and does not get to make the ones it leaves to the decider.

Two artifacts accompany it and are not repeated here:

- **The design system** — delivered separately, and committed into `ui/` by sub-project 0 as its tokens, components and guidelines, so that every UI issue can cite it from the tree: type, colour in both themes, thirty-odd components with their states, and composed screens.
- **The functional mockup** — delivered separately: the six screens with sample data, kept as a reference for *what* each screen does. Its visual style is superseded by the design system and is not a reference for anything.

## 1. What this is, and why now

The architecture promises a front end with two load-bearing views — run comparison and per-node execution replay — and calls the second the platform's differentiating capability (ADR-012, ADR-014, `docs/system-architecture.md` § 6.5, § 11.1). No milestone carried it. This design makes the front end the next milestone, **M4 — The front end**, and shifts the rest of the roadmap by one: the calibrated judge becomes M5, control flow M6, cloud-native M7, custom benchmarks M8. `docs/OPEN_QUESTIONS.md` cites M6 and M7 by number and is corrected in the same change.

Two arguments carried the ordering. Every milestone so far shipped its user-facing surface with it (`compare` in M2, its parameter diff in M3), so a front end deferred to a late milestone contradicts how the project already works — and is the fate that befell visual authoring, parked in a "post-M7 horizon". And the replay view is how qualitative defects — a degenerate fusion, a reranker discarding everything, an absurd assembled context — become visible; metrics average them away.

Two things this design changes about accepted decisions, and which therefore need their own ADRs:

- **ADR-014** places visual graph *editing* in a later trajectory and makes YAML-first authoring the v0 path. This milestone builds the editor now. YAML stays the source of truth and the git-able object (§ 5 below); what changes is *when* the canvas arrives, not what it edits. ADR-016 supersedes it, in its own PR (`docs/adr/README.md` process rules 3 and 4).
- **#327** asked four coupled questions about the front end. § 10 lists the answers this design gives.

**Roadmap rule**, to be written into `docs/AGENT_WORKFLOW.md`: *every milestone that adds a user-visible capability includes its UI slice in its exit criterion.* M5 brings judged metrics and their intervals into Compare and the judge into the palette; M6 brings Branch and Loop into the canvas and the branch taken into Replay — and resolves `docs/OPEN_QUESTIONS.md` § 7 at that point, not before.

## 2. Scope

### Exit criterion of M4

> **Parity.** Everything the CLI does, the UI does without a file or a terminal: `bench` with its three benchmark formats (`beir/`, `beir-qa/`, `squad/`), its `Remote` bindings and its store; `compare`; `validate` (the canonical hash and the located edge errors, shown without running). And the converse holds: a pipeline composed in the UI and exported as YAML hashes identically under `ragondin validate` — the proof that there is one representation.
>
> **Journey.** `ragondin ui` with no argument, on an empty workspace: download SciFact, compose a hybrid pipeline, launch it, compare it with dense-only, replay one query node by node in both.

`serve` is outside parity: it does not exist in the CLI yet.

### Six sub-projects

Each gets its own plan and its own issues; the two-crate rule per issue holds throughout.

| # | Sub-project | Delivers | Depends on |
|---|---|---|---|
| 0 | Toolchain and governance | `ui/` (TypeScript, Vite, React, React Flow), CI, npm audit, the ADR-012 lint, the design system as tokens and components | — |
| 1 | Service and API | `runtime/ragondin-api`, `ragondin ui`, a `RunStore` trait, `/api/v1` built as a contract and not yet promised | 0 |
| 1b | Registry and settings | workspace, pinned benchmark manifest, services with identity probe, first launch | 1 |
| 2 | Viewer | Runs grouped by pipeline, the Pipeline matrix, Compare at N with charts and a baseline, stage pairing, Replay per node and side by side, per-node metrics read from traces | 1 |
| 3 | Launcher | a one-run queue, SSE progress, cancellation, refusal of an existing hash, prefix runs | 1b, the composition root |
| 4 | Editor | the React Flow canvas, this build's palette, live-validated parameters, YAML files and layouts, fork from a run, "Run up to this node" | 2 |

Order: 0 → 1 → 1b → 2 → 3 → 4. The viewer precedes the editor because its renderer is half the editor and is proven on runs that already exist; the launcher precedes the editor because launching an existing YAML is a small step over the viewer and closes the launch → look → compare loop early.

### Out of scope, explicitly

Authentication, multi-user, a remote store, cost accounting (nothing records it), Branch and Loop, the judge, corpus or question generation. Each has a later milestone.

## 3. UX and design

The design system carries the detail; this section records what it commits the product to.

**Thesis.** *Ragondin is an instrument for reading rankings.* The chrome is graphite and mute; every hue on screen is data a legend explains. The signature is the **rank strip** — ten cells that fill at the ranks where the gold passages landed — on every node in replay, in drill-downs, on the cover. Depth is reserved for what moves (node cards) and what floats (menus, panels); everything else is flat, separated by rules.

**Six screens, widest to narrowest.** Runs → Pipeline → Compare → Replay → Editor → Setup. One primary action per screen. Every view has URL state (`#compare/…`, `#replay/…/q/…`) so a link pasted into an issue reproduces it. The workspace indicator is always visible.

**Three journeys dictate the information architecture.** *First run*: Setup → download SciFact → Editor, opening on a retrieval-only example that runs without any service → Launch → Runs. *Iterate*: Runs → Fork → one parameter → Launch → Compare, in four clicks. *Investigate*: Compare → histogram → "Replay the N regressions" → side by side → the node that changed; three screens, and the context follows.

**The evaluation model, made visible.**

- A run is one pipeline on one benchmark; its metrics are what that benchmark's ground truth allows (ADR-008). **Pipeline** shows this as a matrix of node × benchmark: the value, the gain over the previous ranking stage — the gain is what is emphasised, not the value, because benchmarks differ in difficulty — and an empty cell that says why, in the cell: "no qrels", "no reference answers", "not run yet" with its button. A column is the most recent run on that benchmark; history lives in Compare.
- **Compare** aligns pipelines by **stage** — retrieval legs → after fusion → after rerank → final ranking → answer — never by node id. A missing stage breaks the line and reads "no stage here", the same treatment as an absent node in side-by-side Replay. The pairing is corrected by hand in "Pair nodes…", remembered per pair of pipelines within the workspace, and the stage label then describes what is compared. Ceiling: baseline plus four runs; a sixth is refused rather than given an invented colour. Compare ends on the verdict sentence, then one action.
- **The per-query histogram** in Compare diverges from zero, and its bins belong to the product, not to a screen. A query's delta is its score in the run minus its score in the baseline — absolute, never relative to the baseline's score — and the same edges apply to every ranking metric alike. Seven bins: below −0.3; −0.3 to −0.1; −0.1 to 0; **unchanged**; 0 to 0.1; 0.1 to 0.3; above 0.3. A value on an edge goes to the bin nearer zero — −0.3 is in the second bin, 0.1 in the fifth — so the bins are symmetric about zero. **Unchanged is a delta of exactly zero**, with no tolerance: a per-query score is a deterministic reading of a stored trace, so a ranking left as it was scores exactly the same, and a tolerance would be a threshold nobody chose. *Decided on 2026-10-01.*
- **Prefix runs** ("Run up to this node", from the node menu or the inspector, stating what is skipped and the cost avoided) live in their parent pipeline's group, labelled.
- Adding a judge changes a run's identity, on purpose (ADR-009; the judge's model, prompt and seed are experiment variables). The UI shows the judge as an evaluation node, distinct in the palette.

**The canvas.** One component in read mode (Replay) and write mode (Editor) — ADR-014 made literal. Node cards in fifteen states; ports typed **by shape**, not colour; an invalid edge refused during the drag, with the reason; a parameter error on the node *and* in the inspector; Branch and Loop neutral, on a diamond tile; a reserved slot for the judge. Pan, zoom, automatic layout on first open, positions remembered after. **No Save button**: continuous saving, every change undoable; the implementation debounces and rewrites the YAML file only when it validates, so git does not see every keystroke. The editor **never overwrites a file changed on disk** since it last read it, and before the first canonical rewrite of a hand-written file (comments, formatting) it says so and offers a new file instead. Fonts and every other asset are served by the binary.

**Colour and data.** Two palettes separated by lightness: node families are quiet pigments (contrast deliberately under 3:1, always beside a glyph and a name), runs are inks with a letter A–D, the baseline a dashed neutral outline. **No chart mixes the two.** Better/worse and good/warning/critical are reserved. One scale per chart, never two axes; a legend always present; colour never the sole carrier (★, strikethrough, dashes).

**Type.** Wix Madefor Display and Text as an optical pair; Atkinson Hyperlegible Mono for hashes and ids; tabular figures; 13–14 px body. No logo yet — the name is set in type.

**States.** Empty (one sentence and the action), in progress (real progress, never a bare spinner), error (what failed, where, what to do — inline, section or toast, never a modal), loaded. A failed run is a first-class object. The application's two confirmation dialogs: deleting a workspace, and the first canonical rewrite of a hand-written pipeline file.

**Accessibility, keyboard, motion.** Visible focus everywhere; the canvas navigable by keyboard (tab, enter, Shift+F10); `prefers-reduced-motion`; motion only answers an action and explains a change. Readable on a phone; editing made for a wide screen, and says so.

**Voice.** English, short sentences, active voice, sentence case; a control says what happens ("Launch" → "Queued"); an error says what happened and how to fix it.

## 4. Architecture

```
ui/ (TypeScript) ──HTTP /api/v1──▶ runtime/ragondin-api ──traits──▶ bin/ragondin (composition root)
                                        │                                │ implements Launcher
                                        │ depends on                     │ with harness + engine
                                        ▼                                ▼
                  experiments · pipeline · config · benchmarks · metrics       engine · components
```

**`runtime/ragondin-api`** — a new, internal crate: the axum router, the handlers, the response types (`serde` + `schemars`), and the **traits** the service consumes: `RunStore`, `PipelineSource`, `Registry`, `Launcher`, `WorkspaceSettings`. It depends on `ragondin-experiments` (the store), `ragondin-pipeline` and `ragondin-config` (lowering a YAML to a graph, validating), `ragondin-benchmarks` (the registry, the qrels), `ragondin-metrics` (per-node metrics recomputed from traces). **Never** on `ragondin-engine`, never on anything under `components/`.

**INV-12, CI-enforced by closure like INV-5** (ADR-C36 § 3): *`ragondin-api` reaches no crate under `engine/` or `components/`, nor `wire/ragondin-remote`* — that crate calls a component over the wire. Its `AGENTS.md` row, its check and the invariant counts land in the PR that creates the crate, so no rule describes a check that does not exist yet. Known blind spot, left to review: `ragondin-proto` is reachable through `ragondin-config`. This is ADR-012 made mechanical: the API crate cannot execute anything; only the binary, by implementing `Launcher`, connects the UI to the data plane.

**`ragondin-experiments`** — extract a `RunStore` trait; `FileSystemRunStore` becomes its first implementation (today it is the only type, with no trait behind it); extend `compare` to N runs with a baseline. **`ragondin-harness`** — a per-query progress observer and a cancellation token, checked between queries; the harness runs as one block today and the launcher needs to see it advance and stop it. This is the only change to the harness. **`bin/ragondin`** — a fifth subcommand `ui` behind a `ui` feature (ADR-C15; `serve` stays the data plane), the `Launcher` implementation reusing `bench`'s path, the assets embedded with `rust-embed`; a lean build's `ragondin ui` says the build does not carry the UI.

**`ui/`** — outside Cargo, its own toolchain. It knows one address — its own. Its API types are **generated** from the Rust crate's JSON Schema, never written by hand.

**Every "local" thing has a cluster twin.** This is the pattern the repository already uses for `ConfigSource` (`LocalFile` | `Stream`), and P2 — standalone first — requires the local instance to work with no cluster.

| Locally (the workspace) | In cluster (M7) | Unified by |
|---|---|---|
| runs: one directory each | object store or persistent volume | `RunStore` |
| pipelines: `pipelines/<name>.yaml` | custom resources — ADR-007 already makes the CR the serialization of the pipeline | `PipelineSource` |
| layout: `<name>.layout.json` | an annotation on the CR | UI metadata, never hashed |
| services: `workspace.toml` | bindings from the deployment, via the controller | ADR-C32: the address is deployment data; the Services screen becomes read-only |
| benchmarks: a directory and the manifest | object store and the same manifest | `Registry`; the digest is the identity everywhere |
| launcher: tokio tasks in-process | submit a run CR, watch the controller | `Launcher` |

**Rule**: `ragondin-api` depends only on traits; the binary picks the backends. Nothing in the API crate knows whether it runs on a laptop or in a pod. M7 is a set of backends, not a rewrite.

**Dependencies admitted by ADR-C36**: `axum` on the 0.7 line (the one `tonic` 0.12 already resolves, so no second server stack enters the lockfile), `schemars`, `rust-embed` (the binary only), the `tokio` features `net`, `sync` and `time`, and one stream utility already in the lockfile. Anything else is a new decision. **Build capabilities, the identity probe and the submission `run_id` all go through `Launcher`**: the API crate never touches a component itself.

## 5. The API — `/api/v1`

**Principles.** JSON, `snake_case`; every response type derives `serde` and `schemars`; the API description is assembled from the `schemars` output, kept as a golden file, with no description-generator crate; `just gen-ui-types` produces the TypeScript types and a CI recipe checks they are current, as `check-adr-index` does for the ADR index. Errors are `application/problem+json` with a stable `code`, a message, a `hint` naming the action, and — for a validation error — the **location** (node, edge). The server serves the assets at `/` and the API only under `/api/`. **It listens on loopback only**: binding to any other address is refused until an authentication layer exists (cloud-native, M7); a remote machine is reached through an SSH tunnel, which keeps the loopback and brings its own authentication. The server answers only requests whose `Host` names the address it serves, refuses a state-changing request whose `Origin` is not its own, and sends a `Content-Security-Policy` whose default source is `'self'` — the main enforcement of ADR-012 in the browser, since it also covers bundled dependencies. The API reports the build's identity; the UI compares it with its own at load and on every reconnection, and reloads rather than keep talking to a different build.

**Status of the API.** Built as a contract — versioned, generated, no ad hoc types — and **not yet promised**: it changes with the UI in the same PR, like everything in-workspace. The CLI and the YAML are the scriptable contract meanwhile. Publishing the API is a later decision, not a migration.

**Resources**, aligned on CLI parity:

| Resource | Does | CLI |
|---|---|---|
| `GET /workspace` | path, settings, version, **build capabilities** (family → local impls, whether `remote` is on) | — |
| `GET /runs` · `GET /runs/{id}` | list; detail = inputs, metrics, config, bindings, the **lowered graph** (nodes, edges, kinds — computed by `ragondin-pipeline`, never by the browser), prefix-of relation | `bench` (read side) |
| `GET /pipelines/{name}/matrix` | the node × benchmark matrix over that pipeline's runs | — |
| `GET /runs/{id}/queries` | the queries with their per-query scores, computed from traces and qrels, and the **per-node metrics** — here rather than on the detail, which reads the store alone and would otherwise load and digest the dataset for every run shown. A node row carries ranking metrics only; the generator's EM and F1 are `metrics.json`'s, or the mean of the per-query scores | — |
| `GET /runs/{id}/trace/{query}` | one query's trace, node by node, with **passage text resolved** from the dataset only when the dataset on disk digests to the run's `dataset_version` and the chunk set to its `index_version`; ids alone otherwise, flagged, saying whether the dataset is absent or different | — |
| `POST /compare` | `{run_ids, baseline, pairing?}` → table, parameter matrix, per-query deltas, per-stage metrics with the pairing used | `compare` |
| `GET/PUT /pipelines/{name}` · `GET/PUT …/layout` | the YAML and its layout | — |
| `POST /pipelines/validate` | canonical hash and located errors, without running | `validate` |
| `POST /runs` | submit `{pipeline, benchmark, bindings, up_to?}` → `202 {job_id, run_id}`, or `409` when that `run_id` already exists in the store or the queue | `bench` |
| `GET /jobs` · `DELETE /jobs/{id}` · `PATCH /jobs/{id}` (reorder) · `GET /jobs/events` (SSE) | the queue, cancellation, reordering, the stream `queued → running {done, total} → done \| failed \| cancelled` | — |
| `GET /benchmarks` · `POST /benchmarks/{name}/download` · `POST /benchmarks/import` | the registry: ready, available (manifest), local import; a download is a job that verifies the digest | — |
| `GET/PUT /services/{family}/{name}` · `POST …/probe` | the bindings and the **identity read** — the same code the composition root runs before a run | `--remote` |

The `run_id` is **announced at submission and decided at execution**: computing it at submission requires constructing the components and reading the `Remote` identities (ADR-C32), so an existing run is refused and an unreachable service fails at once, not after an hour; the id the store receives is the one the harness computes from what actually ran, and if it differs from the announced one the job reports the difference and never files a run under the announced id (P4).

**The trace document** keeps its hand-rendered shape and gets no version now, under three rules (ADR-C36): its shape has one Rust definition in `ragondin-experiments`, which both the harness and `ragondin-api` compile against; a stored trace that cannot be read is reported, never repaired; the first change to the shape that a stored trace could not satisfy adds a version in the same change.

## 6. Storage — the workspace

```
<workspace>/
  workspace.toml                datasets directory, services (family/name → uri) — deployment data, never hashed
  pipelines/<name>.yaml         the source of truth, git-able; what the editor opens and saves
  pipelines/<name>.layout.json  node positions, never in the hash
  pipelines/<name>.pairing/     stage pairings by hand, keyed by the other pipeline's name
  layouts/<hash>.json           the layout copied at launch, so a run's replay has it
  runs/<run_id>/                the store as it is today (inputs, metrics, config, traces, bindings)
  jobs/<job_id>.json            the queue's state; a run in progress lives here, never under runs/
  cache/                        reconstructible derived data (per-query scores, per-node metrics) — never a truth
  datasets/                     the default when workspace.toml names none
```

`runs/` is the store unchanged: `ragondin bench --store <ws>/runs` writes where the UI reads — parity in the other direction. A run stays immutable and either complete or absent; the store never writes anything derived (its rule today, kept). A layout with no file → automatic layout (dagre), and the UI says so.

**Pipelines as files, with a light history.** The YAML is the source of truth (ADR-014, YAML-first) and the git-able object; the UI does not invent a second version system. The store holds the text of every version that ran, since every run carries its `config.yaml`, but not which name each version was launched under: that is the run's launch record, a fact about a past launch kept outside its identity, which the Pipeline screen reads beside the current hash matches, never instead of them (ADR-C39). "Fork this run" writes the run's `config.yaml` to a new file. Objects in an internal store were rejected: they would become, in cluster, a second truth competing with the custom resources.

**Traits and their file backends.** `RunStore` in `ragondin-experiments` (its home), extracted from `FileSystemRunStore`; `PipelineSource`, `Registry`, `WorkspaceSettings` in `ragondin-api`, file implementations in a `fs` module; `Launcher` a trait in `ragondin-api`, implemented in the binary.

**The benchmark manifest.** A file versioned with the binary (`ragondin-benchmarks`): one entry per dataset — name, format, URL, sha256, licence, size. The registry knows nothing else; "verified" means the digest on disk is the manifest's. A benchmark is never fetched from a moving source (`docs/system-architecture.md` § 9.1, the CRAG note): the digest is what a run's identity hashes.

## 7. The launcher

**The job**: `{id, run_id, pipeline: YAML snapshot + name, benchmark, bindings, up_to, state, created_at}`, `state ∈ Queued | Running {done, total, started_at} | Done {run_id} | Failed {error, at_node?} | Cancelled`. Persisted under `jobs/` at every transition.

1. *Submission* — parse, validate, construct the components, read identities, compute `run_id`. `409` if present. Otherwise `Queued`.
2. *Queue* — FIFO, **one worker**. A queued job can be reordered or cancelled. Benchmark downloads have their own, IO-bound queue, running alongside.
3. *Execution* — on a dedicated thread (the ONNX Runtime is heavy); the harness receives the progress observer and the cancellation token.
4. *End* — `Done`: the run is written under `runs/` as one block, as today. `Failed`: the job keeps the error, the failing node, and **the partial traces** of the queries already executed under `jobs/<id>/partial/` — a failed run replays up to the broken node — but the store sees none of it.
5. *Restart* — when `ragondin ui` starts, a `Running` job becomes `Failed {"interrupted"}`; `Queued` jobs resume. Relaunching is one click, same `run_id`.

**Why one run at a time.** A run measures latency as well as quality; two runs sharing a CPU and an ONNX Runtime measure contention, not the pipeline. The worker count is a parameter set to 1; the day it becomes N, the concurrency degree is recorded on the run beside the bindings, as provenance, and Compare warns when two compared runs differ on it. The store changes nothing until then.

**Progress** feeds the job state, broadcast over SSE; the median latency accumulates as it goes, so "1.84 s / query" shows during the run.

## 8. Error handling

**Rust** — ADR-C13: typed errors in `ragondin-api` (`ApiError`), `anyhow` only in the binary. Each variant renders as `problem+json` with a stable code:

| Code | When | The hint says |
|---|---|---|
| `pipeline_invalid` | validation refused | the node, the edge, the kind expected and found — the words `ragondin validate` uses |
| `impl_not_in_build` | an `impl:` this binary lacks | "rebuild with the feature, or bind a Remote under this name" |
| `service_unreachable` | probe or submission | the address, the network error, the identity last read |
| `run_exists` (409) | submitting a present `run_id` | the link to the run or the job |
| `run_unreadable` | a run whose configuration carries a schema version this build does not read, or whose trace does not parse | what was read and what this build expects — **reported, never repaired** |
| `dataset_absent` · `dataset_differs` | passage text not resolvable: no dataset on disk, or one whose digests are not the run's | not an error: a degraded response with a flag; the UI shows ids and a banner naming which of the two it is |

**UI** — three levels and never a modal: *inline* (a field, a node, an edge — where one corrects), *section* (a panel that could not load, with retry), *toast* for an asynchronous outcome ("Run failed at `rerank` — open"). An error that needs action does not dismiss itself. A broken SSE stream shows ("disconnected, retrying") and reconnects; the state never pretends to be current.

**Forbidden**: a swallowing `catch`, a `fetch` without a rendered error path, a spinner without real progress, an `unwrap` in `ragondin-api`. API-crate PRs are reviewed for silent failures specifically: every error path is read for what it swallows.

## 9. Tests, CI and governance

### Rust — test-driven by the repository's rule

- **`ragondin-api`**: handlers tested against in-memory trait implementations (`FakeRunStore`, `FakeLauncher`) with `tower::ServiceExt::oneshot`; the API description as a **golden** — an API change is a reviewed diff; a `problem+json` contract test per code.
- **Backend conformance**: `RunStore`, `PipelineSource`, `Registry` each get a conformance suite on the model of `ragondin-conformance`; `FileSystemRunStore` passes it; M7's object-store backend will. This is what makes "M7 is a set of backends" checkable rather than promised.
- **Launcher**: the state machine against a fake harness — progress, cancellation between queries, recovery on restart by re-reading `jobs/`, refusal of an unreachable service at submission.
- **Per-node metrics**: an invariant test — nDCG recomputed from the trace at the last node **equals** the metric the harness recorded, on the `pytrec_eval`-checked fixtures of #235. A divergence means the trace or the metric lies.
- **Binary** (`assert_cmd`): `ragondin ui` in a lean build answers its message; a `ui` build without built assets compiles and serves a notice, and the release job asserts the real assets are present; a non-loopback bind is refused. And **the parity test, the exit criterion mechanised**: a pipeline written through `PUT /pipelines` then exported hashes identically under `ragondin validate`.
- **End to end**: submission through the API → run → compare, with the in-process generator service #326 already uses, under `--features ui,remote`, `#[ignore]` like the calibrations.

### UI

- Generated types current (CI).
- Components (Vitest, Testing Library): the node card in **every** state, the table, the chips, the inline error.
- Contract: MSW mocks derived from the OpenAPI examples; and a **fixture workspace** — runs recorded from the repository's calibration fixtures — that the dev server serves *and* the Rust tests use. One truth on both sides.
- **Playwright against the real binary** with that workspace: the three journeys of § 3 (first run with the stub components, fork and compare, investigate down to side-by-side replay); axe for accessibility; one keyboard-only journey across the canvas.
- No visual diffing at first (brittle); revisit.

### CI and governance

- **New jobs**: `ui` (pinned Node, `npm ci`, lint, typecheck, unit, build), `ui-licenses` (audit with an allowlist mirroring `deny.toml`), generated-types freshness, Playwright with the `--features ui` binary. **`just check` grows** a `check-ui`, so the single gate covers both worlds.
- **Invariants**: the closure check of § 4 for `ragondin-api`; and a scan, documented as best-effort, that *`ui/` talks only to its own origin* — an ESLint rule forbidding `fetch`, `XMLHttpRequest`, `WebSocket` and `EventSource` outside `ui/src/api/`, plus a test that this module's base URL is relative.
- **npm dependencies** — the `[workspace.dependencies]` rule transposed: `ui/DEPENDENCIES.md` lists each *runtime* dependency with its role and reason; adding one is named in the PR under its own heading; lockfile committed, `npm ci` only, `npm audit` failing on *high* with a justified ignore list like `deny.toml`'s.
- **Documentation in the same PR**: `runtime/ragondin-api/ARCHITECTURE.md`; `ui/ARCHITECTURE.md` — the load-bearing-crate rule extends to `ui/`, not a crate but one in the sense that matters; the design system cited from `ui/` as ADRs are from Rust; `AGENTS.md` gains the `ui/` rules; `docs/AGENT_WORKFLOW.md` the roadmap and the milestone rule.

### Stack, and what was rejected

**Vite + React + React Flow (xyflow)**, a typed client router for URL state, CSS from the design system's tokens, no UI kit. Rejected: **Rust → WebAssembly** (one language, but no React Flow equivalent, a heavy framework and a `wasm32` target, and a browser build depending on `ragondin-pipeline` would be a new INV-1 consumer nobody planned); **server-rendered HTML** (caps at Runs, Compare and Setup — replay and the editor are interactive canvases); **Next.js** (a server framework whose value — SSR, API routes, a Node process — is either irrelevant to a local tool or duplicates the Rust API, and a second process to run and secure, against ADR-C15).

## 10. What this design answers for #327

| Question | This design's answer |
|---|---|
| 1. Where the front end runs | 1a — `ragondin ui`: the binary serves a JSON API and embedded assets; the browser never reads files; the graph is lowered by `ragondin-pipeline` on the server |
| 2. What it consumes | 2b, with 2c's stance — an API layer with its own versioned, generated types stands between the on-disk files and the UI; the UI is in-workspace and the API is not yet promised |
| 3. What replay shows for a ranking | 3b, amended — ids, documents, scores and the stored context text from the record; passages resolved against the dataset at read time only when its digests are the run's, degraded and flagged otherwise; ADR-C28 stands. Recorded trigger: once a chunker component produces chunks inside the pipeline, this stops working and needs a new decision |
| 4. Toolchain and governance | 4a — TypeScript under `ui/`, with the governance of § 9 |

Plus two decisions the issue did not ask and this design needs: **the roadmap change** (M4 is the front end; the judge and everything after shift by one; the milestone rule) — governance, resolved by amending `docs/AGENT_WORKFLOW.md` and the GitHub milestones, no ADR; and **the supersession of ADR-014**'s trajectory — its own ADR.

The decision produced ADR-C36 (questions 1–4 and INV-12), ADR-016 (superseding ADR-014), the roadmap amendment, and the M4 milestone with the exit criterion of § 2. The M4 journey is mechanised against a locally served fixture benchmark, with one `#[ignore]` run over the real SciFact download like the calibrations. `just check` requires Node from now on; `cargo build` never does.

## 11. Deliberately left open

- **Where the benchmark manifest's URLs point** for each dataset (BEIR's public server, Hugging Face mirrors): a sub-project 1b question, answered per entry with a licence check.
- **Whether prefix runs land in M4 or M5**: designed here so the API and the Runs grouping leave room; the editor issue decides. If they need any change to `ragondin-pipeline`'s public API, that change escalates under INV-1.
- **The editor is part of M4, without a time-box.** The product owner decided that M4 does not close without the editor: composing a pipeline in the UI is in the exit criterion, and the judge waits if the editor is late. No viewer issue may depend on an editor issue all the same — the viewer's renderer is the editor's base, never the reverse.
- **The pairing-by-hand persistence format**: a sub-project 2 leaf choice, recorded in `ragondin-api`'s `ARCHITECTURE.md`.
- **A logo**: the design system sets the name in type; a mark is later work.
