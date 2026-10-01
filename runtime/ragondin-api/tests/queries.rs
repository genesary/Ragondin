//! `GET /api/v1/runs/{id}/queries`: every query the run executed, with its
//! scores read from the trace against the run's own ground truth, its
//! duration, and the per-node metrics.

mod support;

use std::sync::Arc;

use axum::http::StatusCode;
use ragondin_benchmarks::manifest::Format;
use ragondin_benchmarks::Benchmark;
use ragondin_experiments::{Run, Trace};
use serde_json::Value;
use support::datasets::{benchmark_fixture, scratch};
use support::runs::{chunk, counted, node, query, ranked, run_over};
use support::{app_over, get, json, send, FakeRegistry, FakeRunStore, FixtureRegistry};

/// `ragondin-harness`' end-to-end fixture pipeline: two stub legs and the
/// fusion that interleaves them, which is the ranking the metrics read.
const STUB_OVER_BEIR_MINI: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: fused
      component: fusion
      impl: stub_interleave
      inputs: [leg_a, leg_b]
    - id: leg_a
      component: retriever
      impl: stub_retriever
      inputs: [question]
      params: { top_k: 2, label: MED-10 }
    - id: leg_b
      component: retriever
      impl: stub_retriever
      inputs: [question]
      params: { top_k: 1, label: \"4983\" }
";

fn beir_mini() -> Benchmark {
    Format::Beir
        .load(&benchmark_fixture("beir-mini"))
        .expect("the fixture loads")
}

/// What the harness's stub pipeline records for every query: `leg_a`
/// invents two chunks of `MED-10`, `leg_b` one of `4983`, and the fusion
/// interleaves them — so the fused ranking names `MED-10` twice.
fn stub_trace(query_id: &str) -> Trace {
    Trace {
        nodes: vec![
            node(
                "leg_a",
                vec![query(query_id)],
                ranked(vec![
                    chunk("MED-10-0", "MED-10", 1.0),
                    chunk("MED-10-1", "MED-10", 0.5),
                ]),
                100,
            ),
            node(
                "leg_b",
                vec![query(query_id)],
                ranked(vec![chunk("4983-0", "4983", 1.0)]),
                20,
            ),
            node(
                "fused",
                vec![counted(2), counted(1)],
                ranked(vec![
                    chunk("MED-10-0", "MED-10", 1.0),
                    chunk("4983-0", "4983", 0.5),
                    chunk("MED-10-1", "MED-10", 0.333_333_333_333_333_3),
                ]),
                3,
            ),
        ],
    }
}

/// The harness's figures for this pipeline over this fixture, derived by hand
/// in `ragondin-harness`' `harness_over_beir_mini.rs`: `q-1` hits at rank 1,
/// `q-2` at rank 2, `0042` misses.
fn harness_means() -> [(&'static str, f64); 3] {
    [
        ("mrr", (1.0 + 0.5) / 3.0),
        ("ndcg@10", (1.0 + 1.0 / f64::log2(3.0)) / 3.0),
        ("recall@10", 2.0 / 3.0),
    ]
}

fn stub_run() -> Run {
    run_over(
        0x51,
        STUB_OVER_BEIR_MINI,
        &beir_mini(),
        ["0042", "q-1", "q-2"]
            .into_iter()
            .map(|id| (id, stub_trace(id)))
            .collect(),
        &harness_means(),
    )
}

async fn get_queries(
    run: &Run,
    registry: Arc<dyn ragondin_api::Registry>,
    path: &str,
    test: &str,
) -> (StatusCode, Value) {
    let workspace = scratch(test);
    let response = send(
        app_over(FakeRunStore::holding([run.clone()]), registry, &workspace),
        get(&format!("/api/v1/runs/{}/queries{path}", run.id)),
    )
    .await;
    (response.status(), json(response).await)
}

fn verified() -> Arc<dyn ragondin_api::Registry> {
    Arc::new(FixtureRegistry::holding([(
        "beir/mini".to_owned(),
        beir_mini(),
    )]))
}

fn entry<'a>(body: &'a Value, id: &str) -> &'a Value {
    body["queries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("query {id} is listed"))
}

fn close(got: &Value, want: f64) -> bool {
    got.as_f64().is_some_and(|got| (got - want).abs() < 1e-12)
}

#[tokio::test(flavor = "multi_thread")]
async fn two_chunks_of_one_document_count_once_as_the_harness_scores_them() {
    let (status, body) = get_queries(&stub_run(), verified(), "", "queries_fold").await;
    assert_eq!(status, StatusCode::OK);

    // Per query: the harness's hand-derived values. Counted twice, `MED-10`
    // would score q-1's nDCG above its ideal and move every mean.
    let q1 = &entry(&body, "q-1")["scores"];
    for metric in ["ndcg@10", "recall@10", "mrr"] {
        assert!(close(&q1[metric], 1.0), "q-1 {metric}: {}", q1[metric]);
    }
    let q2 = &entry(&body, "q-2")["scores"];
    assert!(close(&q2["ndcg@10"], 1.0 / f64::log2(3.0)));
    assert!(close(&q2["recall@10"], 1.0));
    assert!(close(&q2["mrr"], 0.5));
    let missed = &entry(&body, "0042")["scores"];
    for metric in ["ndcg@10", "recall@10", "mrr"] {
        assert!(close(&missed[metric], 0.0), "0042 {metric}");
    }

    // And the mean at the last ranking node is the harness's figure.
    assert_eq!(body["ranking_node"], "fused");
    let fused = body["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["node"] == "fused")
        .unwrap();
    for (metric, want) in harness_means() {
        assert!(close(&fused["metrics"][metric], want), "{metric}");
    }
    assert_eq!(fused["judged_queries"], 3);
}

/// `ragondin-harness`' own fold fixture — its unit test
/// `a_chunk_ranking_collapses_to_its_documents_by_first_occurrence` — where a
/// second chunk of `d-1` stands *before* a judged document: folded by first
/// occurrence the documents are `[d-1, d-2, d-3]` and `d-3` is 3rd; counted
/// per chunk it would be 4th, and every metric would move.
#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_document_ahead_of_a_judged_one_does_not_push_it_down() {
    use ragondin_benchmarks::Qrels;
    use ragondin_types::{DocId, Document, Query, QueryId};

    let document = |id: &str| Document {
        id: DocId::new(id),
        text: format!("the text of {id}"),
        metadata: Default::default(),
    };
    let mut qrels = Qrels::new();
    qrels.insert(QueryId::new("q"), DocId::new("d-3"), 1);
    let benchmark = Benchmark::new(
        vec![document("d-1"), document("d-2"), document("d-3")],
        vec![Query {
            id: QueryId::new("q"),
            text: "which one?".to_owned(),
        }],
        qrels,
    );
    let trace = Trace {
        nodes: vec![node(
            "leg",
            vec![query("q")],
            ranked(vec![
                chunk("d-1#2", "d-1", 0.9),
                chunk("d-2#0", "d-2", 0.8),
                chunk("d-1#0", "d-1", 0.7),
                chunk("d-3#1", "d-3", 0.6),
            ]),
            1,
        )],
    };
    let run = run_over(
        0x52,
        "pipeline:\n  inputs: [question]\n  nodes:\n    - id: leg\n      component: retriever\n      impl: dense\n      inputs: [question]\n",
        &benchmark,
        vec![("q", trace)],
        &[("mrr", 1.0 / 3.0), ("ndcg@10", 0.5), ("recall@10", 1.0)],
    );
    let registry = Arc::new(FixtureRegistry::holding([("fold".to_owned(), benchmark)]));

    let (_, body) = get_queries(&run, registry, "", "queries_fold_ahead").await;

    let scores = &entry(&body, "q")["scores"];
    assert!(
        close(&scores["mrr"], 1.0 / 3.0),
        "d-3 is 3rd: {}",
        scores["mrr"]
    );
    assert!(
        close(&scores["ndcg@10"], 0.5),
        "1 / log2(4): {}",
        scores["ndcg@10"]
    );
    assert!(close(&scores["recall@10"], 1.0));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_listing_names_the_metrics_and_sums_each_querys_node_durations() {
    let (_, body) = get_queries(&stub_run(), verified(), "", "queries_listing").await;
    assert_eq!(
        body["metrics"],
        serde_json::json!(["mrr", "ndcg@10", "recall@10"])
    );
    let ids: Vec<&str> = body["queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["0042", "q-1", "q-2"], "the run's order: by query id");
    for id in ids {
        assert_eq!(entry(&body, id)["duration_nanos"], 123, "{id}");
    }
    assert!(
        body["answer_node"].is_null(),
        "the pipeline ends in a ranking"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_filter_keeps_exactly_the_queries_with_no_gold_document_in_the_top_k() {
    let run = stub_run();
    // The fused documents are [MED-10, 4983]. q-1's gold MED-10 is 1st, q-2's
    // gold 4983 is 2nd, and 0042's gold is nowhere.
    let ids = |body: &Value| -> Vec<String> {
        body["queries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_str().unwrap().to_owned())
            .collect()
    };
    let (status, at_1) =
        get_queries(&run, verified(), "?missing_gold_at=1", "queries_filter_1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&at_1), ["0042", "q-2"]);
    let (_, at_2) = get_queries(&run, verified(), "?missing_gold_at=2", "queries_filter_2").await;
    assert_eq!(ids(&at_2), ["0042"]);
    let (_, at_10) =
        get_queries(&run, verified(), "?missing_gold_at=10", "queries_filter_10").await;
    assert_eq!(ids(&at_10), ["0042"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_filter_is_parameter_invalid() {
    for path in [
        "?missing_gold_at=0",
        "?missing_gold_at=ten",
        "?missing_gold_at=",
        "?other=1",
    ] {
        let (status, body) = get_queries(&stub_run(), verified(), path, "queries_bad_filter").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(body["code"], "parameter_invalid", "{path}");
    }
}

/// A `%` must be followed by two hexadecimal digits: a sign is not one, so
/// `%+1` and `%-1` are malformed escapes, refused as such rather than decoded
/// to a byte.
#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_escape_is_refused_as_one() {
    for path in [
        "?missing_gold_at=%+1",
        "?missing_gold_at=%-1",
        "?missing_gold_at=%1",
        "?missing_gold_at=%zz",
    ] {
        let (status, body) =
            get_queries(&stub_run(), verified(), path, "queries_malformed_escape").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(body["code"], "parameter_invalid", "{path}");
        assert!(
            body["detail"].as_str().unwrap().contains("percent-encoded"),
            "{path}: refused as a malformed escape, not as a value: {}",
            body["detail"]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_filter_is_parameter_invalid() {
    let (status, body) = get_queries(
        &stub_run(),
        verified(),
        "?missing_gold_at=1&missing_gold_at=2",
        "queries_repeated_filter",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "parameter_invalid");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_percent_encoded_filter_is_read_as_its_decoded_form() {
    for path in ["?missing%5Fgold%5Fat=1", "?missing_gold_at=%31"] {
        let (status, body) =
            get_queries(&stub_run(), verified(), path, "queries_encoded_filter").await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        let ids: Vec<&str> = body["queries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["0042", "q-2"], "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn without_the_runs_dataset_the_queries_are_listed_unscored_and_flagged() {
    let run = stub_run();
    let (status, body) = get_queries(&run, Arc::new(FakeRegistry), "", "queries_absent").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ground_truth"]["status"], "dataset_absent");
    assert!(
        body["ground_truth"]["benchmark"].is_null(),
        "no registry entry holds this digest"
    );
    assert_eq!(
        body["metrics"],
        serde_json::json!(["mrr", "ndcg@10", "recall@10"])
    );
    for id in ["0042", "q-1", "q-2"] {
        assert_eq!(entry(&body, id)["scores"], serde_json::json!({}), "{id}");
        assert_eq!(entry(&body, id)["duration_nanos"], 123, "{id}");
    }
    for node in body["nodes"].as_array().unwrap() {
        assert!(node["metrics"].is_null(), "{node}");
        assert_eq!(node["produces_ranking"], true, "{node}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_filter_needs_the_ground_truth_and_says_when_it_is_absent() {
    let (status, body) = get_queries(
        &stub_run(),
        Arc::new(FakeRegistry),
        "?missing_gold_at=1",
        "queries_absent_filter",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "dataset_absent");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_run_is_run_not_found() {
    let workspace = scratch("queries_unknown_run");
    let response = send(
        app_over(FakeRunStore::default(), verified(), &workspace),
        get(&format!("/api/v1/runs/{}/queries", "cd".repeat(32))),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(response).await["code"], "run_not_found");
}
