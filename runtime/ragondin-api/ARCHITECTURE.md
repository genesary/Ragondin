# ARCHITECTURE — ragondin-api

**Status: internal, and not an API boundary.** INV-1 names the three crates
that are, and this is not among them. ADR-C36 § 2 applies ADR-C21's test —
who would be broken by a change, and do we compile them — and finds no one
outside this build: the only consumer of `/api/v1` is the UI under `ui/`,
built by this repository's gate from the same commit, embedded into the binary
that serves it, and refused by the build-identity check when it meets another
build. The Rust crate itself is consumed only by the binary. So the API
changes with the UI, in the same pull request, like everything in-workspace.
It is *built* as a contract — every path under `/api/v1`, every type its own,
the description a golden file — so that making it a boundary one day, by a
decision issue and its ADR, costs a decision and not a rewrite.

## What lives here

The JSON API the front end consumes (ADR-C36 § 2): the router, its handlers,
the response types, the typed errors, and the traits the service consumes.

| Piece | Role |
|---|---|
| `router` | Builds the whole server from `Backends`, a `ServerConfig` and the UI's `Assets`, all passed in by the binary, wraps it in the layers last, and returns a `Server` |
| `Server` | The enveloped server: what `serve` listens with, and nothing a route can be added to |
| `serve` | Listens with a `Server` on the listener the binary bound: the one place the server meets a socket, so the binary names no HTTP stack |
| `assets` | `Assets`, the table of the UI's files the binary hands in as data, and the routes that serve it: the single-page fallback, `GET`/`HEAD` only, `content_type_for` |
| `Backends` | The five backends, each an `Arc<dyn …>`: `RunStore`, `PipelineSource`, `Registry`, `WorkspaceSettings`, `Launcher` |
| `backends` | The four traits this crate defines, and the values they exchange |
| `response` | Every type the API serializes — the response bodies and `Problem` |
| `error` | `ApiError`, its stable codes, its `application/problem+json` rendering |
| `layers` | The server's defence of its origin, as Tower layers on the router |
| `description` | The API description, assembled from the declared operations and the `schemars` schemas |
| `derived` | The data derived from a stored run and its benchmark: per-query scores, per-node ranking metrics, the gold filter |
| `cache` | The workspace's `cache/`: those derived figures, reconstructible, never a truth |
| `fs` | The file backends: `FsRegistry` today |
| `conformance` | The suite every `Registry` backend passes, behind the `conformance` feature |

Served today: `GET /api/v1/workspace`, `GET /api/v1/runs`,
`GET /api/v1/runs/{id}`, `GET /api/v1/runs/{id}/queries` and
`GET /api/v1/runs/{id}/trace/{query}` — the last two described in
§ *Derived data*. `/api`, `/api/` and every other path below them are
the API's too: an unknown one answers `route_not_found`, and a method an
endpoint does not serve answers `method_not_allowed` with axum's `Allow`
header — problem bodies both, never an empty 404 or the assets' fallback.
(axum 0.7's `nest` leaves `/api/` to the outer router, so it is routed there
explicitly; `tests/layers.rs` pins both spellings.) A path that only begins
with the same letters, such as `/apix`, is not under `/api`, and the assets
answer it. Nothing here binds a port:
the listener, the loopback-only rule and the embedded assets themselves are
the binary's, which hands the listener to `serve` and the assets in as data
(§ The assets).

## INV-12: this crate reaches no engine and no component

`ragondin-api` must not depend, directly or through any other crate, on any
crate under `engine/` or `components/`, nor on `wire/ragondin-remote`
(ADR-C36 § 3; `AGENTS.md` § Invariants holds the binding row). It cannot
execute a pipeline, construct a component or call one, in process or over the
wire. The only path from the UI to the data plane is `Launcher`, implemented by
the binary. `scripts/check-invariants.py` walks this crate's `--all-features`
closure, so `ragondin-harness` and `ragondin-server`, which reach the engine,
are refused through it.

**The blind spot, left to review:** `ragondin-proto` is reachable through
`ragondin-config`, which INV-12 allows. A client hand-built here over its
generated stubs would call a service without `ragondin-remote`, and the check
would not see it. The sign in a diff is a `tonic` channel or a generated
client type in this crate.

Its workspace dependencies today are `ragondin-experiments` — the `RunStore`
trait, the `Run` record, the typed `Trace`, `lower_configuration`, and the
walk to a run's ranking node — `ragondin-pipeline`, for the `LogicalPipeline` that lowering yields,
`ragondin-benchmarks`, for the manifest, the download, the verification and
the import the `Registry` file backend is written over, and for the digests
and the chunk derivation passage text is verified against,
`ragondin-metrics`, for the per-query and per-node scores and the fold
they are computed over, and
`ragondin-types`, for the ids they are computed over. `ragondin-benchmarks`
and `ragondin-metrics` reach no engine and no component: their closure is the
core's `ragondin-types` and its readers, and `just check-invariants` walks it
— `scripts/test-check-invariants.py` holds a case in which `ragondin-benchmarks`
reaches the engine and INV-12 fails through it. `ragondin-config` is within
INV-12 and arrives with the endpoints that read it. **`ragondin-harness` is
not a dependency, and may not become one** (INV-12 refuses it through the
engine). The two rules the derived data shares with it are therefore
defined where both crates reach them, once, and `derived.rs` calls them as the
harness does: the chunk-to-document fold is `ragondin-metrics`'
`documents_by_first_occurrence`, and ADR-C30 § 3's walk is
`ragondin-experiments`' `terminal` and `ranking_node`. A change to either rule
reaches the figures the harness writes and the figures this crate reads back
in the same build — ADR-C36's "one definition, used by the writer and the
reader alike", applied to the rules as § 2 applies it to the trace shape.

## The traits and their backends

The router holds every backend as an `Arc<dyn …>` it was handed. Nothing is a
static, a global or a registry (INV-6's reasoning, applied to the backends by
ADR-C36 § 1), and nothing here knows whether it runs on a laptop or in a
cluster: the binary picks each backend.

| Trait | Defined in | Local backend | In a cluster |
|---|---|---|---|
| `RunStore` | `ragondin-experiments` | `FileSystemRunStore` there | an object store or volume |
| `PipelineSource` | here | `fs`, over `pipelines/<name>.yaml` and its layout — not written yet | custom resources |
| `WorkspaceSettings` | here | `fs`, over `workspace.toml` — not written yet | the deployment's bindings, read-only |
| `Registry` | here | `fs::FsRegistry`, over the benchmark manifest and the datasets directory | an object store and the same manifest |
| `Launcher` | here | the binary, over the composition root | a run custom resource and the controller |

- **Async with `async_trait`** for the four defined here (frozen decision): a
  file backend reads the disk and a cluster backend talks to an API server.
  `Launcher::capabilities` is synchronous — it reads what the build registered.
- **`RunStore` is synchronous** (its crate says why), so a handler runs each
  store call on a blocking thread with `tokio::task::spawn_blocking` rather
  than stalling an async worker on the disk.
- **`Launcher` carries the shapes the design document § 7 gives**: `probe`
  returns the identity a `Remote` service reports, `identity` the run id a
  `Submission` announces, and `execute` a `Job`'s terminal `JobState`.
  **`execute` is provisional**, and its doc comment says so: the job model
  and its queue (#349) settle its progress and cancellation. Nothing calls it
  yet, and the crate is internal, so widening it owes no one a deprecation.
  `Registry::download` is settled — § *The `Registry` file backend* says
  how.
- **The backends return `ApiError`.** They live in this crate or are written
  for it, and an error that already carries its code needs no second mapping.

## The `Registry` file backend

`fs::FsRegistry` is the `Registry` over a datasets directory and a manifest —
`ragondin_benchmarks::manifest::manifest()` in the binary, entries a local
server serves in a test. It holds no logic of its own about datasets: the
download rules, the verification, the import and the digest are
`ragondin_benchmarks::datasets` and `ragondin_benchmarks::identity`, the one
definition ADR-C36 § 4 allows. This backend finds the entry a name means, runs
the call on a blocking thread — each loads a dataset whole or writes one —
supplies the HTTP transport, and converts what comes back in `convert.rs`.

- **The listing** is the manifest's entries in manifest order, then the local
  imports by name. A manifest entry is `ready` when `<datasets>/<dir>`
  digests to its `dataset_version`, `available` (with its size) when nothing
  is there, `differs` with both digests, or `unreadable` with the adapter's
  error; in every state it carries its `licence` and `licence_url`, so a
  downloaded dataset keeps the notice it was obtained under. An import is
  `local` when it still digests to what it digested to at import, and
  `differs` or `unreadable` otherwise; an import whose `ragondin-local.json`
  this build cannot read is listed `unreadable` under its directory's name,
  with format `unknown`, and fails neither the listing nor its neighbours. The
  ground truth — `qrels`, `reference_answers`, `both` — is read off the loaded
  dataset's `CarriedPieces`, and is `null` when nothing loaded.
- **The listing digests every dataset on every call.** Each entry on disk is
  loaded whole and digested, and nothing is cached: correct, and slow for a
  large corpus. The endpoint that serves it (#342) should know this before
  calling it per request; a cache is `cache/`'s business, not this backend's.
  `dataset` has the same cost, one dataset per call, and the derived-data
  endpoints pay it on every request — § *Derived data* says why, and what
  `cache/` saves them.
- **What the trait gained**, since the backend showed the trait lacked it:
  `verify`, one entry by name; `download(name, progress, cancel)`, returning
  the verified entry — progress after every chunk through a `ProgressSink`,
  cancellation through an `Arc<AtomicBool>` checked after every chunk (the
  shape the harness's cancellation took), and a deadline the download applies
  itself. That settles the provisional shape. `import` returns the entry it
  registered, and an import may not take the directory of an entry of *this
  registry's* manifest. And `dataset(dataset_version)`, for the derived-data
  endpoints: a run names no benchmark, only the digest of the one it was
  evaluated on, so the backend finds every benchmark *pinned* to that digest
  — manifest entries in manifest order, then imports — and answers the first
  one on disk that loads and digests to it (`Verified`, with the dataset
  loaded), or else the first one's state: `Absent`, `Differs` with the digest
  found, `Unreadable` with the adapter's error. A digest no benchmark is
  pinned to is `Unknown`, whatever the disk holds: nothing is resolved by
  closeness (ADR-C36 § 4). It loads through `Format::load` and digests with
  `ragondin_benchmarks::identity::dataset_version` rather than calling
  `datasets::verify`, which discards what it loaded.
  `BenchmarkEntry`, which the trait exchanges, moved to
  `response.rs` as a response type — the way `Settings` already carries
  `ServiceBinding` — and `BenchmarkStatus` is gone, replaced by
  `BenchmarkState`'s five states.
- **`sweep_staging`** removes the staging directories an interrupted download
  or import left. **Nothing calls it yet**: the binary must call
  `FsRegistry::sweep_staging()` once at startup, before the queue runs —
  owned by the issue that wires the workspace (#342) — since a running
  download's staging directory has the same shape. Until then an interrupted
  attempt leaves a `.`-named directory, which the listing ignores.
- **The conformance suite** (`src/conformance.rs`, behind the `conformance`
  feature, the model `ragondin-experiments` set for `RunStore`) checks the
  contract `Registry`'s documentation states: a fresh listing; a download
  reported `ready` by the listing and by `verify`; a failed download that
  leaves the benchmark `available`, as before; a cancelled one; unknown
  names; an import, including a name holding NUL refused as
  `import_refused`; and `dataset`, which finds a downloaded and an imported
  benchmark by their digests, answers `Absent` before the download,
  `Unknown` for a digest nothing is pinned to, `Differs` once a downloaded
  benchmark's content is changed and `Unreadable` once it is broken — the
  fixture's `alter` hook does both behind the registry's back, as a person
  editing the datasets directory would. `tests/registry_conformance.rs` runs
  it against `FsRegistry`, with a faithful and a corrupted source served by a
  dependency-free local HTTP server — no test touches the network. The only
  route that calls the registry is the derived data's, through `dataset`; the
  listing, the download and the import are the workspace's endpoints (#342).

### `reqwest`, the transport

`ragondin-benchmarks` speaks no HTTP; its download is handed a `Fetcher`, and
this crate's is `reqwest`, the workspace's one client (ADR-C33 § 2), with its
entry's features and none appended. It lives here, and not in
`ragondin-benchmarks`, because `ragondin-harness` and the binary depend on that
crate and never download: with the client there, both would compile an HTTP
and TLS stack — `rustls`, `ring` and its C build, `webpki-roots`, `url` and
`idna` — for nothing, against ADR-C14's lean default build.

- **On the server's runtime.** The download runs on a blocking thread; the
  fetcher drives each request with `Handle::current().block_on(…)` on the
  runtime the handler ran on, and starts no runtime of its own.
- **Timeouts.** 30 s to connect and 60 s per read, so a server that stops
  sending fails the read. A server that trickles is caught by the download's
  overall deadline, which `ragondin-benchmarks` applies (a minute plus the
  size at 32 KiB/s).
- **Redirects are followed** — `reqwest`'s default, up to ten — because a
  pinned URL on a dataset hub answers with a redirect to its storage.
  Integrity does not rest on the host that served the bytes: it rests on the
  manifest's SHA-256 for each file and `dataset_version` for the snapshot.
- **TLS verifies against `webpki-roots`**, the bundle the workspace entry
  selects, not the system's store. A network that intercepts TLS with its own
  authority therefore fails every download, and nothing here overrides that;
  such a user imports the dataset instead.

## Response types are this crate's own

Every type the API serializes is in `src/response.rs`, derives `serde` and
`schemars`, and is converted to in `src/convert.rs` from the experiment
plane's and the core's types — never one of them serialized directly
(ADR-C36 § 2, for the reason INV-9 gives for the wire format).
`tests/response_types.rs` holds `response.rs` to naming no workspace crate at
all. A source scan, and best-effort: a type defined elsewhere and serialized
anyway is what it does not see, and the description's schema list, generated
from `response.rs`'s types only, is where a reviewer would see one arrive.

**The lowered graph is computed on the server**, by the lowering
`ragondin-experiments`' `compare` already runs — `lower_configuration`, which
parses the kept text into `ragondin-pipeline`'s `RawPipeline` and validates
it. This crate calls that function; it does not parse YAML itself. A node
carries its id, family, `impl:` name and parameters; an edge carries its
producer, consumer, port and the kind of value its producer puts on it — the
query for a declared input, and otherwise `produced_kind` of the producing
node. The nodes come in the canonical order (by id), the edges grouped by
consuming node, in port order. `prefix_of` is present and always absent: no
run is recorded as a prefix yet.

**A run the store lists and cannot load** is listed in `GET /runs` under
`unreadable`, with the store's reason, rather than dropped or failing the whole
listing — reported, never repaired.

**A field serialized on every response is required in its schema**, nullable
when it can be null: `RunDetail::prefix_of`, `Location::node` and
`Location::edge` carry a `transform` that lists every property as required,
since `schemars` would otherwise leave an `Option` out and a generated client
would type it as possibly absent. `Problem::location`, omitted when there is
none, stays optional. `Problem::code`'s schema is an enum of
`ApiError::CODES`, so a generated client can narrow on it.

## The error codes

`ApiError` is typed with `thiserror` (ADR-C13); `anyhow` is not a dependency.
Each variant renders as `application/problem+json` — `type`
(`urn:ragondin:problem:<code>`), `title`, `status`, `detail`, `code`, `hint`,
and `location` for a validation failure. `ApiError::CODES` lists them. A code
no handler raises yet still exists, so a later endpoint adds a handler, not a
code.

| Code | Status | When | Raised today |
|---|---|---|---|
| `pipeline_invalid` | 422 | validation refused a document; `location` names the node and edge | no |
| `impl_not_in_build` | 422 | an `impl:` this binary lacks | no |
| `service_unreachable` | 502 | a probe or a submission reached no service | no |
| `run_exists` | 409 | a submission's run id is already stored or queued | no |
| `run_unreadable` | 500 | a stored run this build cannot read: torn, malformed, or a configuration that no longer lowers | `GET /runs/{id}` |
| `run_not_found` | 404 | no run under this id, or a string that is not a run id | `GET /runs/{id}` and below |
| `query_not_found` | 404 | a query id the run's traces do not hold | `GET /runs/{id}/trace/{query}` |
| `parameter_invalid` | 400 | a query parameter the endpoint does not take, given twice, or a value it cannot read | `GET /runs/{id}/queries`, `GET /runs/{id}/trace/{query}` |
| `dataset_absent` | 404 | ground truth needed, and the run's dataset is not on disk or pinned by nothing | `GET /runs/{id}/queries?missing_gold_at=` |
| `dataset_differs` | 409 | ground truth needed, and the dataset on disk is not the run's: its digest differs, or it does not load (the detail says which) | `GET /runs/{id}/queries?missing_gold_at=` |
| `benchmark_not_found` | 404 | a benchmark name the registry does not know, or a download of one the manifest does not hold | `FsRegistry` |
| `benchmark_exists` | 409 | a download or an import whose directory is already there | `FsRegistry` |
| `download_failed` | 502 | a fetch that failed, a file of the wrong size or digest, a snapshot of the wrong `dataset_version`, or a deadline passed; the detail names both values | `FsRegistry` |
| `download_cancelled` | 409 | a download whose cancellation flag was set; nothing was kept | `FsRegistry` |
| `import_refused` | 422 | an import name outside `[A-Za-z0-9_-][A-Za-z0-9._-]*` (64 bytes at most, no trailing `.`, no Windows device name), a path that cannot be read, or a corpus its adapter refuses — the adapter's error in the detail | `FsRegistry` |
| `backend_failed` | 500 | a backend failed otherwise — listing the store, say | `GET /runs`, `GET /workspace` |
| `host_refused` | 421 | the `Host` layer refused the request | every path |
| `origin_refused` | 403 | the `Origin` layer refused the request | every path |
| `route_not_found` | 404 | a path under `/api` that names no endpoint | the API's fallback |
| `method_not_allowed` | 405 | an endpoint asked for with a method it does not serve; `Allow` lists the ones it does | each endpoint |

Choices made here (`AGENTS.md` § Rules of engagement), since the design
document § 8 lists seven codes and leaves the rest to the implementation:

- **An unknown run id is a 404, `run_not_found`**, not `run_unreadable`. The
  two are different facts — nothing is there, versus something is there this
  build cannot read — and a client acts differently on each. A string that
  cannot be a run id names no run either, and gets the same answer.
- **Thirteen codes beyond the design's seven**: `run_not_found` for the above;
  `query_not_found`, for the same reason one level down — a run that exists
  and a query it did not execute; `parameter_invalid`, because an unknown,
  repeated or malformed query parameter is refused rather than ignored, on
  every endpoint that reads the query string, and a silently ignored filter
  would answer a question nobody asked;
  `backend_failed`, because a backend's I/O failure is none of the seven and
  a problem body must carry some code; `host_refused` and `origin_refused`,
  so that the layers' refusals are problem bodies like every other error;
  `route_not_found` and `method_not_allowed`, so that axum's own empty 404
  and 405 never reach the UI. `run_not_found` would be wrong for an unknown
  path: it tells the client a run is missing, and a client acts on that. And
  five for the registry: `benchmark_not_found`, `benchmark_exists`,
  `download_failed`, `download_cancelled` and `import_refused`. `dataset_differs` is not reused for
  a download whose digest differs: it says the run's dataset is not the one on
  disk, and its hint tells the reader to restore a run's version — the wrong
  action for a download, which left nothing on disk. A digest refused is a
  502, the upstream having served other bytes than the pinned ones; a refused
  import is a 422, the request's to correct. A cancelled download is its own
  code, so the queue can tell it from a failure. A disk that cannot be
  written is `backend_failed`, whichever of the two hit it — and so is a
  pinned snapshot that does not load, or a manifest path outside its
  directory: the manifest pinned those bytes, so the defect is this build's,
  not the source's.
- **`dataset_absent` and `dataset_differs` have statuses**, 404 and 409, for
  an endpoint that needs the ground truth and cannot degrade: the
  `missing_gold_at` filter, which is a question about qrels. Where an endpoint
  can show ids and unscored queries instead — the trace, the unfiltered
  listing — the design document § 8 makes them a flag in a `200` response,
  not an error, and they are: `passages` and `ground_truth`, whose statuses
  add `dataset_unreadable` (on disk, does not load — the hint differs from a
  differing digest's) and, for passages only, `index_differs`. A dataset that
  does not load still answers the filter with `dataset_differs`, its detail
  saying that nothing loaded: one more code would buy the client nothing it
  can act on differently.

## Derived data

`GET /runs/{id}/queries` and `GET /runs/{id}/trace/{query}` serve what the
run record does not hold: per-query scores, per-node ranking metrics, and
passage text. **All of it is derived data: computed here on read, cached
under the workspace's `cache/`, and never written into the run** — the store
writes nothing derived (its rule), and `cache/` is reconstructible data that
is never a truth (the design document § 6). Deleting `cache/` changes no
response; `tests/replay.rs` deletes it and compares.

### What a figure is

A reading of the trace against the run's own ground truth, with
`ragondin-metrics` and the rules the harness scores by, called rather than
restated (`derived.rs`):

- **The metrics are the ones the run recorded**, by name: `ndcg@<k>`,
  `recall@<k>`, `mrr` (uncut, as the harness records it), `exact_match`,
  `token_f1`, each at the cutoff its own name states. Other names — a latency
  percentile — are not per-query figures and are not listed.
- **Documents, folded from chunks by first occurrence**: several chunks of one
  document count once, at the rank of the best —
  `ragondin_metrics::documents_by_first_occurrence`, the fold the harness
  applies (§ *INV-12* says why it lives there).
- **The output ranking is found by ADR-C30 § 3's walk**, by port position: a
  terminal generator's context port names a context builder, whose chunks port
  names the ranking; a terminal builder is entered at its chunks port; any
  other terminal node is its own ranking — `ragondin_experiments::ranking_node`,
  the walk the harness scores at. The answer is the terminal node's
  (`ragondin_experiments::terminal`), when the core says it produces one.
- **Per node**: every node whose output is a ranking is scored on the same
  metrics, per query, and averaged over the judged queries for which it
  produced one — a query without qrels is in no mean, as in the harness, and
  `judged_queries` says how many each mean is over, so the front end can label
  it. A context builder, a generator and a failed node have no ranking and no
  metric, and `produces_ranking` says which nodes have one.
- **Means are summed in the benchmark's query order** and divided once, the
  harness's order, so the figure at the last ranking node is `metrics.json`'s.
  **That equality is an invariant test** (`tests/per_node_metrics.rs`): on the
  harness-recorded fixture run, and on `ragondin-metrics`' `pytrec_eval`-checked
  fixtures — the M2 regression fixture, the SciFact and NFCorpus calibration
  fixtures, and the SQuAD generation fixture, whose per-query EM and F1 also
  average to its `metrics.json`. A divergence means the trace or the metric
  lies. The test pins this crate's reading to those fixtures. That this crate
  and the harness apply the same rules needs no test of its own: both call the
  one fold and the one walk (§ *INV-12*), so a change to either reaches both.
- **Node rows carry ranking metrics only.** The generator's EM and token-F1
  are a run-level figure: `metrics.json`'s, or the mean of the per-query
  scores, never a node row.
- **The gold filter**: `?missing_gold_at=<k>` keeps the judged queries none of
  whose documents graded above 0 is in the top `k` of the output ranking.
  It is the only parameter the listing takes, and the trace takes none; a
  name and a value are percent-decoded, and any other parameter, the same one
  twice, or a value that is not a positive integer is `parameter_invalid`.
  It is stated in the operation's `description` in `api/v1.json`, not
  declared under `parameters`: the UI's type generator refuses a query
  parameter (`ui/ARCHITECTURE.md` § The generated types) until the screen
  that first sends one extends it.
- **No pagination.** The listing answers every query of the run at once —
  SQuAD's thousand in one body. Paging it is a change to this endpoint when a
  screen needs it.
- **Durations**: a query's is the sum of its nodes' `duration_nanos`, each
  component's own time. The run's latency percentiles are `metrics.json`'s and
  are not recomputed.

### Passage text, and the ground truth, only against the run's own dataset

ADR-C36 § 4: a passage's text is shown only when the dataset on disk digests
to the run's `dataset_version` **and** the chunk set derived from it digests to
its `index_version`. The dataset is found through `Registry::dataset`; the
chunk set is `ragondin_benchmarks::identity::CorpusIndex`, the one derivation
the writer and the reader share. Otherwise the ids are shown alone, and a
`DatasetCheck` says why — `dataset_absent` (not on disk, or pinned by no
benchmark the registry knows) or `dataset_differs` (a digest differs, or the
dataset does not load) — with the benchmark's name, the digests the run
recorded and, as far as they were computed, the digests found. A chunk id the
verified chunk set does not hold has no text.

**Two verdicts, by what each needs.** ADR-C36 § 4 conditions the *text* on
both digests. A score needs only the qrels and the reference answers, which
are the dataset's, so the scores and per-node metrics — the listing's
`ground_truth`, the trace's `scores` and node `metrics` — are gated on
`dataset_version` alone, and `ground_truth` compares no chunk set
(`found.index_version` is `null`). The passages are gated on both: a dataset
that verifies with a chunk set that does not is `index_differs`, no text,
both chunk-set digests side by side, and the scores still read. A dataset
that does not load is `dataset_unreadable`, with the adapter's error in
`detail`. With anything but `verified`, the queries are listed with their
durations and no scores, and the nodes with no metrics.

**The trigger ADR-C36 § 4 records.** This resolution holds while chunks are
derived outside the pipeline, by `CorpusIndex`. **The day a chunker component
produces chunks inside a pipeline, a chunk's text can no longer be derived
without executing that component**, which INV-12 forbids this crate; what
replay shows for such chunks is then a new decision, to be met deliberately
rather than discovered.

### The cache: a choice made here

`cache/<run_id>/derived.json`, one JSON file per run, written only once the
dataset has verified: the per-query scores and the per-node metrics. It is
keyed on everything they were computed from — the file format, the build, the
run id, the `dataset_version`, and a digest of the run's own content (every
trace document and every metric, through `ragondin_benchmarks::identity`'s
encoder under a domain of this crate's) — and is used only when the whole key
still holds; anything else is a miss, recomputed and overwritten. **No clock
is read**: a dataset changed on disk no longer verifies, so its cache is never
consulted, and one restored verifies again and finds its file valid. The
content digest is there because a run deleted and launched again keeps its
id, which digests the inputs, while a nondeterministic component may give it
other traces. The build is there because another build may score
differently: **the cache is correct only if `ServerConfig::build` changes
with every change to the code** — the commit and a dirty flag, which the
binary's build identity (#365) provides; its doc comment says so.

- **Figures are stored as the bits of their doubles**, not as decimals:
  `serde_json`'s default reader rounds a decimal to within an ulp of the
  double written, so a decimal cache would answer one ulp away from a computed
  response. (The same rounding is why the invariant test compares the
  harness-recorded run to `metrics.json` within two ulps rather than bit for
  bit: `metrics.json` is read with that reader.)
- **Written whole or not at all**: to a `.partial` file of this process and
  thread, then renamed over the file; a write or a rename that fails removes
  the `.partial` file. A file that does not parse is a miss and is
  overwritten — it is derived data, so rebuilding it loses nothing, unlike a
  stored run, which is reported and never repaired.
- **A cache that cannot be read or written fails nothing.** The figures are
  computed anyway and served, and the reason is reported in the listing's
  `cache_error`, so a read-only workspace stays usable and a broken cache is
  never silent.
- **What it saves, and what it does not.** **The dataset is loaded and
  digested on every request** to either endpoint — the verdict cannot be
  trusted without it, and the qrels and the passage text come from the load
  anyway. The cache saves every per-query and per-node figure of the listing.
  The trace endpoint reads nothing from it: it derives the chunk set to
  resolve text, and one query's figures cost nothing. On a large corpus the
  load is the cost of every request, so **a memo on keeping a verified
  dataset in memory between requests is owed before the screens that call
  these endpoints per interaction — the matrix (#346) and replay (#350) —
  land**.

**Per-node metrics are served by `GET /runs/{id}/queries`**, beside the
per-query scores they are computed with, and not by `GET /runs/{id}` as the
design document § 5's table once listed them (its row now points here): the
detail endpoint reads the store alone, and putting the metrics there would
make every run's detail load and digest its dataset.

## The assets

The UI's files reach the server as **data, not as a `Router`**: an `Assets`
table — `get(path) -> Option<Asset>`, an `Asset` being bytes and a content
type — that the binary implements over its `rust-embed` table (or its notice
page) and hands to `router`. The routes that serve it are written here, beside
the envelope that must cover them, and `serve` owns `axum::serve`. So `axum`
is used by this crate only, as ADR-C36 § 6 says — the product owner's ruling
on #339's review, which had first handed the assets in as a `Router` and so
put `axum` in the binary — and the binary, naming no HTTP stack, has no way
to add a route outside the layers short of a manifest change a reviewer sees.

What a request outside `/api` gets, from `src/assets.rs`:

- **`GET` and `HEAD` only.** Any other method is `405` with `Allow: GET, HEAD`
  (a plain response, not a problem body: it is not the API's).
- **`/` is `index.html`.** A path naming a file of the table is that file,
  with the content type the table gives — the binary's gives
  `content_type_for(path)`, a fixed table of the extensions a Vite build emits,
  text types in UTF-8, anything else `application/octet-stream`.
- **The single-page fallback**: any other path whose last segment has no `.`
  is a client-side route and is answered with `index.html`, so the UI routes
  itself. Any other path is a missing file and a `404`: a `<script>` tag
  answered with HTML would hide the error.
- **The path is looked up as sent**, without its leading `/`, never resolved
  or percent-decoded: `..` is a key the table does not hold, so a table in
  memory cannot be walked out of. `/assets/../../etc/passwd` has no `.` in its
  last segment and is answered with the index page, never a file.
- **`/api` never falls through**: every path under it is the API's
  (§ What lives here), whatever the table holds.
- **`NoAssets`** is the empty table: every path outside `/api` is a `404`.

`tests/assets.rs` pins each rule against a fake table; `tests/server.rs`
serves one over a real loopback listener with `serve`.

## The layers

The server's defence of its origin (ADR-C36 § 1), as Tower layers on the
router — the network envelope, which is where ADR-C10 puts Tower, and the only
place this crate uses it: no handler and no trait is a `tower::Service`
(INV-11). Each is `axum::middleware::from_fn_with_state`, outermost first.

**This crate owns the whole envelope, and applies it last.** `router` builds
the routes that serve the UI's assets (§ The assets), nests the API under
`/api` beside them, and only then wraps the result in the four layers. The reason is how axum's
`Router::layer` works: it wraps the routes that exist when it is called, and a
route merged or a fallback set afterwards answers outside the layers — a
foreign `Host` accepted, no content security policy, no build identity. The
UI's own page is exactly what the policy must reach (ADR-C36 § 5 makes it the
layer that holds), so leaving the order to the caller would leave the defence
to be remembered.

**And it returns a `Server`, not a `Router`**, so the order cannot be undone
after the fact. `Server` is a newtype with no method that adds a route,
merges a router or sets a fallback, and no conversion back into a `Router`;
it offers `into_make_service()`, which `serve` hands to `axum::serve`, and
`tower::Service` over one request, which is what a connection calls (the
envelope, where ADR-C10 puts Tower; INV-11 is about components). Two
`compile_fail` doc tests on `Server` prove that `.route(…)` and
`Router::merge(server)` do not compile, and `tests/server.rs` serves one with
`serve` over a real connection. The
alternative, a public `envelope(Router, &ServerConfig)` documented as "apply
last", was rejected because it is correct only while the binary never adds a
route after calling it, and nothing would say when it did — the same hole a
returned `Router` leaves. What remains possible is deliberate: a caller can
write a second router of its own around a `Server` and answer routes it adds
itself — a new server outside this envelope written by hand, which in the
binary would first need `axum` in its manifest. `tests/layers.rs` checks that
the index page and a client-side route carry both headers and are refused on
a foreign `Host`.

1. **The build identity**, `x-ragondin-build: <ServerConfig::build>`, on every
   response — refusals and 404s included — so the UI can compare builds on
   any answer. `GET /workspace` reports the same value in its body.
2. **The content security policy**, `default-src 'self'; frame-ancestors
   'none'`, on every response. `default-src` does not govern framing, and a
   page that launches runs and writes files is a clickjacking target, so no
   page may frame this one. Nothing is added for styles. `'unsafe-inline'` would be needed only for a
   `<style>` element or a `style=` attribute present in served HTML; the UI's
   stylesheets are bundled files served from this origin, and React writes a
   component's inline styles through the DOM's style properties, which a
   `style-src` does not govern. If a later UI dependency needs more, the
   policy grows in the pull request that brings it, argued here.
3. **The `Host` check**: the request's `Host` — or, without one, its URI's
   authority, where HTTP/2 carries it — must equal `ServerConfig::served`,
   ignoring ASCII case; otherwise `host_refused`. A `Host` header that is
   present and not text is refused — the URI is read only when the header is
   absent. It compares the authority the server was given and resolves
   nothing, so `localhost` is refused by a server given `127.0.0.1:7878`.
   This is the defence against DNS rebinding. **One authority, today**: a
   browser reaching the server through an SSH tunnel as `localhost:<port>`, or
   on a local port other than the one served, sends a `Host` the check
   refuses. The binary decided to keep one (#339): the served authority is the
   listener's own address, the one `ragondin ui` prints, and a tunnel that
   forwards the same port keeps it (`bin/ragondin/ARCHITECTURE.md` § The ui
   subcommand says why a set is not needed yet).
4. **The `Origin` check**, on `POST`, `PUT`, `PATCH` and `DELETE`: `Origin`
   must be `http://` followed by `ServerConfig::served`; otherwise
   `origin_refused`. **A missing `Origin` is refused**, a choice made here: a
   browser sends one on every such request, and the embedded UI is this API's
   only client, so a state-changing request without one is not the UI's.

## The API description is a golden file

`src/description.rs` assembles one OpenAPI-shaped document from `OPERATIONS` —
each operation's method, path, summary and response schema name — and the
`schemars` schemas of the response types and `Problem`, generated with the
OpenAPI 3 settings so references point under `#/components/schemas`. Minimal on
purpose: it exists for the diff and for the UI's type generator, and no OpenAPI
generator crate is a dependency (ADR-C36 § 6 admits none). The description
carries no promise; it makes a change visible, it does not forbid one.

It is committed at `api/v1.json`. `tests/description.rs` regenerates it and
fails unless the bytes are equal; `just gen-api-description` rewrites it
(through `examples/gen-api-description.rs`), and the diff it leaves is the API
change a reviewer reads. The same test file checks that every operation the
table declares is routed, since the router is written in code and the table
beside it: the two cannot drift silently.

## Dependencies

All admitted by ADR-C36 § 6, each argued in its root `Cargo.toml` comment:
`axum` on the 0.7 line `tonic` 0.12 already resolves (`default-features =
false`; `json` for the endpoints, and `tokio` and `http1` for `axum::serve`,
inside `serve`,
named now because appending a feature to the entry later would escalate — one
`hyper` in `Cargo.lock`, which no core crate reaches), `schemars`, and `tokio`,
whose workspace entry now names `net`, `sync` and `time`. `tower`, the
workspace's serving-envelope entry, is a dependency for the `Service` trait
`Server` implements, which needs no feature; the tests also use
`ServiceExt::oneshot`, whose `util` feature comes from `axum`'s own
requirement on the same `tower`, unified by Cargo, rather than from a feature
appended to the workspace entry. `hyper` is on the
INV-4 deny-list, so the core cannot reach any of this.

`ragondin-metrics` and `ragondin-types` are workspace crates of the core and
the evaluation plane, within INV-12, for § *Derived data*. `ragondin-harness`
is not; the two rules the derived data shares with it are defined in
`ragondin-metrics` and `ragondin-experiments`, which both crates reach
(§ *INV-12*).

`reqwest` is a dependency for the `Registry` file backend's transport, with
its workspace entry's features and none appended, on the one `hyper` already
in `Cargo.lock` — § *`reqwest`, the transport* says why it is here and not in
`ragondin-benchmarks`. `sha2` is a dev-dependency, for the digests of what a
test's local server serves. Neither is a new `[workspace.dependencies]`
entry.
