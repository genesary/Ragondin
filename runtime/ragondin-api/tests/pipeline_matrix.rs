//! `GET /api/v1/pipelines/{name}/matrix`: one workspace pipeline's node ×
//! benchmark matrix over the runs of its current canonical form and of its
//! prefixes — the most recent run per benchmark, the gain per stage, and the
//! reason for every empty cell.

mod support;

use std::sync::Arc;

use axum::http::StatusCode;
use ragondin_api::Server;
use ragondin_benchmarks::identity::dataset_version;
use ragondin_benchmarks::{Benchmark, Qrels, ReferenceAnswers};
use ragondin_experiments::{
    lower_configuration, ConfigDocument, PrefixOf, Run, RunProvenance, RunTimes, Trace, TraceChunk,
    TraceSummary, UnixMillis,
};
use ragondin_pipeline::PipelineHash;
use ragondin_types::{DocId, Document, Query, QueryId};
use serde_json::Value;
use support::datasets::scratch;
use support::runs::{chunk, counted, node, query, ranked, run_over};
use support::{fakes, get, json, router_over, send, FakeRunStore, FixtureRegistry, HeldPipelines};

const NAME: &str = "hybrid-rerank-gen";

const HYBRID_RERANK_GEN: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
      params: { top_k: 50 }
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [bm25, dense]
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, rrf]
    - id: concat
      component: context_builder
      impl: concat
      inputs: [question, rerank]
    - id: generate
      component: generator
      impl: answerer
      inputs: [question, concat]
";

/// [`HYBRID_RERANK_GEN`] cut at `rerank`: "Run up to this node", or the same
/// document written by hand.
const UP_TO_RERANK: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
      params: { top_k: 50 }
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [bm25, dense]
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, rrf]
";

/// [`HYBRID_RERANK_GEN`] with another generator: a second pipeline that
/// [`UP_TO_RERANK`] is also a prefix of.
fn other_generator() -> String {
    HYBRID_RERANK_GEN.replace("impl: answerer", "impl: summarizer")
}

/// A benchmark of two judged queries, `<tag>1` and `<tag>2`, each with one
/// gold document `<tag>-g<n>`, and reference answers when `answers` is set.
/// The tag makes the corpora, and so the `dataset_version`s, differ.
fn benchmark(tag: &str, answers: bool) -> Benchmark {
    let document = |id: String| Document {
        text: format!("the text of {id}"),
        id: DocId::new(id),
        metadata: Default::default(),
    };
    let mut corpus: Vec<Document> = (1..=2).map(|n| document(format!("{tag}-g{n}"))).collect();
    corpus.extend((1..=3).map(|n| document(format!("{tag}-x{n}"))));
    let queries = (1..=2)
        .map(|n| Query {
            id: QueryId::new(format!("{tag}{n}")),
            text: format!("question {n}"),
        })
        .collect();
    let mut qrels = Qrels::new();
    for n in 1..=2 {
        qrels.insert(
            QueryId::new(format!("{tag}{n}")),
            DocId::new(format!("{tag}-g{n}")),
            1,
        );
    }
    let benchmark = Benchmark::new(corpus, queries, qrels);
    if !answers {
        return benchmark;
    }
    let mut references = ReferenceAnswers::new();
    for n in 1..=2 {
        references.insert(
            QueryId::new(format!("{tag}{n}")),
            vec![format!("answer {n}")],
        );
    }
    benchmark.with_reference_answers(references)
}

fn scifact() -> Benchmark {
    benchmark("sci", false)
}

fn squad() -> Benchmark {
    benchmark("sq", true)
}

fn nq() -> Benchmark {
    benchmark("nq", false)
}

/// A ranking of query `<tag><n>` with its gold document at `rank` behind
/// filler.
fn ranking(tag: &str, n: usize, rank: usize) -> Vec<TraceChunk> {
    let mut chunks: Vec<TraceChunk> = (1..rank)
        .map(|f| {
            let id = format!("{tag}-x{f}");
            chunk(&id, &id, 1.0 / f as f64)
        })
        .collect();
    let gold = format!("{tag}-g{n}");
    chunks.push(chunk(&gold, &gold, 0.01));
    chunks
}

/// Where each ranking node puts the gold document: every stage does better
/// than the one before, the dense leg better than bm25.
const RANKS: [(&str, usize); 4] = [("bm25", 3), ("dense", 2), ("rrf", 2), ("rerank", 1)];

/// One query's trace through the nodes `generating` says: the four ranking
/// nodes, then the context and the answer when it is set.
fn trace(tag: &str, n: usize, generating: bool) -> Trace {
    trace_through(tag, n, &RANKS, generating)
}

/// One query's trace through the ranking nodes `ranks` names, each putting
/// the gold document at its rank, then the context and the answer when
/// `generating` is set.
fn trace_through(tag: &str, n: usize, ranks: &[(&str, usize)], generating: bool) -> Trace {
    let q = format!("{tag}{n}");
    let mut nodes: Vec<_> = ranks
        .iter()
        .map(|(id, rank)| node(id, vec![query(&q)], ranked(ranking(tag, n, *rank)), 10))
        .collect();
    if generating {
        let context = ranking(tag, n, 1);
        nodes.push(node(
            "concat",
            vec![query(&q), counted(1)],
            TraceSummary::Context {
                chunks: context,
                text: "the context".to_owned(),
            },
            10,
        ));
        nodes.push(node(
            "generate",
            vec![query(&q)],
            TraceSummary::Answer {
                text: format!("answer {n}"),
            },
            10,
        ));
    }
    Trace { nodes }
}

/// A run of `config` over `benchmark` under `run_id(id)`, started at
/// `started` when it is known.
fn run(id: u8, config: &str, benchmark: &Benchmark, tag: &str, started: Option<u64>) -> Run {
    let generating = config.contains("component: generator");
    let traces: Vec<(String, Trace)> = (1..=2)
        .map(|n| (format!("{tag}{n}"), trace(tag, n, generating)))
        .collect();
    let mut metrics = vec![("ndcg@10", 0.9), ("mrr", 0.9)];
    if generating && !benchmark.reference_answers().is_empty() {
        metrics.extend([("exact_match", 0.5), ("token_f1", 0.75)]);
    }
    let mut run = run_over(
        id,
        config,
        benchmark,
        traces
            .iter()
            .map(|(q, t)| (q.as_str(), t.clone()))
            .collect(),
        &metrics,
    );
    run.times = started.map(|at| RunTimes::new(UnixMillis::new(at), UnixMillis::new(at + 1)));
    run
}

fn id(byte: u8) -> String {
    support::runs::run_id(byte).to_string()
}

/// The router over `runs`, the workspace `documents` and the benchmarks
/// `beir/scifact`, `squad/dev` and `beir/nq`, caching under `test`'s scratch
/// directory.
fn app(test: &str, documents: &[(&str, &str)], runs: Vec<Run>) -> Server {
    let mut backends = fakes(FakeRunStore::holding(runs));
    backends.pipelines = Arc::new(HeldPipelines {
        files: documents
            .iter()
            .map(|(name, document)| ((*name).to_owned(), (*document).to_owned()))
            .collect(),
    });
    backends.registry = Arc::new(FixtureRegistry::holding([
        ("beir/scifact".to_owned(), scifact()),
        ("squad/dev".to_owned(), squad()),
        ("beir/nq".to_owned(), nq()),
    ]));
    router_over(backends, &scratch(test))
}

async fn matrix(app: Server, path: &str) -> Value {
    let response = send(app, get(path)).await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    json(response).await
}

async fn matrix_of(app: Server) -> Value {
    matrix(app, &format!("/api/v1/pipelines/{NAME}/matrix")).await
}

/// The column of the benchmark `name`.
fn column<'a>(body: &'a Value, name: &str) -> &'a Value {
    body["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|column| column["benchmark_names"][0] == name)
        .unwrap_or_else(|| panic!("no column for {name}: {body:#}"))
}

/// The cell of row `node` in `column`.
fn cell<'a>(body: &Value, column: &'a Value, node: &str) -> &'a Value {
    let row = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .position(|row| row["node"] == node)
        .unwrap_or_else(|| panic!("no row {node}"));
    &column["cells"][row]
}

fn feeding<'a>(body: &'a Value, run: &str) -> Option<&'a Value> {
    body["feeding_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feeding| feeding["run"] == run)
}

#[tokio::test]
async fn one_column_per_benchmark_with_the_metric_each_node_s_ground_truth_allows() {
    let app = app(
        "matrix-columns",
        &[(NAME, HYBRID_RERANK_GEN)],
        vec![
            run(1, HYBRID_RERANK_GEN, &scifact(), "sci", Some(1_000)),
            run(2, HYBRID_RERANK_GEN, &squad(), "sq", Some(2_000)),
        ],
    );

    let body = matrix_of(app).await;

    assert_eq!(body["pipeline"], NAME);
    let rows: Vec<(&str, &str, &str)> = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["node"].as_str().unwrap(),
                row["family"].as_str().unwrap(),
                row["produces"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("bm25", "retriever", "chunks"),
            ("dense", "retriever", "chunks"),
            ("rrf", "fusion", "chunks"),
            ("rerank", "reranker", "chunks"),
            ("concat", "context_builder", "context"),
            ("generate", "generator", "answer"),
        ]
    );
    assert_eq!(body["columns"].as_array().unwrap().len(), 2);

    let scifact = column(&body, "beir/scifact");
    let squad = column(&body, "squad/dev");
    assert_eq!(scifact["run"], id(1));
    assert_eq!(squad["run"], id(2));
    assert_eq!(scifact["ground_truth"], "qrels");
    assert_eq!(squad["ground_truth"], "both");
    for column in [scifact, squad] {
        let rerank = cell(&body, column, "rerank");
        assert_eq!(rerank["kind"], "measured", "{rerank:#}");
        assert_eq!(rerank["metrics"]["ndcg@10"], 1.0);
        assert_eq!(cell(&body, column, "concat")["kind"], "not_scored");
    }
    let generate = cell(&body, squad, "generate");
    assert_eq!(generate["kind"], "measured");
    assert_eq!(generate["metrics"]["exact_match"], 0.5);
    assert_eq!(generate["metrics"]["token_f1"], 0.75);
    assert_eq!(generate["gain"], serde_json::json!({"kind": "unstaged"}));
    assert_eq!(
        cell(&body, scifact, "generate")["kind"],
        "no_reference_answers"
    );
    assert_eq!(body["missing"], serde_json::json!([]));
}

#[tokio::test]
async fn the_gain_is_over_the_previous_ranking_stage_as_the_run_s_own_figures_give_it() {
    let app_for = || {
        app(
            "matrix-gain",
            &[(NAME, HYBRID_RERANK_GEN)],
            vec![run(1, HYBRID_RERANK_GEN, &scifact(), "sci", Some(1_000))],
        )
    };

    let body = matrix_of(app_for()).await;
    let figures =
        json(send(app_for(), get(&format!("/api/v1/runs/{}/queries", id(1)))).await).await;
    let value = |node: &str| -> f64 {
        figures["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["node"] == node)
            .unwrap()["metrics"]["ndcg@10"]
            .as_f64()
            .unwrap()
    };

    let scifact = column(&body, "beir/scifact");
    for leg in ["bm25", "dense"] {
        let leg_cell = cell(&body, scifact, leg);
        assert_eq!(leg_cell["metrics"]["ndcg@10"].as_f64().unwrap(), value(leg));
        assert_eq!(
            leg_cell["gain"],
            serde_json::json!({"kind": "first_stage"}),
            "a leg has no stage before it"
        );
    }
    let best_leg = value("bm25").max(value("dense"));
    assert_eq!(
        cell(&body, scifact, "rrf")["gain"]["values"]["ndcg@10"]
            .as_f64()
            .unwrap(),
        value("rrf") - best_leg
    );
    assert_eq!(
        cell(&body, scifact, "rerank")["gain"]["values"]["ndcg@10"]
            .as_f64()
            .unwrap(),
        value("rerank") - value("rrf")
    );
}

#[tokio::test]
async fn a_benchmark_never_run_is_a_column_only_when_asked_for_and_reads_not_run_yet() {
    let app_for = || {
        app(
            "matrix-available",
            &[(NAME, HYBRID_RERANK_GEN)],
            vec![run(1, HYBRID_RERANK_GEN, &scifact(), "sci", Some(1_000))],
        )
    };

    let body = matrix_of(app_for()).await;
    assert_eq!(body["columns"].as_array().unwrap().len(), 1);

    let body = matrix(
        app_for(),
        &format!("/api/v1/pipelines/{NAME}/matrix?include_available=true"),
    )
    .await;
    let names: Vec<&Value> = body["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|column| &column["benchmark_names"][0])
        .collect();
    assert_eq!(names, ["beir/nq", "beir/scifact", "squad/dev"]);
    let never_run = column(&body, "beir/nq");
    assert_eq!(never_run["run"], Value::Null);
    assert_eq!(never_run["dataset_version"], dataset_version(&nq()));
    for cell in never_run["cells"].as_array().unwrap() {
        assert_eq!(
            cell,
            &serde_json::json!({"kind": "not_run_yet", "benchmark": "beir/nq"})
        );
    }
    let missing: Vec<&Value> = body["missing"]
        .as_array()
        .unwrap()
        .iter()
        .map(|missing| &missing["benchmark"])
        .collect();
    assert_eq!(missing, ["beir/nq", "squad/dev"]);
    assert_eq!(
        body["missing"][0]["nodes"],
        serde_json::json!(["bm25", "dense", "rrf", "rerank", "concat", "generate"])
    );

    let refused = send(
        app_for(),
        get(&format!(
            "/api/v1/pipelines/{NAME}/matrix?include_available=maybe"
        )),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(refused).await["code"], "parameter_invalid");
}

#[tokio::test]
async fn the_most_recent_run_on_a_benchmark_fills_its_column_and_the_older_only_feeds() {
    let body = matrix_of(app(
        "matrix-recent",
        &[(NAME, HYBRID_RERANK_GEN)],
        vec![
            run(9, HYBRID_RERANK_GEN, &scifact(), "sci", Some(1_000)),
            run(1, HYBRID_RERANK_GEN, &scifact(), "sci", Some(2_000)),
        ],
    ))
    .await;

    assert_eq!(column(&body, "beir/scifact")["run"], id(1));
    assert_eq!(feeding(&body, &id(1)).unwrap()["fills_column"], true);
    let older = feeding(&body, &id(9)).expect("the older run is listed");
    assert_eq!(older["fills_column"], false);
    assert_eq!(older["started_at_ms"], 1_000);
    assert_eq!(older["pipeline_names"], serde_json::json!([NAME]));
    assert_eq!(older["prefix_of"], Value::Null);
}

#[tokio::test]
async fn a_run_with_a_time_fills_the_column_over_one_without_whichever_id_is_greater() {
    for (timed, untimed) in [(1, 9), (9, 1)] {
        let body = matrix_of(app(
            &format!("matrix-timed-{timed}"),
            &[(NAME, HYBRID_RERANK_GEN)],
            vec![
                run(timed, HYBRID_RERANK_GEN, &scifact(), "sci", Some(1_000)),
                run(untimed, HYBRID_RERANK_GEN, &scifact(), "sci", None),
            ],
        ))
        .await;

        assert_eq!(column(&body, "beir/scifact")["run"], id(timed));
        assert_eq!(feeding(&body, &id(untimed)).unwrap()["fills_column"], false);
    }
}

#[tokio::test]
async fn two_runs_with_no_time_are_chosen_between_by_run_id() {
    let body = matrix_of(app(
        "matrix-untimed",
        &[(NAME, HYBRID_RERANK_GEN)],
        vec![
            run(9, HYBRID_RERANK_GEN, &scifact(), "sci", None),
            run(1, HYBRID_RERANK_GEN, &scifact(), "sci", None),
        ],
    ))
    .await;

    assert_eq!(column(&body, "beir/scifact")["run"], id(1));
}

#[tokio::test]
async fn a_prefix_run_fills_the_nodes_it_shares_and_says_where_it_stops() {
    let body = matrix_of(app(
        "matrix-prefix",
        &[(NAME, HYBRID_RERANK_GEN)],
        vec![run(1, UP_TO_RERANK, &scifact(), "sci", Some(1_000))],
    ))
    .await;

    let prefixed = column(&body, "beir/scifact");
    assert_eq!(prefixed["run"], id(1));
    assert_eq!(prefixed["up_to"], "rerank");
    for node in ["bm25", "dense", "rrf", "rerank"] {
        assert_eq!(cell(&body, prefixed, node)["kind"], "measured", "{node}");
    }
    for node in ["concat", "generate"] {
        assert_eq!(
            cell(&body, prefixed, node),
            &serde_json::json!({"kind": "prefix_stops", "up_to": "rerank"}),
            "{node}"
        );
    }
    assert_eq!(
        feeding(&body, &id(1)).unwrap()["prefix_of"],
        serde_json::json!({"pipeline": NAME, "up_to": "rerank"})
    );
    assert_eq!(
        body["missing"],
        serde_json::json!([{
            "benchmark": "beir/scifact",
            "dataset_version": dataset_version(&scifact()),
            "nodes": ["concat", "generate"],
        }])
    );
}

#[tokio::test]
async fn a_prefix_of_two_pipelines_counts_as_a_prefix_in_each() {
    let other = other_generator();
    for name in [NAME, "other-generator"] {
        let body = matrix(
            app(
                &format!("matrix-two-parents-{name}"),
                &[(NAME, HYBRID_RERANK_GEN), ("other-generator", &other)],
                vec![run(1, UP_TO_RERANK, &scifact(), "sci", Some(1_000))],
            ),
            &format!("/api/v1/pipelines/{name}/matrix"),
        )
        .await;

        assert_eq!(column(&body, "beir/scifact")["up_to"], "rerank", "{name}");
        assert_eq!(
            feeding(&body, &id(1)).unwrap()["prefix_of"]["pipeline"],
            name
        );
    }
}

#[tokio::test]
async fn a_run_of_the_same_canonical_form_under_another_name_fills_its_cell() {
    let fork = "fork-of-hybrid";
    let body = matrix_of(app(
        "matrix-fork",
        &[(NAME, HYBRID_RERANK_GEN), (fork, HYBRID_RERANK_GEN)],
        // Launched first under the fork's name: its record says so.
        vec![launched(
            run(1, HYBRID_RERANK_GEN, &scifact(), "sci", Some(1_000)),
            RunProvenance::named(fork),
        )],
    ))
    .await;

    assert_eq!(column(&body, "beir/scifact")["run"], id(1));
    let feeding = feeding(&body, &id(1)).unwrap();
    // Two facts, never resolved into one: the record, and the content.
    assert_eq!(feeding["launched_as"]["name"], fork);
    assert_eq!(feeding["pipeline_names"], serde_json::json!([fork, NAME]));
    assert_eq!(feeding["content_since_changed"], Value::Null);
}

/// `run`, with the launch record `record`.
fn launched(mut run: Run, record: RunProvenance) -> Run {
    run.provenance = Some(record);
    run
}

fn hash_of(document: &str) -> PipelineHash {
    lower_configuration(&ConfigDocument::new(document))
        .expect("a test pipeline lowers")
        .content_hash()
}

#[tokio::test]
async fn a_run_launched_as_the_pipeline_whose_content_has_since_changed_fills_no_cell() {
    let earlier = HYBRID_RERANK_GEN.replace("top_k: 50", "top_k: 10");
    let body = matrix_of(app(
        "matrix-since-changed",
        &[(NAME, HYBRID_RERANK_GEN)],
        vec![
            launched(
                run(1, &earlier, &scifact(), "sci", Some(1_000)),
                RunProvenance::named(NAME),
            ),
            launched(
                run(2, &earlier, &squad(), "sq", Some(1_000)),
                RunProvenance::named(NAME),
            ),
            run(3, HYBRID_RERANK_GEN, &squad(), "sq", Some(500)),
        ],
    ))
    .await;

    // Its only run on scifact is of the earlier content: the column links it,
    // and no cell is filled from it.
    let scifact = column(&body, "beir/scifact");
    assert_eq!(scifact["run"], Value::Null);
    for cell in scifact["cells"].as_array().unwrap() {
        assert_eq!(
            cell,
            &serde_json::json!({"kind": "not_run_on_this_version", "run": id(1)})
        );
    }
    // On squad, the run of the current content fills the column, though the
    // earlier-content run is more recent.
    assert_eq!(column(&body, "squad/dev")["run"], id(3));

    let since_changed = feeding(&body, &id(1)).expect("listed as a feeding run");
    assert_eq!(since_changed["fills_column"], false);
    assert_eq!(since_changed["launched_as"]["name"], NAME);
    // The matrix is the pipeline's own, so its name is held, as `GET /runs` says it.
    assert_eq!(since_changed["launched_as"]["held"], "exactly");
    assert_eq!(since_changed["pipeline_names"], serde_json::json!([]));
    assert_eq!(since_changed["prefix_of"], Value::Null);
    // The parameter difference against the current document, the current
    // document first: `compare`'s configuration matrix.
    let since = &since_changed["content_since_changed"];
    assert_eq!(since["launched"], "as_pipeline");
    let difference = &since["difference"];
    assert_eq!(difference["kind"], "compared");
    assert_eq!(
        difference["parameters"],
        serde_json::json!([{
            "node": "dense",
            "key": {"kind": "param", "name": "top_k"},
            "values": [
                { "kind": "int", "value": "50" },
                { "kind": "int", "value": "10" },
            ],
        }])
    );
    assert_eq!(feeding(&body, &id(2)).unwrap()["fills_column"], false);
    // The benchmark it ran on is still missing for this version.
    assert_eq!(body["missing"][0]["benchmark"], "beir/scifact");
}

#[tokio::test]
async fn a_run_recorded_as_a_prefix_of_the_current_version_fills_by_its_record() {
    // Not a prefix by structure — its declared input is named otherwise — but
    // its record says it was cut from this version of the pipeline.
    let renamed = UP_TO_RERANK.replace("question", "query");
    let body = matrix_of(app(
        "matrix-recorded-prefix",
        &[(NAME, HYBRID_RERANK_GEN)],
        vec![launched(
            run(1, &renamed, &scifact(), "sci", Some(1_000)),
            RunProvenance::prefix(NAME, PrefixOf::new("rerank", hash_of(HYBRID_RERANK_GEN))),
        )],
    ))
    .await;

    let prefixed = column(&body, "beir/scifact");
    assert_eq!(prefixed["run"], id(1));
    assert_eq!(prefixed["up_to"], "rerank");
    assert_eq!(cell(&body, prefixed, "rerank")["kind"], "measured");
    assert_eq!(
        cell(&body, prefixed, "generate"),
        &serde_json::json!({"kind": "prefix_stops", "up_to": "rerank"})
    );
    let feeding = feeding(&body, &id(1)).unwrap();
    assert_eq!(
        feeding["prefix_of"],
        serde_json::json!({"pipeline": NAME, "up_to": "rerank"})
    );
    assert_eq!(
        feeding["launched_as"]["prefix_of"],
        serde_json::json!({
            "up_to": "rerank",
            "parent_pipeline_hash": hash_of(HYBRID_RERANK_GEN).to_string(),
        })
    );
    assert_eq!(feeding["content_since_changed"], Value::Null);
}

#[tokio::test]
async fn a_structural_prefix_fills_even_when_its_record_names_another_parent() {
    let other = other_generator();
    let body = matrix_of(app(
        "matrix-prefix-other-parent",
        &[(NAME, HYBRID_RERANK_GEN), ("other-generator", &other)],
        vec![launched(
            run(1, UP_TO_RERANK, &scifact(), "sci", Some(1_000)),
            RunProvenance::prefix("other-generator", PrefixOf::new("rerank", hash_of(&other))),
        )],
    ))
    .await;

    assert_eq!(column(&body, "beir/scifact")["up_to"], "rerank");
    let feeding = feeding(&body, &id(1)).unwrap();
    assert_eq!(feeding["prefix_of"]["pipeline"], NAME);
    assert_eq!(feeding["launched_as"]["name"], "other-generator");
}

#[tokio::test]
async fn a_run_of_other_content_counts_nowhere() {
    let changed = HYBRID_RERANK_GEN.replace("top_k: 50", "top_k: 10");
    let body = matrix_of(app(
        "matrix-other-content",
        &[(NAME, HYBRID_RERANK_GEN)],
        vec![run(1, &changed, &scifact(), "sci", Some(1_000))],
    ))
    .await;

    assert_eq!(body["columns"], serde_json::json!([]));
    assert_eq!(body["feeding_runs"], serde_json::json!([]));
}

#[tokio::test]
async fn a_pipeline_the_workspace_does_not_hold_is_pipeline_not_found() {
    let response = send(
        app("matrix-not-found", &[], Vec::new()),
        get("/api/v1/pipelines/nothing/matrix"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(response).await["code"], "pipeline_not_found");
}

#[tokio::test]
async fn without_the_run_s_dataset_a_ranking_cell_is_unverified_and_the_answer_is_recorded() {
    // The registry knows only `beir/nq`: the run's own dataset is pinned by
    // nothing, so nothing on disk is the ground truth it was scored on.
    let mut backends = fakes(FakeRunStore::holding([run(
        1,
        HYBRID_RERANK_GEN,
        &squad(),
        "sq",
        Some(1_000),
    )]));
    backends.pipelines = Arc::new(HeldPipelines {
        files: vec![(NAME.to_owned(), HYBRID_RERANK_GEN.to_owned())],
    });
    backends.registry = Arc::new(FixtureRegistry::holding([("beir/nq".to_owned(), nq())]));

    let body = matrix_of(router_over(backends, &scratch("matrix-unverified"))).await;

    let column = &body["columns"][0];
    assert_eq!(column["benchmark_names"], serde_json::json!([]));
    assert_eq!(column["dataset_check"]["status"], "dataset_absent");
    // Read off the metrics the run recorded: ranking and answer metrics both.
    assert_eq!(column["ground_truth"], "both");
    assert_eq!(cell(&body, column, "rerank")["kind"], "unverified");
    assert_eq!(cell(&body, column, "generate")["metrics"]["token_f1"], 0.75);
    assert_eq!(body["missing"], serde_json::json!([]));
}

/// Two retrievers, a reranker over one of them, and the fusion of the
/// reranked leg with the other: a reranker upstream of the fusion, a layout
/// the stage derivation reads by position and marks as a guess.
const RERANK_INTO_FUSION: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, bm25]
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [rerank, dense]
";

#[tokio::test]
async fn where_the_stages_are_a_guess_the_gain_says_so_rather_than_compute_one() {
    let name = "rerank-into-fusion";
    let traces: Vec<(String, Trace)> = (1..=2)
        .map(|n| (format!("sci{n}"), trace_through("sci", n, &RANKS, false)))
        .collect();
    let mut guessed = run_over(
        1,
        RERANK_INTO_FUSION,
        &scifact(),
        traces
            .iter()
            .map(|(q, t)| (q.as_str(), t.clone()))
            .collect(),
        &[("ndcg@10", 0.9)],
    );
    guessed.times = Some(RunTimes::new(UnixMillis::new(1), UnixMillis::new(2)));
    let body = matrix(
        app(
            "matrix-guessed",
            &[(name, RERANK_INTO_FUSION)],
            vec![guessed],
        ),
        &format!("/api/v1/pipelines/{name}/matrix"),
    )
    .await;

    let scifact = column(&body, "beir/scifact");
    assert_eq!(
        cell(&body, scifact, "bm25")["gain"],
        serde_json::json!({"kind": "first_stage"})
    );
    for node in ["rerank", "rrf"] {
        let staged = cell(&body, scifact, node);
        assert_eq!(staged["kind"], "measured", "{node}");
        assert_eq!(
            staged["gain"],
            serde_json::json!({"kind": "ambiguous"}),
            "{node}: no gain is served as fact over a guessed stage"
        );
    }
}

#[tokio::test]
async fn a_full_run_fills_the_column_over_a_newer_prefix_and_nothing_existing_is_missing() {
    let body = matrix_of(app(
        "matrix-full-over-prefix",
        &[(NAME, HYBRID_RERANK_GEN)],
        vec![
            run(1, HYBRID_RERANK_GEN, &scifact(), "sci", Some(1_000)),
            run(2, UP_TO_RERANK, &scifact(), "sci", Some(2_000)),
        ],
    ))
    .await;

    let scifact = column(&body, "beir/scifact");
    assert_eq!(scifact["run"], id(1));
    assert_eq!(scifact["up_to"], Value::Null);
    let prefix = feeding(&body, &id(2)).expect("the prefix run feeds the matrix");
    assert_eq!(prefix["fills_column"], false);
    assert_eq!(prefix["prefix_of"]["up_to"], "rerank");
    // A run of the whole pipeline on scifact exists: launching it again would
    // be refused as `run_exists`, so nothing is proposed.
    assert_eq!(body["missing"], serde_json::json!([]));
}

#[tokio::test]
async fn without_the_run_s_dataset_a_ranking_row_is_unverified_whatever_the_run_recorded() {
    // The run recorded answer metrics only, so nothing it holds says whether
    // its benchmark carries qrels: the ranking rows are unverified, not
    // `no_qrels`.
    let mut answers_only = run(1, HYBRID_RERANK_GEN, &squad(), "sq", Some(1_000));
    let mut recorded = ragondin_experiments::Metrics::default();
    recorded.insert("exact_match", 0.5);
    answers_only.metrics = recorded;
    let mut backends = fakes(FakeRunStore::holding([answers_only]));
    backends.pipelines = Arc::new(HeldPipelines {
        files: vec![(NAME.to_owned(), HYBRID_RERANK_GEN.to_owned())],
    });
    backends.registry = Arc::new(FixtureRegistry::holding([("beir/nq".to_owned(), nq())]));

    let body = matrix_of(router_over(backends, &scratch("matrix-unverified-answers"))).await;

    let column = &body["columns"][0];
    for node in ["bm25", "dense", "rrf", "rerank"] {
        assert_eq!(cell(&body, column, node)["kind"], "unverified", "{node}");
    }
    assert_eq!(
        cell(&body, column, "generate")["metrics"]["exact_match"],
        0.5
    );
}

#[tokio::test]
async fn a_prefix_of_an_earlier_version_is_said_to_be_one() {
    let earlier = HYBRID_RERANK_GEN.replace("top_k: 50", "top_k: 10");
    let earlier_prefix = UP_TO_RERANK.replace("top_k: 50", "top_k: 10");
    let body = matrix_of(app(
        "matrix-earlier-prefix",
        &[(NAME, HYBRID_RERANK_GEN)],
        vec![launched(
            run(1, &earlier_prefix, &scifact(), "sci", Some(1_000)),
            RunProvenance::prefix(NAME, PrefixOf::new("rerank", hash_of(&earlier))),
        )],
    ))
    .await;

    assert_eq!(
        cell(&body, column(&body, "beir/scifact"), "rerank"),
        &serde_json::json!({"kind": "not_run_on_this_version", "run": id(1)})
    );
    let since = &feeding(&body, &id(1)).unwrap()["content_since_changed"];
    // Never "an earlier version of" the pipeline (ADR-C39 § 2): a prefix of
    // one, cut where its record says.
    assert_eq!(since["launched"], "as_prefix");
    assert_eq!(since["difference"]["kind"], "compared");
}

#[tokio::test]
async fn a_run_with_no_single_output_counts_nowhere_whatever_its_record_says() {
    // Both legs of the current pipeline and nothing after them: a subset of
    // its nodes, with two terminal nodes, so no output the harness scores.
    let legs = "\
pipeline:
  inputs: [question]
  nodes:
    - id: bm25
      component: retriever
      impl: bm25
      inputs: [question]
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
      params: { top_k: 50 }
";
    let traces: Vec<(String, Trace)> = (1..=2)
        .map(|n| {
            (
                format!("sci{n}"),
                trace_through("sci", n, &RANKS[..2], false),
            )
        })
        .collect();
    let two_outputs = launched(
        run_over(
            1,
            legs,
            &scifact(),
            traces
                .iter()
                .map(|(q, t)| (q.as_str(), t.clone()))
                .collect(),
            &[("ndcg@10", 0.9)],
        ),
        RunProvenance::named(NAME),
    );
    let body = matrix_of(app(
        "matrix-two-outputs",
        &[(NAME, HYBRID_RERANK_GEN)],
        vec![two_outputs],
    ))
    .await;

    assert_eq!(body["columns"], serde_json::json!([]));
    assert_eq!(body["feeding_runs"], serde_json::json!([]));
}
