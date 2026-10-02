//! `GET /api/v1/runs/{id}/trace/{query}`: one query's trace, node by node,
//! with passage text resolved only against the run's own dataset (ADR-C36
//! § 4) — through the `Registry` file backend, over a copy of
//! `ragondin-benchmarks`' miniature BEIR fixture on disk.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::http::StatusCode;
use ragondin_api::fs::FsRegistry;
use ragondin_benchmarks::manifest::Format;
use ragondin_benchmarks::Benchmark;
use ragondin_experiments::{Run, Trace};
use serde_json::Value;
use support::datasets::{beir_mini_entry, benchmark_fixture, copy_dir, scratch, version_of};
use support::runs::{
    chunk, documents, failed, generation_trace, query, ranked, run_over, GENERATION,
};
use support::{app_over, get, json, send, FakeRunStore};

/// The manifest name the run's dataset is pinned under, and its directory.
const NAME: &str = "beir/mini";
const DIR: &str = "mini";

fn beir_mini() -> Benchmark {
    Format::Beir
        .load(&benchmark_fixture("beir-mini"))
        .expect("the fixture loads")
}

/// A generation run over the miniature fixture: `q-1` runs to its answer;
/// `q-2`'s reranker failed, so the trace stops there.
fn the_run() -> Run {
    let q1 = generation_trace(
        "q-1",
        documents(&["4983", "MED-10"]),
        documents(&["MED-10", "4983", "MED-12"]),
        2,
        "on the mat",
    );
    let q2 = Trace {
        nodes: vec![
            support::runs::node("leg", vec![query("q-2")], ranked(documents(&["4983"])), 500),
            failed(
                "reranked",
                vec![query("q-2"), support::runs::counted(1)],
                "the reranker timed out",
                700,
            ),
        ],
    };
    run_over(
        0x41,
        GENERATION,
        &beir_mini(),
        vec![("q-1", q1), ("q-2", q2)],
        &[
            ("exact_match", 0.0),
            ("mrr", 0.5),
            ("ndcg@10", 0.5),
            ("recall@10", 0.5),
            ("token_f1", 0.0),
        ],
    )
}

/// A workspace whose datasets directory holds the fixture under [`DIR`]
/// when `on_disk`, and a registry whose manifest pins [`NAME`] to the
/// fixture's digest.
fn workspace(test: &str, on_disk: bool) -> (PathBuf, FsRegistry) {
    let workspace = scratch(test);
    let datasets = workspace.join("datasets");
    fs::create_dir_all(&datasets).unwrap();
    let fixture = benchmark_fixture("beir-mini");
    if on_disk {
        copy_dir(&fixture, &datasets.join(DIR));
    }
    let version = version_of(Format::Beir, &fixture);
    let registry = FsRegistry::new(
        datasets,
        vec![beir_mini_entry(NAME, "https://example.invalid", &version)],
    );
    (workspace, registry)
}

async fn trace_of(
    workspace: &Path,
    registry: FsRegistry,
    run: &Run,
    query: &str,
) -> (StatusCode, Value) {
    let response = send(
        app_over(
            FakeRunStore::holding([run.clone()]),
            Arc::new(registry),
            workspace,
        ),
        get(&format!("/api/v1/runs/{}/trace/{query}", run.id)),
    )
    .await;
    (response.status(), json(response).await)
}

async fn queries_of(
    workspace: &Path,
    registry: FsRegistry,
    run: &Run,
    parameters: &str,
) -> (StatusCode, Value) {
    let response = send(
        app_over(
            FakeRunStore::holding([run.clone()]),
            Arc::new(registry),
            workspace,
        ),
        get(&format!("/api/v1/runs/{}/queries{parameters}", run.id)),
    )
    .await;
    (response.status(), json(response).await)
}

fn node<'a>(body: &'a Value, id: &str) -> &'a Value {
    body["nodes"]
        .as_array()
        .expect("the trace lists its nodes")
        .iter()
        .find(|node| node["node"] == id)
        .unwrap_or_else(|| panic!("node {id} is in the trace"))
}

/// Every chunk the trace names, ranking and context alike.
fn chunks(body: &Value) -> Vec<&Value> {
    body["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["output"]["chunks"].as_array())
        .flatten()
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_verified_dataset_resolves_the_text_of_every_named_chunk() {
    let (workspace, registry) = workspace("replay_verified", true);
    let run = the_run();

    let (status, body) = trace_of(&workspace, registry, &run, "q-1").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["run"], run.id.to_string());
    assert_eq!(body["query"], "q-1");
    assert_eq!(body["passages"]["status"], "verified");
    assert_eq!(body["passages"]["benchmark"], NAME);
    let named = chunks(&body);
    assert_eq!(named.len(), 2 + 3 + 2, "two rankings and a context");
    for chunk in &named {
        assert!(
            chunk["text"].is_string(),
            "every named chunk has its text: {chunk}"
        );
    }
    let med_10 = &node(&body, "reranked")["output"]["chunks"][0];
    assert_eq!(med_10["chunk"], "MED-10");
    assert!(med_10["text"]
        .as_str()
        .unwrap()
        .contains("The cat sat on the mat."));
    assert_eq!(node(&body, "prompt")["output"]["kind"], "context");
    assert!(node(&body, "prompt")["output"]["chunks"][1]["text"]
        .as_str()
        .unwrap()
        .contains("no title field"));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_absent_dataset_is_flagged_and_the_ids_are_kept_without_text() {
    let (workspace, registry) = workspace("replay_absent", false);
    let run = the_run();

    let (status, body) = trace_of(&workspace, registry, &run, "q-1").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["passages"]["status"], "dataset_absent");
    assert_eq!(body["passages"]["benchmark"], NAME);
    assert_eq!(
        body["passages"]["expected"]["dataset_version"],
        run.inputs.dataset_version
    );
    assert!(body["passages"]["found"].is_null());
    let named = chunks(&body);
    assert_eq!(named.len(), 7);
    for chunk in &named {
        assert!(
            chunk["text"].is_null(),
            "no text without the dataset: {chunk}"
        );
    }
    let ids: Vec<&str> = node(&body, "reranked")["output"]["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chunk| chunk["chunk"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["MED-10", "4983", "MED-12"], "the ids are intact");
}

#[tokio::test(flavor = "multi_thread")]
async fn one_altered_byte_makes_the_dataset_differ_and_both_digests_are_reported() {
    let (workspace, registry) = workspace("replay_differs", true);
    let corpus = workspace.join("datasets").join(DIR).join("corpus.jsonl");
    let original = fs::read_to_string(&corpus).unwrap();
    let altered = original.replacen("The cat sat", "The bat sat", 1);
    assert_eq!(
        altered.len(),
        original.len(),
        "one byte, not a length change"
    );
    assert_ne!(altered, original);
    fs::write(&corpus, altered).unwrap();
    let run = the_run();

    let (status, body) = trace_of(&workspace, registry, &run, "q-1").await;

    assert_eq!(status, StatusCode::OK);
    let passages = &body["passages"];
    assert_eq!(passages["status"], "dataset_differs");
    assert_eq!(passages["benchmark"], NAME);
    assert_eq!(
        passages["expected"]["dataset_version"],
        run.inputs.dataset_version
    );
    assert_eq!(
        passages["expected"]["index_version"],
        run.inputs.index_version
    );
    let found = passages["found"]["dataset_version"].as_str().unwrap();
    assert_eq!(found.len(), 64);
    assert_ne!(found, run.inputs.dataset_version);
    for chunk in chunks(&body) {
        assert!(
            chunk["text"].is_null(),
            "no text from another corpus: {chunk}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dataset_on_disk_that_does_not_load_is_unreadable_with_nothing_found() {
    let (workspace, registry) = workspace("replay_unreadable_dataset", true);
    fs::remove_file(workspace.join("datasets").join(DIR).join("corpus.jsonl")).unwrap();

    let (status, body) = trace_of(&workspace, registry.clone(), &the_run(), "q-1").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["passages"]["status"], "dataset_unreadable");
    assert!(body["passages"]["found"].is_null());
    assert!(body["passages"]["detail"]
        .as_str()
        .unwrap()
        .contains("does not load"));
    let listing = queries_of(&workspace, registry, &the_run(), "").await.1;
    assert_eq!(listing["ground_truth"]["status"], "dataset_unreadable");
}

/// The dataset is the run's, but the chunk set this build derives from it is
/// not the one the run retrieved over — the derivation moved. ADR-C36 § 4
/// conditions the *text* on both digests, so the passages are `index_differs`
/// with the chunk set's two digests side by side and no text; the scores
/// depend on the dataset alone, so they are read as usual.
#[tokio::test(flavor = "multi_thread")]
async fn a_chunk_set_that_is_not_the_runs_hides_the_text_and_keeps_the_scores() {
    let (workspace, registry) = workspace("replay_index_differs", true);
    let mut run = the_run();
    run.inputs.index_version = "f".repeat(64);

    let (status, body) = trace_of(&workspace, registry.clone(), &run, "q-1").await;

    assert_eq!(status, StatusCode::OK);
    let passages = &body["passages"];
    assert_eq!(passages["status"], "index_differs");
    assert_eq!(
        passages["found"]["dataset_version"],
        run.inputs.dataset_version
    );
    let found = passages["found"]["index_version"].as_str().unwrap();
    assert_ne!(found, run.inputs.index_version);
    assert_eq!(
        passages["expected"]["index_version"],
        run.inputs.index_version
    );
    for chunk in chunks(&body) {
        assert!(chunk["text"].is_null(), "{chunk}");
    }
    assert!(body["scores"]["ndcg@10"].is_number(), "{}", body["scores"]);
    assert!(node(&body, "reranked")["metrics"].is_object());

    // The listing reads its scores, twice — the second time from the cache —
    // and the filter, which needs only the ground truth, answers.
    for _ in 0..2 {
        let (status, listing) = queries_of(&workspace, registry.clone(), &run, "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listing["ground_truth"]["status"], "verified");
        assert!(listing["queries"][0]["scores"]["ndcg@10"].is_number());
    }
    let (status, _) = queries_of(&workspace, registry, &run, "?missing_gold_at=3").await;
    assert_eq!(status, StatusCode::OK);
}

/// A run deleted and launched again keeps its id — the id digests the
/// inputs — but a nondeterministic component may give it other traces. The
/// cache must not serve the old run's figures for the new one.
#[tokio::test(flavor = "multi_thread")]
async fn a_rerun_under_the_same_id_with_other_traces_is_recomputed_not_served_stale() {
    let (workspace, registry) = workspace("replay_cache_rerun", true);
    let first = the_run();
    let (_, before) = queries_of(&workspace, registry.clone(), &first, "").await;
    let q1 = |body: &Value| {
        body["queries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == "q-1")
            .unwrap()["scores"]
            .clone()
    };
    // q-1 judges MED-10 (grade 2): first at rank 1, then at rank 3.
    assert_eq!(q1(&before)["mrr"], 1.0);

    let mut rerun = first.clone();
    rerun.traces.insert(
        ragondin_types::QueryId::new("q-1"),
        ragondin_experiments::TraceDocument::from(generation_trace(
            "q-1",
            documents(&["4983", "MED-10"]),
            documents(&["4983", "MED-12", "MED-10"]),
            2,
            "on the mat",
        )),
    );
    let (_, after) = queries_of(&workspace, registry, &rerun, "").await;

    assert_eq!(after["ground_truth"]["status"], "verified");
    assert!(
        (q1(&after)["mrr"].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-12,
        "recomputed from the new traces: {}",
        q1(&after)
    );
}

/// The cache is never a truth: a workspace where it cannot be written still
/// answers, with every figure, and says the cache failed.
#[tokio::test(flavor = "multi_thread")]
async fn a_cache_that_cannot_be_written_does_not_fail_the_request() {
    let (workspace, registry) = workspace("replay_cache_unwritable", true);
    // A file where the cache directory would go: nothing can be created
    // under it.
    fs::write(workspace.join("cache"), "not a directory").unwrap();
    let run = the_run();

    let (status, body) = queries_of(&workspace, registry, &run, "").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ground_truth"]["status"], "verified");
    assert!(body["queries"][0]["scores"]["ndcg@10"].is_number());
    let cache = body["cache_error"]
        .as_str()
        .expect("the failure is reported");
    assert!(cache.contains("cache"), "{cache}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_parameter_the_trace_does_not_take_is_parameter_invalid() {
    let (workspace, registry) = workspace("replay_parameter", true);
    let run = the_run();
    let response = send(
        app_over(
            FakeRunStore::holding([run.clone()]),
            Arc::new(registry),
            &workspace,
        ),
        get(&format!(
            "/api/v1/runs/{}/trace/q-1?missing_gold_at=1",
            run.id
        )),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(response).await["code"], "parameter_invalid");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_node_that_produced_no_ranking_or_failed_has_no_ranking_metric() {
    let (workspace, registry) = workspace("replay_nodes", true);
    let run = the_run();

    let (_, body) = trace_of(&workspace, registry.clone(), &run, "q-1").await;
    for ranking in ["leg", "reranked"] {
        assert!(node(&body, ranking)["metrics"].is_object(), "{ranking}");
        assert_eq!(node(&body, ranking)["output"]["kind"], "ranking");
    }
    for other in ["prompt", "answer"] {
        assert!(node(&body, other)["metrics"].is_null(), "{other}");
    }
    assert_eq!(node(&body, "answer")["output"]["text"], "on the mat");
    assert_eq!(node(&body, "answer")["duration_nanos"], 4_000);

    let (_, body) = trace_of(&workspace, registry, &run, "q-2").await;
    let reranked = node(&body, "reranked");
    assert!(reranked["output"].is_null());
    assert_eq!(reranked["error"], "the reranker timed out");
    assert!(reranked["metrics"].is_null(), "a failed node has no metric");
    assert!(node(&body, "leg")["metrics"].is_object());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_query_the_run_did_not_execute_is_query_not_found() {
    let (workspace, registry) = workspace("replay_unknown_query", true);
    let (status, body) = trace_of(&workspace, registry, &the_run(), "q-9").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "query_not_found");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_run_is_run_not_found() {
    let (workspace, registry) = workspace("replay_unknown_run", true);
    let response = send(
        app_over(FakeRunStore::default(), Arc::new(registry), &workspace),
        get(&format!("/api/v1/runs/{}/trace/q-1", "ab".repeat(32))),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(response).await["code"], "run_not_found");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trace_that_does_not_read_is_run_unreadable_not_repaired() {
    let (workspace, registry) = workspace("replay_unreadable", true);
    let mut run = the_run();
    run.traces.insert(
        ragondin_types::QueryId::new("q-1"),
        ragondin_experiments::TraceDocument::new(
            serde_json::json!({ "nodes": [{ "node": "leg" }] }),
        ),
    );
    let (status, body) = trace_of(&workspace, registry, &run, "q-1").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "run_unreadable");
    assert!(body["detail"].as_str().unwrap().contains("leg"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_cache_is_reconstructible_every_response_is_identical_without_it() {
    let (workspace, registry) = workspace("replay_cache", true);
    let run = the_run();
    let app = || {
        app_over(
            FakeRunStore::holding([run.clone()]),
            Arc::new(registry.clone()),
            &workspace,
        )
    };
    let paths = [
        format!("/api/v1/runs/{}/queries", run.id),
        format!("/api/v1/runs/{}/queries?missing_gold_at=1", run.id),
        format!("/api/v1/runs/{}/trace/q-1", run.id),
        format!("/api/v1/runs/{}/trace/q-2", run.id),
    ];
    let mut first = Vec::new();
    for path in &paths {
        let response = send(app(), get(path)).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        first.push(json(response).await);
    }
    let cache = workspace.join("cache").join(run.id.to_string());
    assert!(
        cache.is_dir(),
        "the derived data is cached under cache/<run_id>/"
    );

    // Served from the cache, then from nothing: the same answers both times.
    for (path, before) in paths.iter().zip(&first) {
        assert_eq!(
            &json(send(app(), get(path)).await).await,
            before,
            "{path}, cached"
        );
    }
    fs::remove_dir_all(workspace.join("cache")).unwrap();
    for (path, before) in paths.iter().zip(&first) {
        assert_eq!(
            &json(send(app(), get(path)).await).await,
            before,
            "{path}, rebuilt"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chunk_the_derived_chunk_set_does_not_hold_has_no_text() {
    let (workspace, registry) = workspace("replay_unknown_chunk", true);
    let trace = Trace {
        nodes: vec![support::runs::node(
            "leg",
            vec![query("q-1")],
            ranked(vec![
                chunk("MED-10#7", "MED-10", 0.9),
                chunk("MED-10", "MED-10", 0.8),
            ]),
            10,
        )],
    };
    let run = run_over(
        0x42,
        "pipeline:\n  inputs: [question]\n  nodes:\n    - id: leg\n      component: retriever\n      impl: dense\n      inputs: [question]\n",
        &beir_mini(),
        vec![("q-1", trace)],
        &[("mrr", 1.0), ("ndcg@10", 1.0), ("recall@10", 1.0)],
    );
    let (_, body) = trace_of(&workspace, registry, &run, "q-1").await;
    assert_eq!(body["passages"]["status"], "verified");
    let named = &node(&body, "leg")["output"]["chunks"];
    assert!(
        named[0]["text"].is_null(),
        "an id the chunk set does not hold resolves to nothing"
    );
    assert!(named[1]["text"].is_string());
}

/// Every passage's grade, in rankings and contexts, and the gold ranks of
/// each node that produced a ranking — counted over documents, so two chunks
/// of one document take one rank, as in every metric.
#[tokio::test(flavor = "multi_thread")]
async fn a_verified_trace_grades_every_passage_and_ranks_the_gold_documents() {
    let (workspace, registry) = workspace("replay_gold", true);
    // q-1's qrels: MED-10 graded 2, MED-12 judged not relevant (0); 4983 is
    // not judged for it.
    let q1 = generation_trace(
        "q-1",
        vec![
            chunk("4983", "4983", 0.9),
            chunk("MED-10#1", "MED-10", 0.8),
            chunk("MED-10", "MED-10", 0.7),
            chunk("MED-12", "MED-12", 0.6),
        ],
        documents(&["MED-12", "MED-10", "4983"]),
        2,
        "on the mat",
    );
    let run = run_over(
        0x43,
        GENERATION,
        &beir_mini(),
        vec![("q-1", q1)],
        &[("mrr", 0.5), ("ndcg@10", 0.5), ("recall@10", 1.0)],
    );

    let (status, body) = trace_of(&workspace, registry, &run, "q-1").await;

    assert_eq!(status, StatusCode::OK);
    let grades = |node_id: &str| -> Vec<Value> {
        node(&body, node_id)["output"]["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|passage| passage["grade"].clone())
            .collect()
    };
    assert_eq!(grades("leg"), [0, 2, 2, 0]);
    assert_eq!(grades("reranked"), [0, 2, 0]);
    assert_eq!(grades("prompt"), [0, 2], "a context's passages are graded");
    // leg's chunks fold to 4983, MED-10, MED-12: MED-10 is at rank 2, though
    // its chunks sit at positions 2 and 3.
    assert_eq!(node(&body, "leg")["gold_ranks"], serde_json::json!([2]));
    assert_eq!(
        node(&body, "reranked")["gold_ranks"],
        serde_json::json!([2])
    );
    for other in ["prompt", "answer"] {
        assert!(
            node(&body, other)["gold_ranks"].is_null(),
            "{other} produced no ranking"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ranking_with_its_gold_document_first_ranks_it_one_and_a_failed_node_has_no_ranks() {
    let (workspace, registry) = workspace("replay_gold_failed", true);
    let run = the_run();

    let (_, body) = trace_of(&workspace, registry, &run, "q-2").await;

    // q-2 judges 4983 relevant (1); leg ranked it first.
    assert_eq!(node(&body, "leg")["gold_ranks"], serde_json::json!([1]));
    assert!(node(&body, "reranked")["gold_ranks"].is_null(), "it failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ranking_that_misses_every_gold_document_has_empty_gold_ranks() {
    let (workspace, registry) = workspace("replay_gold_none", true);
    let trace = Trace {
        nodes: vec![support::runs::node(
            "leg",
            vec![query("q-1")],
            ranked(documents(&["4983", "MED-12"])),
            10,
        )],
    };
    let run = run_over(
        0x44,
        "pipeline:\n  inputs: [question]\n  nodes:\n    - id: leg\n      component: retriever\n      impl: dense\n      inputs: [question]\n",
        &beir_mini(),
        vec![("q-1", trace)],
        &[("mrr", 0.0)],
    );

    let (_, body) = trace_of(&workspace, registry, &run, "q-1").await;

    assert_eq!(node(&body, "leg")["gold_ranks"], serde_json::json!([]));
}

/// Without the run's own ground truth, nothing about gold is guessed: every
/// grade, every node's gold ranks and the query's text are `null`, and
/// `passages` carries the reason, as it does for passage text.
#[tokio::test(flavor = "multi_thread")]
async fn without_the_runs_dataset_no_grade_gold_rank_or_query_text_is_served() {
    for (test, status) in [
        ("replay_gold_absent", "dataset_absent"),
        ("replay_gold_differs", "dataset_differs"),
        ("replay_gold_unreadable", "dataset_unreadable"),
    ] {
        let (workspace, registry) = workspace(test, status != "dataset_absent");
        let corpus = workspace.join("datasets").join(DIR).join("corpus.jsonl");
        match status {
            "dataset_differs" => {
                let original = fs::read_to_string(&corpus).unwrap();
                fs::write(&corpus, original.replacen("The cat sat", "The bat sat", 1)).unwrap();
            }
            "dataset_unreadable" => fs::remove_file(&corpus).unwrap(),
            _ => {}
        }

        let (code, body) = trace_of(&workspace, registry.clone(), &the_run(), "q-1").await;

        assert_eq!(code, StatusCode::OK, "{test}");
        assert_eq!(body["passages"]["status"], status, "{test}");
        assert!(body["text"].is_null(), "{test}: no query text");
        for chunk in chunks(&body) {
            assert!(chunk["grade"].is_null(), "{test}: {chunk}");
        }
        for entry in body["nodes"].as_array().unwrap() {
            assert!(entry["gold_ranks"].is_null(), "{test}: {entry}");
        }

        let (_, listing) = queries_of(&workspace, registry, &the_run(), "").await;
        assert_eq!(listing["ground_truth"]["status"], status, "{test}");
        for entry in listing["queries"].as_array().unwrap() {
            assert!(entry["text"].is_null(), "{test}: {entry}");
        }
    }
}

/// The grades, the gold ranks and the query's text are the dataset's, as the
/// scores are: a chunk set that is not the run's hides the passage text and
/// nothing else.
#[tokio::test(flavor = "multi_thread")]
async fn a_chunk_set_that_is_not_the_runs_keeps_the_grades_and_the_query_text() {
    let (workspace, registry) = workspace("replay_gold_index_differs", true);
    let mut run = the_run();
    run.inputs.index_version = "f".repeat(64);

    let (_, body) = trace_of(&workspace, registry, &run, "q-1").await;

    assert_eq!(body["passages"]["status"], "index_differs");
    assert_eq!(body["text"], "where did the cat sit?");
    assert_eq!(
        node(&body, "reranked")["gold_ranks"],
        serde_json::json!([1])
    );
    assert_eq!(node(&body, "reranked")["output"]["chunks"][0]["grade"], 2);
    let named = chunks(&body);
    assert!(!named.is_empty());
    for chunk in named {
        assert!(chunk["grade"].is_number(), "graded: {chunk}");
        assert!(
            chunk["text"].is_null(),
            "no text from another chunk set: {chunk}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_verified_dataset_serves_each_querys_text_on_the_trace_and_the_listing() {
    let (workspace, registry) = workspace("replay_query_text", true);
    let run = the_run();

    let (_, body) = trace_of(&workspace, registry.clone(), &run, "q-1").await;
    assert_eq!(body["query"], "q-1");
    assert_eq!(body["text"], "where did the cat sit?");

    let (status, listing) = queries_of(&workspace, registry, &run, "").await;
    assert_eq!(status, StatusCode::OK);
    let texts: Vec<(&str, &str)> = listing["queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["id"].as_str().unwrap(),
                entry["text"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        texts,
        [
            ("q-1", "where did the cat sit?"),
            ("q-2", "which document has no title?"),
        ]
    );
}
