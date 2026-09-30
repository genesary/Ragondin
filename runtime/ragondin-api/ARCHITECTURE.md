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
| `router` | Builds the router from `Backends` and a `ServerConfig`, both passed in by the binary |
| `Backends` | The five backends, each an `Arc<dyn …>`: `RunStore`, `PipelineSource`, `Registry`, `WorkspaceSettings`, `Launcher` |
| `backends` | The four traits this crate defines, and the values they exchange |
| `response` | Every type the API serializes — the response bodies and `Problem` |
| `error` | `ApiError`, its stable codes, its `application/problem+json` rendering |
| `layers` | The server's defence of its origin, as Tower layers on the router |
| `description` | The API description, assembled from the declared operations and the `schemars` schemas |
| `fs` | The home of the file backends — empty today |

Served today: `GET /api/v1/workspace`, `GET /api/v1/runs`,
`GET /api/v1/runs/{id}`. Nothing here binds a port: the listener, the
loopback-only rule and the embedded assets are the binary's.

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
trait, the `Run` record, and `lower_configuration` — and `ragondin-pipeline`,
for the `LogicalPipeline` that lowering yields. `ragondin-config`,
`ragondin-benchmarks` and `ragondin-metrics` are within INV-12 and arrive with
the endpoints that read them.

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
| `Registry` | here | `fs`, over the benchmark manifest and the datasets directory — not written yet | an object store and the same manifest |
| `Launcher` | here | the binary, over the composition root | a run custom resource and the controller |

- **Async with `async_trait`** for the four defined here (frozen decision): a
  file backend reads the disk and a cluster backend talks to an API server.
  `Launcher::capabilities` is synchronous — it reads what the build registered.
- **`RunStore` is synchronous** (its crate says why), so a handler runs each
  store call on a blocking thread with `tokio::task::spawn_blocking` rather
  than stalling an async worker on the disk.
- **`Launcher` carries the shapes the design document § 7 gives**: `probe`
  returns the identity a `Remote` service reports, `identity` the run id a
  `Submission` announces, and `execute` a `Job`'s terminal `JobState`. The
  crate is internal, so the job model may widen `execute` — progress,
  cancellation — without owing anyone a deprecation.
- **The backends return `ApiError`.** They live in this crate or are written
  for it, and an error that already carries its code needs no second mapping.

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
| `backend_failed` | 500 | a backend failed otherwise — listing the store, say | `GET /runs`, `GET /workspace` |
| `host_refused` | 421 | the `Host` layer refused the request | every path |
| `origin_refused` | 403 | the `Origin` layer refused the request | every path |

Choices made here (`AGENTS.md` § Rules of engagement), since the design
document § 8 lists seven codes and leaves the rest to the implementation:

- **An unknown run id is a 404, `run_not_found`**, not `run_unreadable`. The
  two are different facts — nothing is there, versus something is there this
  build cannot read — and a client acts differently on each. A string that
  cannot be a run id names no run either, and gets the same answer.
- **Four codes beyond the design's seven**: `run_not_found` for the above;
  `backend_failed`, because a backend's I/O failure is none of the seven and
  a problem body must carry some code; `host_refused` and `origin_refused`,
  so that the layers' refusals are problem bodies like every other error.
- **`dataset_absent` and `dataset_differs` have statuses**, 404 and 409, for
  an endpoint that needs the text and cannot degrade. Where replay can show
  ids instead, the design document § 8 makes them a flag in a `200` response,
  not an error; that endpoint decides which it is.

## The layers

The server's defence of its origin (ADR-C36 § 1), as Tower layers on the
router — the network envelope, which is where ADR-C10 puts Tower, and the only
place this crate uses it: no handler and no trait is a `tower::Service`
(INV-11). Each is `axum::middleware::from_fn_with_state`, outermost first:

1. **The build identity**, `x-ragondin-build: <ServerConfig::build>`, on every
   response — refusals and 404s included — so the UI can compare builds on
   any answer. `GET /workspace` reports the same value in its body.
2. **The content security policy**, `default-src 'self'`, on every response.
   Nothing is added for styles. `'unsafe-inline'` would be needed only for a
   `<style>` element or a `style=` attribute present in served HTML; the UI's
   stylesheets are bundled files served from this origin, and React writes a
   component's inline styles through the DOM's style properties, which a
   `style-src` does not govern. If a later UI dependency needs more, the
   policy grows in the pull request that brings it, argued here.
3. **The `Host` check**: the request's `Host` — or, without one, its URI's
   authority, where HTTP/2 carries it — must equal `ServerConfig::served`,
   ignoring ASCII case; otherwise `host_refused`. It compares the authority
   the server was given and resolves nothing, so `localhost` is refused by a
   server given `127.0.0.1:7878`. This is the defence against DNS rebinding.
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
false`, `json` only — one `hyper` in `Cargo.lock`), `schemars`, and `tokio`,
whose workspace entry now names `net`, `sync` and `time`. `tower` is a
dev-dependency, for `ServiceExt::oneshot` in the tests; its `util` feature
comes from `axum`'s own requirement on the same `tower`, unified by Cargo,
rather than from a feature appended to the workspace entry. `hyper` is on the
INV-4 deny-list, so the core cannot reach any of this.
