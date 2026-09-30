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
//!
//! # Why two closures
//!
//! `fresh` builds an empty store. `tear` damages the stored run under an id
//! so that it is no longer whole — for the file backend, deleting one of its
//! files. The trait deliberately offers no way to damage a run, and a torn run
//! is a state every backend can reach (a crash, a hand, a partial copy), so
//! how to reach it is the backend's to supply: the suite checks that it is
//! *reported*, which is the part the contract owns.
//!
//! A failed check panics with the case it belongs to, as a test assertion
//! does.

use std::collections::BTreeMap;

use ragondin_pipeline::PipelineHash;
use ragondin_types::QueryId;
use serde_json::json;

use crate::run::{ConfigDocument, Run, RunBinding, RunId, RunInputs, TraceDocument};
use crate::store::{RunStore, RunStoreError};

/// Runs every case against stores built by `fresh`, damaging a stored run
/// with `tear` where a case needs a torn one; panics on the first failure.
///
/// `fresh` must return a new, empty store on every call. `tear(store, id)` is
/// called once, on a store holding a complete run under `id`, and must leave
/// that run incomplete.
pub fn assert_run_store_conformance<S, F, T>(mut fresh: F, mut tear: T)
where
    S: RunStore,
    F: FnMut() -> S,
    T: FnMut(&S, &RunId),
{
    round_trip(&fresh());
    unknown_id(&fresh());
    listing(&fresh());
    non_finite_metric(&fresh());
    incomplete_run(&fresh(), &mut tear);
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
    }
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
