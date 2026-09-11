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
| `FileSystemRunStore` | `save`, `load` by id, `compare` by two ids — one directory per run |
| `compare` | The metric-by-metric diff behind `ragondin compare` |

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
  the door.
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
  saves are ordinary. Nothing clears a staging directory on the way in, and
  nothing may: a unique name has nothing to clear, and clearing a shared one
  reaches into a directory another live writer owns. Whoever renames first wins,
  and the others find the run already there and report success.
- **A leading dot under the store root means "not a run".** A `.partial`
  directory is removed when its `save` returns, success or failure, but a crash
  inside one leaves it, nothing sweeps them, and they **accumulate** until
  someone deletes them. Anything that lists the root — a future `ragondin runs`,
  the comparison view — must skip entries whose name begins with `.`: a run id
  is 64 hex digits, so parsing one of these as an id fails rather than
  misreading, but only if the lister expects the convention.
- **A torn directory is reported, never repaired.** `save` checks that a
  destination already under the id holds all four files, and reports
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
- **The configuration is kept verbatim, and the traces are opaque.** The store
  writes the configuration document as it was handed in — the text whose
  canonical logical form hashes to the `pipeline` digest beside it — and never
  re-serializes one out of an in-memory pipeline type, which would put a second,
  drifting spelling of the configuration in the store. A trace is likewise the
  harness's rendering of `ragondin-engine`'s `ExecutionTrace` (INV-10), held as
  a JSON document: this crate does not depend on the engine, and the trace's
  shape belongs to the engine and moves with it.
- **Metrics are recorded here, never computed here.** `ragondin-metrics` scores
  one query and the harness averages over a query set; a `Metrics` value is the
  figure a comparison puts side by side. The names are not a fixed catalogue —
  quality, cost and latency all land in the same map (§6.5).
- **A metric JSON cannot write is refused on the way in.** `serde_json` writes a
  non-finite float as `null`, and `null` does not read back as an `f64`, so a
  run stored with one would be unreadable for good under an id whose existence
  says it is done. `save` refuses it (`RunStoreError::NotFinite`) before it
  creates anything. It is not the per-query metrics that produce one —
  `ragondin-metrics` guards each of its zero denominators, `ndcg_at_k` returning
  `0.0` when nothing is relevant — but an aggregate has denominators of its own:
  a mean over an empty query set, or a cost-per-query where the count is zero,
  is `0.0 / 0.0`.
