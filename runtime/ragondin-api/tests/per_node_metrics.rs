//! The invariant the per-node metrics rest on: read from `traces.json` at the
//! last ranking node, a ranking metric **equals** what `metrics.json` holds.
//! A divergence means the trace or the metric lies (the design document § 9).
//!
//! Checked over `GET /runs/{id}/queries`, against two kinds of stored value:
//!
//! - the run under `tests/fixtures/runs`, **recorded by the harness**, whose
//!   metrics are the harness's own arithmetic — equal to the precision the
//!   store writes: `metrics.json` and a response body are both JSON, written
//!   with the shortest decimal that names the double and read back by
//!   `serde_json`, whose default reader rounds to within an ulp of it, so two
//!   equal figures compare within [`ULPS`] units in the last place;
//! - the `pytrec_eval`-checked fixtures of `ragondin-metrics` — the M2
//!   regression fixture, the M3 calibration fixtures and the SQuAD generation
//!   fixture — stored with the reference implementation's means, equal to the
//!   precision those fixtures are checked to: `1e-12`, the tolerance of
//!   `ragondin-metrics`' own parity tests, which absorbs a different summation
//!   order and nothing else.

mod support;

use std::sync::Arc;

use ragondin_benchmarks::identity::dataset_version;
use ragondin_benchmarks::{Benchmark, Qrels, ReferenceAnswers};
use ragondin_experiments::{Trace, TraceSummary};
use ragondin_metrics::documents_by_first_occurrence;
use ragondin_types::{DocId, Document, Query, QueryId};
use serde_json::Value;
use support::calibration::{beir_calibration, pytrec_eval_parity, squad_generation, FixtureRun};
use support::datasets::scratch;
use support::{app_over, fixture_run, get, json, send, FakeRunStore, FixtureRegistry, FIXTURE_RUN};

/// The tolerance `ragondin-metrics`' parity fixtures are held to.
const TOLERANCE: f64 = 1e-12;

/// How far apart, in units in the last place, two figures the harness and
/// this crate computed identically may read back from JSON.
const ULPS: u64 = 2;

/// Whether `got` and `stored` are one double, as far as a JSON round trip
/// through `serde_json`'s default reader can tell.
fn same_as_stored(got: f64, stored: f64) -> bool {
    got.to_bits().abs_diff(stored.to_bits()) <= ULPS
}

/// The benchmark `ragondin-harness`' generation-regime test evaluates, which
/// the fixture run was recorded over: rebuilt here, and proved to be the same
/// one by its digest.
fn generation_regime_benchmark() -> Benchmark {
    let document = |id: &str| Document {
        id: DocId::new(id),
        text: format!("the text of {id}"),
        metadata: Default::default(),
    };
    let query = |id: &str, text: &str| Query {
        id: QueryId::new(id),
        text: text.to_owned(),
    };
    let mut qrels = Qrels::new();
    qrels.insert(QueryId::new("q-1"), DocId::new("doc-b"), 1);
    qrels.insert(QueryId::new("q-2"), DocId::new("doc-a"), 1);
    qrels.insert(QueryId::new("q-4"), DocId::new("doc-c"), 1);
    let mut references = ReferenceAnswers::new();
    references.insert(
        QueryId::new("q-1"),
        vec![
            "no match at all".to_owned(),
            "Stub chunk 0 of doc-a, for alpha.".to_owned(),
        ],
    );
    references.insert(QueryId::new("q-2"), vec!["chunk zero".to_owned()]);
    references.insert(QueryId::new("q-3"), vec!["gamma".to_owned()]);
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

async fn queries(run: &ragondin_experiments::Run, benchmark: Benchmark, test: &str) -> Value {
    let workspace = scratch(test);
    let registry = FixtureRegistry::holding([("fixture".to_owned(), benchmark)]);
    let response = send(
        app_over(
            FakeRunStore::holding([run.clone()]),
            Arc::new(registry),
            &workspace,
        ),
        get(&format!("/api/v1/runs/{}/queries", run.id)),
    )
    .await;
    assert_eq!(response.status(), 200, "{test}");
    json(response).await
}

fn node<'a>(body: &'a Value, id: &str) -> &'a Value {
    body["nodes"]
        .as_array()
        .expect("the per-node metrics are listed")
        .iter()
        .find(|node| node["node"] == id)
        .unwrap_or_else(|| panic!("node {id} is listed"))
}

/// The mean of `metric` over the per-query scores that carry it, in the
/// benchmark's query order — the harness's order.
fn per_query_mean(body: &Value, benchmark: &Benchmark, metric: &str) -> f64 {
    let (mut sum, mut count) = (0.0, 0usize);
    for query in benchmark.queries() {
        let entry = body["queries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == query.id.as_str())
            .unwrap_or_else(|| panic!("query {} is listed", query.id.as_str()));
        if let Some(value) = entry["scores"][metric].as_f64() {
            sum += value;
            count += 1;
        }
    }
    sum / count as f64
}

#[tokio::test(flavor = "multi_thread")]
async fn the_harness_recorded_run_reads_back_its_own_metrics_to_the_stored_precision() {
    let run = fixture_run();
    let benchmark = generation_regime_benchmark();
    assert_eq!(
        dataset_version(&benchmark),
        run.inputs.dataset_version,
        "the rebuilt benchmark is the one the fixture run was recorded over"
    );

    let body = queries(&run, benchmark.clone(), "per_node_harness_run").await;

    assert_eq!(body["ground_truth"]["status"], "verified");
    assert_eq!(body["ranking_node"], "fused");
    let fused = node(&body, "fused");
    for metric in ["ndcg@10", "recall@10", "mrr"] {
        let (got, stored) = (
            fused["metrics"][metric].as_f64().unwrap(),
            run.metrics.get(metric).unwrap(),
        );
        assert!(
            same_as_stored(got, stored),
            "{metric} at the last ranking node is {got}, metrics.json holds {stored}"
        );
    }
    for metric in ["exact_match", "token_f1"] {
        let (got, stored) = (
            per_query_mean(&body, &benchmark, metric),
            run.metrics.get(metric).unwrap(),
        );
        assert!(
            same_as_stored(got, stored),
            "{metric} averaged over the per-query scores is {got}, metrics.json holds {stored}"
        );
    }
    // The ranking nodes each have a figure; the builder and the generator
    // produced no ranking and have none.
    for ranking in ["leg_a", "leg_b", "fused"] {
        assert_eq!(node(&body, ranking)["produces_ranking"], true, "{ranking}");
        assert!(node(&body, ranking)["metrics"].is_object(), "{ranking}");
        assert_eq!(node(&body, ranking)["judged_queries"], 3, "{ranking}");
    }
    for other in ["context", "answer"] {
        assert_eq!(node(&body, other)["produces_ranking"], false, "{other}");
        assert!(node(&body, other)["metrics"].is_null(), "{other}");
    }
    assert_eq!(FIXTURE_RUN, run.id.to_string());
}

fn assert_invariant(fixture: &FixtureRun, body: &Value) {
    let name = &fixture.name;
    assert_eq!(body["ground_truth"]["status"], "verified", "{name}");
    assert_eq!(body["ranking_node"], "reranked", "{name}");
    let last = node(body, "reranked");
    let mut checked = 0;
    for (metric, stored) in fixture.run.metrics.iter() {
        if metric == "exact_match" || metric == "token_f1" {
            let got = per_query_mean(body, &fixture.benchmark, metric);
            assert!(
                (got - stored).abs() < TOLERANCE,
                "{name}: {metric} averaged over the per-query scores is {got}, metrics.json holds {stored}"
            );
        } else {
            let got = last["metrics"][metric]
                .as_f64()
                .unwrap_or_else(|| panic!("{name}: {metric} at the last ranking node"));
            assert!(
                (got - stored).abs() < TOLERANCE,
                "{name}: {metric} at the last ranking node is {got}, metrics.json holds {stored}"
            );
        }
        checked += 1;
    }
    assert!(checked >= 3, "{name}: every ranking metric was checked");

    // Query by query, too: the per-query score at the output is the one the
    // reference implementation computed.
    for (query, expected) in &fixture.expected {
        let entry = body["queries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == query.as_str())
            .unwrap();
        for (metric, want) in expected {
            let got = entry["scores"][metric].as_f64().unwrap();
            assert!(
                (got - want).abs() < TOLERANCE,
                "{name}: query {query}: {metric} is {got}, the reference scored {want}"
            );
        }
    }

    // The retriever's ranking is the frozen one reversed: a second ranking
    // node, scored, and not the one `metrics.json` records.
    assert!(node(body, "leg")["metrics"].is_object(), "{name}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_m2_regression_fixture_reads_back_its_metrics_at_the_last_ranking_node() {
    for fixture in pytrec_eval_parity(0x10) {
        let body = queries(&fixture.run, fixture.benchmark.clone(), "per_node_m2").await;
        assert_invariant(&fixture, &body);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_m3_calibration_fixtures_read_back_their_metrics_at_the_last_ranking_node() {
    for fixture in [
        beir_calibration(0x20, "scifact", "dense_only"),
        beir_calibration(0x21, "scifact", "hybrid_rerank"),
        beir_calibration(0x22, "nfcorpus", "dense_only"),
    ] {
        let body = queries(&fixture.run, fixture.benchmark.clone(), "per_node_m3").await;
        assert_invariant(&fixture, &body);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_squad_fixture_per_query_exact_match_and_f1_average_to_its_metrics() {
    let fixture = squad_generation(0x30);
    let body = queries(&fixture.run, fixture.benchmark.clone(), "per_node_squad").await;
    assert_eq!(body["answer_node"], "answer");
    assert_invariant(&fixture, &body);
    for other in ["prompt", "answer"] {
        assert!(node(&body, other)["metrics"].is_null(), "{other}");
    }
}

/// Which passages are gold, as the trace serves them, is the qrels read at
/// the ranks the metric computation sees: on the harness-recorded run, for
/// every query and every node that produced a ranking, `gold_ranks` is the
/// 1-based rank of every document graded above 0 in the ranking folded by
/// `ragondin_metrics::documents_by_first_occurrence` — the fold the figures
/// above are computed over — and each passage's `grade` is its document's
/// grade, `0` when the qrels do not judge it. A query without qrels has no
/// gold to show: both are `null`, as its ranking scores are absent.
#[tokio::test(flavor = "multi_thread")]
async fn the_gold_ranks_of_every_ranking_node_are_the_ranks_the_metrics_see() {
    let run = fixture_run();
    let benchmark = generation_regime_benchmark();
    let workspace = scratch("per_node_gold_ranks");
    let registry = Arc::new(FixtureRegistry::holding([(
        "fixture".to_owned(),
        benchmark.clone(),
    )]));
    let (mut judged_rankings, mut unjudged_rankings) = (0, 0);
    for (query, document) in &run.traces {
        let trace = Trace::try_from(document).expect("the fixture's traces read");
        let response = send(
            app_over(
                FakeRunStore::holding([run.clone()]),
                registry.clone(),
                &workspace,
            ),
            get(&format!("/api/v1/runs/{}/trace/{}", run.id, query.as_str())),
        )
        .await;
        assert_eq!(response.status(), 200);
        let body = json(response).await;
        let judgments = benchmark
            .qrels()
            .for_query(query)
            .filter(|judgments| !judgments.is_empty());
        for entry in &trace.nodes {
            let served = node(&body, entry.node.as_str());
            let at = format!("query {}, node {}", query.as_str(), entry.node.as_str());
            let Some(TraceSummary::RankedChunks { chunks }) = &entry.output else {
                assert!(served["gold_ranks"].is_null(), "{at}");
                continue;
            };
            let passages = served["output"]["chunks"].as_array().unwrap();
            let Some(judgments) = judgments else {
                assert!(served["gold_ranks"].is_null(), "{at}");
                assert!(
                    passages.iter().all(|passage| passage["grade"].is_null()),
                    "{at}"
                );
                unjudged_rankings += 1;
                continue;
            };
            let folded = documents_by_first_occurrence(chunks.iter().map(|chunk| &chunk.document));
            let ranks: Vec<u64> = folded
                .iter()
                .enumerate()
                .filter(|(_, document)| judgments.get(*document).is_some_and(|grade| *grade > 0))
                .map(|(position, _)| position as u64 + 1)
                .collect();
            assert_eq!(served["gold_ranks"], serde_json::json!(ranks), "{at}");
            assert_eq!(passages.len(), chunks.len(), "{at}");
            for (chunk, passage) in chunks.iter().zip(passages) {
                let grade = judgments.get(&chunk.document).copied().unwrap_or(0);
                assert_eq!(passage["grade"], grade, "{at}");
            }
            judged_rankings += 1;
        }
    }
    assert!(judged_rankings > 0, "a judged ranking was compared");
    assert!(
        unjudged_rankings > 0,
        "an unjudged query's ranking was seen"
    );
}
