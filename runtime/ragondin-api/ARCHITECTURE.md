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
| `router` | Builds the whole server from `Backends`, a `ServerConfig` and the assets `Router`, all passed in by the binary, wraps it in the layers last, and returns a `Server` |
| `Server` | The enveloped server: what `axum::serve` listens with, and nothing a route can be added to |
| `Backends` | The five backends, each an `Arc<dyn …>`: `RunStore`, `PipelineSource`, `Registry`, `WorkspaceSettings`, `Launcher` |
| `backends` | The four traits this crate defines, and the values they exchange |
| `response` | Every type the API serializes — the response bodies and `Problem` |
| `error` | `ApiError`, its stable codes, its `application/problem+json` rendering |
| `layers` | The server's defence of its origin, as Tower layers on the router |
| `description` | The API description, assembled from the declared operations and the `schemars` schemas |
| `fs` | The file backends: `FsRegistry` today |
| `conformance` | The suite every `Registry` backend passes, behind the `conformance` feature |

Served today: `GET /api/v1/workspace`, `GET /api/v1/runs`,
`GET /api/v1/runs/{id}`. `/api`, `/api/` and every other path below them are
the API's too: an unknown one answers `route_not_found`, and a method an
endpoint does not serve answers `method_not_allowed` with axum's `Allow`
header — problem bodies both, never an empty 404 or the assets' fallback.
(axum 0.7's `nest` leaves `/api/` to the outer router, so it is routed there
explicitly; `tests/layers.rs` pins both spellings.) A path that only begins
with the same letters, such as `/apix`, is not under `/api`, and the assets
answer it. Nothing here binds a port:
the listener, the loopback-only rule and the embedded assets themselves are
the binary's, which hands the assets in as a `Router`.

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
trait, the `Run` record, and `lower_configuration` — `ragondin-pipeline`, for
the `LogicalPipeline` that lowering yields, and `ragondin-benchmarks`, for the
manifest, the download, the verification and the import the `Registry` file
backend is written over. `ragondin-benchmarks` reaches no engine and no
component: its closure is the core's `ragondin-types` and its readers, and
`just check-invariants` walks it — `scripts/test-check-invariants.py` holds a
case in which it reaches the engine and INV-12 fails through it. `ragondin-config`
and `ragondin-metrics` are within INV-12 and arrive with the endpoints that read
them.

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
  large corpus. The endpoint that serves it (#342) and the one that resolves
  passages (#343) should know this before calling it per request; a cache is
  `cache/`'s business, not this backend's.
- **What the trait gained**, since the backend showed the trait lacked it:
  `verify`, one entry by name; `download(name, progress, cancel)`, returning
  the verified entry — progress after every chunk through a `ProgressSink`,
  cancellation through an `Arc<AtomicBool>` checked after every chunk (the
  shape the harness's cancellation took), and a deadline the download applies
  itself. That settles the provisional shape. `import` returns the entry it
  registered, and an import may not take the directory of an entry of *this
  registry's* manifest. `BenchmarkEntry`, which the trait exchanges, moved to
  `response.rs` as a response type — the way `Settings` already carries
  `ServiceBinding` — and `BenchmarkStatus` is gone, replaced by
  `BenchmarkState`'s five states.
- **`sweep_staging`** removes the staging directories an interrupted download
  or import left. The binary calls it once at startup, before the queue runs:
  a running download's staging directory has the same shape.
- **The conformance suite** (`src/conformance.rs`, behind the `conformance`
  feature, the model `ragondin-experiments` set for `RunStore`) checks the
  contract `Registry`'s documentation states: a fresh listing; a download
  reported `ready` by the listing and by `verify`; a failed download that
  leaves the benchmark `available`, as before; a cancelled one; unknown
  names; and an import, including a name holding NUL refused as
  `import_refused`. `tests/registry_conformance.rs` runs it against
  `FsRegistry`, with a faithful and a corrupted source served by a
  dependency-free local HTTP server — no test touches the network. No route
  calls the registry yet: the endpoints that do are the workspace's (#342).

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
| `run_not_found` | 404 | no run under this id, or a string that is not a run id | `GET /runs/{id}` |
| `dataset_absent` | 404 | passage text asked for, no dataset on disk | no |
| `dataset_differs` | 409 | passage text asked for, the dataset on disk is not the run's | no |
| `benchmark_not_found` | 404 | a benchmark name the registry does not know, or a download of one the manifest does not hold | `FsRegistry` |
| `benchmark_exists` | 409 | a download or an import whose directory is already there | `FsRegistry` |
| `download_failed` | 502 | a fetch that failed, a file of the wrong size or digest, a snapshot of the wrong `dataset_version`, or a deadline passed; the detail names both values | `FsRegistry` |
| `download_cancelled` | 409 | a download whose cancellation flag was set; nothing was kept | `FsRegistry` |
| `import_refused` | 422 | an import name outside `[A-Za-z0-9_-][A-Za-z0-9._-]*` (64 bytes at most), a path that cannot be read, or a corpus its adapter refuses — the adapter's error in the detail | `FsRegistry` |
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
- **Eleven codes beyond the design's seven**: `run_not_found` for the above;
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
  an endpoint that needs the text and cannot degrade. Where replay can show
  ids instead, the design document § 8 makes them a flag in a `200` response,
  not an error; that endpoint decides which it is.

## The layers

The server's defence of its origin (ADR-C36 § 1), as Tower layers on the
router — the network envelope, which is where ADR-C10 puts Tower, and the only
place this crate uses it: no handler and no trait is a `tower::Service`
(INV-11). Each is `axum::middleware::from_fn_with_state`, outermost first.

**This crate owns the whole envelope, and applies it last.** `router` takes
the assets — the UI's pages and the fallback that serves them on every
client-side route — as a `Router`, nests the API under `/api` beside them, and
only then wraps the result in the four layers. The reason is how axum's
`Router::layer` works: it wraps the routes that exist when it is called, and a
route merged or a fallback set afterwards answers outside the layers — a
foreign `Host` accepted, no content security policy, no build identity. The
UI's own page is exactly what the policy must reach (ADR-C36 § 5 makes it the
layer that holds), so leaving the order to the caller would leave the defence
to be remembered.

**And it returns a `Server`, not a `Router`**, so the order cannot be undone
after the fact. `Server` is a newtype with no method that adds a route,
merges a router or sets a fallback, and no conversion back into a `Router`;
it offers `into_make_service()` for `axum::serve`, and `tower::Service` over
one request, which is what a connection calls (the envelope, where ADR-C10
puts Tower; INV-11 is about components). Two `compile_fail` doc tests on
`Server` prove that `.route(…)` and `Router::merge(server)` do not compile,
and `tests/server.rs` proves by compiling that `axum::serve` accepts it. The
alternative, a public `envelope(Router, &ServerConfig)` documented as "apply
last", was rejected because it is correct only while the binary never adds a
route after calling it, and nothing would say when it did — the same hole a
returned `Router` leaves. What remains possible is deliberate: the binary can
write a second router of its own around a `Server` and answer routes it adds
itself, which is a new server outside this envelope written by hand, and a
diff a reviewer sees. `tests/layers.rs` checks that a route and a fallback in
the assets carry both headers and are refused on a foreign `Host`.

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
   refuses. Whether `ServerConfig::served` becomes a set of authorities is the
   binary's to decide when it binds the listener (#339).
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
named now because appending a feature to the entry later would escalate — one
`hyper` in `Cargo.lock`, which no core crate reaches), `schemars`, and `tokio`,
whose workspace entry now names `net`, `sync` and `time`. `tower`, the
workspace's serving-envelope entry, is a dependency for the `Service` trait
`Server` implements, which needs no feature; the tests also use
`ServiceExt::oneshot`, whose `util` feature comes from `axum`'s own
requirement on the same `tower`, unified by Cargo, rather than from a feature
appended to the workspace entry. `hyper` is on the
INV-4 deny-list, so the core cannot reach any of this.

`reqwest` is a dependency for the `Registry` file backend's transport, with
its workspace entry's features and none appended, on the one `hyper` already
in `Cargo.lock` — § *`reqwest`, the transport* says why it is here and not in
`ragondin-benchmarks`. `sha2` is a dev-dependency, for the digests of what a
test's local server serves. Neither is a new `[workspace.dependencies]`
entry.
