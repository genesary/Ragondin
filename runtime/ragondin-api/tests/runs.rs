//! `GET /api/v1/runs` and `GET /api/v1/runs/{id}` over an in-memory store.

mod support;

use axum::http::StatusCode;
use ragondin_experiments::{ConfigDocument, Run};
use serde_json::json;
use support::{app, fixture_run, get, json, send, FakeRunStore, FIXTURE_RUN};

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
    assert_eq!(body, json!({ "runs": [], "unreadable": [] }));
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
    assert_eq!(body["prefix_of"], serde_json::Value::Null);
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
