//! The generation regime, end to end over stubs (ADR-8, ADR-C30).
//!
//! The regime follows the pieces the benchmark carries: qrels score the
//! retrieval metrics, reference answers score `exact_match` and `token_f1`, and
//! a benchmark carrying both scores both. Each family averages over its own
//! judged set. Once a pipeline ends in an answer, the retrieval metrics read
//! the ranking that fed its context builder (ADR-C30 § 3), and the answer is
//! read from the generator's entry in the trace — the place a stored run's
//! `traces.json` holds it (ADR-C31 § 5).
//!
//! The benchmark is written in this file, and every expected number is derived
//! by hand below from what the stubs fabricate. **No number here measures
//! retrieval or generation.**

use std::path::PathBuf;

use ragondin_benchmarks::{Benchmark, Qrels, ReferenceAnswers};
use ragondin_config::{ConfigSource, LocalFile};
use ragondin_engine::EngineContext;
use ragondin_experiments::{ConfigDocument, Run};
use ragondin_harness::{evaluate, CorpusIndex, Evaluation, HarnessError};
use ragondin_pipeline::{LogicalPipeline, ParamValue};
use ragondin_stub::{StubContextBuilder, StubFusion, StubGenerator, StubRetriever};
use ragondin_types::{DocId, Document, Query, QueryId};

/// The model name the stub generator is constructed to serve, and the one
/// `stub-generation.yaml` asks for.
const SERVED: &str = "stub-model";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A composition root's wiring, through the public `register_*` API only.
fn stub_context() -> EngineContext {
    let mut ctx = EngineContext::new();
    ctx.register_retriever(
        "stub_retriever",
        Box::new(|config| {
            let label = match config.get("label") {
                Some(ParamValue::String(label)) => label.clone(),
                other => return Err(format!("`label` must be a string, found {other:?}").into()),
            };
            Ok(Box::new(StubRetriever::new(label)))
        }),
    );
    ctx.register_fusion("stub_interleave", Box::new(|_| Ok(Box::new(StubFusion))));
    ctx.register_context_builder(
        "stub_context_builder",
        Box::new(|_| Ok(Box::new(StubContextBuilder))),
    );
    ctx.register_generator(
        "stub_generator",
        Box::new(|_| Ok(Box::new(StubGenerator::new(SERVED)))),
    );
    ctx
}

async fn pipeline(name: &str) -> (LogicalPipeline, ConfigDocument) {
    let path = fixture(name);
    let pipeline = LocalFile::new(&path)
        .load()
        .await
        .expect("the checked-in fixture is a valid configuration");
    let text = std::fs::read_to_string(&path).expect("the fixture is readable");
    (pipeline, ConfigDocument::new(text))
}

fn query(id: &str, text: &str) -> Query {
    Query {
        id: QueryId::new(id),
        text: text.to_string(),
    }
}

fn document(id: &str) -> Document {
    Document {
        id: DocId::new(id),
        text: format!("the text of {id}"),
        metadata: Default::default(),
    }
}

/// Which pieces [`benchmark`] is built with.
#[derive(Clone, Copy)]
struct Pieces {
    qrels: bool,
    references: bool,
}

/// Four queries, each carrying a different combination of the two pieces.
///
/// Every query retrieves the same fused ranking, `[doc-a, doc-b]`, and every
/// answer is the first line of a context holding `doc-a`'s chunk alone:
/// "stub chunk 0 of `doc-a` for `<query text>`", which normalizes to the seven
/// tokens `stub chunk 0 of doca for <query text>` (backticks and the hyphen
/// are ASCII punctuation, removed before articles).
///
/// | query | qrels | references |
/// |---|---|---|
/// | `q-1` "alpha" | `doc-b` at 1 | the answer itself, spelled differently |
/// | `q-2` "beta" | `doc-a` at 1 | "chunk zero" |
/// | `q-3` "gamma" | — | "gamma" |
/// | `q-4` "delta" | `doc-c` at 1 | — |
fn benchmark(pieces: Pieces) -> Benchmark {
    let mut qrels = Qrels::new();
    if pieces.qrels {
        qrels.insert(QueryId::new("q-1"), DocId::new("doc-b"), 1);
        qrels.insert(QueryId::new("q-2"), DocId::new("doc-a"), 1);
        qrels.insert(QueryId::new("q-4"), DocId::new("doc-c"), 1);
    }
    let mut references = ReferenceAnswers::new();
    if pieces.references {
        references.insert(
            QueryId::new("q-1"),
            vec![
                "no match at all".to_string(),
                "Stub chunk 0 of doc-a, for alpha.".to_string(),
            ],
        );
        references.insert(QueryId::new("q-2"), vec!["chunk zero".to_string()]);
        references.insert(QueryId::new("q-3"), vec!["gamma".to_string()]);
    }
    Benchmark::new(
        vec![document("doc-a"), document("doc-b"), document("doc-c")],
        vec![
            query("q-1", "alpha"),
            query("q-2", "beta"),
            query("q-3", "gamma"),
            query("q-4", "delta"),
        ],
        qrels,
    )
    .with_reference_answers(references)
}

async fn run(name: &str, benchmark: &Benchmark) -> Result<Run, HarnessError> {
    let (pipeline, config) = pipeline(name).await;
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
}

fn names(run: &Run) -> Vec<&str> {
    run.metrics.iter().map(|(name, _)| name).collect()
}

fn assert_metric(run: &Run, name: &str, expected: f64) {
    let value = run
        .metrics
        .get(name)
        .unwrap_or_else(|| panic!("`{name}` is reported: {:?}", names(run)));
    assert!(
        (value - expected).abs() < 1e-12,
        "`{name}` is {value}, expected {expected}"
    );
}

/// The retrieval means, over the three judged queries `q-1`, `q-2`, `q-4`.
///
/// Read over the fused ranking `[doc-a, doc-b]` — the node the context
/// builder's chunks port names — and not over the one-chunk context:
///
/// - `q-1` judges `doc-b`, ranked 2nd: nDCG@10 `1 / log2(3)`, recall 1, RR 1/2.
///   Over the context, which holds `doc-a` alone, every value would be 0.
/// - `q-2` judges `doc-a`, ranked 1st: every value 1.
/// - `q-4` judges `doc-c`, ranked nowhere: every value 0.
/// - `q-3` carries no qrels and is not in this denominator.
fn assert_retrieval(run: &Run) {
    assert_metric(run, "ndcg@10", (1.0 / f64::log2(3.0) + 1.0) / 3.0);
    assert_metric(run, "recall@10", 2.0 / 3.0);
    assert_metric(run, "mrr", (0.5 + 1.0) / 3.0);
}

/// The generation means, over the three queries with references `q-1`..`q-3`.
///
/// - `q-1`: the second reference normalizes to the answer exactly — EM 1,
///   F1 1 — and the maximum over references is taken.
/// - `q-2`: the answer's seven tokens against "chunk zero" share one token,
///   `chunk`: precision 1/7, recall 1/2, F1 `2·(1/7)(1/2)/(1/7+1/2) = 2/9`.
/// - `q-3`: against "gamma", one shared token: precision 1/7, recall 1, F1
///   `2·(1/7)/(8/7) = 1/4`.
/// - `q-4` carries no reference and is not in this denominator.
fn assert_generation(run: &Run) {
    assert_metric(run, "exact_match", 1.0 / 3.0);
    assert_metric(run, "token_f1", (1.0 + 2.0 / 9.0 + 1.0 / 4.0) / 3.0);
}

#[tokio::test]
async fn a_benchmark_with_qrels_and_references_scores_both_families_over_a_generation_pipeline() {
    let benchmark = benchmark(Pieces {
        qrels: true,
        references: true,
    });

    let run = run("stub-generation.yaml", &benchmark)
        .await
        .expect("the stub generation pipeline scores this benchmark");

    assert_retrieval(&run);
    assert_generation(&run);
    assert_eq!(
        names(&run),
        ["exact_match", "mrr", "ndcg@10", "recall@10", "token_f1"],
        "both families, by name, and nothing else"
    );
    assert_eq!(run.traces.len(), 4, "every query runs and leaves its trace");
}

#[tokio::test]
async fn a_benchmark_with_references_only_reports_the_generation_metrics_alone() {
    let benchmark = benchmark(Pieces {
        qrels: false,
        references: true,
    });

    let run = run("stub-generation.yaml", &benchmark)
        .await
        .expect("a generation-only benchmark is a valid one (ADR-C30 § 5)");

    assert_generation(&run);
    assert_eq!(names(&run), ["exact_match", "token_f1"]);
}

#[tokio::test]
async fn a_benchmark_with_qrels_only_reports_the_retrieval_metrics_alone_over_a_generation_pipeline(
) {
    let benchmark = benchmark(Pieces {
        qrels: true,
        references: false,
    });

    let run = run("stub-generation.yaml", &benchmark)
        .await
        .expect("a generation pipeline is scored on the ranking that fed it");

    assert_retrieval(&run);
    assert_eq!(names(&run), ["mrr", "ndcg@10", "recall@10"]);
}

#[tokio::test]
async fn a_pipeline_ending_in_a_context_builder_is_scored_on_its_chunks_port() {
    // ADR-C30 § 3: the walk is entered at the builder, and reads the fused
    // ranking — the same numbers as through the generator.
    let benchmark = benchmark(Pieces {
        qrels: true,
        references: false,
    });

    let run = run("stub-context.yaml", &benchmark)
        .await
        .expect("a context-terminal pipeline is scored on the ranking that fed it");

    assert_retrieval(&run);
    assert_eq!(names(&run), ["mrr", "ndcg@10", "recall@10"]);
}

#[tokio::test]
async fn reference_answers_run_through_a_pipeline_that_produces_no_answer_are_refused() {
    // ADR-C30 § 5: the benchmark asked for answers to be scored, and a run that
    // dropped that family would read as a smaller evaluation reported as the
    // whole one. Refused with a variant of its own, never `NothingToScore`.
    let benchmark = benchmark(Pieces {
        qrels: true,
        references: true,
    });

    for (name, expected) in [
        ("stub-over-beir-mini.yaml", "chunks"),
        ("stub-context.yaml", "context"),
    ] {
        let refused = run(name, &benchmark)
            .await
            .expect_err("a pipeline with no answer cannot be scored against references");

        let HarnessError::NoAnswer { query, trace, kind } = &refused else {
            panic!("{name}: expected NoAnswer, got {refused:?}")
        };
        assert_eq!(
            query,
            &QueryId::new("q-1"),
            "the first query, in benchmark order"
        );
        assert_eq!(
            *kind, expected,
            "{name}: the kind the pipeline produced instead"
        );
        assert!(
            !trace.as_value()["nodes"]
                .as_array()
                .expect("the trace lists its nodes")
                .is_empty(),
            "the refused query's trace travels with the error"
        );
        let message = refused.to_string();
        assert!(
            message.contains("`q-1`") && message.contains(expected),
            "the message names the query and the kind: {message}"
        );
    }
}

#[tokio::test]
async fn the_stored_trace_names_the_generators_answer_under_answer_text() {
    // What `traces.json` readers — the per-query fixture, `compare` — read:
    // the generator's output entry carries the answer text the run scored.
    let benchmark = benchmark(Pieces {
        qrels: true,
        references: true,
    });

    let run = run("stub-generation.yaml", &benchmark)
        .await
        .expect("the stub generation pipeline scores this benchmark");

    let nodes = run.traces[&QueryId::new("q-1")].as_value()["nodes"]
        .as_array()
        .expect("the trace lists its nodes")
        .clone();
    let generator = nodes
        .iter()
        .find(|node| node["node"] == "answer")
        .expect("the generator ran and is traced");
    assert_eq!(
        generator["output"],
        serde_json::json!({"answer": {"text": "stub chunk 0 of `doc-a` for `alpha`"}})
    );
}

#[tokio::test]
async fn reference_answers_move_the_dataset_version_and_so_the_run_id() {
    let with = run(
        "stub-generation.yaml",
        &benchmark(Pieces {
            qrels: true,
            references: true,
        }),
    )
    .await
    .expect("scored");
    let without = run(
        "stub-generation.yaml",
        &benchmark(Pieces {
            qrels: true,
            references: false,
        }),
    )
    .await
    .expect("scored");

    assert_ne!(
        with.inputs.dataset_version, without.inputs.dataset_version,
        "two benchmarks differing only in their references are two datasets (ADR-C30 § 5)"
    );
    assert_ne!(with.id, without.id);
}
