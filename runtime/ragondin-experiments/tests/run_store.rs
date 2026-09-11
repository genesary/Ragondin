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
    compare, ConfigDocument, FileSystemRunStore, Run, RunId, RunInputs, RunStoreError,
    TraceDocument,
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

/// A pipeline's content hash, built the one way a caller outside
/// `ragondin-pipeline` can build one without a pipeline: through its
/// `Deserialize`. The harness gets its own from `LogicalPipeline::content_hash`.
fn pipeline_hash() -> PipelineHash {
    serde_json::from_str("\"bed7a3e04d0eae5efbcd4dbd0d1d1ff1d4c2b0e7e2a2d3f4a5b6c7d8e9f00112\"")
        .expect("64 lowercase hex digits are a pipeline hash")
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

    let metrics = fs::read_to_string(dir.join("metrics.json")).expect("metrics.json must exist");
    assert!(
        metrics.contains("\"ndcg@10\""),
        "metrics.json holds the metrics by name: {metrics}"
    );

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
fn a_run_id_round_trips_through_its_hex_form() {
    let id = run_id(0x0f);
    let hex = id.to_string();

    assert_eq!(hex.len(), 64, "a run id renders as 64 hex digits: {hex}");
    assert_eq!(hex.parse::<RunId>().expect("its own rendering parses"), id);
    assert!(
        "../../etc".parse::<RunId>().is_err(),
        "only a digest is a run id — a store keyed by one cannot be walked out of"
    );
}
