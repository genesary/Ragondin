//! The M2 exit criterion, as a test: **hybrid retrieval with reranking beats
//! dense-only retrieval on a BEIR-shaped benchmark, reproducibly.** It is the
//! milestone's definition of done, mechanized so that "v0 is done" is a green
//! test rather than a judgment.
//!
//! Everything here runs through the built binary: `ragondin bench` for each of
//! the two configurations in `fixtures/exit-criterion/`, then `ragondin
//! compare` over the two runs it stored. The path is the real one (P1) — the
//! same `EngineContext`, planner, executor and harness a serving driver would
//! use, over the real components: BM25, the ONNX embedder over the in-memory
//! vector store, RRF and the ONNX cross-encoder. No stub, no re-implementation.
//!
//! # The dataset is curated, and the gap is by construction
//!
//! A real BEIR subset under a real embedder would make the hybrid-over-dense
//! gap a property of two trained models, and neither fits a test that has to be
//! fast, offline and deterministic. So the corpus, the queries and the two
//! models were built together — `fixtures/exit-criterion/models/generate.py`
//! says how. The fixture embedder places one distractor per query on the
//! query's own direction, so dense retrieval ranks it above the judged answer;
//! BM25 and the fixture cross-encoder score lexical overlap, and the answer is
//! what overlaps. The *numbers* are real — nDCG over what each pipeline
//! actually returned — and so is the path; only the models are toys. The issue
//! that defines this test allows exactly that, on the condition that the
//! pipeline path is the real one, and that condition is what every test below
//! drives.
//!
//! # What runs when
//!
//! Every test needs `bm25` and `onnx` both, so this file is compiled only under
//! both features: `just test-features` (and CI's `--all-features` run) is
//! where it runs, and `just test` compiles it away.

#![cfg(all(feature = "bm25", feature = "onnx"))]

use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use ragondin_experiments::{FileSystemRunStore, Run, RunId};

/// The baseline: the dense retriever alone.
const DENSE_ONLY: &str = "dense-only.yaml";
/// The challenger: BM25 and dense, fused by RRF, reranked by the cross-encoder.
const HYBRID_RERANK: &str = "hybrid-rerank.yaml";
/// The metric the criterion is stated in: nDCG at the cutoff `bench` reports.
const NDCG: &str = "ndcg@10";

/// The fixture directory: the two configurations, the dataset, the models.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exit-criterion")
}

fn path(buf: &Path) -> &str {
    buf.to_str().expect("UTF-8 path")
}

/// A run store of this test's own, emptied first so a run left by a previous,
/// killed run of this test cannot decide this one.
fn store(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("exit-criterion")
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

/// Spawns the built binary **from the fixture directory**, so the model paths
/// the committed configurations name resolve without an absolute path in them.
fn ragondin(args: &[&str]) -> Output {
    Command::cargo_bin("ragondin")
        .expect("the binary under test is built by `cargo test`")
        .current_dir(fixtures())
        .args(args)
        .output()
        .expect("the binary runs")
}

/// The `run <id>` line `bench` prints first.
fn reported_run_id(summary: &str) -> RunId {
    let first = summary.lines().next().expect("a summary has a first line");
    let id = first
        .strip_prefix("run ")
        .unwrap_or_else(|| panic!("the summary opens with the run id: {summary}"));
    id.parse().expect("the printed id is a run id")
}

/// Evaluates one configuration over the fixture dataset into `store`, and
/// returns the run as the store holds it — the record, not the summary.
fn bench(config: &str, store: &Path) -> Run {
    let output = ragondin(&[
        "bench",
        config,
        "--benchmark",
        "beir/dataset",
        "--datasets",
        path(&fixtures()),
        "--store",
        path(store),
    ]);
    assert!(
        output.status.success(),
        "`bench {config}` failed: {}",
        stderr(&output)
    );

    FileSystemRunStore::new(store)
        .load(&reported_run_id(&stdout(&output)))
        .expect("the run bench reported is the run bench saved")
}

fn ndcg(run: &Run) -> f64 {
    run.metrics
        .get(NDCG)
        .expect("a retrieval run scores nDCG at the cutoff")
}

#[test]
fn hybrid_retrieval_with_reranking_beats_dense_only() {
    let store = store("quality");

    let dense = bench(DENSE_ONLY, &store);
    let hybrid = bench(HYBRID_RERANK, &store);

    // The baseline has to have found something: a dense leg that retrieved
    // nothing would score zero and lose to anything, and the criterion would
    // be met by a broken embedder.
    assert!(
        ndcg(&dense) > 0.0,
        "dense-only retrieved nothing: {:?}",
        dense.metrics
    );
    assert!(
        ndcg(&hybrid) > ndcg(&dense),
        "hybrid+rerank scored {NDCG} {:.4}, dense-only {:.4}",
        ndcg(&hybrid),
        ndcg(&dense)
    );
}

#[test]
fn the_same_configuration_evaluated_twice_is_the_same_run() {
    // P4: a run is named by the content of its inputs, so identical inputs
    // are one run — the same id, and, the path being deterministic, the same
    // numbers. Two stores rather than one, so the second evaluation is a
    // second evaluation and not a store declining to overwrite the first.
    for config in [DENSE_ONLY, HYBRID_RERANK] {
        let first = bench(config, &store(&format!("first-{config}")));
        let second = bench(config, &store(&format!("second-{config}")));

        assert_eq!(first.id, second.id, "{config}: two run ids for one input");
        assert_eq!(first.inputs, second.inputs, "{config}");
        assert_eq!(first.metrics, second.metrics, "{config}");
    }
}

#[test]
fn compare_reports_the_hybrid_win() {
    let store = store("compare");
    let dense = bench(DENSE_ONLY, &store);
    let hybrid = bench(HYBRID_RERANK, &store);

    let output = ragondin(&[
        "compare",
        &dense.id.to_string(),
        &hybrid.id.to_string(),
        "--store",
        path(&store),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    // `compare` names the side that scored higher, never the side that is
    // better — it knows no metric's direction — so the test supplies it: nDCG
    // goes up, and the hybrid run is the right-hand one.
    let report = stdout(&output);
    let line = report
        .lines()
        .find(|line| line.starts_with(&format!("{NDCG}:")))
        .unwrap_or_else(|| panic!("compare reports {NDCG}:\n{report}"));
    assert!(
        line.contains("-> right +"),
        "the hybrid run is the right-hand one and must score higher:\n{report}"
    );
}
