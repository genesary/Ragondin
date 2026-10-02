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
    compare, ConfigDocument, FileSystemRunStore, Metrics, PrefixOf, Run, RunBinding, RunId,
    RunIdParseError, RunInputs, RunProvenance, RunStore, RunStoreError, RunTimes, TraceDocument,
    UnixMillis,
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
        bindings: Vec::new(),
        times: None,
        provenance: None,
    }
}

fn times(started: u64, finished: u64) -> RunTimes {
    RunTimes::new(UnixMillis::new(started), UnixMillis::new(finished))
}

fn a_binding(family: &str, name: &str, uri: &str) -> RunBinding {
    RunBinding {
        family: family.to_owned(),
        name: name.to_owned(),
        uri: uri.to_owned(),
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
fn a_run_s_bindings_read_back_with_it_in_the_order_they_were_given() {
    let store = store("bindings_round_trip");
    let mut written = a_run(run_id(0x12), &[("ndcg@10", 0.42)]);
    written.bindings = vec![
        a_binding("generator", "vllm", "http://localhost:8000"),
        a_binding("embedder", "bge", "http://10.0.0.7:50051"),
    ];

    store.save(&written).expect("the run must be writable");
    let read = store.load(&written.id).expect("the run must read back");

    assert_eq!(read, written);
    // Beside the four files a run always had, in a file of its own, so the
    // record a person reads says where each bound component answered.
    let bindings = fs::read_to_string(
        store
            .root()
            .join(written.id.to_string())
            .join("bindings.json"),
    )
    .expect("the bindings are kept");
    assert!(bindings.contains("http://localhost:8000"), "{bindings}");
}

#[test]
fn a_run_stored_before_bindings_were_recorded_reads_back_as_bound_to_nothing() {
    // A run directory as a store wrote it before this field existed: the four
    // files and no `bindings.json`. The change is additive (ADR-C32
    // Consequences), so it is still a complete run, and it was bound to
    // nothing.
    let store = store("bindings_absent");
    let run = a_run(run_id(0x13), &[("ndcg@10", 0.42)]);
    store.save(&run).expect("the run must be writable");
    let dir = store.root().join(run.id.to_string());
    fs::remove_file(dir.join("bindings.json")).expect("the new file is there to remove");

    let read = store.load(&run.id).expect("an older run still reads");

    assert!(read.bindings.is_empty(), "{:?}", read.bindings);
    store
        .save(&run)
        .expect("and it counts as stored, not as torn");
}

#[test]
fn no_times_file_is_written_for_unknown_times() {
    let store = store("times_unknown_no_file");
    let run = a_run(run_id(0x14), &[("ndcg@10", 0.42)]);
    store.save(&run).expect("the run must be writable");

    // Absent is how "unknown" is spelled. A file holding `null` would give one
    // fact two spellings.
    let dir = store.root().join(run.id.to_string());
    assert!(!dir.join("times.json").exists());
}

#[test]
fn known_times_are_written_as_exactly_started_ms_and_finished_ms() {
    let store = store("times_known_file");
    let mut run = a_run(run_id(0x15), &[("ndcg@10", 0.42)]);
    run.times = Some(times(1_700_000_000_000, 1_700_000_004_250));
    store.save(&run).expect("the run must be writable");

    let text = fs::read_to_string(store.root().join(run.id.to_string()).join("times.json"))
        .expect("known times are kept in a file of their own");
    let value: serde_json::Value = serde_json::from_str(&text).expect("the file is JSON");
    assert_eq!(
        value,
        serde_json::json!({"started_ms": 1_700_000_000_000u64, "finished_ms": 1_700_000_004_250u64})
    );
}

#[test]
fn a_deleted_times_file_reads_as_unknown() {
    let store = store("times_deleted");
    let mut run = a_run(run_id(0x16), &[("ndcg@10", 0.42)]);
    run.times = Some(times(1_000, 2_000));
    store.save(&run).expect("the run must be writable");
    let dir = store.root().join(run.id.to_string());
    fs::remove_file(dir.join("times.json")).expect("the times file is there to remove");

    let read = store
        .load(&run.id)
        .expect("a run without times still reads");

    assert_eq!(
        read.times, None,
        "an absent file is unknown, never an estimate"
    );
    store
        .save(&run)
        .expect("and it counts as stored, not as torn");
}

#[test]
fn the_fixture_without_times_reads_as_unknown() {
    // A run directory the harness wrote before times were recorded.
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stored-before-typed-trace");
    let id: RunId = "b41e0752792e728f5dd893043b42d2a2d71f0b0039157a177e0a267e0420ea6f"
        .parse()
        .expect("the fixture is named by a run id");
    assert!(!root.join(id.to_string()).join("times.json").exists());

    let run = FileSystemRunStore::new(&root)
        .load(&id)
        .expect("a run stored before times were recorded still loads");

    assert_eq!(run.times, None);
}

#[test]
fn a_malformed_times_file_is_malformed() {
    let store = store("times_malformed");
    let mut run = a_run(run_id(0x17), &[("ndcg@10", 0.42)]);
    run.times = Some(times(1_000, 2_000));
    store.save(&run).expect("the run must be writable");
    let path = store.root().join(run.id.to_string()).join("times.json");
    fs::write(&path, "{}").expect("the times file is writable");

    match store.load(&run.id) {
        Err(RunStoreError::Malformed { path: reported, .. }) => assert_eq!(reported, path),
        other => panic!("a times file that does not parse is malformed, got {other:?}"),
    }
}

#[test]
fn times_are_not_part_of_identity() {
    // The harness computes the id before any time exists, so one run saved
    // with times and the same run saved without them are one run. This test
    // holds the store to that; the guard on the digest itself is
    // `RECORDED_RUN_ID` in `ragondin-harness`'s `identity.rs`, which fails if
    // anything new reaches the identity tuple.
    let store = store("times_not_identity");
    let without = a_run(run_id(0x18), &[("ndcg@10", 0.42)]);
    let mut with = without.clone();
    with.times = Some(times(1_000, 2_000));
    assert_eq!(with.id, without.id);

    store.save(&with).expect("the run must be writable");
    store.save(&without).expect("the same run saves again");

    assert_eq!(store.ids().expect("the store lists"), vec![with.id]);
    assert_eq!(
        store.load(&with.id).expect("the run reads back").times,
        with.times,
        "the first record is the one kept"
    );
}

fn provenance_file(store: &FileSystemRunStore, run: &Run) -> PathBuf {
    store
        .root()
        .join(run.id.to_string())
        .join("provenance.json")
}

fn a_parent_hash() -> PipelineHash {
    PipelineHash::from_digest([0x5e; 32])
}

#[test]
fn no_provenance_file_is_written_for_no_record() {
    let store = store("provenance_none_no_file");
    let run = a_run(run_id(0x1a), &[("ndcg@10", 0.42)]);
    store.save(&run).expect("the run must be writable");

    // Absent is how "no record" is spelled. `{}` is an empty record, a fact of
    // its own, and `null` would give "no record" a second spelling.
    assert!(!provenance_file(&store, &run).exists());
}

#[test]
fn a_record_is_written_with_only_the_keys_of_the_fields_that_are_set() {
    let store = store("provenance_keys");
    let mut named = a_run(run_id(0x1b), &[("ndcg@10", 0.42)]);
    named.provenance = Some(RunProvenance::named("hybrid"));
    let mut prefix = a_run(run_id(0x1c), &[("ndcg@10", 0.42)]);
    prefix.provenance = Some(RunProvenance::prefix(
        "hybrid",
        PrefixOf::new("fused", a_parent_hash()),
    ));
    store.save(&named).expect("the run must be writable");
    store.save(&prefix).expect("the run must be writable");

    let read = |run: &Run| -> serde_json::Value {
        let text = fs::read_to_string(provenance_file(&store, run))
            .expect("a record is kept in a file of its own");
        serde_json::from_str(&text).expect("the file is JSON")
    };
    assert_eq!(read(&named), serde_json::json!({"name": "hybrid"}));
    assert_eq!(
        read(&prefix),
        serde_json::json!({
            "name": "hybrid",
            "prefix_of": {
                "up_to": "fused",
                "parent_pipeline_hash": a_parent_hash().to_string(),
            },
        })
    );
}

#[test]
fn a_deleted_provenance_file_reads_none() {
    let store = store("provenance_deleted");
    let mut run = a_run(run_id(0x1d), &[("ndcg@10", 0.42)]);
    run.provenance = Some(RunProvenance::named("hybrid"));
    store.save(&run).expect("the run must be writable");
    fs::remove_file(provenance_file(&store, &run)).expect("the record is there to remove");

    let read = store
        .load(&run.id)
        .expect("a run without a record still reads");

    assert_eq!(read.provenance, None);
    store
        .save(&run)
        .expect("and it counts as stored, not as torn");
}

#[test]
fn a_malformed_provenance_file_is_malformed() {
    let store = store("provenance_malformed");
    let mut run = a_run(run_id(0x1e), &[("ndcg@10", 0.42)]);
    run.provenance = Some(RunProvenance::named("hybrid"));
    store.save(&run).expect("the run must be writable");
    let path = provenance_file(&store, &run);

    // Not JSON, a known field of the wrong type, and a `prefix_of` missing
    // half of itself: none of them is a record.
    for text in [
        "{ not json",
        r#"{"name": 3}"#,
        r#"{"name": "hybrid", "prefix_of": {"up_to": "fused"}}"#,
    ] {
        fs::write(&path, text).expect("the record is writable");
        match store.load(&run.id) {
            Err(RunStoreError::Malformed { path: reported, .. }) => assert_eq!(reported, path),
            other => panic!("{text}: a record that does not parse is malformed, got {other:?}"),
        }
    }
}

#[test]
fn an_unknown_provenance_field_is_ignored() {
    // Fields are additive (ADR-C39 § 1): a record written by a later build,
    // with a field this one does not know, reads its known fields.
    let store = store("provenance_unknown_field");
    let mut run = a_run(run_id(0x1f), &[("ndcg@10", 0.42)]);
    run.provenance = Some(RunProvenance::named("hybrid"));
    store.save(&run).expect("the run must be writable");
    fs::write(
        provenance_file(&store, &run),
        r#"{"name": "hybrid", "concurrency": 4}"#,
    )
    .expect("the record is writable");

    let read = store.load(&run.id).expect("the record still reads");

    assert_eq!(read.provenance, Some(RunProvenance::named("hybrid")));
}

#[test]
fn an_empty_record_reads_back_as_an_empty_record_not_as_none() {
    let store = store("provenance_empty");
    let mut run = a_run(run_id(0x20), &[("ndcg@10", 0.42)]);
    run.provenance = Some(RunProvenance::default());
    store.save(&run).expect("the run must be writable");

    let text = fs::read_to_string(provenance_file(&store, &run))
        .expect("an empty record is still a record, kept in its file");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).expect("the file is JSON"),
        serde_json::json!({})
    );
    assert_eq!(
        store.load(&run.id).expect("the run reads back").provenance,
        Some(RunProvenance::default())
    );
}

#[test]
fn a_prefix_record_without_a_name_is_read_though_none_can_be_built() {
    // The constructors require a name beside `prefix_of`; the reader stays
    // tolerant, since the matrix's cell rule (ADR-C39 § 6) reads only the
    // parent hash.
    let store = store("provenance_prefix_no_name");
    let mut run = a_run(run_id(0x21), &[("ndcg@10", 0.42)]);
    run.provenance = Some(RunProvenance::named("hybrid"));
    store.save(&run).expect("the run must be writable");
    fs::write(
        provenance_file(&store, &run),
        serde_json::json!({"prefix_of": {
            "up_to": "fused",
            "parent_pipeline_hash": a_parent_hash().to_string(),
        }})
        .to_string(),
    )
    .expect("the record is writable");

    let read = store
        .load(&run.id)
        .expect("the record reads")
        .provenance
        .expect("a record");

    assert_eq!(read.name(), None);
    let prefix = read.prefix_of().expect("its prefix is read");
    assert_eq!(prefix.up_to(), "fused");
    assert_eq!(prefix.parent_pipeline_hash(), &a_parent_hash());
}

#[test]
fn provenance_is_not_part_of_identity() {
    // The harness computes the id before any record exists, so one run saved
    // with a record and the same run saved without one are one run (INV-8).
    // The guard on the digest itself is `RECORDED_RUN_ID` in
    // `ragondin-harness`'s `identity.rs`.
    let store = store("provenance_not_identity");
    let without = a_run(run_id(0x23), &[("ndcg@10", 0.42)]);
    let mut with = without.clone();
    with.provenance = Some(RunProvenance::named("hybrid"));
    assert_eq!(with.id, without.id);

    store.save(&with).expect("the run must be writable");
    store.save(&without).expect("the same run saves again");

    assert_eq!(store.ids().expect("the store lists"), vec![with.id]);
    assert_eq!(
        store.load(&with.id).expect("the run reads back").provenance,
        with.provenance,
        "the first record is the one kept"
    );
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
    // Six of them, recorded in reverse name order: recording order and name
    // order cannot be mistaken for each other, and an unordered map would have
    // to guess one arrangement out of seven hundred and twenty to look like
    // this one. Four left that at one in twenty-four, which a reseeded hash
    // order reaches about once in five runs of a mutation.
    metrics.insert("recall@10", 0.75);
    metrics.insert("precision@10", 0.5);
    metrics.insert("ndcg@10", 0.42);
    metrics.insert("mrr@10", 0.6);
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
            "  \"mrr@10\": 0.6,\n",
            "  \"ndcg@10\": 0.42,\n",
            "  \"precision@10\": 0.5,\n",
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

#[test]
fn many_threads_saving_one_run_all_report_success_and_the_run_loads() {
    let store = store("concurrent");
    let run = a_run(run_id(0x18), &[("ndcg@10", 0.42)]);

    // The store is `Clone` and `Sync`, so saving from several threads is an
    // ordinary thing to do — and every one of them stages the *same run id*,
    // which is the collision a staging path keyed by process alone would have.
    // A failure here is a caller told its save failed for a run that is in
    // fact stored, which invites re-running an expensive benchmark.
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let store = store.clone();
            let run = run.clone();
            scope.spawn(move || {
                for _ in 0..50 {
                    store
                        .save(&run)
                        .expect("every concurrent save reports success");
                }
            });
        }
    });

    assert_eq!(
        store.load(&run.id).expect("the run must read back"),
        run,
        "whichever writer won, what is stored is the run"
    );
    assert_eq!(
        staged_entries(&store),
        Vec::<String>::new(),
        "no staging directory survives its save"
    );
}

#[test]
fn saving_over_a_torn_directory_reports_it_rather_than_claiming_success() {
    let store = store("torn_destination");
    let run = a_run(run_id(0x19), &[("ndcg@10", 0.42)]);
    store.save(&run).expect("the run must be writable");

    let dir = store.root().join(run.id.to_string());
    let metrics_file = dir.join("metrics.json");
    fs::remove_file(&metrics_file).expect("the file must be removable");

    // The alternative would be to answer `Ok(())` for a directory that does
    // not load, or to delete it and write this run over it — and a run's
    // metrics and traces are not determined by its id, so the deletion could
    // destroy the only copy of a judge's scores.
    match store.save(&run) {
        Err(RunStoreError::Incomplete { path }) => assert_eq!(path, metrics_file),
        other => panic!("a torn destination must be named, got {other:?}"),
    }
    assert!(
        dir.join("traces.json").is_file(),
        "the files that are there are left where they are"
    );
    assert_eq!(staged_entries(&store), Vec::<String>::new());
}

/// The trait is object-safe and `Send + Sync`: a reader holds one backend
/// behind a pointer and shares it across threads.
#[test]
fn a_run_store_is_usable_behind_a_shared_pointer() {
    let backend = store("shared_pointer");
    let run = a_run(run_id(0x71), &[("ndcg@10", 0.42)]);
    backend.save(&run).expect("the run must be writable");

    let shared: std::sync::Arc<dyn RunStore> = std::sync::Arc::new(backend);
    let ids = std::thread::spawn(move || shared.ids())
        .join()
        .expect("the reader thread finishes")
        .expect("the store lists its runs");

    assert_eq!(ids, vec![run.id]);
}

#[test]
fn a_store_whose_root_was_never_created_lists_no_runs() {
    // The root is created by the first `save`, so an unused store has none —
    // and holds no runs, rather than failing to list them.
    let store = store("ids_no_root");
    assert!(!store.root().exists());

    assert_eq!(
        RunStore::ids(&store).expect("an absent root lists"),
        Vec::new()
    );
}

#[test]
fn the_listing_skips_whatever_under_the_root_is_not_a_run_directory() {
    // A staging directory a crashed `save` left, a file named like a run, a
    // directory whose name is not a run id: none of them is a run. A torn run
    // directory *is* listed — its id names it, and `load` is where it is
    // reported incomplete.
    let store = store("ids_skip");
    let stored = a_run(run_id(0x72), &[("ndcg@10", 0.42)]);
    store.save(&stored).expect("the run must be writable");
    let root = store.root();
    let torn = run_id(0x73);
    fs::create_dir_all(root.join(torn.to_string())).expect("a torn directory");
    fs::create_dir_all(root.join(format!(".{}.1-0.partial", run_id(0x74)))).expect("staging");
    fs::write(root.join(run_id(0x75).to_string()), "not a directory").expect("a file");
    fs::create_dir_all(root.join("notes")).expect("a stray directory");
    fs::create_dir_all(root.join(run_id(0xab).to_string().to_uppercase())).expect("uppercase");

    let ids = RunStore::ids(&store).expect("the root lists");

    assert_eq!(ids, vec![stored.id, torn]);
    assert!(matches!(
        store.load(&torn),
        Err(RunStoreError::Incomplete { .. })
    ));
}

#[cfg(unix)]
#[test]
fn the_listing_does_not_follow_a_symbolic_link() {
    // The store writes directories, never links; a link under the root is
    // something a person put there, and the listing reads each entry's own
    // type rather than what it points at.
    let store = store("ids_symlink");
    let stored = a_run(run_id(0x81), &[("ndcg@10", 0.42)]);
    store.save(&stored).expect("the run must be writable");
    let root = store.root();
    std::os::unix::fs::symlink(
        root.join(stored.id.to_string()),
        root.join(run_id(0x82).to_string()),
    )
    .expect("a link is creatable");

    assert_eq!(
        RunStore::ids(&store).expect("the root lists"),
        vec![stored.id]
    );
}
