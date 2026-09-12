//! The `compare` subcommand, exercised as a process.
//!
//! Like `validate`, `compare`'s contract is not a function's return value: it
//! is an **exit status** and what lands on **stdout** — a diff meant for a
//! terminal. Every test here writes runs straight through
//! `ragondin-experiments` into a store of its own, then spawns the built
//! binary against that store.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Output;

use assert_cmd::Command;
use ragondin_experiments::{ConfigDocument, FileSystemRunStore, Run, RunId, RunInputs};
use ragondin_pipeline::PipelineHash;

fn ragondin(args: &[&str]) -> Output {
    Command::cargo_bin("ragondin")
        .expect("the binary under test is built by `cargo test`")
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

/// A store of this test's own, under `CARGO_TARGET_TMPDIR`, emptied first so a
/// run left by a previous, killed run of this test cannot decide this one.
fn store(test_name: &str) -> FileSystemRunStore {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("compare")
        .join(test_name);
    let _ = std::fs::remove_dir_all(&root);
    FileSystemRunStore::new(root)
}

fn run_id(byte: u8) -> RunId {
    RunId::from_digest([byte; 32])
}

/// A run whose metrics are the ones given, its other fields fixed so a
/// comparison test varies exactly one thing.
fn a_run(id: RunId, metrics: &[(&str, f64)]) -> Run {
    Run {
        id,
        inputs: RunInputs {
            pipeline: PipelineHash::from_digest([0xbe; 32]),
            dataset_version: "beir/scifact@2021-05-01".to_owned(),
            index_version: "bm25-ram@7".to_owned(),
            model_hashes: BTreeMap::new(),
            engine_version: "0.0.0".to_owned(),
        },
        metrics: metrics.iter().copied().collect(),
        config: ConfigDocument::new("schema_version: 1\nnodes: []\n"),
        traces: BTreeMap::new(),
    }
}

#[test]
fn compare_prints_a_metric_by_metric_diff_and_exits_zero() {
    let store = store("diff");
    let dense = a_run(run_id(0x11), &[("ndcg@10", 0.64), ("recall@10", 0.75)]);
    let hybrid = a_run(run_id(0x22), &[("ndcg@10", 0.71), ("recall@10", 0.75)]);
    store.save(&dense).expect("the dense run must be writable");
    store
        .save(&hybrid)
        .expect("the hybrid run must be writable");

    let output = ragondin(&[
        "compare",
        &dense.id.to_string(),
        &hybrid.id.to_string(),
        "--store",
        store.root().to_str().expect("UTF-8 path"),
    ]);

    assert!(
        output.status.success(),
        "comparing two stored runs exits 0, got {:?}: {}",
        output.status.code(),
        stderr(&output)
    );

    let report = stdout(&output);
    assert!(
        report.contains("ndcg@10") && report.contains("0.6400") && report.contains("0.7100"),
        "the diff must show the metric the two runs disagree on with both values, got:\n{report}"
    );
    assert!(
        report.contains("recall@10") && report.contains("0.7500"),
        "the diff must also show the metric the two runs agree on, got:\n{report}"
    );
}

#[test]
fn comparing_a_run_with_itself_reports_identical() {
    let store = store("self");
    let run = a_run(run_id(0x33), &[("ndcg@10", 0.71), ("recall@10", 0.75)]);
    store.save(&run).expect("the run must be writable");

    let output = ragondin(&[
        "compare",
        &run.id.to_string(),
        &run.id.to_string(),
        "--store",
        store.root().to_str().expect("UTF-8 path"),
    ]);

    assert!(
        output.status.success(),
        "comparing a run with itself exits 0, got {:?}: {}",
        output.status.code(),
        stderr(&output)
    );
    assert!(
        stdout(&output).contains("identical"),
        "got:\n{}",
        stdout(&output)
    );
}

#[test]
fn comparing_an_unknown_run_id_is_a_diagnosis_not_a_crash() {
    let store = store("unknown");
    let known = a_run(run_id(0x44), &[("ndcg@10", 0.71)]);
    store.save(&known).expect("the known run must be writable");

    let output = ragondin(&[
        "compare",
        &known.id.to_string(),
        &run_id(0x55).to_string(),
        "--store",
        store.root().to_str().expect("UTF-8 path"),
    ]);

    assert!(!output.status.success(), "an unknown run id exits non-zero");
    let report = stderr(&output);
    assert!(!report.contains("panicked"), "got:\n{report}");
    assert!(
        report.contains(&run_id(0x55).to_string()),
        "the report must name the run id that was not found, got:\n{report}"
    );
}
