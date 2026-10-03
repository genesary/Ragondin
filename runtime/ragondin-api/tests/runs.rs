//! `GET /api/v1/runs` and `GET /api/v1/runs/{id}` over an in-memory store.

mod support;

use std::sync::Arc;

use axum::http::StatusCode;
use ragondin_experiments::{
    ConfigDocument, PrefixOf, Run, RunProvenance, RunTimes, Trace, TraceDocument, UnixMillis,
};
use ragondin_types::QueryId;
use serde_json::json;
use support::datasets::scratch;
use support::{
    app, app_over, app_with_backends, fakes, fixture_run, get, json, send, FakeRegistry,
    FakeRunStore, HeldPipelines, PinningRegistry, FIXTURE_RUN,
};

const OTHER_RUN: &str = "00000000000000000000000000000000000000000000000000000000000000aa";

fn another_run() -> Run {
    let mut run = fixture_run();
    run.id = OTHER_RUN.parse().unwrap();
    run.metrics = [("ndcg@10", 0.25)].into_iter().collect();
    run
}

#[tokio::test]
async fn the_listing_holds_every_run_the_store_holds_in_its_order() {
    let response = send(
        app(FakeRunStore::holding([fixture_run(), another_run()])),
        get("/api/v1/runs"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    let ids: Vec<&str> = body["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [OTHER_RUN, FIXTURE_RUN]);
    assert_eq!(body["runs"][0]["metrics"], json!({ "ndcg@10": 0.25 }));
    assert_eq!(
        body["runs"][1]["pipeline"],
        "821bafbd3fa0f1531297806d89843e81c4d8565489ca124706d4ef543936df88"
    );
    assert_eq!(body["unreadable"], json!([]));
}

#[tokio::test]
async fn an_empty_store_lists_nothing() {
    let body = json(send(app(FakeRunStore::default()), get("/api/v1/runs")).await).await;
    assert_eq!(body, json!({ "runs": [], "unreadable": [], "shapes": {} }));
}

#[tokio::test]
async fn the_listing_carries_the_run_s_times_or_null() {
    let mut timed = another_run();
    timed.times = Some(RunTimes::new(
        UnixMillis::new(1_700_000_000_000),
        UnixMillis::new(1_700_000_004_250),
    ));
    let store = || FakeRunStore::holding([fixture_run(), timed.clone()]);

    let body = json(send(app(store()), get("/api/v1/runs")).await).await;

    // Read from the run, never computed: the fixture was stored with no
    // `times.json`, so both of its times are unknown.
    assert_eq!(body["runs"][0]["id"], OTHER_RUN);
    assert_eq!(body["runs"][0]["started_at_ms"], 1_700_000_000_000u64);
    assert_eq!(body["runs"][0]["finished_at_ms"], 1_700_000_004_250u64);
    assert_eq!(body["runs"][1]["started_at_ms"], serde_json::Value::Null);
    assert_eq!(body["runs"][1]["finished_at_ms"], serde_json::Value::Null);

    let detail = json(send(app(store()), get(&format!("/api/v1/runs/{OTHER_RUN}"))).await).await;
    assert_eq!(detail["started_at_ms"], 1_700_000_000_000u64);
    assert_eq!(detail["finished_at_ms"], 1_700_000_004_250u64);
    let detail = json(send(app(store()), get(&format!("/api/v1/runs/{FIXTURE_RUN}"))).await).await;
    assert_eq!(detail["started_at_ms"], serde_json::Value::Null);
    assert_eq!(detail["finished_at_ms"], serde_json::Value::Null);
}

#[tokio::test]
async fn every_benchmark_pinned_to_the_digest_is_named() {
    let pinned = fixture_run().inputs.dataset_version;
    let mut elsewhere = another_run();
    elsewhere.inputs.dataset_version = "0".repeat(64);
    let mut backends = fakes(FakeRunStore::holding([fixture_run(), elsewhere]));
    backends.registry = Arc::new(PinningRegistry {
        pins: vec![
            ("squad/mini".to_owned(), pinned.clone()),
            ("beir/other".to_owned(), "f".repeat(64)),
            // A manifest entry and an import pinned to one digest: both are
            // the run's dataset exactly, so both are named, sorted.
            ("beir/fixture".to_owned(), pinned),
        ],
    });

    let body = json(send(app_with_backends(backends), get("/api/v1/runs")).await).await;

    assert_eq!(body["runs"][0]["id"], OTHER_RUN);
    assert_eq!(body["runs"][0]["benchmark_names"], json!([]));
    assert_eq!(
        body["runs"][1]["benchmark_names"],
        json!(["beir/fixture", "squad/mini"])
    );
}

#[tokio::test]
async fn every_pipeline_sharing_the_hash_is_named() {
    let text = fixture_run().config.as_str().to_owned();
    let mut backends = fakes(FakeRunStore::holding([fixture_run()]));
    backends.pipelines = Arc::new(HeldPipelines {
        files: vec![
            // One canonical form, two spellings: the second only adds a
            // comment, which the canonical form does not see (INV-8).
            ("stub-copy".to_owned(), format!("# a copy\n{text}")),
            ("other".to_owned(), "pipeline: {}\n".to_owned()),
            ("stub".to_owned(), text),
        ],
    });

    let body = json(send(app_with_backends(backends), get("/api/v1/runs")).await).await;

    assert_eq!(
        body["runs"][0]["pipeline_names"],
        json!(["stub", "stub-copy"])
    );
}

#[tokio::test]
async fn a_run_no_workspace_document_hashes_to_names_no_pipeline() {
    let body = json(
        send(
            app(FakeRunStore::holding([fixture_run()])),
            get("/api/v1/runs"),
        )
        .await,
    )
    .await;

    assert_eq!(body["runs"][0]["pipeline_names"], json!([]));
    assert_eq!(body["runs"][0]["benchmark_names"], json!([]));
}

#[tokio::test]
async fn the_listing_carries_each_pipeline_s_shape_once() {
    // Two runs of one pipeline.
    let store = || FakeRunStore::holding([fixture_run(), another_run()]);

    let body = json(send(app(store()), get("/api/v1/runs")).await).await;
    let detail = json(send(app(store()), get(&format!("/api/v1/runs/{FIXTURE_RUN}"))).await).await;

    let shapes = body["shapes"].as_object().expect("the shapes are a map");
    let hash = fixture_run().inputs.pipeline.to_string();
    assert_eq!(shapes.keys().collect::<Vec<_>>(), [&hash]);
    assert_eq!(shapes[&hash], detail["graph"]);
}

#[tokio::test]
async fn a_run_the_store_cannot_read_is_listed_as_unreadable_not_dropped() {
    let store = FakeRunStore::holding([fixture_run()]);
    store.tear(OTHER_RUN, "torn");
    let body = json(send(app(store), get("/api/v1/runs")).await).await;

    assert_eq!(body["runs"].as_array().unwrap().len(), 1);
    assert_eq!(body["unreadable"][0]["id"], OTHER_RUN);
    assert!(body["unreadable"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("incomplete"));
}

#[tokio::test]
async fn the_detail_returns_the_stored_fields() {
    let run = fixture_run();
    let response = send(
        app(FakeRunStore::holding([run.clone()])),
        get(&format!("/api/v1/runs/{FIXTURE_RUN}")),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["id"], FIXTURE_RUN);
    assert_eq!(
        body["inputs"],
        json!({
            "pipeline": "821bafbd3fa0f1531297806d89843e81c4d8565489ca124706d4ef543936df88",
            "dataset_version": "331a9c8c4092d6d8d53b70cdd46ded8551fa44198e009e3875929be29f87dbac",
            "index_version": "0a8c37834f826b9c7de98b43c6268bd3f06b2f56442161afa0963d4e655533a4",
            "model_hashes": {},
            "engine_version": "0.0.0",
        })
    );
    assert_eq!(body["metrics"]["mrr"], 0.5);
    assert_eq!(body["configuration"], run.config.as_str());
    assert_eq!(body["bindings"], json!([]));
    assert_eq!(body["launched_as"], serde_json::Value::Null);
}

#[tokio::test]
async fn the_listing_carries_the_launch_record_or_null() {
    let mut named = another_run();
    named.provenance = Some(RunProvenance::named("hybrid"));
    let mut cut = fixture_run();
    cut.id = "00000000000000000000000000000000000000000000000000000000000000bb"
        .parse()
        .unwrap();
    cut.provenance = Some(RunProvenance::prefix(
        "hybrid",
        PrefixOf::new("fused", fixture_run().inputs.pipeline),
    ));

    let body = json(
        send(
            app(FakeRunStore::holding([fixture_run(), named, cut])),
            get("/api/v1/runs"),
        )
        .await,
    )
    .await;

    let by_id = |id: &str| {
        body["runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|run| run["id"] == id)
            .cloned()
            .expect("the run is listed")
    };
    assert_eq!(
        by_id(OTHER_RUN)["launched_as"],
        json!({ "name": "hybrid", "prefix_of": null })
    );
    assert_eq!(
        by_id("00000000000000000000000000000000000000000000000000000000000000bb")["launched_as"],
        json!({
            "name": "hybrid",
            "prefix_of": {
                "up_to": "fused",
                "parent_pipeline_hash": fixture_run().inputs.pipeline.to_string(),
            },
        })
    );
    // Stored without `provenance.json`: no record, never one inferred from
    // the documents sharing its hash.
    assert_eq!(by_id(FIXTURE_RUN)["launched_as"], serde_json::Value::Null);
}

#[tokio::test]
async fn a_recorded_name_and_the_hash_matches_are_reported_independently() {
    let text = fixture_run().config.as_str().to_owned();
    // `hybrid` was edited since the run: one parameter changed, so its
    // canonical form is no longer the run's.
    let edited = text.replacen("top_k: 1", "top_k: 2", 1);
    assert_ne!(edited, text, "the document was edited");
    // Still a valid pipeline, so it is indexed: it just holds another form.
    let lowered = ragondin_experiments::lower_configuration(&ConfigDocument::new(&edited))
        .expect("the edited document lowers");
    assert_ne!(lowered.content_hash(), fixture_run().inputs.pipeline);
    let mut run = fixture_run();
    run.provenance = Some(RunProvenance::named("hybrid"));
    let mut backends = fakes(FakeRunStore::holding([run.clone()]));
    backends.pipelines = Arc::new(HeldPipelines {
        files: vec![("hybrid".to_owned(), edited)],
    });

    let body = json(send(app_with_backends(backends), get("/api/v1/runs")).await).await;

    assert_eq!(body["runs"][0]["launched_as"]["name"], "hybrid");
    assert_eq!(body["runs"][0]["pipeline_names"], json!([]));

    // A fork still holding the run's content: both facts, unchanged, side by
    // side — the record does not take the fork's name, nor the fork the
    // record's.
    let mut backends = fakes(FakeRunStore::holding([run.clone()]));
    backends.pipelines = Arc::new(HeldPipelines {
        files: vec![
            (
                "hybrid".to_owned(),
                text.replacen("top_k: 1", "top_k: 2", 1),
            ),
            ("hybrid-fork".to_owned(), text.clone()),
        ],
    });

    let body = json(send(app_with_backends(backends), get("/api/v1/runs")).await).await;

    assert_eq!(
        body["runs"][0]["launched_as"],
        json!({ "name": "hybrid", "prefix_of": null })
    );
    assert_eq!(body["runs"][0]["pipeline_names"], json!(["hybrid-fork"]));

    // An unedited fork: `hybrid` and `hybrid-fork` both hold the run's
    // content. The record still names `hybrid` alone, and the hash matches
    // name both — neither fact absorbs the other.
    let mut backends = fakes(FakeRunStore::holding([run]));
    backends.pipelines = Arc::new(HeldPipelines {
        files: vec![
            ("hybrid-fork".to_owned(), text.clone()),
            ("hybrid".to_owned(), text),
        ],
    });

    let body = json(send(app_with_backends(backends), get("/api/v1/runs")).await).await;

    assert_eq!(
        body["runs"][0]["launched_as"],
        json!({ "name": "hybrid", "prefix_of": null })
    );
    assert_eq!(
        body["runs"][0]["pipeline_names"],
        json!(["hybrid", "hybrid-fork"])
    );
}

#[tokio::test]
async fn the_detail_carries_the_launch_record() {
    let mut run = fixture_run();
    run.provenance = Some(RunProvenance::prefix(
        "hybrid",
        PrefixOf::new("fused", fixture_run().inputs.pipeline),
    ));

    let body = json(
        send(
            app(FakeRunStore::holding([run])),
            get(&format!("/api/v1/runs/{FIXTURE_RUN}")),
        )
        .await,
    )
    .await;

    assert_eq!(
        body["launched_as"],
        json!({
            "name": "hybrid",
            "prefix_of": {
                "up_to": "fused",
                "parent_pipeline_hash": fixture_run().inputs.pipeline.to_string(),
            },
        })
    );
    // The old `prefix_of` string is gone: the record's `prefix_of` is the
    // one place a prefix run says what it was cut from.
    assert!(body.get("prefix_of").is_none());
}

#[tokio::test]
async fn the_detail_carries_the_graph_lowered_from_the_stored_document() {
    let body = json(
        send(
            app(FakeRunStore::holding([fixture_run()])),
            get(&format!("/api/v1/runs/{FIXTURE_RUN}")),
        )
        .await,
    )
    .await;
    let graph = &body["graph"];

    assert_eq!(
        graph["inputs"],
        json!([{ "id": "question", "kind": "query" }])
    );

    // Sorted by id: the canonical order the lowering gives, whatever order
    // the document listed them in.
    assert_eq!(
        graph["nodes"],
        json!([
            {
                "id": "answer", "family": "generator", "implementation": "stub_generator",
                "parameters": { "served_model": "stub-model", "template": "{context}" },
            },
            {
                "id": "context", "family": "context_builder",
                "implementation": "stub_context_builder",
                "parameters": { "budget": 1 },
            },
            {
                "id": "fused", "family": "fusion", "implementation": "stub_interleave",
                "parameters": {},
            },
            {
                "id": "leg_a", "family": "retriever", "implementation": "stub_retriever",
                "parameters": { "label": "doc-a", "top_k": 1 },
            },
            {
                "id": "leg_b", "family": "retriever", "implementation": "stub_retriever",
                "parameters": { "label": "doc-b", "top_k": 1 },
            },
        ])
    );

    // One edge per entry of a node's `inputs`, in port order, carrying the
    // kind of what travels along it.
    assert_eq!(
        graph["edges"],
        json!([
            { "from": "question", "to": "answer", "port": 0, "kind": "query" },
            { "from": "context", "to": "answer", "port": 1, "kind": "context" },
            { "from": "question", "to": "context", "port": 0, "kind": "query" },
            { "from": "fused", "to": "context", "port": 1, "kind": "chunks" },
            { "from": "leg_a", "to": "fused", "port": 0, "kind": "chunks" },
            { "from": "leg_b", "to": "fused", "port": 1, "kind": "chunks" },
            { "from": "question", "to": "leg_a", "port": 0, "kind": "query" },
            { "from": "question", "to": "leg_b", "port": 0, "kind": "query" },
        ])
    );
}

#[tokio::test]
async fn an_unknown_run_id_is_a_404_run_not_found() {
    let response = send(
        app(FakeRunStore::default()),
        get(&format!("/api/v1/runs/{OTHER_RUN}")),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers()["content-type"],
        "application/problem+json"
    );
    assert_eq!(json(response).await["code"], "run_not_found");
}

#[tokio::test]
async fn a_string_that_is_no_run_id_is_a_404_run_not_found() {
    let response = send(app(FakeRunStore::default()), get("/api/v1/runs/not-a-run")).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(response).await["code"], "run_not_found");
}

#[tokio::test]
async fn a_torn_run_is_run_unreadable() {
    let store = FakeRunStore::default();
    store.tear(OTHER_RUN, "torn");
    let response = send(app(store), get(&format!("/api/v1/runs/{OTHER_RUN}"))).await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json(response).await["code"], "run_unreadable");
}

#[tokio::test]
async fn a_stored_document_that_no_longer_lowers_is_run_unreadable_not_guessed() {
    let mut run = fixture_run();
    run.config = ConfigDocument::new("version: 99\npipeline:\n  inputs: [question]\n  nodes: []\n");
    let response = send(
        app(FakeRunStore::holding([run])),
        get(&format!("/api/v1/runs/{FIXTURE_RUN}")),
    )
    .await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = json(response).await;
    assert_eq!(body["code"], "run_unreadable");
    assert!(body["detail"].as_str().unwrap().contains("schema version"));
}

#[tokio::test]
async fn every_metric_in_the_listing_carries_its_family() {
    let body = json(
        send(
            app(FakeRunStore::holding([fixture_run(), another_run()])),
            get("/api/v1/runs"),
        )
        .await,
    )
    .await;

    for run in body["runs"].as_array().unwrap() {
        let metrics: Vec<&String> = run["metrics"].as_object().unwrap().keys().collect();
        let families: Vec<&String> = run["metric_families"].as_object().unwrap().keys().collect();
        assert_eq!(families, metrics, "{run}");
    }
    let fixture = &body["runs"][1];
    assert_eq!(fixture["id"], FIXTURE_RUN);
    assert_eq!(
        fixture["metric_families"],
        json!({
            "exact_match": "answers",
            "mrr": "ranking",
            "ndcg@10": "ranking",
            "recall@10": "ranking",
            "token_f1": "answers",
        })
    );
}

#[tokio::test]
async fn an_unknown_metric_is_listed_as_unknown_and_kept() {
    let mut run = another_run();
    run.metrics.insert("foo_score", 0.125);

    let body = json(send(app(FakeRunStore::holding([run])), get("/api/v1/runs")).await).await;

    assert_eq!(body["runs"][0]["metrics"]["foo_score"], 0.125);
    assert_eq!(body["runs"][0]["metric_families"]["foo_score"], "unknown");
    assert_eq!(body["runs"][0]["metric_families"]["ndcg@10"], "ranking");
}

/// Each query's latency is the sum of its trace's durations; the listing's
/// is the lower median of those, over the queries.
fn lower_median_of_query_latencies(run: &Run) -> u64 {
    let mut latencies: Vec<u64> = run
        .traces
        .values()
        .map(|document| {
            let trace = Trace::try_from(document).unwrap();
            trace.nodes.iter().map(|node| node.duration_nanos).sum()
        })
        .collect();
    latencies.sort_unstable();
    latencies[(latencies.len() - 1) / 2]
}

#[tokio::test]
async fn the_listing_s_latency_is_the_lower_median_of_query_latencies() {
    let run = fixture_run();
    let expected = lower_median_of_query_latencies(&run);
    // The fixture's four queries take 37 836, 17 249, 18 458 and 15 126 ns:
    // of the two middle values, the lower.
    assert_eq!(expected, 17_249);

    let body = json(send(app(FakeRunStore::holding([run])), get("/api/v1/runs")).await).await;

    assert_eq!(body["runs"][0]["median_query_latency_nanos"], expected);
}

const THIRD_RUN: &str = "00000000000000000000000000000000000000000000000000000000000000bb";

#[tokio::test]
async fn a_run_with_no_trace_that_reads_has_no_latency() {
    let mut empty = another_run();
    empty.traces.clear();
    let mut torn = fixture_run();
    for document in torn.traces.values_mut() {
        *document = TraceDocument::new(json!({ "nodes": [{ "node": "leg" }] }));
    }
    // One trace that does not read: the median is over the three that do.
    let mut partly = fixture_run();
    partly.id = THIRD_RUN.parse().unwrap();
    partly.traces.insert(
        QueryId::new("q-1"),
        TraceDocument::new(json!({ "nodes": "not a list" })),
    );

    let body = json(
        send(
            app(FakeRunStore::holding([empty, torn, partly])),
            get("/api/v1/runs"),
        )
        .await,
    )
    .await;

    let latency = |id: &str| {
        body["runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|run| run["id"] == id)
            .unwrap()["median_query_latency_nanos"]
            .clone()
    };
    assert_eq!(latency(OTHER_RUN), serde_json::Value::Null);
    assert_eq!(latency(FIXTURE_RUN), serde_json::Value::Null);
    // 15 126, 17 249 and 18 458: the middle one.
    assert_eq!(latency(THIRD_RUN), 17_249);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_latency_needs_no_dataset() {
    // A registry with no dataset at all: the run's is not on disk.
    let workspace = scratch("runs_latency_no_dataset");
    let run = fixture_run();
    let expected = lower_median_of_query_latencies(&run);

    let body = json(
        send(
            app_over(
                FakeRunStore::holding([run]),
                Arc::new(FakeRegistry),
                &workspace,
            ),
            get("/api/v1/runs"),
        )
        .await,
    )
    .await;

    assert_eq!(body["runs"][0]["median_query_latency_nanos"], expected);
    assert!(body.get("cache_error").is_none(), "{body}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_latency_is_served_from_the_cache_the_second_time() {
    let workspace = scratch("runs_latency_cache");
    let run = fixture_run();
    let listing = || async {
        json(
            send(
                app_over(
                    FakeRunStore::holding([run.clone()]),
                    Arc::new(FakeRegistry),
                    &workspace,
                ),
                get("/api/v1/runs"),
            )
            .await,
        )
        .await
    };

    let first = listing().await;
    let file = workspace
        .join("cache")
        .join(FIXTURE_RUN)
        .join("latency.json");
    assert!(
        file.is_file(),
        "the latency is cached under cache/<run_id>/"
    );
    assert_eq!(listing().await, first, "the same body, from the cache");

    // The figure the second listing serves is the file's, not one computed
    // again from the traces: a value written there by hand is what comes
    // back.
    let mut cached: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    cached["median_query_latency_nanos"] = json!(42);
    std::fs::write(&file, serde_json::to_vec(&cached).unwrap()).unwrap();
    assert_eq!(listing().await["runs"][0]["median_query_latency_nanos"], 42);

    // Removed, it is computed again: the cache is never a truth.
    std::fs::remove_dir_all(workspace.join("cache")).unwrap();
    assert_eq!(listing().await, first, "the same body, rebuilt");
}

#[tokio::test]
async fn a_pipeline_document_carries_the_graph_a_run_of_it_serves() {
    // The fixture run's own document, stored as a workspace pipeline with a
    // comment the graph does not see.
    let text = format!("# kept\n{}", fixture_run().config.as_str());
    let backends = || {
        let mut backends = fakes(FakeRunStore::holding([fixture_run()]));
        backends.pipelines = Arc::new(HeldPipelines {
            files: vec![
                ("stub".to_owned(), text.clone()),
                ("broken".to_owned(), "pipeline: [".to_owned()),
            ],
        });
        backends
    };

    let pipeline =
        json(send(app_with_backends(backends()), get("/api/v1/pipelines/stub")).await).await;
    let run = json(
        send(
            app_with_backends(backends()),
            get(&format!("/api/v1/runs/{FIXTURE_RUN}")),
        )
        .await,
    )
    .await;

    assert!(pipeline["graph"].is_object(), "{pipeline}");
    assert_eq!(pipeline["graph"], run["graph"]);

    // A document that does not validate opens as text alone: `graph` is
    // there, and null.
    let broken = json(
        send(
            app_with_backends(backends()),
            get("/api/v1/pipelines/broken"),
        )
        .await,
    )
    .await;
    assert!(broken["error"].is_object(), "{broken}");
    assert_eq!(
        broken.get("graph"),
        Some(&serde_json::Value::Null),
        "{broken}"
    );
}
