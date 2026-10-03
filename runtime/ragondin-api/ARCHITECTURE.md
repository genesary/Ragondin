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
| `request` | Every request body the API reads, and the types its query parameters and request headers are read into |
| `extract` | The extractors every handler reads its input through — `ApiPath`, `ApiQuery`, `ApiHeaders`, `ApiJson` — `NoParameters`, and `ApiInput`, the bound a handler's every argument meets (ADR-C37) |
| `routes` | Every route of the `/api` router, listed once: built into the router, and recorded for the description, each registered through the `ApiInput` guard |
| `error` | `ApiError`, its stable codes, its `application/problem+json` rendering |
| `layers` | The server's defence of its origin, as Tower layers on the router |
| `description` | The API description, assembled from the declared operations and the `schemars` schemas |
| `derived` | The data derived from a stored run and its benchmark: per-query scores, per-node ranking metrics, each passage's grade and each node's gold ranks, the gold filter, each query's text |
| `cache` | The workspace's `cache/`: those derived figures, reconstructible, never a truth |
| `endpoints` | The handlers of the workspace's endpoints — pipelines, benchmarks, services — and of `POST /compare` |
| `stages` | A pipeline's stages, derived from its nodes' kinds and positions, by which a comparison aligns runs |
| `lineage` | Which workspace pipeline a run is a run of, by canonical hash — interim: ADR-C39 decides run → pipeline identity; the code does not follow it yet (#392) |
| `comparison` | The runs aligned by stage with the pairs drawn by hand, and the bins of the per-query deltas |
| `validation` | A pipeline document checked as `ragondin validate` checks a file |
| `fs` | The workspace on disk and its file backends: `Workspace`, `FsSettings`, `FsPipelines`, `FsRegistry` |
| `conformance` | The suite every `Registry` backend passes, behind the `conformance` feature |

Served today, under `/api/v1`: `GET /workspace`; `GET /runs`,
`GET /runs/{id}`, `GET /runs/{id}/queries` and `GET /runs/{id}/trace/{query}`
— the last two described in § *Derived data*; `POST /compare`, described
in § *Compare*; `GET /pipelines`,
`POST /pipelines/validate`, `GET`/`PUT /pipelines/{name}`,
`GET`/`PUT /pipelines/{name}/layout`; `GET /benchmarks`,
`POST /benchmarks/import`; `GET /services`,
`PUT`/`DELETE /services/{family}/{name}`,
`POST /services/{family}/{name}/probe` — those described in § *The workspace
on disk*. `/api`, `/api/` and every other path below them are
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

**The blind spot, left to review:** `ragondin-config`, which INV-12 allows,
no longer depends on `ragondin-proto`; the M7 `Stream` source adds that edge
back, and from then `ragondin-proto` is reachable through it. A client
hand-built here over its generated stubs would call a service without
`ragondin-remote`, and the check would not see it. The sign in a diff is a
`tonic` channel or a generated client type in this crate.

Its workspace dependencies today are `ragondin-config` — `parse_document`,
the one definition of a pipeline document's load, and `incompatible_wiring`,
the CLI's report for an edge of the wrong kind (§ The pipelines); its closure
is `ragondin-pipeline`, the YAML parser and two macro crates, no RPC or HTTP
stack — `ragondin-experiments` — the `RunStore`
trait, the `Run` record, the typed `Trace`, `lower_configuration`, the
walk to a run's ranking node, and `compare_runs` — `ragondin-pipeline`, for the `LogicalPipeline`
that lowering yields,
`ragondin-benchmarks`, for the manifest, the download, the verification and
the import the `Registry` file backend is written over, and for the digests
and the chunk derivation passage text is verified against,
`ragondin-metrics`, for the per-query and per-node scores and the fold
they are computed over, and
`ragondin-types`, for the ids they are computed over. `ragondin-benchmarks`
and `ragondin-metrics` reach no engine and no component: their closure is the
core's `ragondin-types` and its readers, and `just check-invariants` walks it
— `scripts/test-check-invariants.py` holds a case in which `ragondin-benchmarks`
reaches the engine and INV-12 fails through it. **`ragondin-harness` is
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
| `PipelineSource` | here | `fs::FsPipelines`, over `pipelines/<name>.yaml`, its layout and its pairings | custom resources |
| `WorkspaceSettings` | here | `fs::FsSettings`, over `workspace.toml` | the deployment's bindings, read-only |
| `Registry` | here | `fs::FsRegistry`, over the benchmark manifest and the datasets directory | an object store and the same manifest |
| `Launcher` | here | the binary, over the composition root | a run custom resource and the controller |

- **Async with `async_trait`** for the four defined here (frozen decision): a
  file backend reads the disk and a cluster backend talks to an API server.
  `Launcher::capabilities` is synchronous — it reads what the build registered.
- **`RunStore` is synchronous** (its crate says why), so a handler runs each
  store call on a blocking thread with `tokio::task::spawn_blocking` rather
  than stalling an async worker on the disk.
- **`Launcher` carries the shapes the design document § 7 gives**: `probe`
  returns the identity a `Remote` service reports — for a served model, where
  the family reports one per served model (ADR-C32 § 4) — `identity` the run
  id a `Submission` announces, and `execute` a `Job`'s terminal `JobState`.
  `check_binding` says whether the composition root would accept a binding,
  in `--remote`'s words: only the binary knows the names it gives `Local`
  components, so a binding is checked there before it is stored.
  `check_document` likewise says whether it would accept a pipeline's keys,
  in `bench`'s words, before a document is stored: only the binary knows
  which keys each component reads.
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
  loaded whole and digested, and nothing is kept: correct, and slow for a
  large corpus. `GET /benchmarks` and `GET /workspace`'s count call it on
  each request; a figure cache is `cache/`'s business, not this backend's.
  `dataset` is the exception: it keeps the datasets it verified loaded in
  memory between calls, and digests one again only when its files change —
  § *The loaded datasets* below.
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
  `datasets::verify`, which discards what it loaded. `pinned()` lists that
  same pinning for every benchmark — `FsRegistry`'s one `pins` helper
  answers both — from the manifest and the import records alone, loading
  nothing; `GET /runs` names a run's benchmarks with it.
  `BenchmarkEntry`, which the trait exchanges, moved to
  `response.rs` as a response type — the way `Settings` already carries
  `ServiceBinding` — and `BenchmarkStatus` is gone, replaced by
  `BenchmarkState`'s five states.
- **`sweep_staging`** removes the staging directories an interrupted download
  or import left. The binary calls it once at startup, before its listener
  opens and so before any download can run, since a running download's
  staging directory has the same shape (`bin/ragondin/ARCHITECTURE.md`
  § The wiring).
- **The conformance suite** (`src/conformance.rs`, behind the `conformance`
  feature, the model `ragondin-experiments` set for `RunStore`) checks the
  contract `Registry`'s documentation states: a fresh listing; a download
  reported `ready` by the listing and by `verify`; a failed download that
  leaves the benchmark `available`, as before; a cancelled one; unknown
  names; an import, including a name holding NUL refused as
  `import_refused`; `pinned`, which names a manifest entry with its digest
  before anything is downloaded and an import with the digest it recorded;
  and `dataset`, which finds a downloaded and an imported
  benchmark by their digests, answers `Absent` before the download,
  `Unknown` for a digest nothing is pinned to, `Differs` once a downloaded
  benchmark that verified has its content changed and `Unreadable` once it
  is broken — the fixture's `alter` hook does both behind the registry's
  back, as a person editing the datasets directory would.
  `tests/registry_conformance.rs` runs
  it against `FsRegistry`, with a faithful and a corrupted source served by a
  dependency-free local HTTP server — no test touches the network. The
  derived data's routes call `dataset`; `GET /benchmarks` lists the registry
  and `POST /benchmarks/import` imports through it; a download is a job on
  the queue's IO lane, and its route arrives with the queue (#349).

### The loaded datasets: a choice made here

`FsRegistry::dataset` keeps each dataset it verified loaded in memory
between calls (`fs/memo.rs`), so per-node and replay requests on one run —
the matrix and the Replay screen call them per interaction — load and digest
the dataset once rather than on every request. `RunDataset::Verified` hands
out a shared `LoadedDataset`: the `Benchmark`, and the `CorpusIndex` derived
from it on first use and then kept, so replay chunks a corpus once too. **It
is an optimisation, never a truth** (the design document § 6): no verdict
changes, and nothing is written to disk.

- **The key** is the directory and the `dataset_version` it is pinned to.
  A slot holds a dataset only once `ragondin_benchmarks::identity` digested it
  to that version — the one definition, called as before; nothing is digested
  a second way. Only `Verified` is kept: a dataset absent, differing or
  unreadable is loaded again on the next call, and whatever was held for that
  directory and version is dropped.
- **The invalidation is a fingerprint**, taken before every serve and before
  every load: every entry under the directory, recursively, with its relative
  path, kind, size and modification time, and on Unix its inode and status
  change time. A symbolic link is stamped by its target and, when that is a
  directory, walked through as the loader reads it; a directory already
  walked — the same device and inode on Unix, the same canonical path
  elsewhere — is not walked again, so a link loop ends, and on Unix a bind
  mount of a directory inside itself too (elsewhere such a mount loop is
  not caught).
  Any difference — one byte rewritten in place, a file added, renamed or
  removed, a change under a linked directory — loads and digests again, and
  the verdict is whatever that digest says. **A fingerprint is not a
  digest**: it decides when to digest, never whether a dataset verifies. It
  is taken *before* the load, and before waiting on another request's load,
  so a file changed meanwhile is a change to the next call, not hidden behind
  what this one loaded. The status change time is there because a user can
  restore a file's size and modification time (`touch -r`, an archive
  extracted over it) but not that.
- **Racy timestamps.** A stamp is only as fine as the filesystem's clock —
  ext4 before Linux 6.13 ticks per jiffy, FAT every two seconds, NFS and SMB
  coarsely — so a same-size rewrite, on the same inode, within one tick of
  the previous write leaves the stamp equal. A dataset any of whose
  modification or status change times falls within two seconds of the moment
  the fingerprint was taken, or after it, is therefore served but not kept
  for later requests: it is held only for the requests whose fingerprint,
  equal to the loader's, was taken before the load *began* — they get the
  files as they were when it began, a state that existed while they were
  in flight — and any request fingerprinted after that, during the load or
  later, loads again, since it may have seen a rewrite the load did not.
  Once its files are older, a rewrite lands on a later tick and is seen.
  **What remains unseen**: timestamps set back by hand on a platform that
  keeps no status change time (not Unix), a file server whose clock runs
  more than the margin behind this machine's, and a directory on overlayfs
  with several lower layers and no `xino`, where two directories can report
  the same device and inode: the second is taken for one already walked and
  skipped, so a change under it goes unseen.
- **The bound is two datasets**, least recently used dropped first: the one
  on screen and the one just left, so moving between two benchmarks' runs
  loads neither again. It is a count, not a size, because nothing measures a
  loaded dataset's memory; and it is that small because a large corpus is
  gigabytes, and a kept dataset holds the corpus and its chunk set (about
  twice the corpus), while a comparison or a matrix reads one benchmark. A
  slot's old dataset is dropped before a reload, so the peak is the bound
  plus the load in flight. A request still holding a dropped dataset keeps
  it until it answers.
- **The cost of a serve** is one walk of the directory: a `stat` per entry,
  O(entries), no file read.
- **Concurrency**: a call fingerprints with no lock held, locks the list only
  to find its slot, then locks that slot alone while it compares and, if it
  must, loads. Several requests for one dataset — the matrix fetching N runs
  at once, right after a download too — wait for one load and share it; a
  request for another dataset is not held up. A slot dropped by the bound
  while its load runs is no longer listed, so a request arriving meanwhile
  makes a new slot and loads again: a duplicate load, never a wrong answer.
- **Tested** in `fs/memo.rs`, with a counting loader — one load while the
  files are unchanged, another after each kind of change (including under a
  linked directory, dated explicitly so no test depends on the clock's
  tick), a link loop, the racy margin and a stamp in the future, the bound
  and its order, one load under concurrent calls, within the margin too, and
  a slow load of one dataset holding up no other — and over the API in
  `tests/dataset_memo.rs`: consecutive `/queries` and `/trace` requests are
  answered from one load, one byte changed between requests is
  `dataset_differs`, and a dataset just written is loaded again by each
  request that follows another's load.

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

## The workspace on disk

`fs::Workspace` is the workspace of the design document § 6, opened: a root,
and every path derived from it.

```text
<root>/
  workspace.toml                the datasets directory and the services — deployment data, never hashed
  pipelines/<name>.yaml         a pipeline document, the source of truth
  pipelines/<name>.layout.json  its layout, never in its hash
  pipelines/<name>.pairing/     its manual pairings, one file per other pipeline — never hashed (§ Compare)
  layouts/                      layouts copied at launch — created here, written by the queue (#349)
  runs/                         the run store, as `bench --store <root>/runs` writes it
  jobs/                         the queue's state — created here, written by the queue (#349)
  cache/                        derived data — created here, written by #343
  datasets/                     benchmarks, when workspace.toml names no other directory
```

`Workspace::open` reads `workspace.toml` **first**, and refuses a malformed
one with `WorkspaceError::Malformed` — the file and the line — before
anything is created: reported, never repaired. Then it creates each missing
directory and a missing `workspace.toml`, with nothing set; an existing
workspace is left as it is. `open_with_store` takes the store's directory
when it is not `<root>/runs` (the binary's `--store`).

### `workspace.toml`, and why it is read by hand

The settings are deployment data (ADR-C32 § 2): an address here stays out of
every pipeline document and every run identity, which is what lets a
workspace be shared or committed without carrying anyone's addresses into an
experiment. The file:

```toml
datasets = "/data/benchmarks"   # optional; relative to the workspace

[services]
"generator/qwen" = "http://127.0.0.1:8080"
```

**A choice made here** (`AGENTS.md` § Rules of engagement): the file is read
and written by `fs/settings_file.rs`, a reader of exactly that subset of TOML,
because a TOML parser is not among the dependencies ADR-C36 § 6 admits, and
that section makes any other entry a new decision — opened as #374, which
weighs `toml_edit`, and the comments a person adds that a write here loses,
against this reader. It reads blank lines and
`#` comments, `datasets` before any table, one `[services]` table of
`"<family>/<name>" = <string>`, basic strings with TOML's escapes and literal
strings, and a comment after a value; it refuses everything else — another
key or table, a duplicate, a value that is not a one-line string, a service
key without its `/`, anything after a value, a control character other than
a tab in a string or a comment, whitespace other than a space or a tab
(named by its code point — a no-break space is `U+00A0`), an
escape TOML does not define (a `\u` takes four hex digits, a `+` not one of
them), and a byte-order mark, named as one — naming the line. Everything it
accepts is valid TOML, so another reader agrees with it; a person who writes
TOML it does not read is told where, rather than misread. Admitting a TOML
crate later replaces one module.

`FsSettings` reads the file on every call, so a hand edit is seen at once.
The datasets directory is `<root>/datasets` when the file names none, and a
relative one is read against the root; a write leaves the default unstated
and writes a directory under the root relative to it. **A write replaces the
file whole** — rendered, written beside as a `.`-named file, flushed, renamed
over — so a reader sees the old file or the new one; it does not keep a
comment a person added. `FsSettings` stores what it is given: whether a
binding is acceptable is `Launcher::check_binding`'s to say, before the
handler writes it.

### The pipelines

`fs::FsPipelines` is the `PipelineSource` over `pipelines/`.

- **The verbatim rule.** A document is stored exactly as it was sent and read
  exactly as it is stored, never parsed and re-serialized — for the reason
  `bench` keeps a run's configuration text verbatim: the comments and the
  formatting a person gave it are theirs, and a document read and written
  back unchanged is byte-identical.
- **The etag rule.** A document's revision — its etag — is the SHA-256 of its
  bytes, in hex: bare in a JSON body, quoted in the `ETag` header. It needs no
  clock, so it cannot race on a filesystem with coarse timestamps, and bytes
  written back unchanged keep it. Every read answers it; every write states
  what it expects, `If-Match: "<etag>"` to replace, `If-Match: *` to replace
  whatever is stored (RFC 9110 § 13.1.1), `If-None-Match: *` to create, and
  is refused `precondition_failed` — with the current etag in `ETag`, in the
  detail and as the problem's `etag` member — when the stored bytes digest to
  anything else, when a creation meets a file or `If-Match: *` meets none, or
  when it states neither, since a write that does not say what it read
  cannot be kept from overwriting a change. This is the server's half of
  ADR-016's promise that the editor never overwrites a file changed since it
  read it; the editor sends the header. **Both headers are declared as
  optional `in: header` parameters of `PUT /pipelines/{name}`**, read through
  `ApiHeaders<PreconditionHeaders>` (§ Request input goes through one
  extractor module), so the UI's generated client sends them and no screen
  writes header plumbing. Which of the two a write states, `*`, and a weak
  `W/` tag read as its strong form, are the handler's to decide
  (`precondition` in `endpoints/pipelines.rs`); the extractor only reads
  them, each trimmed.
- **A write is checked, validated, then stored**: the document is lowered
  (`validation::lower`, `pipeline_invalid`) and handed to
  `Launcher::check_document` with the workspace's bindings — `bench`'s key
  refusals, in its words (§ Validation, in the CLI's words) — then the
  backend validates it, checks the precondition, and writes it beside the
  file and renames it over — so a refused write changes nothing on disk.
  Writes from this process are serialised; an editor outside it saving
  between the check and the rename is the one race left, and the next
  write's precondition reports it. `tests/pipelines.rs` sends eight writes
  naming one etag at once and finds one stored and seven refused.
- **Names** are one file name in the import names' alphabet,
  `[A-Za-z0-9_-][A-Za-z0-9._-]*`, 64 bytes at most, no trailing `.`, not a
  device name Windows reserves (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`,
  `LPT1`–`LPT9`, in any case, with or without an extension — the rule
  `ragondin-benchmarks` applies to an import, written again because it is
  private there), and not `validate`, which the router gives
  `POST /pipelines/validate` (a static segment outranks a parameter, so a
  pipeline under that name could not be read). A read of any other name is
  `pipeline_not_found`, a write `request_invalid`. **A name that differs
  from a stored one only in case is `request_invalid`, naming the stored
  one** — for a write, a read and a layout read or write alike: on a
  filesystem that ignores case the two are one file, so a write would
  replace the other behind its etag, a read would answer the other under
  this name, and a layout would land beside the other; refused on every
  filesystem, so the answer does not depend on which. Names keep their case
  otherwise, so a file a person named `Hybrid.yaml` is still listed and
  read as `Hybrid`. The listing skips a file whose stem is not a name —
  staging files and hidden ones included — and is sorted by name; each entry
  carries its etag, its modified time, and its hash or its validation error,
  computed on the request. The modified time goes through
  `UnixMillis::from_system_time`, the rule every time in the API follows: a
  time before the epoch is unknown, `null`, never `0`.
- **The layout format** — a choice made here, the design leaving it open
  (ADR-C36 § 7): `pipelines/<name>.layout.json`, JSON,
  `{"version": 1, "nodes": {"<node id>": {"x": <number>, "y": <number>}}}`.
  `version` is `1`, the only one this build reads or writes; another is
  `request_invalid` on a write and `backend_failed` on a read, never
  guessed at. A node id the document does not hold is kept, not pruned: the
  layout is UI metadata the editor owns. The layout is a separate file and
  never enters the document's etag or hash (INV-8); it has no etag of its
  own, last writer wins, which costs a position, never a pipeline. Writing a
  layout needs the pipeline to exist (`pipeline_not_found` otherwise); it is
  re-serialized as JSON, since the verbatim rule is the document's.

### Validation, in the CLI's words

`validation::check` runs the load `ragondin validate` runs, by calling
`ragondin-config`'s `parse_document` on the text — the one definition of it,
which `LocalFile` calls on a file's contents: the version peeked, the document
parsed into `RawPipeline` (INV-9: the wire schema, never an internal type),
`validate` — and renders `content_hash` over the canonical logical form
(INV-8). It is the one place this crate renders a document's hash: the
listing, a write and `lineage.rs` all read it there. `bin/ragondin`'s
`tests/ui.rs` posts every fixture configuration under `bin/ragondin/tests`,
and one document per refusal the load can make, and compares the answer with
what `ragondin validate` prints for the same file: the hash, or the refusal
byte for byte, less the file path. `tests/pipelines.rs` pins every refusal's
whole problem body.

- **The words** are the CLI's, less the file path a request has none of:
  `DocumentError`'s heading ("could not parse configuration", "configuration
  is not a valid pipeline", "the configuration is written in a schema version
  this build cannot read"), a colon, and the cause `ragondin validate` prints
  under `caused by:`; and for an edge of the wrong kind, `ragondin-config`'s
  `incompatible_wiring` report — the one the CLI prints — with "the
  configuration" as its subject where the CLI names the file. Neither is
  written here.
- **The location**: a kind mismatch names its consumer and the edge; an
  unknown component, a non-finite parameter, a dangling input, a duplicate id
  or an id that is both an input and a node names the node; a cycle names its
  first node. **What stays unlocated**: a syntax or shape error, which
  `serde_yaml` locates by line and column (in the detail) and not by node; an
  input-arity error; and a dangling input's edge, whose port the error does
  not carry. Locating those would need `ragondin-pipeline`'s errors to carry
  more, which is an INV-1 change and not this crate's.
- **`POST /pipelines/validate` adds nothing**: it answers what `ragondin
  validate` answers, which applies none of the composition root's checks
  (ADR-C32 § 2). The bin parity test covers a fixture with a URL-valued
  parameter, which both accept.
- **A write adds the composition root's key refusals**, through
  `Launcher::check_document`: the binary runs `bench`'s own `check_keys` —
  a `dense` node's keys by the nature of the embedder it names, a
  `cross_encoder`'s, a bound reranker's, one embedder per pipeline — with the
  workspace's bindings deciding which names are bound, and answers
  `pipeline_invalid` in `bench`'s words, naming the node. A key no component
  of the node's nature reads is refused rather than hashed as inert (ADR-C32
  § 1), whatever its value; a URL-valued key the component reads is a key
  like any other. This crate does not look at values: it cannot know a
  component's keys (INV-12), and a guess at what an address looks like
  refused read parameters and passed unread ones. The one refusal `bench`
  adds that depends on the build — an `onnx` embedder without the `onnx`
  feature — is not made: a stored document is not a run.
- **The load is written once**, in `ragondin-config`: this crate,
  `LocalFile::load` and `ragondin-experiments`' `lower_configuration` each
  call `parse_document` and add only their own words around its verdict.

### The services and the probe

The bindings are `WorkspaceSettings`' services. A `PUT` asks
`Launcher::check_binding` first — the composition root's refusals, in
`--remote`'s words — then replaces the name's address or appends the binding;
a `DELETE` removes it, or answers `service_not_found`. A service write holds
a lock across its read and write of the settings, so two writes do not each
start from what the other replaces. Neither touches a pipeline document or a
run: an address is not in either.

`POST /services/{family}/{name}/probe` reads the address the workspace binds
the name to (`service_not_found` when there is none) and asks the launcher to
read its identity, with the body's optional `served_model` — no body at all
is a body with none. **What a probe
learnt is this server's memory, not the workspace's**: per binding, the
address last probed, whether that probe read an identity, and the identity
last read with its address. `GET /services` reports a binding `connected`,
with its identity, only when its last probe at its current address read one;
an unreachable probe's detail carries the identity last read under the name,
and the address it was read at when that was another; `GET /workspace`
counts the connected ones. Nothing of it is written: a restart forgets, and
the Setup screen probes again.

### `GET /workspace`'s counts

Computed on each request, nothing cached: the pipeline documents listed,
the runs the store lists, the benchmarks whose state is `ready` or `local` —
the registry verifies every dataset to say so (§ The `Registry` file
backend) — and the services connected.

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
runs `ragondin-config`'s `parse_document` over the kept text. This crate calls
that function; it does not parse YAML itself. A node
carries its id, family, `impl:` name and parameters; an edge carries its
producer, consumer, port and the kind of value its producer puts on it — the
query for a declared input, and otherwise `produced_kind` of the producing
node. The nodes come in the canonical order (by id), the edges grouped by
consuming node, in port order. `prefix_of` is present and always absent: no
run is recorded as a prefix yet.

**A run the store lists and cannot load** is listed in `GET /runs` under
`unreadable`, with the store's reason, rather than dropped or failing the whole
listing — reported, never repaired.

**What the listing says of each run, beyond its stored fields.**
`RunSummary` carries:

- `started_at_ms` and `finished_at_ms` — `RunDetail` carries them too — read
  from `Run::times` and never computed: `null` when the run recorded none.
  This crate stamps no run; a job's own times are the job's.
- `pipeline_names`: every workspace document whose canonical hash is the
  run's, from `lineage::pipelines_by_hash`, sorted. A list, never a pick:
  several documents can be one canonical form. It is the content fact of
  the two ADR-C39 § 4 exposes about a run's pipeline; the launch record it
  sits beside is not served yet, and the two are never resolved into one
  name.
- `benchmark_names`: every registry entry pinned to the run's
  `dataset_version`, a manifest entry or an import, sorted — the pinning
  `Registry::dataset` locates by, read through `Registry::pinned`, which
  loads nothing: naming a benchmark is not verifying it.
- `metric_families`: keyed exactly like `metrics`, each metric's family as
  `ragondin-metrics`' catalogue gives it (`Metric::parse`, then
  `Metric::family`) — `ranking` or `answers` — and `unknown` for a name the
  catalogue does not know. Such a metric is kept in `metrics`, never
  dropped; the family says only that nothing here knows how to read it.
- `median_query_latency_nanos`: the lower median
  (`ragondin_experiments::lower_median`), over the run's queries, of each
  query's latency (`Trace::latency_nanos`, the sum of its trace's node
  durations); `null` when no trace reads. A trace that does not read, or
  whose durations overflow, is left out of the median rather than failing
  the listing. It is derived data: read from the traces alone — never from
  the dataset, so a run whose dataset is not on disk has it — and cached as
  `cache/<run_id>/latency.json` (§ *The cache: a choice made here*). It is not
  the run's wall time, `finished_at_ms − started_at_ms`, which counts
  preparation too.

`RunListing::shapes` carries each listed pipeline's graph once, keyed by its
canonical hash, by the conversion `GET /runs/{id}` serves (`convert::shape`
and `convert::detail` share `graph`), so a screen draws every group's shape
from the listing. One canonical hash is one canonical form and so one graph;
it is lowered from the first of its runs whose document lowers, and a
pipeline none of whose documents lowers has no entry. The workspace's
pipelines and the registry's pins are each read once per request, and a
failure of either fails the listing, by design, rather than answering with
every name list silently empty. A cache that fails does not: its first
reason is `RunListing::cache_error`, and every latency is computed anyway.

**A field serialized on every response is required in its schema**, nullable
when it can be null: `RunDetail::prefix_of`, the two times of `RunDetail`
and `RunSummary`, `RunSummary::median_query_latency_nanos`,
`QueryScores::text` and `QueryScores::duration_nanos`,
`MetricRow::direction`, `PipelineSummary::modified_ms`, `Location::node` and
`Location::edge` carry a `transform` that lists every property as required,
since `schemars` would otherwise leave an `Option` out and a generated client
would type it as possibly absent. `Problem::location`, `Problem::etag` and
`Problem::name`, each omitted when there is none, stay optional, as do
`RunListing::cache_error`, omitted while the cache works so a working
listing reads as it always has, and `NodePair::label`,
which a request may leave out. **A request body refuses a
field it does not read** (`deny_unknown_fields`), so a misspelled field is
`request_invalid` rather than dropped; its schema says
`additionalProperties: false`, which the UI's type generator reads as the
closed object TypeScript gives anyway. `Problem::code`'s schema is an enum of
`ApiError::CODES`, so a generated client can narrow on it.

## Request input goes through one extractor module

**ADR-C37 requires that a handler of the `/api` router reads request input
only through this crate's own extractors**, defined in one module,
`src/extract.rs`: `ApiPath<T>`, `ApiQuery<T>`, `ApiHeaders<T>` and
`ApiJson<T>`, each with
`Rejection = ApiError`, so every refusal is a problem body (ADR-C37 § 2). No
such handler takes `axum::extract::Query`, `axum::extract::Path`,
`axum::extract::Json` or a `HeaderMap` to read a request header, and none
reads `Uri::query()`. The fallbacks that take the `Uri` only to name the
request, and the assets fallback `assets::serve`, which reads the `Method` and
the `Uri` to choose the file it serves, are outside the rule, as are the
layers, which are the envelope. A query string is validated as strict
percent-encoded UTF-8 before axum's `Query` deserializes it; a query parameter
type is a closed struct of self-validating values, and it and every request
header a handler reads are declared in the description from their schemas
(ADR-C37 § 3 to § 5). A new endpoint that reads raw input is the sign a
reviewer looks for.

**Every handler takes an `ApiQuery`**: `ApiQuery<NoParameters>` when it
takes no parameter — an empty braced struct that refuses any — and
`ApiQuery<RunQueriesParameters>` for `GET /runs/{id}/queries`, whose one
value, `missing_gold_at`, is the newtype `MissingGoldAt`. The parameter and
header types are in `src/request.rs`. **The description reads them off the
routes**: every route is listed once, in `routes::api`, and read twice — by
`routes::Builder`, which makes the axum router, and by `routes::Declared`,
which records each handler's `ApiQuery` and `ApiHeaders` types — so
`description.rs` declares the types the handlers take, never a second list
kept beside them; a route without an operation, or the reverse, stops the
description. A header type names its headers in `HeaderFields::NAMES`, read
once per type rather than from the schema on every request, and the
description checks that list against the type's schema. Two tests hold the
rule over every operation in `OPERATIONS` (`tests/extractors.rs`): an
undeclared query parameter answers `parameter_invalid`, and a path segment
that does not decode to UTF-8 answers `parameter_invalid` naming the path
parameter.

**How it is enforced.** By the compiler, for every handler: `routes::Routes::route`,
the one way a route of the `/api` router is registered, takes a handler only
when axum's `Handler<T, S>` types its arguments as `T = (M, T1, …, Tn)` with
every `Ti` an `ApiInput` — `State`, `ApiPath`, `ApiQuery`, `ApiHeaders` or
`ApiJson` (`ApiInputs`, implemented by macro for up to eight arguments). A
handler that takes axum's `Path`, `Query` or `Json` — bare, in an `Option`
or in a `Result` — `Bytes`, `RawQuery`, the `Uri`, the `Request` or a
`HeaderMap` does not compile, whatever it is imported as.

**`Routes::route` is the only way in, by `clippy.toml`.** The guard binds
only what is registered through it, so `clippy.toml` — in this package, not
at the workspace root, where it would reach every crate — refuses, under
`disallowed-methods`, every method that adds a route, a service, a fallback
or a layer:

- on `axum::Router`: `route`, `route_service`, `nest`, `nest_service`,
  `merge`, `layer`, `route_layer`, `fallback`, `fallback_service` and
  `method_not_allowed_fallback`;
- on `axum::routing::MethodRouter`: `on_service`, `fallback`,
  `fallback_service`, `layer` and `route_layer`;
- the free functions `axum::routing::on_service`, `any_service` and each
  method's `*_service`;
- `axum::middleware::from_fn` and `from_fn_with_state`.

Two sites allow them, each saying why:

- `routes::Builder::into_router` — the one `Router::route` an /api handler
  meets, every method router built through the guard, and the
  `MethodRouter::fallback` that answers a method it does not serve with
  `method_not_allowed`;
- `router` in `lib.rs` — the one assembly site: the nest under `/api`, the
  bare-prefix route and the naming fallback (ADR-C37 § 2's exception: they
  take the `Uri` and the `Method` only to name the request), the assets'
  fallback service (`assets::endpoint`, outside the `/api` router), and the
  envelope's layer stack (`layers::envelope`, ADR-C10), applied last with one
  `Router::layer`.

One more allow covers `from_fn_with_state` alone: it sits on
`layers::middleware`, a one-line wrapper that makes one of the envelope's
middleware functions a layer. `layers::envelope`, which stacks them, carries
no allow, so a route, a nested router or a service added there is refused
like anywhere else. A route, a service or a fallback added outside the two
sites fails `just clippy`. Within them, review holds the line. `clippy.toml` also refuses `axum::extract::Path`,
`axum::extract::Query` and `axum::http::HeaderMap` under `disallowed-types`
in the crate's other code, and `src/extract.rs` alone allows them, saying
why. `axum::extract::Json` is not on that list, because
it *is* `axum::Json`, the response every handler returns, and
`disallowed-types` cannot tell an argument from a return type; the guard
refuses it as an argument.

Choices made here (`AGENTS.md` § Rules of engagement), each within what
ADR-C37 decides:

- **`name` is present exactly when the extractor knows it.** A path value is
  named by axum's rejection, for a tuple by the route's own parameter list,
  and — when a value's own type refuses it without a key or a position — by
  the route's parameter if it has only one; among several, none is guessed.
  A query value that is not percent-encoded UTF-8 is named when its
  name decodes, and not when the name itself does not. A header sent twice
  is named by its wire spelling, `If-Match`. A refusal from the
  deserializer — an unknown, repeated or unreadable parameter — names none:
  serde's reason is text, and a name parsed out of it would be a guess. A
  value type describes itself instead (`` `missing_gold_at` is a positive
  integer ``).
- **A header value is read as trimmed text**, into a JSON object of strings
  the header type is deserialized from, so its fields read strings: they are
  `Option<String>` where the header's meaning is the handler's to read —
  the precondition's `*` and weak tags stay in `precondition`, as they were.
  A value that is not text is `request_invalid`, as before the headers were
  declared.
- **An empty body reads as JSON `null`**, so `ApiJson<Option<T>>` reads no
  body as `None` — what the probe needs, a context builder sending none —
  and every other body type refuses it, `request_invalid`, as before. A body
  that is the literal `null` is refused whatever the type, so the probe
  refuses it as it did when it read bytes.
- **A query type's closure is checked on its schema**, `additionalProperties:
  false`, which refuses a struct without `deny_unknown_fields`, a map, and a
  flattened map. A `#[serde(flatten)]` field under `deny_unknown_fields`
  leaves no mark on the schema — `schemars` renders the struct closed — and
  serde then refuses an unknown parameter too, so the schema says what the
  type does; the rule against it (ADR-C37 § 4) is review's to hold.

## The error codes

`ApiError` is typed with `thiserror` (ADR-C13); `anyhow` is not a dependency.
Each variant renders as `application/problem+json` — `type`
(`urn:ragondin:problem:<code>`), `title`, `status`, `detail`, `code`, `hint`,
`location` for a validation failure, and `name` for a `parameter_invalid`
whose parameter is known — absent otherwise, never guessed (§ Request input
goes through one extractor module). `ApiError::CODES` lists them. A code
no handler raises yet still exists, so a later endpoint adds a handler, not a
code.

| Code | Status | When | Raised today |
|---|---|---|---|
| `pipeline_invalid` | 422 | validation refused a document, or — on a write — the composition root refused its keys; `location` names the node and edge when they can be named | `POST /pipelines/validate`, `PUT /pipelines/{name}` |
| `impl_not_in_build` | 422 | an `impl:` this binary lacks, or — with the feature named — a `Remote` component a build without `remote` cannot construct | the probe, in a build without `remote` |
| `service_unreachable` | 502 | a probe or a submission reached no service; the detail carries the address, the network error and the identity last read under the name | the probe |
| `run_exists` | 409 | a submission's run id is already stored or queued | no |
| `run_unreadable` | 500 | a stored run this build cannot read: torn, malformed, or a configuration that no longer lowers | `GET /runs/{id}` |
| `run_not_found` | 404 | no run under this id, or a string that is not a run id | `GET /runs/{id}` and below |
| `query_not_found` | 404 | a query id the run's traces do not hold | `GET /runs/{id}/trace/{query}` |
| `parameter_invalid` | 400 | a query parameter the endpoint does not take, given twice, or a value it cannot read; a query string that is not percent-encoded UTF-8; a path value that does not decode to UTF-8 or does not read, naming the path parameter; a header the endpoint reads, sent twice, naming it. `name` carries the parameter when it is known | every endpoint |
| `dataset_absent` | 404 | ground truth needed, and the run's dataset is not on disk or pinned by nothing | `GET /runs/{id}/queries?missing_gold_at=` |
| `dataset_differs` | 409 | ground truth needed, and the dataset on disk is not the run's: its digest differs, or it does not load (the detail says which) | `GET /runs/{id}/queries?missing_gold_at=` |
| `benchmark_not_found` | 404 | a benchmark name the registry does not know, or a download of one the manifest does not hold | `FsRegistry` |
| `benchmark_exists` | 409 | a download or an import whose directory is already there | `FsRegistry` |
| `download_failed` | 502 | a fetch that failed, a file of the wrong size or digest, a snapshot of the wrong `dataset_version`, or a deadline passed; the detail names both values | `FsRegistry` |
| `download_cancelled` | 409 | a download whose cancellation flag was set; nothing was kept | `FsRegistry` |
| `import_refused` | 422 | an import name outside `[A-Za-z0-9_-][A-Za-z0-9._-]*` (64 bytes at most, no trailing `.`, no Windows device name), a path that cannot be read, or a corpus its adapter refuses — the adapter's error in the detail | `FsRegistry` |
| `pipeline_not_found` | 404 | no pipeline of this name, or a name that is not one file name | `GET /pipelines/{name}`, the layout endpoints |
| `precondition_failed` | 412 | a pipeline write whose `If-Match` names another revision or, as `*`, meets no file, whose `If-None-Match: *` meets an existing file, or that states neither; the current etag in `ETag`, the detail and the `etag` member | `PUT /pipelines/{name}` |
| `binding_refused` | 422 | a binding the composition root would refuse on `--remote`, in its words | `PUT /services/{family}/{name}`, the probe |
| `service_not_found` | 404 | no service bound under this family and name | `DELETE /services/…`, the probe |
| `request_invalid` | 400 | a body that is not the operation's JSON — a field missing, or one it does not read — a pipeline name that is not one file name on a write or differs from a stored one only in case, a layout of another version, a probe its family cannot answer as asked; a body that fails to buffer — a connection that breaks off mid-body; a precondition header whose value is not text | every endpoint that reads a body |
| `backend_failed` | 500 | a backend failed otherwise — listing the store, say | `GET /runs`, `GET /workspace`, the file backends |
| `host_refused` | 421 | the `Host` layer refused the request | every path |
| `origin_refused` | 403 | the `Origin` layer refused the request | every path |
| `route_not_found` | 404 | a path under `/api` that names no endpoint | the API's fallback |
| `method_not_allowed` | 405 | an endpoint asked for with a method it does not serve; `Allow` lists the ones it does | each endpoint |
| `runs_not_comparable` | 409 | runs evaluated on different benchmarks — the detail names both `dataset_version`s — or more than a baseline and four runs, naming the ceiling | `POST /compare` |
| `body_too_large` | 413 | a body over axum's default body limit, which `ApiJson` reads under | every endpoint that reads a body |

Choices made here (`AGENTS.md` § Rules of engagement), since the design
document § 8 lists seven codes and leaves the rest to the implementation:

- **An unknown run id is a 404, `run_not_found`**, not `run_unreadable`. The
  two are different facts — nothing is there, versus something is there this
  build cannot read — and a client acts differently on each. A string that
  cannot be a run id names no run either, and gets the same answer.
- **Thirteen codes beyond the design's seven** before the workspace's
  endpoints, eighteen with them: `run_not_found` for the above;
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
- **Five codes for the workspace's endpoints**, each a different action for
  the client. `pipeline_not_found` and `service_not_found` are their own
  404s for the reason `run_not_found` is. `precondition_failed` is HTTP's 412
  for all three ways a write's precondition fails, a missing one included:
  a 428 would be a sixth code for one case, and the detail says which.
  `binding_refused` is a 422 because the request named something the
  composition root refuses, and its detail is the binary's words, untouched;
  it was `backend_failed` (500) before, which told the client the server had
  failed. `request_invalid` is the 400 every malformed body gets, as a
  problem body rather than axum's plain-text rejection — so a body is read
  by `ApiJson`, buffered and parsed here; it is not `parameter_invalid`,
  which names a parameter, so a client can tell which part of its request
  to correct.
- **Two refusals of a body's buffering, which ADR-C37 made problem bodies.**
  A body over the limit is `body_too_large`, its own code because its
  status is HTTP's 413 and its action differs — send less, not send
  otherwise. A body that fails to buffer is `request_invalid`: the request's
  body could not be read, the client's action is to send it again, and a
  code of its own would buy it nothing it can act on differently.
- **An invalid path value is `parameter_invalid`**, naming the path
  parameter (ADR-C37 § 4) — never `run_not_found` or `pipeline_not_found`,
  which would tell the client a thing is absent when its request named none.
- **`runs_not_comparable` for `POST /compare`**, a 409 as the issue that
  added it asked: `pipeline_invalid` would tell the client a document is
  wrong, and `request_invalid` that the body is — here the request is
  well-formed and the runs it names cannot stand side by side, and the
  client's action is to pick other runs.
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
run record does not hold: per-query scores, per-node ranking metrics, which
passages are gold, each query's text, and passage text. **All of it is derived data: computed here on read, cached
under the workspace's `cache/`, and never written into the run** — the store
writes nothing derived (its rule), and `cache/` is reconstructible data that
is never a truth (the design document § 6). Deleting `cache/` changes no
response; `tests/replay.rs` deletes it and compares.

### What a figure is

A reading of the trace against the run's own ground truth, with
`ragondin-metrics` and the rules the harness scores by, called rather than
restated (`derived.rs`):

- **The metrics are the ones the run recorded whose name
  `ragondin-metrics`' catalogue knows** (`Metric::parse`): `ndcg@<k>`,
  `recall@<k>`, `mrr` (uncut, as the harness records it), `exact_match`,
  `token_f1`, each at the cutoff its own name states and read by the family
  the catalogue gives it. The harness writes its names through the same
  catalogue, so the name written and the name read have one spelling. Other
  names — a latency percentile — are not per-query figures and are not
  listed here; `GET /runs` lists their stored values as family `unknown`.
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
- **Which passages are gold**, on the trace: each passage — in a ranking or
  a context — carries `grade`, its document's grade in the query's qrels,
  `0` when they do not judge it (the closed world every metric reads them
  under); and each node that produced a ranking carries `gold_ranks`, the
  1-based ranks of the documents graded above 0 in its ranking, folded by
  `documents_by_first_occurrence` — the ranking its metrics score, so the
  ranks are the ones a metric sees, and two chunks of one document take
  one rank. Empty when the node ranked no gold document; `null` for a node
  with no ranking. Read from the qrels by `derived::grade` and
  `derived::gold_ranks`, beside the gold filter, which shares their rule of
  what is gold. `tests/per_node_metrics.rs` compares `gold_ranks` with the
  fold, and each node's `mrr` with its first gold rank, over every ranking
  node of the harness-recorded run; that run has one chunk per document, so
  `tests/replay.rs` pins the fold itself, with a gold document after a
  collapsed duplicate.
  **A query without qrels has no grade and no gold ranks**, `null`, a
  choice made here: it is unjudged, as its absent scores say, and a `0`
  would read as judged not relevant.
- **Each query's text** is the dataset's: `text` on each query of the
  listing and on the trace's header, for Replay's query selector.
- **Node rows carry ranking metrics only.** The generator's EM and token-F1
  are a run-level figure: `metrics.json`'s, or the mean of the per-query
  scores, never a node row.
- **The gold filter**: `?missing_gold_at=<k>` keeps the judged queries none of
  whose documents graded above 0 is in the top `k` of the output ranking.
  It is the only parameter the listing takes, and the trace takes none; a
  name and a value are percent-decoded, and any other parameter, the same one
  twice, or a value that is not a positive integer is `parameter_invalid`.
  It is declared as an optional `in: query` parameter in `api/v1.json`, from
  `RunQueriesParameters`, so the UI's generated client sends it.
- **No pagination.** The listing answers every query of the run at once —
  SQuAD's thousand in one body. Paging it is a change to this endpoint when a
  screen needs it.
- **Durations**: a query's is its latency, `Trace::latency_nanos` — the sum
  of its nodes' `duration_nanos`, each component's own time, in checked
  addition: `null` for a sum past `u64`, which only a malformed trace
  reaches. A latency percentile a run's `metrics.json` holds is served as
  stored and not recomputed.

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
`dataset_version` alone, and so are the passages' `grade`, the nodes'
`gold_ranks` and each query's `text`, which read the qrels and the queries
and no chunk (a choice made here: ADR-C36 § 4 conditions a passage's
*text* on the chunk set, and a grade or a query's text is not derived from
it), and `ground_truth` compares no chunk set
(`found.index_version` is `null`). The passages are gated on both: a dataset
that verifies with a chunk set that does not is `index_differs`, no text,
both chunk-set digests side by side, and the scores still read. A dataset
that does not load is `dataset_unreadable`, with the adapter's error in
`detail`. With anything but `verified`, the queries are listed with their
durations and no scores or text, the nodes with no metrics and no gold
ranks, and the passages with no grade — `null`, never guessed, the reason
in `ground_truth` or `passages`.

**The trigger ADR-C36 § 4 records.** This resolution holds while chunks are
derived outside the pipeline, by `CorpusIndex`. **The day a chunker component
produces chunks inside a pipeline, a chunk's text can no longer be derived
without executing that component**, which INV-12 forbids this crate; what
replay shows for such chunks is then a new decision, to be met deliberately
rather than discovered.

### The cache: a choice made here

`cache/<run_id>/derived.json`, one JSON file per run, written only once the
dataset has verified: the per-query scores and the per-node metrics. Beside
it, `cache/<run_id>/latency.json` holds the run's median query latency for
`GET /runs`, written whether or not the dataset is on disk, since it reads
the traces alone. Each is keyed on everything its figures were computed
from — the file format, the build, the
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
  computed anyway and served, and the reason is reported in the response's
  `cache_error` — `RunQueries`', or `RunListing`'s for the latency — so a
  read-only workspace stays usable and a broken cache is never silent.
- **What it saves, and what it does not.** `latency.json` saves only the
  parse of each of a run's traces into a `Trace`, and its sum. It saves no
  load: every `GET /runs` still loads every run, traces included, from the
  store. And it costs a digest of every trace on every request, hit or miss:
  `Key::of` serializes each trace document and hashes it to build the key
  the file is checked against, so a hit may cost more than computing the
  sums directly. That trade-off is not measured yet. For `derived.json`:
  the cache saves every per-query
  and per-node figure of the listing; it saves no load. The trace endpoint
  reads nothing from it: it reads the chunk set to resolve text, and one
  query's figures cost nothing. **The load is the registry's to save**: every
  request to either endpoint, and `POST /compare`, asks `Registry::dataset`
  for a verdict — it cannot be trusted otherwise, and the qrels and the
  passage text come from the dataset — and the file backend answers from the
  dataset it keeps loaded while the files are unchanged, chunk set included
  (§ *The loaded datasets*).

**Per-node metrics are served by `GET /runs/{id}/queries`**, beside the
per-query scores they are computed with, and not by `GET /runs/{id}` as the
design document § 5's table once listed them (its row now points here): the
detail endpoint reads the store alone, and putting the metrics there would
make every run's detail load and digest its dataset.

## Compare

`POST /compare` compares runs of one benchmark against a baseline (the
design document § 3, § 5). Its options travel in the JSON body —
`{run_ids, baseline, pairing?}`, read as every body is, refusing a field it
does not read — and the query string takes no parameter, `NoParameters`
(ADR-C37 § 4: a list travels in a JSON body). Two to five run ids, each once, the baseline among them, or
`request_invalid`; the response lists the baseline first, then the others
in the order given.

### The table and the matrix

The metric table — every metric any run recorded, each run's value, the best
of each row by the direction `ragondin-metrics`' catalogue gives it (none for
a name the catalogue does not know: its `direction` is `null` and its `best`
empty, its deltas reported all the same), each run's delta to the baseline —
and the parameter matrix — every parameter not identical across
the runs — are `ragondin-experiments`' `compare_runs`, converted in
`convert.rs`: the computation `ragondin compare` prints for two runs, so the
two cannot drift. Runs of different `dataset_version`s are refused there, and
answered `runs_not_comparable` naming both. **The ceiling is here**: more than
five run ids is `runs_not_comparable` naming it, before any run is loaded —
the design system has four run inks, and a sixth run is refused rather than
given an invented colour (ADR-016).

### The stages

`stages.rs` derives each run's stages from its lowered graph — nothing but
the nodes' kinds and positions, read through `ragondin-pipeline`'s public
surface, so no notion is added to the core (INV-1):

- **retrieval legs** — every retriever node;
- **after fusion** — the fusion node;
- **after rerank** — the reranker node;
- **final ranking** — the node the run's retrieval metrics were read at:
  ADR-C30 § 3's walk, `ragondin_experiments::ranking_node`, through
  `derived::Outputs`, the node `GET /runs/{id}/queries` names as
  `ranking_node`. Every pipeline has one;
- **answer** — the terminal node, when it produces an answer.

A row is kept for each stage at least one run has, in that order; a run
without the stage has an `absent` cell — "no stage here", never a zero. A
present cell lists its nodes, each with its ranking metrics, and per metric
the best of them with its node — for the legs, the best leg.

Choices made here (`AGENTS.md` § Rules of engagement):

- **Every retriever is a leg.** The narrower rule — a retriever that feeds
  a fusion or is the ranking output — agrees with it on the dense-only,
  hybrid and reranked pipelines, and on a retriever feeding a reranker
  directly gives the leg no stage at all, where this gives it its own.
- **An ambiguous graph is read by position, and says so.** With two
  fusions or two rerankers, the one furthest from the inputs — the longest
  path to it — is the stage, ties by id; that, or a reranker upstream of the
  fusion, marks the derivation a guess, and the after-fusion and after-rerank
  rows answer `confidence: low`, so the UI offers the manual pairing.
- **A stage's metrics are the per-node figures `GET /runs/{id}/queries`
  serves**, from the same function, `handlers::figures`, and the same cache
  entry: a stage figure is that endpoint's node row, and the averaging rules
  stay `derived.rs`'s alone. The dataset is resolved once per comparison —
  the runs share one `dataset_version` — and loaded as § *Derived data* says;
  without `verified`, no cell carries a metric and `ground_truth` says why.

### The manual pairing

When the automatic pairing is wrong — two retrievers against one — a person
pairs nodes by hand. The rows are the baseline's: a pair (the baseline's
node, the other run's node) moves the other run's node out of the stage its
kind gives it and into the stage of the baseline's node, and that row then
answers `source: manual`, under the pair's label when it has one. Only the
stages a kind decides take part — legs, after fusion, after rerank; the final
ranking and the answer are the walk's, and a pair never moves them.

- **A run is matched to its workspace pipeline by content — interim
  behaviour.** A run names no pipeline. ADR-C39 decides run → pipeline
  identity; until the code follows it, a run's pipeline is the one document
  under `pipelines/` whose canonical hash is the run's (INV-8: the
  canonical form, never the text), reported as each run's `pipeline`. No
  document, or several, is `null`, and such a run pairs automatically only.
  So a document edited since a run no longer names that run, and its pairing
  reaches the runs of what the file holds now. The index is `lineage.rs`,
  `pub(crate)` for the pipeline matrix to read too.
- **Pairings apply between the baseline's pipeline and each other run's**,
  read with `PipelineSource::read_pairing` and listed, oriented from the
  baseline's, in the response's `pairings`. A pairing between two runs
  neither of which is the baseline is not applied — the rows are the
  baseline's — and the body may not keep one (`request_invalid`): a pairing
  kept through a comparison is one that comparison shows.
- **The file**, a choice made here: `pipelines/<pipeline>.pairing/<other>.json`,
  `{"version": 1, "pipeline": "<pipeline>", "other": "<other>", "pairs":
  [{"node": "<in pipeline>", "other": "<in other>", "label": "<optional>"}]}`,
  written and read through types of its own, so a change to the API's
  `Pairing` cannot change it unseen. Both names are inside, so a file renamed
  by hand is detected: names that are not the two its path gives, another
  version, or a file that is not this JSON are `backend_failed`, naming the
  file — reported, never repaired; a reset removes it. It is UI metadata like
  a layout, never in a hash (INV-8).
- **Read in both directions, kept in one.** A read for (B, A) finds the
  file kept for (A, B) and turns each pair around. A write is whole — beside
  the file, renamed over it — and removes the file kept the other way round,
  so one pair of pipelines has one pairing.
- **Kept through `POST /compare`**, as the design document § 5 gives the
  body a `pairing`: the body's pairing replaces the one kept for its two
  pipelines, and one with no pairs removes it — "Reset to automatic". It is
  checked early, writing nothing — two different pipelines, the baseline's
  and another compared run's, both in the workspace (`pipeline_not_found`),
  every node a retriever, fusion or reranker of its pipeline's current
  document and none paired twice (`request_invalid`) — and applied to the
  comparison in place of what disk holds. It is written **last**, once the
  response is built and every step that can refuse has run, so a refused
  request — an unreadable run, runs not comparable — changes nothing on disk.

### The per-query deltas and their bins

For each run other than the baseline, and each ranking metric both recorded,
a query's delta is its score in the run minus its score in the baseline —
the per-query scores of `GET /runs/{id}/queries`, so a query judged on
nothing, or without a ranking at either output, has none, and
`judged_queries` counts those that do. Seven bins partition the queries
with a delta — `much_worse`, `worse`, `slightly_worse`, `unchanged`,
`slightly_better`, `better`, `much_better` — each with its bounds and its
queries.

**The bin edges are the product's**, fixed by
`docs/design/2026-09-29-front-end-design.md` § 3. UX and design: the delta's
sign, and its absolute magnitude against 0.1 and 0.3, applied to every
ranking metric alike; **a bound belongs to the bin nearer zero** (−0.3 is
`worse`, 0.1 `slightly_better`), so the bins are symmetric; and
**`unchanged` is a delta of exactly zero.** A per-query score is a
deterministic reading of a stored trace, so a ranking left as it was gives
exactly the same score, and a tolerance would be a threshold nobody chose.
Moving the edges is a change to that design document first, then to
`comparison::BINS` and `bin_of`.

### The latency

Each run's nodes that ran, in the canonical order, with the median of their
`duration_nanos` over the queries — `ragondin_experiments::lower_median`, the
lower of the two middle values over an even count, a duration that occurred,
the one median of durations `GET /runs`' median query latency is also taken
by — and how many queries that is. This is one node's duration, not a
query's latency. A latency percentile a run's `metrics.json` holds is in the
table as stored, with no direction, and is not recomputed.

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
`hyper` in `Cargo.lock`, which no core crate reaches; and `query`, appended
by ADR-C37 § 1 for `ApiQuery`, the one grant that decision makes). `query`
brings no crate: `serde_urlencoded` 0.7.1 was already in the closure through
`reqwest`, so `Cargo.lock` lists the same packages at the same versions, and
gains only the `axum → serde_urlencoded` edge in `axum`'s dependency list.
Then `schemars`, and `tokio`, whose workspace entry now names `net`, `sync` and `time`. `tower`, the
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
`ragondin-benchmarks`. `sha2` is a normal dependency, for a pipeline
document's etag, and the tests use it for the digests of what a local server
serves. `serde_yaml` is not a dependency: a pipeline document is parsed by
`ragondin-config`. None is a new `[workspace.dependencies]` entry, and none
has a feature appended. No
TOML crate is a dependency (§ `workspace.toml`, and why it is read by hand).
