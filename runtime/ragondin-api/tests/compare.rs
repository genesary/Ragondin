//! `POST /api/v1/compare`: runs of one benchmark against a baseline — the
//! metric table, the configuration matrix, the stages the runs are aligned
//! by, the manual pairing, the per-query deltas and their bins, and the
//! latency per node.

mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use axum::http::StatusCode;
use ragondin_api::fs::{FsPipelines, Workspace};
use ragondin_api::{router, Backends, NoAssets, Server, ServerConfig};
use ragondin_benchmarks::{Benchmark, Qrels};
use ragondin_experiments::{Run, Trace, TraceChunk};
use ragondin_types::{DocId, Document, Query, QueryId};
use serde_json::{json, Value};
use support::datasets::scratch;
use support::runs::{chunk, counted, node, query, ranked, run_over};
use support::{fakes, json, send, write_request, FakeRunStore, FixtureRegistry, BUILD, SERVED};

const DENSE_ONLY: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: dense
      component: retriever
      impl: dense
      inputs: [question]
      params: { top_k: 100 }
";

const HYBRID: &str = "\
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
";

const HYBRID_RERANK: &str = "\
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

/// One retriever and the reranker over it: the pipeline a person pairs by
/// hand with [`HYBRID_RERANK`], since its one leg stands where the other's
/// fusion does.
const COLBERT_RERANK: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: colbert
      component: retriever
      impl: colbert
      inputs: [question]
    - id: rerank
      component: reranker
      impl: cross_encoder
      inputs: [question, colbert]
";

/// Seven judged queries, `q1`…`q7`, each with one gold document `g<n>`, and
/// `q8`, judged on nothing.
fn benchmark() -> Benchmark {
    let document = |id: &str| Document {
        id: DocId::new(id),
        text: format!("the text of {id}"),
        metadata: Default::default(),
    };
    let mut documents: Vec<Document> = (1..=7).map(|n| document(&format!("g{n}"))).collect();
    documents.extend((1..=4).map(|n| document(&format!("x{n}"))));
    let queries = (1..=8)
        .map(|n| Query {
            id: QueryId::new(format!("q{n}")),
            text: format!("question {n}"),
        })
        .collect();
    let mut qrels = Qrels::new();
    for n in 1..=7 {
        qrels.insert(
            QueryId::new(format!("q{n}")),
            DocId::new(format!("g{n}")),
            1,
        );
    }
    Benchmark::new(documents, queries, qrels)
}

/// A ranking with query `n`'s gold document at `rank` (from 1) behind
/// filler documents, or filler alone when `rank` is `None`.
fn ranking(n: usize, rank: Option<usize>) -> Vec<TraceChunk> {
    let fill = rank.map_or(1, |rank| rank - 1);
    let mut chunks: Vec<TraceChunk> = (1..=fill)
        .map(|f| chunk(&format!("x{f}"), &format!("x{f}"), 1.0 / f as f64))
        .collect();
    if rank.is_some() {
        chunks.push(chunk(&format!("g{n}"), &format!("g{n}"), 0.01));
    }
    chunks
}

/// Where the gold document stands for each query, at the baseline's output
/// and at the candidates': the reciprocal ranks move by −0.5, −0.25, −0.05,
/// 0, +0.05, +0.25 and +1 — one query in each of the seven bins.
const BASELINE_RANKS: [Option<usize>; 8] = [
    Some(1),
    Some(2),
    Some(4),
    Some(1),
    Some(5),
    Some(4),
    None,
    Some(1),
];
const CANDIDATE_RANKS: [Option<usize>; 8] = [
    Some(2),
    Some(4),
    Some(5),
    Some(1),
    Some(4),
    Some(2),
    Some(1),
    Some(1),
];

fn queries() -> impl Iterator<Item = (usize, String)> {
    (1..=8).map(|n| (n, format!("q{n}")))
}

fn dense_only_run(id: u8) -> Run {
    let traces = queries()
        .map(|(n, q)| {
            let trace = Trace {
                nodes: vec![node(
                    "dense",
                    vec![query(&q)],
                    ranked(ranking(n, BASELINE_RANKS[n - 1])),
                    100 * n as u64,
                )],
            };
            (q, trace)
        })
        .collect::<Vec<_>>();
    run_over(
        id,
        DENSE_ONLY,
        &benchmark(),
        traces
            .iter()
            .map(|(q, t)| (q.as_str(), t.clone()))
            .collect(),
        &[("mrr", 0.5), ("ndcg@10", 0.6), ("latency_p50_ms", 40.0)],
    )
}

fn hybrid_run(id: u8) -> Run {
    let traces = queries()
        .map(|(n, q)| {
            let trace = Trace {
                nodes: vec![
                    node("bm25", vec![query(&q)], ranked(ranking(n, None)), 10),
                    node(
                        "dense",
                        vec![query(&q)],
                        ranked(ranking(n, BASELINE_RANKS[n - 1])),
                        20,
                    ),
                    node(
                        "rrf",
                        vec![counted(1), counted(1)],
                        ranked(ranking(n, CANDIDATE_RANKS[n - 1])),
                        1,
                    ),
                ],
            };
            (q, trace)
        })
        .collect::<Vec<_>>();
    run_over(
        id,
        HYBRID,
        &benchmark(),
        traces
            .iter()
            .map(|(q, t)| (q.as_str(), t.clone()))
            .collect(),
        &[("mrr", 0.6), ("ndcg@10", 0.7), ("latency_p50_ms", 45.0)],
    )
}

fn hybrid_rerank_run(id: u8) -> Run {
    let traces = queries()
        .map(|(n, q)| {
            let trace = Trace {
                nodes: vec![
                    node("bm25", vec![query(&q)], ranked(ranking(n, None)), 10),
                    node(
                        "dense",
                        vec![query(&q)],
                        ranked(ranking(n, BASELINE_RANKS[n - 1])),
                        20,
                    ),
                    node(
                        "rrf",
                        vec![counted(1), counted(1)],
                        ranked(ranking(n, BASELINE_RANKS[n - 1])),
                        1,
                    ),
                    node(
                        "rerank",
                        vec![query(&q), counted(1)],
                        ranked(ranking(n, CANDIDATE_RANKS[n - 1])),
                        300,
                    ),
                ],
            };
            (q, trace)
        })
        .collect::<Vec<_>>();
    run_over(
        id,
        HYBRID_RERANK,
        &benchmark(),
        traces
            .iter()
            .map(|(q, t)| (q.as_str(), t.clone()))
            .collect(),
        &[("mrr", 0.7), ("recall@10", 0.9), ("latency_p50_ms", 400.0)],
    )
}

fn colbert_rerank_run(id: u8) -> Run {
    let traces = queries()
        .map(|(n, q)| {
            let trace = Trace {
                nodes: vec![
                    node(
                        "colbert",
                        vec![query(&q)],
                        ranked(ranking(n, BASELINE_RANKS[n - 1])),
                        50,
                    ),
                    node(
                        "rerank",
                        vec![query(&q), counted(1)],
                        ranked(ranking(n, CANDIDATE_RANKS[n - 1])),
                        300,
                    ),
                ],
            };
            (q, trace)
        })
        .collect::<Vec<_>>();
    run_over(
        id,
        COLBERT_RERANK,
        &benchmark(),
        traces
            .iter()
            .map(|(q, t)| (q.as_str(), t.clone()))
            .collect(),
        &[("mrr", 0.7)],
    )
}

/// The router over `runs`, a registry holding the fixture benchmark, and
/// `backends`' pipelines, working in `workspace`.
fn app(runs: Vec<Run>, workspace: &Path, pipelines: Option<FsPipelines>) -> Server {
    let mut backends: Backends = fakes(FakeRunStore::holding(runs));
    backends.registry = Arc::new(FixtureRegistry::holding([(
        "beir/fixture".to_owned(),
        benchmark(),
    )]));
    if let Some(pipelines) = pipelines {
        backends.pipelines = Arc::new(pipelines);
    }
    router(
        backends,
        ServerConfig {
            served: SERVED.to_owned(),
            build: BUILD.to_owned(),
            workspace: workspace.to_path_buf(),
        },
        Arc::new(NoAssets),
    )
}

async fn post_compare(app: Server, body: Value) -> (StatusCode, Value) {
    let response = send(app, write_request("POST", "/api/v1/compare", &body, &[])).await;
    (response.status(), json(response).await)
}

fn ids(runs: &[&Run]) -> Vec<String> {
    runs.iter().map(|run| run.id.to_string()).collect()
}

fn row<'a>(body: &'a Value, stage: &str) -> &'a Value {
    body["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["stage"] == stage)
        .unwrap_or_else(|| panic!("a `{stage}` row in {}", body["stages"]))
}

/// The node ids of a cell, or `None` for "no stage here".
fn cell(row: &Value, column: usize) -> Option<Vec<String>> {
    let cell = &row["cells"][column];
    match cell["kind"].as_str() {
        Some("absent") => None,
        Some("present") => Some(
            cell["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|node| node["node"].as_str().unwrap().to_owned())
                .collect(),
        ),
        other => panic!("a cell is present or absent, not {other:?}: {cell}"),
    }
}

fn strings(names: &[&str]) -> Option<Vec<String>> {
    Some(names.iter().map(|name| (*name).to_owned()).collect())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dense_only_pipeline_pairs_with_a_hybrid_rerank_one_by_stage() {
    let (dense, rerank) = (dense_only_run(0x01), hybrid_rerank_run(0x02));
    let workspace = scratch("compare_by_stage");

    let (status, body) = post_compare(
        app(vec![dense.clone(), rerank.clone()], &workspace, None),
        json!({ "run_ids": ids(&[&dense, &rerank]), "baseline": dense.id.to_string() }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let stages: Vec<&str> = body["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["stage"].as_str().unwrap())
        .collect();
    assert_eq!(
        stages,
        [
            "retrieval_legs",
            "after_fusion",
            "after_rerank",
            "final_ranking"
        ]
    );
    let legs = row(&body, "retrieval_legs");
    assert_eq!(cell(legs, 0), strings(&["dense"]));
    assert_eq!(cell(legs, 1), strings(&["bm25", "dense"]));
    assert_eq!(cell(row(&body, "after_fusion"), 0), None);
    assert_eq!(cell(row(&body, "after_fusion"), 1), strings(&["rrf"]));
    assert_eq!(cell(row(&body, "after_rerank"), 0), None);
    assert_eq!(cell(row(&body, "after_rerank"), 1), strings(&["rerank"]));
    let last = row(&body, "final_ranking");
    assert_eq!(cell(last, 0), strings(&["dense"]));
    assert_eq!(cell(last, 1), strings(&["rerank"]));
    for row in body["stages"].as_array().unwrap() {
        assert_eq!(row["source"], "automatic", "{row}");
        assert_eq!(row["confidence"], "high", "{row}");
        assert_eq!(row["label"], Value::Null, "{row}");
    }
    // A missing stage is an explicit absence, never a zero.
    assert_eq!(
        row(&body, "after_fusion")["cells"][0],
        json!({ "kind": "absent" })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn each_stage_cell_carries_its_node_s_metric_over_the_judged_queries() {
    let (dense, rerank) = (dense_only_run(0x01), hybrid_rerank_run(0x02));
    let workspace = scratch("compare_stage_metrics");

    let (_, body) = post_compare(
        app(vec![dense.clone(), rerank.clone()], &workspace, None),
        json!({ "run_ids": ids(&[&dense, &rerank]), "baseline": dense.id.to_string() }),
    )
    .await;

    let reciprocal = |rank: Option<usize>| rank.map_or(0.0, |rank| 1.0 / rank as f64);
    let mean =
        |ranks: &[Option<usize>]| ranks[..7].iter().map(|r| reciprocal(*r)).sum::<f64>() / 7.0;
    let last = row(&body, "final_ranking");
    let baseline = last["cells"][0]["nodes"][0]["metrics"]["mrr"]
        .as_f64()
        .unwrap();
    let candidate = last["cells"][1]["nodes"][0]["metrics"]["mrr"]
        .as_f64()
        .unwrap();
    assert!(
        (baseline - mean(&BASELINE_RANKS)).abs() < 1e-12,
        "{baseline}"
    );
    assert!(
        (candidate - mean(&CANDIDATE_RANKS)).abs() < 1e-12,
        "{candidate}"
    );
    // Of the two legs, the best is named per metric: `dense` finds the gold
    // documents where `bm25` finds none.
    let legs = &row(&body, "retrieval_legs")["cells"][1];
    assert_eq!(legs["best"]["mrr"]["node"], "dense");
    assert!((legs["best"]["mrr"]["value"].as_f64().unwrap() - mean(&BASELINE_RANKS)).abs() < 1e-12);
}

#[tokio::test(flavor = "multi_thread")]
async fn three_runs_give_the_metric_table_with_best_and_deltas_and_the_differing_parameters() {
    let (dense, hybrid, rerank) = (
        dense_only_run(0x01),
        hybrid_run(0x02),
        hybrid_rerank_run(0x03),
    );
    let workspace = scratch("compare_table");

    let (status, body) = post_compare(
        app(
            vec![dense.clone(), hybrid.clone(), rerank.clone()],
            &workspace,
            None,
        ),
        json!({
            "run_ids": ids(&[&hybrid, &dense, &rerank]),
            "baseline": dense.id.to_string(),
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    // The baseline first, then the others in the order given.
    let runs: Vec<&str> = body["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(runs, ids(&[&dense, &hybrid, &rerank]));
    let metric = |name: &str| {
        body["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap_or_else(|| panic!("a `{name}` row"))
            .clone()
    };
    let mrr = metric("mrr");
    assert_eq!(mrr["direction"], "higher");
    assert_eq!(mrr["values"], json!([0.5, 0.6, 0.7]));
    assert_eq!(mrr["best"], json!([rerank.id.to_string()]));
    let deltas: Vec<f64> = mrr["deltas"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_f64().unwrap())
        .collect();
    assert!(
        (deltas[0]).abs() < 1e-12
            && (deltas[1] - 0.1).abs() < 1e-12
            && (deltas[2] - 0.2).abs() < 1e-12
    );
    let latency = metric("latency_p50_ms");
    assert_eq!(latency["direction"], "lower");
    assert_eq!(latency["best"], json!([dense.id.to_string()]));
    let recall = metric("recall@10");
    assert_eq!(recall["values"], json!([null, null, 0.9]));
    assert_eq!(recall["deltas"], json!([null, null, null]));

    let matrix = &body["configuration"];
    assert_eq!(matrix["kind"], "compared");
    assert_eq!(matrix["same_logical_form"], false);
    let rows: Vec<(String, Value)> = matrix["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["node"].as_str().unwrap().to_owned(), row["key"].clone()))
        .collect();
    // `dense`'s family and `impl:` are one value across the three; its
    // `top_k` is not.
    assert!(rows.contains(&(
        "dense".to_owned(),
        json!({ "kind": "param", "name": "top_k" })
    )));
    assert!(!rows.contains(&("dense".to_owned(), json!({ "kind": "impl" }))));
    assert!(rows.contains(&("rerank".to_owned(), json!({ "kind": "component" }))));
    let top_k = matrix["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["node"] == "dense")
        .unwrap();
    assert_eq!(top_k["values"], json!([100, 50, 50]));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_seven_bins_partition_the_judged_queries_and_list_them() {
    let (dense, rerank) = (dense_only_run(0x01), hybrid_rerank_run(0x02));
    let workspace = scratch("compare_bins");

    let (_, body) = post_compare(
        app(vec![dense.clone(), rerank.clone()], &workspace, None),
        json!({ "run_ids": ids(&[&dense, &rerank]), "baseline": dense.id.to_string() }),
    )
    .await;

    assert_eq!(body["ground_truth"]["status"], "verified");
    let deltas = body["query_deltas"].as_array().unwrap();
    assert_eq!(deltas.len(), 1, "one entry per run other than the baseline");
    assert_eq!(deltas[0]["run"], rerank.id.to_string());
    let mrr = deltas[0]["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|metric| metric["metric"] == "mrr")
        .unwrap();
    // `q8` is judged on nothing: it is in no bin.
    assert_eq!(mrr["judged_queries"], 7);
    let bins = mrr["bins"].as_array().unwrap();
    let names: Vec<&str> = bins
        .iter()
        .map(|bin| bin["bin"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "much_worse",
            "worse",
            "slightly_worse",
            "unchanged",
            "slightly_better",
            "better",
            "much_better"
        ]
    );
    let total: u64 = bins.iter().map(|bin| bin["count"].as_u64().unwrap()).sum();
    assert_eq!(total, 7);
    let listed: Vec<Vec<&str>> = bins
        .iter()
        .map(|bin| {
            bin["queries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|q| q.as_str().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(
        listed,
        [
            vec!["q1"],
            vec!["q2"],
            vec!["q3"],
            vec!["q4"],
            vec!["q5"],
            vec!["q6"],
            vec!["q7"]
        ]
    );
    let q7 = mrr["deltas"]
        .as_array()
        .unwrap()
        .iter()
        .find(|delta| delta["query"] == "q7")
        .unwrap();
    assert_eq!(q7["delta"], 1.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_latency_of_each_node_is_its_median_over_the_queries() {
    let (dense, rerank) = (dense_only_run(0x01), hybrid_rerank_run(0x02));
    let workspace = scratch("compare_latency");

    let (_, body) = post_compare(
        app(vec![dense.clone(), rerank.clone()], &workspace, None),
        json!({ "run_ids": ids(&[&dense, &rerank]), "baseline": dense.id.to_string() }),
    )
    .await;

    let latency = body["latency"].as_array().unwrap();
    assert_eq!(latency[0]["run"], dense.id.to_string());
    // 100, 200, … 800 ns: the lower of the two middle values.
    assert_eq!(
        latency[0]["nodes"],
        json!([{ "node": "dense", "family": "retriever", "median_nanos": 400, "queries": 8 }])
    );
    let families: Vec<(&str, &str)> = latency[1]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            (
                node["node"].as_str().unwrap(),
                node["family"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        families,
        [
            ("bm25", "retriever"),
            ("dense", "retriever"),
            ("rerank", "reranker"),
            ("rrf", "fusion")
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_of_different_benchmarks_are_refused_naming_both_versions() {
    let dense = dense_only_run(0x01);
    let mut elsewhere = hybrid_run(0x02);
    elsewhere.inputs.dataset_version = "elsewhere@1".to_owned();
    let workspace = scratch("compare_benchmarks_differ");

    let (status, body) = post_compare(
        app(vec![dense.clone(), elsewhere.clone()], &workspace, None),
        json!({ "run_ids": ids(&[&dense, &elsewhere]), "baseline": dense.id.to_string() }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "runs_not_comparable");
    let detail = body["detail"].as_str().unwrap();
    assert!(detail.contains("elsewhere@1"), "{detail}");
    assert!(detail.contains(&dense.inputs.dataset_version), "{detail}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sixth_run_is_refused_naming_the_ceiling() {
    let runs: Vec<Run> = (1..=6).map(dense_only_run).collect();
    let workspace = scratch("compare_ceiling");
    let all: Vec<&Run> = runs.iter().collect();

    let (status, body) = post_compare(
        app(runs.clone(), &workspace, None),
        json!({ "run_ids": ids(&all), "baseline": runs[0].id.to_string() }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "runs_not_comparable");
    let detail = body["detail"].as_str().unwrap();
    assert!(
        detail.contains("a baseline and at most four runs"),
        "{detail}"
    );

    // Five is the ceiling, not over it.
    let (status, body) = post_compare(
        app(runs.clone(), &workspace, None),
        json!({ "run_ids": ids(&all[..5]), "baseline": runs[0].id.to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_that_does_not_name_its_baseline_among_its_runs_is_invalid() {
    let (dense, hybrid, rerank) = (
        dense_only_run(0x01),
        hybrid_run(0x02),
        hybrid_rerank_run(0x03),
    );
    let workspace = scratch("compare_invalid");
    let app = || {
        app(
            vec![dense.clone(), hybrid.clone(), rerank.clone()],
            &workspace,
            None,
        )
    };

    for body in [
        json!({ "run_ids": ids(&[&dense, &hybrid]), "baseline": rerank.id.to_string() }),
        json!({ "run_ids": ids(&[&dense]), "baseline": dense.id.to_string() }),
        json!({ "run_ids": ids(&[&dense, &dense]), "baseline": dense.id.to_string() }),
        json!({ "run_ids": ids(&[&dense, &hybrid]), "baseline": dense.id.to_string(), "colour": "red" }),
    ] {
        let (status, problem) = post_compare(app(), body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {problem}");
        assert_eq!(problem["code"], "request_invalid", "{body}");
    }
}

/// A workspace holding `hybrid-rerank` and `colbert-rerank` as pipeline
/// documents, whose runs are therefore named after them.
fn paired_workspace(test: &str) -> (std::path::PathBuf, Workspace) {
    let root = scratch(test);
    let workspace = Workspace::open(&root).expect("a scratch directory opens as a workspace");
    fs::write(
        workspace.pipelines().join("hybrid-rerank.yaml"),
        HYBRID_RERANK,
    )
    .unwrap();
    fs::write(
        workspace.pipelines().join("colbert-rerank.yaml"),
        COLBERT_RERANK,
    )
    .unwrap();
    (root, workspace)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manual_pairing_is_kept_read_in_both_directions_and_reset_to_automatic() {
    let (hybrid, colbert) = (hybrid_rerank_run(0x01), colbert_rerank_run(0x02));
    let (root, workspace) = paired_workspace("compare_manual_pairing");
    let app = || {
        app(
            vec![hybrid.clone(), colbert.clone()],
            &root,
            Some(FsPipelines::new(&workspace)),
        )
    };
    let runs = ids(&[&hybrid, &colbert]);

    // Pair `rrf` with `colbert`, both the candidates the rerankers see.
    let (status, body) = post_compare(
        app(),
        json!({
            "run_ids": runs,
            "baseline": hybrid.id.to_string(),
            "pairing": {
                "pipeline": "hybrid-rerank",
                "other": "colbert-rerank",
                "pairs": [{ "node": "rrf", "other": "colbert", "label": "candidates before rerank" }],
            },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let file = workspace
        .pipelines()
        .join("hybrid-rerank.pairing/colbert-rerank.json");
    assert!(
        file.is_file(),
        "the pairing is kept under the first pipeline's name"
    );
    let kept: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(kept["pipeline"], "hybrid-rerank");
    assert_eq!(kept["other"], "colbert-rerank");
    assert_eq!(body["runs"][0]["pipeline"], "hybrid-rerank");
    assert_eq!(body["runs"][1]["pipeline"], "colbert-rerank");
    let fusion = row(&body, "after_fusion");
    assert_eq!(fusion["source"], "manual");
    assert_eq!(fusion["label"], "candidates before rerank");
    assert_eq!(cell(fusion, 0), strings(&["rrf"]));
    assert_eq!(cell(fusion, 1), strings(&["colbert"]));
    assert_eq!(fusion["cells"][1]["nodes"][0]["paired_by_hand"], true);
    // `colbert` left its automatic stage: the legs row has no stage there.
    assert_eq!(cell(row(&body, "retrieval_legs"), 1), None);
    assert_eq!(row(&body, "retrieval_legs")["source"], "automatic");

    // The next comparison remembers it, in the other direction too.
    let (status, body) = post_compare(
        app(),
        json!({ "run_ids": runs, "baseline": colbert.id.to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["pairings"],
        json!([{
            "pipeline": "colbert-rerank",
            "other": "hybrid-rerank",
            "pairs": [{ "node": "colbert", "other": "rrf", "label": "candidates before rerank" }],
        }])
    );
    let legs = row(&body, "retrieval_legs");
    assert_eq!(legs["source"], "manual");
    assert_eq!(cell(legs, 0), strings(&["colbert"]));
    assert_eq!(cell(legs, 1), strings(&["bm25", "dense", "rrf"]));

    // "Reset to automatic": no pair is the automatic pairing, and the file goes.
    let (status, body) = post_compare(
        app(),
        json!({
            "run_ids": runs,
            "baseline": colbert.id.to_string(),
            "pairing": { "pipeline": "colbert-rerank", "other": "hybrid-rerank", "pairs": [] },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!file.exists(), "reset removes the file");
    assert_eq!(body["pairings"], json!([]));
    for row in body["stages"].as_array().unwrap() {
        assert_eq!(row["source"], "automatic", "{row}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_naming_a_node_its_pipeline_lacks_is_refused_and_nothing_is_kept() {
    let (hybrid, colbert) = (hybrid_rerank_run(0x01), colbert_rerank_run(0x02));
    let (root, workspace) = paired_workspace("compare_pairing_refused");

    let (status, body) = post_compare(
        app(
            vec![hybrid.clone(), colbert.clone()],
            &root,
            Some(FsPipelines::new(&workspace)),
        ),
        json!({
            "run_ids": ids(&[&hybrid, &colbert]),
            "baseline": hybrid.id.to_string(),
            "pairing": {
                "pipeline": "hybrid-rerank",
                "other": "colbert-rerank",
                "pairs": [{ "node": "fused", "other": "colbert" }],
            },
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "request_invalid");
    assert!(body["detail"].as_str().unwrap().contains("fused"));
    assert!(!workspace.pipelines().join("hybrid-rerank.pairing").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_no_pipeline_document_matches_has_no_pipeline_name() {
    let (dense, rerank) = (dense_only_run(0x01), hybrid_rerank_run(0x02));
    let (root, workspace) = paired_workspace("compare_unnamed");

    let (_, body) = post_compare(
        app(
            vec![dense.clone(), rerank.clone()],
            &root,
            Some(FsPipelines::new(&workspace)),
        ),
        json!({ "run_ids": ids(&[&dense, &rerank]), "baseline": dense.id.to_string() }),
    )
    .await;

    assert_eq!(body["runs"][0]["pipeline"], Value::Null);
    assert_eq!(body["runs"][1]["pipeline"], "hybrid-rerank");
}
