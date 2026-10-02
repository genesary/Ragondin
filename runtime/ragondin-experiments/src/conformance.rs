//! The conformance suite every [`RunStore`] backend passes, behind this
//! crate's `conformance` feature so that a production build does not carry
//! it.
//!
//! The model is `ragondin-conformance`'s for components: one entry point, called
//! from the backend's own tests, that checks the contract [`RunStore`]'s
//! documentation states — never how a backend meets it. It is what makes
//! "another deployment is another backend" checkable rather than promised:
//! [`FileSystemRunStore`](crate::FileSystemRunStore) passes it in this crate's
//! tests, and the next backend passes the same one.
//!
//! ```ignore
//! assert_run_store_conformance(
//!     || MyStore::empty(),                     // a fresh, empty store per case
//!     |store, id| store.corrupt_for_test(id),  // leave the run under `id` torn
//!     || MyStore::holding_a_run_from_v0(),     // a store, and a run it kept before provenance
//! );
//! ```
//!
//! # What it checks
//!
//! Each case gets a store of its own from `fresh`, so no case's writes decide
//! another's outcome, and a backend with external state makes one per case:
//!
//! 1. **Round trip.** A run with every field filled — metrics, model hashes,
//!    bindings, and two traces, one of them a document that is *not* a valid
//!    [`Trace`](crate::Trace) — reads back equal to the run written. That
//!    second trace is the check that `save` and `load` move a trace without
//!    parsing it (ADR-C28). A second `save` of the same run succeeds and
//!    changes nothing, and a `save` of a *different* run under the same id
//!    succeeds and leaves the first one stored.
//! 2. **Unknown id.** `load` of an id never saved is
//!    [`NotFound`](RunStoreError::NotFound), naming the id.
//! 3. **Listing.** A fresh store lists nothing; after several saves, a repeat
//!    among them, `ids` lists every run exactly once, in ascending order of
//!    its hex rendering.
//! 4. **Non-finite metric.** A run with a `NaN` metric is refused as
//!    [`NotFinite`](RunStoreError::NotFinite), naming the metric, and nothing
//!    is stored: the id neither lists nor loads.
//! 5. **Incomplete run.** A run `tear` has damaged is reported
//!    [`Incomplete`](RunStoreError::Incomplete) by `load` and by `save`,
//!    never repaired, and is still listed by `ids`.
//! 6. **Times.** A run's [`RunTimes`] round-trip when known; a run saved with
//!    none reads back with none, so a backend never invents a time; a rerun
//!    under a stored id keeps the first record's times, as it keeps the rest
//!    of the first record; and a `finished` earlier than `started` reads back
//!    as written.
//! 7. **Provenance.** A run's [`RunProvenance`] round-trips, with a name
//!    alone and with a name and a [`PrefixOf`]; a run saved with none reads
//!    back with none, so a backend never invents a record, not even an empty
//!    one; a rerun under a stored id keeps the first record's; and a run the
//!    backend stored before the record existed reads back with none.
//!
//! What the suite cannot check is that a backend ignores a field of the record
//! it does not know: the trait gives no way to put such bytes into a store.
//! That obligation is every backend's all the same, and each proves it in its
//! own tests — the file backend in `tests/run_store.rs`,
//! `an_unknown_provenance_field_is_ignored`.
//!
//! # Why three closures
//!
//! `fresh` builds an empty store. `tear` damages the stored run under an id
//! so that it is no longer whole — for the file backend, deleting one of its
//! files. The trait deliberately offers no way to damage a run, and a torn run
//! is a state every backend can reach (a crash, a hand, a partial copy), so
//! how to reach it is the backend's to supply: the suite checks that it is
//! *reported*, which is the part the contract owns.
//!
//! `before` returns a store holding a run as the backend kept it before the
//! launch record existed, and that run's id — for the file backend, the
//! committed fixture directory, which has no `provenance.json`. Saving a run
//! with no record through today's `save` is not the same thing: that is the
//! "none round-trips" case, and it cannot show that a record written by an
//! older build of the backend still reads. How an older build laid a run out
//! is the backend's to know, so it supplies one.
//!
//! A failed check panics with the case it belongs to, as a test assertion
//! does.

use std::collections::BTreeMap;

use ragondin_pipeline::PipelineHash;
use ragondin_types::QueryId;
use serde_json::json;

use crate::run::{
    ConfigDocument, PrefixOf, Run, RunBinding, RunId, RunInputs, RunProvenance, RunTimes,
    TraceDocument, UnixMillis,
};
use crate::store::{RunStore, RunStoreError};

/// Runs every case against stores built by `fresh`, damaging a stored run
/// with `tear` where a case needs a torn one, and reading the older run
/// `before` gives; panics on the first failure.
///
/// `fresh` must return a new, empty store on every call. `tear(store, id)` is
/// called once, on a store holding a complete run under `id`, and must leave
/// that run incomplete. `before()` is called once, and returns a store and
/// the id of a complete run in it that the backend stored before the launch
/// record existed.
pub fn assert_run_store_conformance<S, F, T, B>(mut fresh: F, mut tear: T, mut before: B)
where
    S: RunStore,
    F: FnMut() -> S,
    T: FnMut(&S, &RunId),
    B: FnMut() -> (S, RunId),
{
    round_trip(&fresh());
    unknown_id(&fresh());
    listing(&fresh());
    non_finite_metric(&fresh());
    incomplete_run(&fresh(), &mut tear);
    times_round_trip(&fresh());
    no_times_round_trip_as_none(&fresh());
    a_rerun_keeps_the_first_record_s_times(&fresh());
    finished_before_started_reads_back_as_written(&fresh());
    provenance_round_trips(&fresh());
    a_prefix_record_round_trips(&fresh());
    no_provenance_round_trips_as_none(&fresh());
    a_rerun_keeps_the_first_record_s_provenance(&fresh());
    let (store, id) = before();
    the_fixture_without_provenance_reads_none(&store, &id);
}

fn run_id(byte: u8) -> RunId {
    RunId::from_digest([byte; 32])
}

/// A run with every field filled, and traces the typed shape both accepts and
/// refuses — the store must not care which.
fn a_run(id: RunId) -> Run {
    let valid = json!({"nodes": [{
        "node": "sparse",
        "inputs": [{"query": {"id": "q-1"}}],
        "output": {"chunks": {"count": 1, "ranked": [
            {"chunk": "c-1", "document": "doc-a", "score": 0.5},
        ]}},
        "duration_nanos": 1_500u64,
        "error": null,
    }]});
    let not_a_trace = json!({"nodes": [{"node": "sparse", "duration_ms": 3}], "extra": [1, 2]});

    Run {
        id,
        inputs: RunInputs {
            pipeline: PipelineHash::from_digest([0xbe; 32]),
            dataset_version: "dataset@1".to_owned(),
            index_version: "index@1".to_owned(),
            model_hashes: BTreeMap::from([
                ("embedder".to_owned(), "sha256:2b1f".to_owned()),
                ("generator".to_owned(), "sha256:9c0d".to_owned()),
            ]),
            engine_version: "0.0.0".to_owned(),
        },
        metrics: [("ndcg@10", 0.42), ("recall@10", 0.75), ("mrr", 0.0)]
            .into_iter()
            .collect(),
        config: ConfigDocument::new("schema_version: 1\nnodes: []\n"),
        traces: BTreeMap::from([
            (QueryId::new("q-1"), TraceDocument::new(valid)),
            (QueryId::new("q-2"), TraceDocument::new(not_a_trace)),
        ]),
        bindings: vec![RunBinding {
            family: "generator".to_owned(),
            name: "vllm".to_owned(),
            uri: "http://localhost:8000".to_owned(),
        }],
        times: None,
        provenance: None,
    }
}

fn times(started: u64, finished: u64) -> RunTimes {
    RunTimes::new(UnixMillis::new(started), UnixMillis::new(finished))
}

/// Saves `run` and reads it back, panicking with `case` on any failure.
fn save_and_load(store: &impl RunStore, run: &Run, case: &str) -> Run {
    store
        .save(run)
        .unwrap_or_else(|error| panic!("{case}: `save` failed: {error}"));
    store
        .load(&run.id)
        .unwrap_or_else(|error| panic!("{case}: `load` failed: {error}"))
}

fn times_round_trip(store: &impl RunStore) {
    let mut run = a_run(run_id(0x55));
    run.times = Some(times(1_700_000_000_000, 1_700_000_004_250));
    let read = save_and_load(store, &run, "times round trip");
    assert_eq!(
        read.times, run.times,
        "times round trip: the times read back are the times written"
    );
    assert_eq!(read, run, "times round trip: and so is the rest of the run");
}

fn no_times_round_trip_as_none(store: &impl RunStore) {
    let run = a_run(run_id(0x66));
    let read = save_and_load(store, &run, "no times round trip");
    assert_eq!(
        read.times, None,
        "no times round trip: a run saved with unknown times reads back unknown — \
         a backend never invents a time"
    );
}

fn a_rerun_keeps_the_first_record_s_times(store: &impl RunStore) {
    let mut first = a_run(run_id(0x77));
    first.times = Some(times(1_000, 2_000));
    save_and_load(store, &first, "rerun keeps the first times");

    let mut rerun = a_run(first.id);
    rerun.times = Some(times(5_000, 9_000));
    let kept = save_and_load(store, &rerun, "rerun keeps the first times");
    assert_eq!(
        kept.times, first.times,
        "rerun keeps the first times: the first record wins, times included"
    );

    // Nor does a rerun with no times erase the ones stored.
    let mut unknown = a_run(first.id);
    unknown.times = None;
    let kept = save_and_load(store, &unknown, "rerun keeps the first times");
    assert_eq!(kept.times, first.times, "rerun keeps the first times");
}

fn finished_before_started_reads_back_as_written(store: &impl RunStore) {
    let mut run = a_run(run_id(0x88));
    run.times = Some(times(9_000, 1_000));
    let read = save_and_load(store, &run, "finished before started");
    assert_eq!(
        read.times, run.times,
        "finished before started: stored and read back as written, neither \
         refused nor reordered"
    );
}

fn provenance_round_trips(store: &impl RunStore) {
    let mut run = a_run(run_id(0x99));
    run.provenance = Some(RunProvenance::named("hybrid"));
    let read = save_and_load(store, &run, "provenance round trip");
    assert_eq!(
        read.provenance, run.provenance,
        "provenance round trip: the record read back is the record written"
    );
    assert_eq!(
        read, run,
        "provenance round trip: and so is the rest of the run"
    );
}

fn a_prefix_record_round_trips(store: &impl RunStore) {
    let mut run = a_run(run_id(0xaa));
    run.provenance = Some(RunProvenance::prefix(
        "hybrid",
        PrefixOf::new("fused", PipelineHash::from_digest([0x5e; 32])),
    ));
    let read = save_and_load(store, &run, "prefix record round trip");
    assert_eq!(
        read.provenance, run.provenance,
        "prefix record round trip: the name, the cut and the parent's hash read back as written"
    );
}

fn no_provenance_round_trips_as_none(store: &impl RunStore) {
    let run = a_run(run_id(0xbb));
    let read = save_and_load(store, &run, "no provenance round trip");
    assert_eq!(
        read.provenance, None,
        "no provenance round trip: a run saved with no record reads back with none — \
         a backend never invents one, not even an empty one"
    );
}

fn a_rerun_keeps_the_first_record_s_provenance(store: &impl RunStore) {
    let mut first = a_run(run_id(0xcc));
    first.provenance = Some(RunProvenance::named("hybrid"));
    save_and_load(store, &first, "rerun keeps the first provenance");

    let mut rerun = a_run(first.id);
    rerun.provenance = Some(RunProvenance::named("forked"));
    let kept = save_and_load(store, &rerun, "rerun keeps the first provenance");
    assert_eq!(
        kept.provenance, first.provenance,
        "rerun keeps the first provenance: the first record wins (ADR-C39 § 1)"
    );

    // Nor does a rerun with no record erase the one stored.
    let mut unknown = a_run(first.id);
    unknown.provenance = None;
    let kept = save_and_load(store, &unknown, "rerun keeps the first provenance");
    assert_eq!(
        kept.provenance, first.provenance,
        "rerun keeps the first provenance"
    );
}

fn the_fixture_without_provenance_reads_none(store: &impl RunStore, id: &RunId) {
    let run = store
        .load(id)
        .unwrap_or_else(|error| panic!("a run stored before provenance: `load` failed: {error}"));
    assert_eq!(
        run.provenance, None,
        "a run stored before provenance: it reads back with none, and is complete"
    );
}

fn round_trip(store: &impl RunStore) {
    let run = a_run(run_id(0x11));
    store
        .save(&run)
        .unwrap_or_else(|error| panic!("round trip: `save` failed: {error}"));
    let read = store
        .load(&run.id)
        .unwrap_or_else(|error| panic!("round trip: `load` failed: {error}"));
    assert_eq!(
        read, run,
        "round trip: a run read back is the run written, every trace as it was"
    );

    store
        .save(&run)
        .unwrap_or_else(|error| panic!("round trip: saving a stored run again failed: {error}"));
    let again = store
        .load(&run.id)
        .unwrap_or_else(|error| panic!("round trip: `load` after a second save: {error}"));
    assert_eq!(again, run, "round trip: a second save changes nothing");

    // A different record under the same id — a rerun whose judge scored
    // differently, say — is not written over the first: the id names the
    // run, and the store keeps what it had.
    let mut rerun = a_run(run.id);
    rerun.metrics.insert("ndcg@10", 0.99);
    rerun.bindings.clear();
    store.save(&rerun).unwrap_or_else(|error| {
        panic!("round trip: saving a rerun under a stored id failed: {error}")
    });
    let kept = store
        .load(&run.id)
        .unwrap_or_else(|error| panic!("round trip: `load` after saving a rerun: {error}"));
    assert_eq!(
        kept, run,
        "round trip: a run already stored under the id is left as it is"
    );
}

fn unknown_id(store: &impl RunStore) {
    let absent = run_id(0x22);
    match store.load(&absent) {
        Err(RunStoreError::NotFound { id }) => {
            assert_eq!(id, absent, "unknown id: `NotFound` names the id asked for")
        }
        other => panic!("unknown id: `load` must report `NotFound`, got {other:?}"),
    }
}

fn listing(store: &impl RunStore) {
    let ids = store
        .ids()
        .unwrap_or_else(|error| panic!("listing: a fresh store failed to list: {error}"));
    assert!(ids.is_empty(), "listing: a fresh store lists {ids:?}");

    // Saved out of order, and one of them twice.
    let saved = [run_id(0xc3), run_id(0x0a), run_id(0x7f), run_id(0x0a)];
    for id in saved {
        store
            .save(&a_run(id))
            .unwrap_or_else(|error| panic!("listing: `save` failed: {error}"));
    }

    let ids = store
        .ids()
        .unwrap_or_else(|error| panic!("listing: `ids` failed: {error}"));
    assert_eq!(
        ids,
        vec![run_id(0x0a), run_id(0x7f), run_id(0xc3)],
        "listing: every stored run once, in ascending order of its hex rendering"
    );
}

fn non_finite_metric(store: &impl RunStore) {
    let mut run = a_run(run_id(0x33));
    run.metrics.insert("mrr", f64::NAN);

    match store.save(&run) {
        Err(RunStoreError::NotFinite { metric }) => assert_eq!(
            metric, "mrr",
            "non-finite metric: `NotFinite` names the metric"
        ),
        other => panic!("non-finite metric: `save` must refuse it, got {other:?}"),
    }
    let ids = store
        .ids()
        .unwrap_or_else(|error| panic!("non-finite metric: `ids` failed: {error}"));
    assert!(
        ids.is_empty(),
        "non-finite metric: a refused run is not stored, yet the store lists {ids:?}"
    );
    assert!(
        matches!(store.load(&run.id), Err(RunStoreError::NotFound { .. })),
        "non-finite metric: a refused run does not load"
    );
}

fn incomplete_run<S: RunStore>(store: &S, tear: &mut impl FnMut(&S, &RunId)) {
    let run = a_run(run_id(0x44));
    store
        .save(&run)
        .unwrap_or_else(|error| panic!("incomplete run: `save` failed: {error}"));
    tear(store, &run.id);

    let ids = store
        .ids()
        .unwrap_or_else(|error| panic!("incomplete run: `ids` failed: {error}"));
    assert!(
        ids.contains(&run.id),
        "incomplete run: a torn run is still listed — `load` is where it is reported — got {ids:?}"
    );

    assert!(
        matches!(store.load(&run.id), Err(RunStoreError::Incomplete { .. })),
        "incomplete run: `load` must report a torn run `Incomplete`, got {:?}",
        store.load(&run.id)
    );
    assert!(
        matches!(store.save(&run), Err(RunStoreError::Incomplete { .. })),
        "incomplete run: `save` over a torn run must report it `Incomplete`, not repair it"
    );
    assert!(
        matches!(store.load(&run.id), Err(RunStoreError::Incomplete { .. })),
        "incomplete run: still torn after the refused `save` — nothing was repaired"
    );
}
