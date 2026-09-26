//! The harness calibrated against two published leaderboard figures, and the
//! M2 exit criterion measured on real data.
//!
//! ADR-10 trusts the harness only once it reproduces a published score to
//! within half a point through an exact search, so that a discrepancy can be
//! blamed on the metric or the encoding and never on approximation;
//! `docs/system-architecture.md` § 9.8 Calibrating the harness against a
//! published leaderboard gives the procedure, and names the two cases it asks
//! for: SciFact first, with binary qrels, then NFCorpus, whose graded qrels
//! "alone can expose a linear-versus-exponential gain bug". Both reproductions
//! live here, run through `ragondin bench` — the real binary, the real
//! components, the in-memory store's brute-force search — over the same
//! reference sentence encoder; and beside the SciFact one, the exit criterion:
//! hybrid retrieval with reranking against dense-only, on that corpus, with a
//! real cross-encoder.
//!
//! One test per dataset, because they cost two very different things and a
//! reproduction that misses is diagnosed one dataset at a time.
//!
//! # Ignored by default, run by `just calibrate`
//!
//! Nothing they need may enter the tree or be fetched by it (ADR-C27 downloads
//! the runtime, never a model), and together they cost the better part of half
//! an hour of CPU: two minutes to embed SciFact's 5 183 passages, some
//! twenty-five to rerank 300 queries over a fused list of a hundred with a real
//! cross-encoder, and a couple of minutes for NFCorpus's 3 633 passages,
//! embedded twice. So they are `#[ignore]`, and two environment variables say
//! where the material was put by hand:
//!
//! - `RAGONDIN_CALIBRATION_DATASETS` — a directory holding `scifact/` and
//!   `nfcorpus/` in the layout the BEIR adapter reads, each unpacked from its
//!   original archive.
//! - `RAGONDIN_CALIBRATION_MODELS` — a directory holding
//!   `all-MiniLM-L6-v2/` and `ms-marco-MiniLM-L6-v2/`, each with a `model.onnx`
//!   and its `tokenizer.json`, exported as recorded. NFCorpus needs only the
//!   first: its case is dense-only.
//!
//! The reference — archive hashes, model revisions, export commands, the
//! numbers the recorded runs produced — is in `bin/ragondin/ARCHITECTURE.md`
//! § Calibration against a published leaderboard; the constants below are the
//! part of that record the tests hold themselves to.
//!
//! # What is frozen here, and what is not
//!
//! The aggregates are: each metric of every run must land within `RECORDED`'s
//! tolerance of what the recorded run scored, and the dataset digest and the
//! model identities must be the recorded ones, so a run over the wrong revision
//! fails by name.
//! A per-query freeze — the ranking each query produced, checked against
//! `pytrec_eval` — is what ADR-10 asks for as the permanent regression fixture,
//! and it is not frozen here. The ranking is recorded: ADR-C28 has each node's
//! output entry in the execution trace name the chunks it produced, in rank
//! order, so a stored run's `traces.json` holds every query's ranking. The
//! fixtures that freeze them — query by query, against `pytrec_eval` — are
//! `eval/ragondin-metrics/tests/scifact_calibration_fixture.rs` and
//! `eval/ragondin-metrics/tests/nfcorpus_calibration_fixture.rs`, where the
//! metrics they guard live.

#![cfg(all(feature = "bm25", feature = "onnx"))]

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Instant;

use assert_cmd::Command;
use ragondin_experiments::{FileSystemRunStore, Run, RunId};

/// Where the SciFact and NFCorpus directories sit.
const DATASETS_VAR: &str = "RAGONDIN_CALIBRATION_DATASETS";
/// Where the two exported models sit.
const MODELS_VAR: &str = "RAGONDIN_CALIBRATION_MODELS";

/// The benchmark selectors `bench` takes: `scifact/` and `nfcorpus/` under the
/// datasets root.
const SCIFACT: &str = "beir/scifact";
const NFCORPUS: &str = "beir/nfcorpus";
const DENSE_ONLY: &str = "dense-only.yaml";
const HYBRID_RERANK: &str = "hybrid-rerank.yaml";
const NFCORPUS_DENSE_ONLY: &str = "nfcorpus-dense-only.yaml";
const NDCG: &str = "ndcg@10";

/// SciFact, `test` split, nDCG@10, as the MTEB leaderboard publishes it for
/// `sentence-transformers/all-MiniLM-L6-v2` at the revision the record pins.
const PUBLISHED_NDCG: f64 = 0.64508;
/// NFCorpus, `test` split, nDCG@10, for the same model at the same revision,
/// read from the same place at reproduction time.
const NFCORPUS_PUBLISHED_NDCG: f64 = 0.31594;
/// ADR-10's tolerance: half a point of nDCG@10.
const PUBLISHED_TOLERANCE: f64 = 0.005;

/// The digests a run over the recorded material records — each original
/// archive as the adapter reads it, and the identity each exported model
/// reports: `<model>+<tokenizer>`, the SHA-256 of the model file, `+`, and the
/// SHA-256 of its `tokenizer.json` (ADR-C32 § 4). A run over anything else is
/// a different calibration, and fails by name.
const RECORDED_DATASET: &str = "9a07f80c0d4f1e9e74912d033a8d1fbd52c54b758dafcaa85c19abacfdee5f29";
const RECORDED_NFCORPUS_DATASET: &str =
    "8046025011c86dcbac3c15f9f52e5cf0ebc534282944b50fe72884cfcb6a112b";
const RECORDED_EMBEDDER: &str = "9348202758f11c56c329d947ae359fea54be1a3d905bfcac4a3521a1eafc0414\
     +da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0";
const RECORDED_RERANKER: &str = "8b0fe5bc3c5ddc752524552d8e081baa7726e389b1d23396e56ad31d69b88d52\
     +d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66";

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
/// NFCorpus is dense-only, so there is one row rather than two: the exit
/// criterion is SciFact's, and § 9.8 asks this dataset for the graded figure
/// and nothing else.
const RECORDED_NFCORPUS_DENSE: [(&str, f64); 3] = [
    ("mrr", 0.5076539387684899),
    ("ndcg@10", 0.31667312754717813),
    ("recall@10", 0.15498797328057862),
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
                "{var} is not set: it names the directory holding {holds}. How that \
                 material is obtained is recorded in `bin/ragondin/ARCHITECTURE.md` \
                 § Calibration against a published leaderboard."
            )
        })
    };
    (
        read(
            DATASETS_VAR,
            "`scifact/` and `nfcorpus/` in the BEIR layout",
        ),
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

/// Evaluates one configuration over one benchmark into `store` and returns the
/// run as the store holds it, printing what it cost.
fn bench(datasets: &Path, models: &Path, benchmark: &str, config: &str, store: &Path) -> Run {
    let started = Instant::now();
    let output = ragondin(
        models,
        &[
            "bench",
            path(&configs().join(config)),
            "--benchmark",
            benchmark,
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
    let dense = bench(&datasets, &models, SCIFACT, DENSE_ONLY, &store("dense"));
    let again = bench(
        &datasets,
        &models,
        SCIFACT,
        DENSE_ONLY,
        &store("dense-again"),
    );
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
    let hybrid = bench(
        &datasets,
        &models,
        SCIFACT,
        HYBRID_RERANK,
        &store_path("dense"),
    );
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

/// The graded half of the calibration: NFCorpus, whose qrels are graded, so the
/// figure it reproduces is one a linear-versus-exponential nDCG gain bug would
/// move (§ 9.8, ADR-10). Dense-only: the exit criterion is SciFact's, and a
/// hybrid case here would buy nothing this does not already say.
///
/// Sequential with the SciFact test above — `just calibrate` passes
/// `--test-threads=1` — for the same reason that one is sequential within
/// itself: each evaluation embeds the corpus once, and two in parallel would
/// contend for every core and interleave their summaries.
#[test]
#[ignore = "needs BEIR NFCorpus and the exported embedder on disk; run with `just calibrate`"]
fn the_harness_reproduces_the_published_nfcorpus_figure_over_graded_qrels() {
    let (datasets, models) = material();

    // The reproduction (ADR-10), twice: P4 says identical inputs are one run.
    let dense = bench(
        &datasets,
        &models,
        NFCORPUS,
        NFCORPUS_DENSE_ONLY,
        &store("nfcorpus"),
    );
    let again = bench(
        &datasets,
        &models,
        NFCORPUS,
        NFCORPUS_DENSE_ONLY,
        &store("nfcorpus-again"),
    );
    assert_eq!(dense.id, again.id, "two run ids for one input");
    assert_eq!(dense.inputs, again.inputs);
    assert_eq!(
        dense.metrics, again.metrics,
        "the same evaluation scored differently"
    );

    // Over the recorded material, and nothing else.
    assert_eq!(
        dense.inputs.dataset_version, RECORDED_NFCORPUS_DATASET,
        "not the recorded NFCorpus"
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
        (ndcg - NFCORPUS_PUBLISHED_NDCG).abs() <= PUBLISHED_TOLERANCE,
        "dense-only {NDCG} {ndcg:.5} is {:.2} points from the published \
         {NFCORPUS_PUBLISHED_NDCG}: see the diagnostic table in \
         `docs/system-architecture.md` § 9.8",
        (ndcg - NFCORPUS_PUBLISHED_NDCG).abs() * 100.0
    );
    assert_recorded(&dense, &RECORDED_NFCORPUS_DENSE, "nfcorpus dense-only");
}

/// The store a name maps to, without emptying it — for reading back what a
/// `bench` in this test already wrote there.
fn store_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("calibration")
        .join(name)
}
