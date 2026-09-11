//! The native run store, over real directories on disk, and the comparison
//! the `ragondin compare` view is built from.
//!
//! Integration tests rather than unit tests because the subject is I/O: what
//! the store must get right is a run that round-trips through files, a layout
//! a person can read in a terminal, and a run id that names nothing.
//!
//! No `tempfile` dependency — dependency versions are declared only at the
//! workspace root, and `CARGO_TARGET_TMPDIR` is a directory Cargo already
//! hands to an integration test.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ragondin_experiments::{
    compare, ConfigDocument, FileSystemRunStore, Metrics, Run, RunId, RunIdParseError, RunInputs,
    RunStoreError, TraceDocument,
};
use ragondin_pipeline::PipelineHash;
use ragondin_types::QueryId;

/// A store root of this test's own, emptied first so that a previous run that
/// was killed before it finished cannot decide this one.
fn store(test_name: &str) -> FileSystemRunStore {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("run_store")
        .join(test_name);
    let _ = fs::remove_dir_all(&root);
    FileSystemRunStore::new(root)
}

/// A run id built from a repeated byte. `ragondin-harness` computes the real
/// one by hashing the identity tuple; this crate only stores by it, so a test
/// id need only be a digest-shaped value.
fn run_id(byte: u8) -> RunId {
    RunId::from_digest([byte; 32])
}

/// A pipeline's content hash, asserted rather than computed: this crate stores
/// the digest and never takes one, so a test needs a digest-shaped value and
/// not a pipeline. The harness gets its own from
/// `LogicalPipeline::content_hash`.
fn pipeline_hash() -> PipelineHash {
    PipelineHash::from_digest([0xbe; 32])
}

/// A run whose metrics are the ones given, and whose other fields are fixed —
/// so a comparison test varies exactly one thing.
fn a_run(id: RunId, metrics: &[(&str, f64)]) -> Run {
    let mut traces = BTreeMap::new();
    traces.insert(
        QueryId::new("q-1"),
        TraceDocument::new(serde_json::json!({
            "nodes": [{ "node": "sparse", "duration_ms": 3 }]
        })),
    );

    Run {
        id,
        inputs: RunInputs {
            pipeline: pipeline_hash(),
            dataset_version: "beir/scifact@2021-05-01".to_owned(),
            index_version: "bm25-ram@7".to_owned(),
            model_hashes: BTreeMap::from([("embedder".to_owned(), "sha256:2b1f".to_owned())]),
            engine_version: "0.0.0".to_owned(),
        },
        metrics: metrics.iter().copied().collect(),
        config: ConfigDocument::new("schema_version: 1\nnodes: []\n"),
        traces,
    }
}

#[test]
fn a_saved_run_reads_back_by_its_id() {
    let store = store("round_trip");
    let written = a_run(run_id(0x11), &[("ndcg@10", 0.42), ("recall@10", 0.75)]);

    store.save(&written).expect("the run must be writable");
    let read = store.load(&written.id).expect("the run must read back");

    // Every field but `id` is a real assertion: the id is the directory name
    // the run was looked up by, so `load` puts it back from the argument and
    // it cannot disagree. What round-trips here is the rest.
    assert_eq!(read, written, "a run read back is the run written");
}

#[test]
fn the_run_directory_is_named_by_the_run_id_and_is_readable_by_a_person() {
    let store = store("layout");
    let run = a_run(run_id(0x22), &[("ndcg@10", 0.42)]);
    store.save(&run).expect("the run must be writable");

    // The directory name is the id, which is what makes the store inspectable
    // from a shell and what lets `load` find a run without an index.
    let dir: PathBuf = store.root().join(run.id.to_string());
    assert!(dir.is_dir(), "one directory per run id: {}", dir.display());

    let config = fs::read_to_string(dir.join("config.yaml")).expect("the config must be kept");
    assert_eq!(
        config, "schema_version: 1\nnodes: []\n",
        "the configuration document is kept verbatim, never re-serialized"
    );

    assert!(
        dir.join("traces.json").is_file(),
        "the per-query traces are part of the run"
    );
    assert!(
        dir.join("inputs.json").is_file(),
        "the identity tuple's components are part of the run"
    );
}

#[test]
fn loading_a_run_the_store_does_not_hold_reports_it_missing() {
    let store = store("missing");
    let absent = run_id(0x33);

    match store.load(&absent) {
        Err(RunStoreError::NotFound { id }) => assert_eq!(id, absent),
        other => panic!("an unknown run id must report NotFound, got {other:?}"),
    }
}

#[test]
fn two_runs_are_compared_metric_by_metric() {
    let left = a_run(run_id(0x44), &[("ndcg@10", 0.42), ("recall@10", 0.75)]);
    let right = a_run(run_id(0x55), &[("ndcg@10", 0.50), ("recall@10", 0.75)]);

    let comparison = compare(&left, &right);

    assert_eq!(comparison.left, left.id);
    assert_eq!(comparison.right, right.id);
    assert!(
        !comparison.is_identical(),
        "runs whose metrics differ are not identical"
    );

    let names: Vec<&str> = comparison.metrics.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["ndcg@10", "recall@10"],
        "every metric of either run is reported, in a fixed order"
    );

    let ndcg = &comparison.metrics[0];
    assert_eq!((ndcg.left, ndcg.right), (Some(0.42), Some(0.50)));
    assert_eq!(
        ndcg.delta().expect("both runs recorded ndcg@10"),
        0.50 - 0.42,
        "the delta reads right minus left"
    );

    let differing: Vec<&str> = comparison.differences().map(|m| m.name.as_str()).collect();
    assert_eq!(
        differing,
        vec!["ndcg@10"],
        "a metric both runs agree on is not a difference"
    );
}

#[test]
fn two_runs_with_the_same_metrics_compare_identical() {
    let left = a_run(run_id(0x66), &[("ndcg@10", 0.42), ("recall@10", 0.75)]);
    let right = a_run(run_id(0x77), &[("ndcg@10", 0.42), ("recall@10", 0.75)]);

    let comparison = compare(&left, &right);

    assert!(
        comparison.is_identical(),
        "two runs recording the same metrics are identical: {comparison:?}"
    );
    assert_eq!(comparison.differences().count(), 0);
}

#[test]
fn a_metric_only_one_run_recorded_is_reported_on_its_own_side() {
    let left = a_run(run_id(0x88), &[("ndcg@10", 0.42)]);
    let right = a_run(run_id(0x99), &[("ndcg@10", 0.42), ("latency_p50_ms", 31.0)]);

    let comparison = compare(&left, &right);

    // Name order, not the order the two runs happened to record them in —
    // these two fixtures disagree about it, which is the point of the pair.
    // This is the order `ragondin compare` renders its rows from.
    let names: Vec<&str> = comparison.metrics.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["latency_p50_ms", "ndcg@10"]);

    let latency = comparison
        .metrics
        .iter()
        .find(|m| m.name == "latency_p50_ms")
        .expect("a metric recorded by one run alone is still reported");
    assert_eq!((latency.left, latency.right), (None, Some(31.0)));
    assert_eq!(
        latency.delta(),
        None,
        "there is no delta against a metric the other run did not record"
    );
    assert!(
        !comparison.is_identical(),
        "a run recording a metric the other does not is not identical to it"
    );
}

#[test]
fn the_store_compares_two_runs_by_their_ids() {
    let store = store("compare_by_id");
    let left = a_run(run_id(0xaa), &[("ndcg@10", 0.42)]);
    let right = a_run(run_id(0xbb), &[("ndcg@10", 0.50)]);
    store.save(&left).expect("the left run must be writable");
    store.save(&right).expect("the right run must be writable");

    let comparison = store
        .compare(&left.id, &right.id)
        .expect("both runs are in the store");

    assert_eq!(comparison.differences().count(), 1);
}

#[test]
fn two_runs_with_no_metrics_at_all_are_vacuously_identical() {
    let comparison = compare(&a_run(run_id(0xcc), &[]), &a_run(run_id(0xdd), &[]));

    assert!(comparison.metrics.is_empty());
    assert!(
        comparison.is_identical(),
        "two runs that scored nothing disagree about nothing"
    );
}

#[test]
fn a_run_id_round_trips_through_its_hex_form() {
    let id = run_id(0x0f);
    let hex = id.to_string();

    assert_eq!(hex.len(), 64, "a run id renders as 64 hex digits: {hex}");
    assert_eq!(hex.parse::<RunId>().expect("its own rendering parses"), id);

    // One digest, one spelling. The uppercase form of a valid id is the near
    // miss that matters: it names the same 32 bytes, and accepting it would
    // put one run in the store under two names.
    assert_eq!(
        hex.to_uppercase().parse::<RunId>(),
        Err(RunIdParseError::NotHex),
        "uppercase is refused rather than folded"
    );
    assert_eq!(
        "abc".parse::<RunId>(),
        Err(RunIdParseError::Length { bytes: 3 })
    );
    assert_eq!(
        "../../etc".parse::<RunId>(),
        Err(RunIdParseError::Length { bytes: 9 }),
        "only a digest is a run id — a store keyed by one cannot be walked out of"
    );
}

#[test]
fn a_metric_that_json_cannot_write_is_refused_before_anything_is_stored() {
    let store = store("not_finite");
    let run = a_run(run_id(0x13), &[("ndcg@10", f64::NAN), ("recall@10", 0.75)]);

    match store.save(&run) {
        Err(RunStoreError::NotFinite { metric }) => assert_eq!(metric, "ndcg@10"),
        other => panic!("a NaN metric must be refused, got {other:?}"),
    }

    // The whole point of refusing on the way in: `serde_json` would have
    // written `null`, `save` would have reported success, and the run would
    // never have read back — under an id whose existence says it is done.
    assert!(
        !store.root().join(run.id.to_string()).exists(),
        "a refused run leaves nothing under its id"
    );
    assert!(matches!(
        store.load(&run.id),
        Err(RunStoreError::NotFound { .. })
    ));

    let infinite = a_run(run_id(0x14), &[("latency_p50_ms", f64::INFINITY)]);
    assert!(
        matches!(store.save(&infinite), Err(RunStoreError::NotFinite { .. })),
        "an infinity has no JSON form either"
    );
}

#[test]
fn metrics_are_written_in_name_order_whatever_order_they_were_recorded_in() {
    let store = store("metrics_bytes");
    let mut metrics = Metrics::default();
    // Four of them, recorded in reverse name order: recording order and name
    // order cannot be mistaken for each other, and an unordered map would have
    // to guess one arrangement out of twenty-four to look like this one.
    metrics.insert("recall@10", 0.75);
    metrics.insert("ndcg@10", 0.42);
    metrics.insert("latency_p50_ms", 31.0);
    metrics.insert("cost_usd", 0.004);

    let mut run = a_run(run_id(0x12), &[]);
    run.metrics = metrics;
    store.save(&run).expect("the run must be writable");

    let written = fs::read_to_string(store.root().join(run.id.to_string()).join("metrics.json"))
        .expect("metrics.json must exist");
    assert_eq!(
        written,
        concat!(
            "{\n",
            "  \"cost_usd\": 0.004,\n",
            "  \"latency_p50_ms\": 31.0,\n",
            "  \"ndcg@10\": 0.42,\n",
            "  \"recall@10\": 0.75\n",
            "}\n"
        ),
        "two runs of one shape diff cleanly only if their key order is fixed"
    );
}

#[test]
fn a_corrupted_file_in_a_run_directory_is_reported_with_its_path() {
    let store = store("malformed");
    let run = a_run(run_id(0x15), &[("ndcg@10", 0.42)]);
    store.save(&run).expect("the run must be writable");

    let metrics_file = store.root().join(run.id.to_string()).join("metrics.json");
    fs::write(&metrics_file, "{ not json").expect("the file must be writable");

    match store.load(&run.id) {
        Err(RunStoreError::Malformed { path, .. }) => assert_eq!(path, metrics_file),
        other => panic!("a corrupt record is malformed, not empty, got {other:?}"),
    }
}

#[test]
fn a_run_directory_missing_one_of_its_files_is_incomplete_rather_than_absent() {
    let store = store("incomplete");
    let run = a_run(run_id(0x16), &[("ndcg@10", 0.42)]);
    store.save(&run).expect("the run must be writable");

    let traces = store.root().join(run.id.to_string()).join("traces.json");
    fs::remove_file(&traces).expect("the file must be removable");

    // A torn run is nameable: a caller asking "is this run stored" gets an
    // answer it can act on, without reading an `io::ErrorKind`, and a run with
    // no traces is never silently handed back as a run.
    match store.load(&run.id) {
        Err(RunStoreError::Incomplete { path }) => assert_eq!(path, traces),
        other => panic!("a torn run must be named as one, got {other:?}"),
    }
}

#[test]
fn a_run_already_stored_is_left_as_it_is_and_nothing_is_staged_beside_it() {
    let store = store("resave");
    let run = a_run(run_id(0x17), &[("ndcg@10", 0.42)]);
    store.save(&run).expect("the run must be writable");

    assert_eq!(
        staged_entries(&store),
        Vec::<String>::new(),
        "a finished save leaves no staging directory behind"
    );

    // A marker a second save would destroy if it rewrote the directory in
    // place. It stands in for the run itself: the id is the digest of the
    // inputs, so a rewrite could only replace the run with itself, while a
    // crash halfway through one would lose it.
    let config_file = store.root().join(run.id.to_string()).join("config.yaml");
    fs::write(&config_file, "touched by hand\n").expect("the file must be writable");

    store
        .save(&run)
        .expect("saving a stored run is not an error");

    assert_eq!(
        fs::read_to_string(&config_file).expect("the file must still be there"),
        "touched by hand\n",
        "a run under an id is that run; the second save rewrites nothing"
    );
    assert_eq!(staged_entries(&store), Vec::<String>::new());
}

/// The staging directories under the store root, if any are left.
fn staged_entries(store: &FileSystemRunStore) -> Vec<String> {
    fs::read_dir(store.root())
        .expect("the store root must exist")
        .map(|entry| entry.expect("a readable entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.'))
        .collect()
}
