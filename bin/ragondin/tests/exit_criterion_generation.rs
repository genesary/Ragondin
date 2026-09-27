//! The M3 exit criterion, as a test: **an end-to-end pipeline whose retrieval
//! is hybrid with reranking answers more questions correctly than the same
//! pipeline over dense-only retrieval, reproducibly** — measured by
//! `exact_match` and `token_f1` against reference answers. It is the
//! milestone's definition of done, mechanized as `exit_criterion.rs` mechanizes
//! M2's, and it is that file's mirror: every choice recorded there — why the
//! dataset is curated, why the ablations, what runs under which features —
//! applies here, and only what generation adds is argued below.
//!
//! Everything runs through the built binary: `ragondin bench` for each
//! configuration in `fixtures/exit-criterion/generation/`, then `ragondin
//! compare` over the two runs it stored. The path is the real one (P1): the
//! same `EngineContext`, planner, executor and harness a serving driver would
//! use; the real retrieval components M2's criterion runs — BM25, the ONNX
//! embedder over the in-memory store, RRF, the ONNX cross-encoder; the real
//! concatenating context builder; and the real `Remote` generator adapter,
//! bound with `--remote generator/answerer=http://127.0.0.1:<port>` (ADR-C32
//! § 2) to a `tonic` server this process starts on an ephemeral loopback port.
//! That server hosts `ragondin-stub`'s `StubGenerator` through
//! `ragondin-remote`'s conversions (`support/remote.rs`), as a Rust-hosted
//! `Remote` service would — so the wire is on the path, and no model is: no
//! LLM, no inference server, no network beyond loopback.
//!
//! # The fixture is M2's, with reference answers, and the gap is by construction
//!
//! The corpus, the queries, the qrels and the two models are M2's curated
//! exit-criterion dataset, untouched (ADR-C30 § 4). Beside its BEIR files sits
//! one addition, `dataset/answers.jsonl`: a hand-written reference answer per
//! query. The generation runs read the directory as `beir-qa/dataset`, which
//! requires that file; M2's runs still read it as `beir/dataset`, which ignores
//! it, so they are the runs they always were.
//!
//! The stub generator answers with **the first line of the context its
//! template places**, trimmed. The context builder is `concat` with the
//! separator `"\n"`, and no passage of the corpus holds a newline or carries a
//! BEIR title (which the adapter would put in front of the text), so the first
//! line of every context is exactly the text of the passage the pipeline
//! ranked first. Each reference answer is therefore the text of that query's
//! one relevant passage — the answer the stub gives over the correct top
//! passage — and a pipeline answers a question exactly when its retrieval puts
//! the right passage first.
//!
//! That makes the gap M2's, read at the first rank: `models/generate.py`
//! designs each query to defeat a different retrieval stage, and dense-only
//! ranks a distractor first on three of the five — `q-cat`, `q-greek`,
//! `q-river` — so it answers two, where hybrid with reranking answers all
//! five. Its wrong answers still share words with the references (`the dog sat
//! on the mat` for `the cat sat on the mat`), so `token_f1` is partial where
//! `exact_match` is zero, and the two metrics move separately. The *numbers*
//! are real — exact match and F1 over what each pipeline actually answered,
//! through the real engine and over the wire — and so is the path; only the
//! models are toys, which is the condition the issue sets.
//!
//! # The ablations are M2's, carried through to an answer
//!
//! The generation ablations remove each retrieval leg and the reranker in
//! turn, exactly as M2's do, and require each removal to cost a question. The
//! context builder cannot be removed — a generator takes a context — and its
//! budget is not ablated: the stub reads only the first line, which is the top
//! passage's whatever the budget lets in after it, so no budget large enough
//! to hold the top passage changes an answer on this fixture, and an ablation
//! that cannot lose a question would prove nothing. The reranker ablation is
//! the one that shows a stage decides answers rather than only a ranking.
//!
//! # What runs when
//!
//! Every test needs `bm25` and `onnx`, for the retrieval components, and
//! `remote`, for the generator adapter and the fake service it binds, so this
//! file is compiled only under all three: `just test-features` (and CI's
//! `--all-features` run) is where it runs, and `just test` compiles it away.

#![cfg(all(feature = "bm25", feature = "onnx", feature = "remote"))]

#[path = "support/remote.rs"]
mod remote;

use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use ragondin_experiments::{FileSystemRunStore, Run, RunId};

/// The baseline: dense-only retrieval, then the context builder and the
/// generator.
const DENSE_ONLY: &str = "generation/dense-only.yaml";
/// The challenger: hybrid retrieval with reranking, then the same context
/// builder and generator.
const HYBRID_RERANK: &str = "generation/hybrid-rerank.yaml";
/// M2's retrieval-only configurations over the same retrieval stages, which
/// score the ranking the generation runs' retrieval metrics must read.
const RETRIEVAL_ONLY: [(&str, &str); 2] = [
    (DENSE_ONLY, "dense-only.yaml"),
    (HYBRID_RERANK, "hybrid-rerank.yaml"),
];
/// The hybrid generation pipeline with one retrieval stage removed, each: a
/// leg, or the reranker — M2's four ablations, each ending in the same context
/// builder and generator.
const ABLATIONS: [&str; 4] = [
    "generation/ablations/bm25-only.yaml",
    "generation/ablations/bm25-rerank.yaml",
    "generation/ablations/dense-rerank.yaml",
    "generation/ablations/fused-no-rerank.yaml",
];

/// The metrics the criterion is stated in (ADR-C30 § 1).
const EXACT_MATCH: &str = "exact_match";
const TOKEN_F1: &str = "token_f1";
/// The retrieval metric M2's criterion is stated in, still reported beside
/// them.
const NDCG: &str = "ndcg@10";

/// The `impl:` name every generation configuration gives its generator: an
/// ordinary name, bound to the fake service on the command line and nowhere
/// in the configuration (ADR-C32 § 1).
const GENERATOR_NAME: &str = "answerer";

/// The fixture directory: the configurations, the dataset, the models.
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
        .join("exit-criterion-generation")
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

/// Runs `bench` with `extra` arguments and returns the run as the store holds
/// it — the record, not the summary.
fn bench_run(config: &str, benchmark: &str, store: &Path, extra: &[&str]) -> Run {
    let datasets = fixtures();
    let mut arguments = vec![
        "bench",
        config,
        "--benchmark",
        benchmark,
        "--datasets",
        path(&datasets),
        "--store",
        path(store),
    ];
    arguments.extend_from_slice(extra);
    let output = ragondin(&arguments);
    assert!(
        output.status.success(),
        "`bench {config}` over {benchmark} failed: {}",
        stderr(&output)
    );

    FileSystemRunStore::new(store)
        .load(&reported_run_id(&stdout(&output)))
        .expect("the run bench reported is the run bench saved")
}

/// Evaluates a generation configuration over the fixture with its reference
/// answers, its generator bound to `generator`.
fn bench(config: &str, store: &Path, generator: &remote::Service) -> Run {
    let binding = format!("generator/{GENERATOR_NAME}={}", generator.uri);
    bench_run(config, "beir-qa/dataset", store, &["--remote", &binding])
}

fn metric(run: &Run, name: &str) -> f64 {
    run.metrics
        .get(name)
        .unwrap_or_else(|| panic!("the run scores {name}: {:?}", run.metrics))
}

#[test]
fn hybrid_retrieval_with_reranking_answers_more_questions_than_dense_only() {
    let store = store("quality");
    let generator = remote::serve_generator();

    let dense = bench(DENSE_ONLY, &store, &generator);
    let hybrid = bench(HYBRID_RERANK, &store, &generator);

    for name in [EXACT_MATCH, TOKEN_F1] {
        // The baseline has to have answered something: a generator whose
        // answers never came back would score zero and lose to anything, and
        // the criterion would be met by a broken wire.
        assert!(
            metric(&dense, name) > 0.0,
            "dense-only answered nothing: {:?}",
            dense.metrics
        );
        assert!(
            metric(&hybrid, name) > metric(&dense, name),
            "hybrid+rerank scored {name} {:.4}, dense-only {:.4}",
            metric(&hybrid, name),
            metric(&dense, name)
        );
    }
}

#[test]
fn the_same_configuration_evaluated_twice_is_the_same_run() {
    // P4: a run is named by the content of its inputs, so identical inputs are
    // one run — the same id and, the path being deterministic, the same
    // numbers. Two stores, so the second evaluation is a second evaluation and
    // not a store declining to overwrite the first; and two services on two
    // ports, since where a component is answered from is provenance, outside
    // a run's identity (ADR-C32 § 2).
    for config in [DENSE_ONLY, HYBRID_RERANK] {
        let first_service = remote::serve_generator();
        let second_service = remote::serve_generator();
        let first = bench(config, &store(&format!("first-{config}")), &first_service);
        let second = bench(config, &store(&format!("second-{config}")), &second_service);

        assert_eq!(first.id, second.id, "{config}: two run ids for one input");
        assert_eq!(first.inputs, second.inputs, "{config}");
        assert_eq!(first.metrics, second.metrics, "{config}");
        assert_ne!(first.bindings, second.bindings, "{config}");
    }
}

#[test]
fn compare_reports_the_hybrid_win_on_exact_match() {
    let store = store("compare");
    let generator = remote::serve_generator();
    let dense = bench(DENSE_ONLY, &store, &generator);
    let hybrid = bench(HYBRID_RERANK, &store, &generator);

    let output = ragondin(&[
        "compare",
        &dense.id.to_string(),
        &hybrid.id.to_string(),
        "--store",
        path(&store),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    // `compare` names the side that scored higher, never the side that is
    // better — it knows no metric's direction — so the test supplies it: exact
    // match goes up, and the hybrid run is the right-hand one.
    let report = stdout(&output);
    let line = report
        .lines()
        .find(|line| line.starts_with(&format!("{EXACT_MATCH}:")))
        .unwrap_or_else(|| panic!("compare reports {EXACT_MATCH}:\n{report}"));
    assert!(
        line.contains("-> right +"),
        "the hybrid run is the right-hand one and must score higher:\n{report}"
    );
}

#[test]
fn retrieval_metrics_are_still_reported_and_read_the_ranking_behind_the_answer() {
    // The fixture carries qrels as well as answers, so a generation run
    // reports both families (ADR-8), and its retrieval metrics read the
    // ranking that fed the context builder, found by port position from the
    // generator (ADR-C30 § 3). That ranking is the one M2's retrieval-only
    // configuration ends in, so the two runs must score it identically — the
    // retrieval win beside the answer win is M2's, read through the walk.
    let store = store("retrieval");
    let generator = remote::serve_generator();

    for (generation, retrieval) in RETRIEVAL_ONLY {
        let answered = bench(generation, &store, &generator);
        let ranked = bench_run(retrieval, "beir/dataset", &store, &[]);

        assert_eq!(
            metric(&answered, NDCG),
            metric(&ranked, NDCG),
            "{generation} reads another ranking than {retrieval} returns"
        );
        for name in [EXACT_MATCH, TOKEN_F1] {
            metric(&answered, name);
        }
    }
}

#[test]
fn every_retrieval_stage_of_the_hybrid_pipeline_decides_an_answer() {
    // M2's tripwire, carried through to an answer: each leg and the reranker
    // removed in turn has to cost a question. A fixture on which a pipeline
    // without the reranker already answered everything would meet the
    // criterion while proving nothing about the stage it left out.
    let store = store("ablations");
    let generator = remote::serve_generator();
    let hybrid = bench(HYBRID_RERANK, &store, &generator);

    for ablation in ABLATIONS {
        let without = bench(ablation, &store, &generator);
        assert!(
            metric(&without, EXACT_MATCH) < metric(&hybrid, EXACT_MATCH),
            "{ablation} scored {EXACT_MATCH} {:.4}, the whole pipeline {:.4}: the stage \
             it removes decides no answer on this fixture",
            metric(&without, EXACT_MATCH),
            metric(&hybrid, EXACT_MATCH)
        );
    }
}
