//! The evaluation driver, end to end over the miniature BEIR fixture.
//!
//! This is the test the crate exists to pass: a benchmark on disk, an
//! `EngineContext` wired from `ragondin-stub` exactly as a composition root
//! wires one, the **same** `ragondin-engine` every serving path will use, and a
//! `Run` on the far side carrying metrics and one trace per query.
//!
//! The components are stubs, so **no number here is a measurement of
//! retrieval**. What the assertions pin is the *wiring*: that every query
//! reaches the engine, that the ranking the executor returns is scored against
//! the fixture's qrels, and that the run is named by the content-addressed
//! tuple (P4).
//!
//! The benchmark fixture is `ragondin-benchmarks`' own, reached by a relative
//! path rather than copied: one miniature BEIR dataset in the repository is one
//! set of expected numbers to keep true.

use std::path::PathBuf;

use ragondin_benchmarks::{BeirAdapter, Benchmark, BenchmarkAdapter};
use ragondin_config::{ConfigSource, LocalFile};
use ragondin_engine::EngineContext;
use ragondin_experiments::{ConfigDocument, FileSystemRunStore, Run};
use ragondin_harness::{evaluate, CorpusIndex, Evaluation, HarnessError};
use ragondin_pipeline::{LogicalPipeline, ParamValue};
use ragondin_stub::{StubFusion, StubRetriever};
use ragondin_types::{DocId, Document, QueryId};

/// The `ragondin-benchmarks` fixture, reached from this crate's manifest
/// directory. It is that crate's test data and stays there: a second copy would
/// be a second dataset to keep in step with these expectations.
fn beir_mini() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ragondin-benchmarks/tests/fixtures/beir-mini")
}

fn pipeline_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stub-over-beir-mini.yaml")
}

fn benchmark() -> Benchmark {
    BeirAdapter::new(beir_mini())
        .load()
        .expect("the checked-in BEIR fixture loads")
}

/// The composition root's job, done here because a test is one: the stubs are
/// registered through the public `register_*` API, with the constructor reading
/// the node's configuration out of `Params` (INV-7 — nothing a third-party
/// crate could not write).
fn stub_context() -> EngineContext {
    let mut ctx = EngineContext::new();
    ctx.register_retriever(
        "stub_retriever",
        Box::new(|config| {
            let label = match config.get("label") {
                Some(ParamValue::String(label)) => label.clone(),
                Some(other) => {
                    return Err(format!("`label` must be a string, found {other:?}").into())
                }
                None => return Err("`label` is required".into()),
            };
            Ok(Box::new(StubRetriever::new(label)))
        }),
    );
    ctx.register_fusion("stub_interleave", Box::new(|_| Ok(Box::new(StubFusion))));
    ctx
}

async fn logical() -> (LogicalPipeline, ConfigDocument) {
    let pipeline = LocalFile::new(pipeline_file())
        .load()
        .await
        .expect("the checked-in fixture is a valid configuration");
    // The document is kept as it was written rather than re-serialized from the
    // pipeline: the store records the text whose canonical logical form hashes
    // to `RunInputs::pipeline`, never a second spelling of it.
    let text = std::fs::read_to_string(pipeline_file()).expect("the fixture is readable");
    (pipeline, ConfigDocument::new(text))
}

/// Runs the whole driver once.
///
/// Builds the `CorpusIndex` from `benchmark`'s own corpus, exactly as a
/// composition root that constructs its components from the same benchmark
/// would: this is the ordinary case, not the mismatched one under test in
/// [`the_recorded_index_version_follows_the_index_argument_not_the_benchmark`].
async fn run_the_harness(benchmark: &Benchmark) -> Run {
    let (pipeline, config) = logical().await;
    let index = CorpusIndex::build(benchmark.corpus());
    evaluate(
        &Evaluation {
            pipeline: &pipeline,
            config: &config,
            benchmark,
            index: &index,
            cutoff: 10,
            model_hashes: Default::default(),
        },
        &stub_context(),
    )
    .await
    .expect("the stub pipeline cannot fail on this benchmark")
}

/// What the fixture's three evaluated queries score, derived by hand.
///
/// Both legs fabricate every answer, so every query retrieves the same
/// document ranking: `leg_a` is labelled `MED-10` and `leg_b` `4983`, the
/// fusion interleaves them, and the chunk list collapses to the document list
/// `[MED-10, 4983]`.
///
/// - `q-1` judges `MED-10` at grade 2 and `MED-12` at 0 — a hit at rank 1, so
///   nDCG@10, recall@10 and the reciprocal rank are all `1`.
/// - `q-2` judges `4983` at grade 1 — a hit at rank 2: nDCG@10 is
///   `1 / log2(3)`, recall@10 is `1`, the reciprocal rank `1/2`.
/// - `0042` judges `0042`, which neither leg names — every score `0`.
///
/// The mean is over the three queries the run executed (the fourth, `q-3`, is
/// judged only in the `train` split and the adapter leaves it out).
const QUERIES: f64 = 3.0;

fn ndcg_expected() -> f64 {
    (1.0 + 1.0 / f64::log2(3.0)) / QUERIES
}

#[tokio::test]
async fn the_harness_scores_the_fixture_benchmark_and_returns_a_run() {
    let run = run_the_harness(&benchmark()).await;

    assert!(
        (run.metrics.get("ndcg@10").expect("nDCG is computed") - ndcg_expected()).abs() < 1e-12,
        "nDCG@10 is the mean over the three evaluated queries: {:?}",
        run.metrics.get("ndcg@10")
    );
    assert!(
        (run.metrics.get("recall@10").expect("recall is computed") - 2.0 / QUERIES).abs() < 1e-12,
        "recall@10: two of the three queries retrieve their one relevant document"
    );
    assert!(
        (run.metrics.get("mrr").expect("MRR is computed") - (1.0 + 0.5) / QUERIES).abs() < 1e-12,
        "MRR: rank 1, rank 2, and a miss"
    );
}

#[tokio::test]
async fn every_executed_query_leaves_its_own_trace_in_the_run() {
    let run = run_the_harness(&benchmark()).await;

    assert_eq!(
        run.traces.keys().cloned().collect::<Vec<_>>(),
        [
            QueryId::new("0042"),
            QueryId::new("q-1"),
            QueryId::new("q-2")
        ],
        "one trace per evaluated query, filed under the query's id"
    );

    // The trace is what the executor returned (INV-10), rendered — node by
    // node, in the order the executor ran them.
    let trace = run.traces[&QueryId::new("q-1")].as_value();
    let nodes = trace["nodes"]
        .as_array()
        .expect("the trace lists its nodes");
    assert_eq!(
        nodes
            .iter()
            .map(|node| node["node"].as_str().expect("a node is named"))
            .collect::<Vec<_>>(),
        ["leg_a", "leg_b", "fused"],
        "execution order, not the canonical order the plan stores"
    );
    assert_eq!(nodes[2]["output"]["chunks"]["count"], 3);
}

#[tokio::test]
async fn the_run_is_named_by_the_tuple_of_its_inputs() {
    let benchmark = benchmark();
    let run = run_the_harness(&benchmark).await;
    let (pipeline, _) = logical().await;

    assert_eq!(
        run.inputs.pipeline,
        pipeline.content_hash(),
        "the pipeline component of identity is the canonical logical form's hash (INV-8)"
    );
    assert_eq!(run.inputs.engine_version, env!("CARGO_PKG_VERSION"));
    assert!(
        run.inputs.model_hashes.is_empty(),
        "the stubs read no model"
    );
    assert!(!run.inputs.dataset_version.is_empty());
    assert!(!run.inputs.index_version.is_empty());
}

#[tokio::test]
async fn two_runs_over_identical_inputs_agree_on_the_id_and_on_every_metric() {
    // P4: the run is content-addressed, so identical inputs are one run.
    //
    // The benchmark is **loaded twice, from disk**, rather than shared between
    // the two runs. `dataset_version` is a digest over the *loaded* benchmark,
    // so a loader that turned one directory into two unequal values — a map
    // whose iteration order varies leaking into the corpus metadata, say —
    // would give one dataset two versions, and a shared value would never see
    // it.
    let first = run_the_harness(&benchmark()).await;
    let second = run_the_harness(&benchmark()).await;

    assert_eq!(
        first.inputs.dataset_version, second.inputs.dataset_version,
        "one directory, loaded twice, is one dataset version"
    );
    assert_eq!(first.id, second.id);
    assert_eq!(first.inputs, second.inputs);
    assert_eq!(
        first.metrics.iter().collect::<Vec<_>>(),
        second.metrics.iter().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn the_run_the_harness_assembles_is_one_the_store_accepts() {
    // The harness returns the record and the caller stores it; this is that
    // call, and it is also what proves no metric arrives non-finite, which the
    // store refuses at its boundary.
    let run = run_the_harness(&benchmark()).await;
    let store = FileSystemRunStore::new(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("runs"));

    store.save(&run).expect("the assembled run is storable");
    let reloaded = store.load(&run.id).expect("it reads back under its own id");

    assert_eq!(reloaded.inputs, run.inputs);
    assert_eq!(
        reloaded.metrics.iter().collect::<Vec<_>>(),
        run.metrics.iter().collect::<Vec<_>>()
    );
    assert_eq!(reloaded.config, run.config);
}

#[tokio::test]
async fn a_query_the_pipeline_cannot_answer_stops_the_run_and_reports_its_trace() {
    // Metrics averaged over the queries that happened to succeed would report a
    // smaller benchmark as the whole one, so the run stops. The trace of the
    // failing query travels with the error: INV-10's reason for pairing the
    // trace with the result is that a failed run's trace is the one worth
    // reading.
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/refusing-pipeline.yaml");
    let pipeline = LocalFile::new(&path)
        .load()
        .await
        .expect("the fixture is a valid configuration");
    let config = ConfigDocument::new(std::fs::read_to_string(&path).expect("readable"));
    let benchmark = benchmark();
    let index = CorpusIndex::build(benchmark.corpus());

    let failed = evaluate(
        &Evaluation {
            pipeline: &pipeline,
            config: &config,
            benchmark: &benchmark,
            index: &index,
            cutoff: 10,
            model_hashes: Default::default(),
        },
        &stub_context(),
    )
    .await
    .expect_err("`top_k: 0` asks the stub for a result that cannot exist");

    let HarnessError::Execute { query, trace, .. } = failed else {
        panic!("the failure is the query's, not the plan's: {failed}")
    };
    assert_eq!(
        query,
        QueryId::new("q-1"),
        "the run stops at the first query it executes, in the benchmark's own \
         order — file order, not the sorted order the traces are filed in"
    );
    assert_eq!(
        trace.as_value()["nodes"][0]["node"],
        "leg",
        "the failing node is in the trace the error carries"
    );
    assert_eq!(
        trace.as_value()["nodes"][0]["output"],
        serde_json::Value::Null
    );
}

#[tokio::test]
async fn a_benchmark_with_nothing_judged_is_refused_rather_than_scored() {
    // A mean over no query is undefined, not zero. Reporting it as `NaN` would
    // be caught two crates away, by the run store refusing to write one.
    let (pipeline, config) = logical().await;
    let unjudged = Benchmark::new(
        Vec::new(),
        vec![ragondin_types::Query {
            id: QueryId::new("q-1"),
            text: "a question nobody judged".to_string(),
        }],
        ragondin_benchmarks::Qrels::new(),
    );
    let index = CorpusIndex::build(unjudged.corpus());

    let refused = evaluate(
        &Evaluation {
            pipeline: &pipeline,
            config: &config,
            benchmark: &unjudged,
            index: &index,
            cutoff: 10,
            model_hashes: Default::default(),
        },
        &stub_context(),
    )
    .await
    .expect_err("nothing is judged, so nothing can be scored");

    assert!(matches!(refused, HarnessError::NothingToScore));
}

#[tokio::test]
async fn the_recorded_index_version_follows_the_index_argument_not_the_benchmark() {
    // ADR-C26: `index_version` names the `CorpusIndex` the caller passed, not
    // one `evaluate` derives from the benchmark itself. A composition root
    // that built its components from a chunk set other than the benchmark's
    // own — the mismatch the decision cannot prevent, only make visible — must
    // still see *that* chunk set's version recorded, not the benchmark's.
    let (pipeline, config) = logical().await;
    let benchmark = benchmark();
    let foreign_index = CorpusIndex::build(&[Document {
        id: DocId::new("foreign-doc"),
        text: "a chunk set the benchmark never named".to_string(),
        metadata: Default::default(),
    }]);
    assert_ne!(
        foreign_index.version(),
        CorpusIndex::build(benchmark.corpus()).version(),
        "the fixture setup is meaningless unless the two chunk sets differ"
    );

    let run = evaluate(
        &Evaluation {
            pipeline: &pipeline,
            config: &config,
            benchmark: &benchmark,
            index: &foreign_index,
            cutoff: 10,
            model_hashes: Default::default(),
        },
        &stub_context(),
    )
    .await
    .expect("the stub pipeline cannot fail on this benchmark");

    assert_eq!(
        run.inputs.index_version,
        foreign_index.version(),
        "index_version follows the index the caller passed, not one `evaluate` \
         derives from the benchmark"
    );
}
