//! The harness calibrated against a published leaderboard figure, and the M2
//! exit criterion measured on real data.
//!
//! ADR-10 trusts the harness only once it reproduces a published score to
//! within half a point through an exact search, so that a discrepancy can be
//! blamed on the metric or the encoding and never on approximation;
//! `docs/system-architecture.md` § 9.8 Calibrating the harness against a
//! published leaderboard gives the procedure. This test is that reproduction,
//! run through `ragondin bench` — the real binary, the real components, the
//! in-memory store's brute-force search — over BEIR SciFact and a reference
//! sentence encoder, and then the exit criterion beside it: hybrid retrieval
//! with reranking against dense-only, on the same corpus, with a real
//! cross-encoder.
//!
//! # Ignored by default, run by `just calibrate`
//!
//! Nothing it needs may enter the tree or be fetched by it (ADR-C27 downloads
//! the runtime, never a model), and it costs the better part of half an hour
//! of CPU: two minutes to embed 5 183 passages, and some twenty-five to rerank
//! 300 queries over a fused list of a hundred with a real cross-encoder. So it
//! is `#[ignore]`, and two environment variables say where
//! the material was put by hand:
//!
//! - `RAGONDIN_CALIBRATION_DATASETS` — a directory holding `scifact/` in the
//!   layout the BEIR adapter reads, unpacked from the original archive.
//! - `RAGONDIN_CALIBRATION_MODELS` — a directory holding
//!   `all-MiniLM-L6-v2/` and `ms-marco-MiniLM-L6-v2/`, each with a `model.onnx`
//!   and its `tokenizer.json`, exported as recorded.
//!
//! The reference — archive hash, model revisions, export commands, the numbers
//! the recorded run produced — is in `bin/ragondin/ARCHITECTURE.md`
//! § Calibration against a published leaderboard; the constants below are the
//! part of that record the test holds itself to.
//!
//! # What is frozen here, and what is not
//!
//! The aggregates are: each metric of both runs must land within `RECORDED`'s
//! tolerance of what the recorded run scored, and the dataset and model digests
//! must be the recorded ones, so a run over the wrong revision fails by name.
//! A per-query freeze — the ranking each query produced, checked against
//! `pytrec_eval` — is what ADR-10 asks for as the permanent regression fixture,
//! and nothing in the workspace exposes a per-query ranking yet; that is a
//! decision, not this test's to make, and it is filed as such.

#![cfg(all(feature = "bm25", feature = "onnx"))]

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Instant;

use assert_cmd::Command;
use ragondin_experiments::{FileSystemRunStore, Run, RunId};

/// Where the SciFact directory sits.
const DATASETS_VAR: &str = "RAGONDIN_CALIBRATION_DATASETS";
/// Where the two exported models sit.
const MODELS_VAR: &str = "RAGONDIN_CALIBRATION_MODELS";

/// The benchmark selector `bench` takes: `scifact/` under the datasets root.
const BENCHMARK: &str = "beir/scifact";
const DENSE_ONLY: &str = "dense-only.yaml";
const HYBRID_RERANK: &str = "hybrid-rerank.yaml";
const NDCG: &str = "ndcg@10";

/// SciFact, `test` split, nDCG@10, as the MTEB leaderboard publishes it for
/// `sentence-transformers/all-MiniLM-L6-v2` at the revision the record pins.
const PUBLISHED_NDCG: f64 = 0.64508;
/// ADR-10's tolerance: half a point of nDCG@10.
const PUBLISHED_TOLERANCE: f64 = 0.005;

/// The digests a run over the recorded material records — the original
/// SciFact archive as the adapter reads it, and the two exported model files.
/// A run over anything else is a different calibration, and fails by name.
const RECORDED_DATASET: &str = "9a07f80c0d4f1e9e74912d033a8d1fbd52c54b758dafcaa85c19abacfdee5f29";
const RECORDED_EMBEDDER: &str = "9348202758f11c56c329d947ae359fea54be1a3d905bfcac4a3521a1eafc0414";
const RECORDED_RERANKER: &str = "8b0fe5bc3c5ddc752524552d8e081baa7726e389b1d23396e56ad31d69b88d52";

/// What the recorded runs scored, metric by metric.
const RECORDED_DENSE: [(&str, f64); 3] = [
    ("mrr", 0.6047248677248677),
    ("ndcg@10", 0.6450816521455768),
    ("recall@10", 0.7833333333333333),
];
const RECORDED_HYBRID: [(&str, f64); 3] = [
    ("mrr", 0.6579272486772487),
    ("ndcg@10", 0.6886092429213343),
    ("recall@10", 0.8122222222222222),
];
/// How far a metric may sit from the recorded one on another machine: ONNX
/// Runtime's summation order moves the last bits of an `f64`, not its fourth
/// decimal. On the machine that recorded them the agreement is exact, and the
/// P4 assertion below is where exactness is required.
const RECORDED_TOLERANCE: f64 = 1e-4;

/// The two directories, or a message saying which is missing and what goes in
/// it. Failing rather than passing quietly: this test only runs when asked
/// for, and an ask that cannot be honoured is an error, not a skip.
fn material() -> (PathBuf, PathBuf) {
    let read = |var: &str, holds: &str| {
        std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| {
            panic!(
                "{var} is not set: it names the directory holding {holds}. \
                 `bin/ragondin/ARCHITECTURE.md` § Calibration against a published \
                 leaderboard says how that material is obtained."
            )
        })
    };
    (
        read(DATASETS_VAR, "`scifact/` in the BEIR layout"),
        read(
            MODELS_VAR,
            "`all-MiniLM-L6-v2/` and `ms-marco-MiniLM-L6-v2/`, each with `model.onnx` and \
             `tokenizer.json`",
        ),
    )
}

fn configs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/calibration")
}

fn path(buf: &Path) -> &str {
    buf.to_str().expect("UTF-8 path")
}

/// A run store of this test's own, emptied first.
fn store(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("calibration")
        .join(name);
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

/// Spawns the built binary from the models directory, where the committed
/// configurations' relative model paths resolve.
fn ragondin(models: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("ragondin")
        .expect("the binary under test is built by `cargo test`")
        .current_dir(models)
        .args(args)
        .output()
        .expect("the binary runs")
}

fn reported_run_id(summary: &str) -> RunId {
    let first = summary.lines().next().expect("a summary has a first line");
    first
        .strip_prefix("run ")
        .unwrap_or_else(|| panic!("the summary opens with the run id: {summary}"))
        .parse()
        .expect("the printed id is a run id")
}

/// Evaluates one configuration over SciFact into `store` and returns the run
/// as the store holds it, printing what it cost.
fn bench(datasets: &Path, models: &Path, config: &str, store: &Path) -> Run {
    let started = Instant::now();
    let output = ragondin(
        models,
        &[
            "bench",
            path(&configs().join(config)),
            "--benchmark",
            BENCHMARK,
            "--datasets",
            path(datasets),
            "--store",
            path(store),
        ],
    );
    assert!(
        output.status.success(),
        "`bench {config}` failed: {}",
        stderr(&output)
    );
    println!(
        "{config}: {:.0} s\n{}",
        started.elapsed().as_secs_f64(),
        stdout(&output)
    );

    FileSystemRunStore::new(store)
        .load(&reported_run_id(&stdout(&output)))
        .expect("the run bench reported is the run bench saved")
}

fn metric(run: &Run, name: &str) -> f64 {
    run.metrics
        .get(name)
        .unwrap_or_else(|| panic!("a retrieval run scores {name}"))
}

fn assert_recorded(run: &Run, recorded: &[(&str, f64)], which: &str) {
    for (name, expected) in recorded {
        let got = metric(run, name);
        assert!(
            (got - expected).abs() <= RECORDED_TOLERANCE,
            "{which} {name}: {got} against the recorded {expected}"
        );
    }
}

/// One run, sequential on purpose: the dense leg is embedded once per
/// evaluation, and three evaluations in parallel would contend for every core.
#[test]
#[ignore = "needs BEIR SciFact and two exported models on disk; run with `just calibrate`"]
fn the_harness_reproduces_the_published_scifact_figure_and_hybrid_beats_dense() {
    let (datasets, models) = material();

    // The reproduction (ADR-10), twice: P4 says identical inputs are one run.
    let dense = bench(&datasets, &models, DENSE_ONLY, &store("dense"));
    let again = bench(&datasets, &models, DENSE_ONLY, &store("dense-again"));
    assert_eq!(dense.id, again.id, "two run ids for one input");
    assert_eq!(dense.inputs, again.inputs);
    assert_eq!(
        dense.metrics, again.metrics,
        "the same evaluation scored differently"
    );

    // Over the recorded material, and nothing else.
    assert_eq!(
        dense.inputs.dataset_version, RECORDED_DATASET,
        "not the recorded SciFact"
    );
    assert_eq!(
        dense
            .inputs
            .model_hashes
            .get("embedder")
            .map(String::as_str),
        Some(RECORDED_EMBEDDER),
        "not the recorded embedder export"
    );

    // The calibration itself: within half a point of the published figure.
    let ndcg = metric(&dense, NDCG);
    assert!(
        (ndcg - PUBLISHED_NDCG).abs() <= PUBLISHED_TOLERANCE,
        "dense-only {NDCG} {ndcg:.5} is {:.2} points from the published {PUBLISHED_NDCG}: \
         see the diagnostic table in `docs/system-architecture.md` § 9.8",
        (ndcg - PUBLISHED_NDCG).abs() * 100.0
    );
    assert_recorded(&dense, &RECORDED_DENSE, "dense-only");

    // The exit criterion, on real data — into the store the dense run is
    // already in, so that `compare` below reads both from one place.
    let hybrid = bench(&datasets, &models, HYBRID_RERANK, &store_path("dense"));
    assert_eq!(
        hybrid
            .inputs
            .model_hashes
            .get("reranker")
            .map(String::as_str),
        Some(RECORDED_RERANKER),
        "not the recorded reranker export"
    );
    assert!(
        metric(&hybrid, NDCG) > ndcg,
        "hybrid+rerank scored {NDCG} {:.5}, dense-only {ndcg:.5}",
        metric(&hybrid, NDCG)
    );
    assert_recorded(&hybrid, &RECORDED_HYBRID, "hybrid+rerank");

    // And `compare` says so, from the same store.
    let output = ragondin(
        &models,
        &[
            "compare",
            &dense.id.to_string(),
            &hybrid.id.to_string(),
            "--store",
            path(&store_path("dense")),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let report = stdout(&output);
    let line = report
        .lines()
        .find(|line| line.starts_with(&format!("{NDCG}:")))
        .unwrap_or_else(|| panic!("compare reports {NDCG}:\n{report}"));
    assert!(line.contains("-> right +"), "{report}");
    println!("{report}");
}

/// The store a name maps to, without emptying it — for reading back what a
/// `bench` in this test already wrote there.
fn store_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("calibration")
        .join(name)
}
