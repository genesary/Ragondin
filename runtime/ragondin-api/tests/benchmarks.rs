//! The benchmark endpoints over the `Registry` file backend:
//! `GET /benchmarks` and `POST /benchmarks/import`.

mod support;

use std::sync::Arc;

use axum::http::StatusCode;
use ragondin_api::fs::FsRegistry;
use serde_json::json;
use support::datasets::{benchmark_fixture, scratch};
use support::{
    app_with_backends, fakes, get, json as body_json, send, write_request, FakeRunStore,
};

fn server(registry: FsRegistry) -> ragondin_api::Server {
    let mut backends = fakes(FakeRunStore::default());
    backends.registry = Arc::new(registry);
    app_with_backends(backends)
}

#[tokio::test(flavor = "multi_thread")]
async fn an_import_is_listed_with_its_state_and_counted_ready() {
    let datasets = scratch("endpoint_import");
    let app = server(FsRegistry::new(datasets.clone(), Vec::new()));
    let source = benchmark_fixture("beir-mini");

    let response = send(
        app.clone(),
        write_request(
            "POST",
            "/api/v1/benchmarks/import",
            &json!({ "name": "mine", "path": source.display().to_string() }),
            &[],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let imported = body_json(response).await;
    assert_eq!(imported["name"], "beir/mine");
    assert_eq!(imported["state"]["kind"], "local");

    let listing = body_json(send(app.clone(), get("/api/v1/benchmarks")).await).await;
    assert_eq!(listing["benchmarks"].as_array().unwrap().len(), 1);
    assert_eq!(listing["benchmarks"][0], imported);
    let workspace = body_json(send(app, get("/api/v1/workspace")).await).await;
    assert_eq!(workspace["counts"]["benchmarks_ready"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_import_the_registry_refuses_answers_its_code() {
    let datasets = scratch("endpoint_import_refused");
    let app = server(FsRegistry::new(datasets, Vec::new()));

    let response = send(
        app,
        write_request(
            "POST",
            "/api/v1/benchmarks/import",
            &json!({ "name": "../escape", "path": "/nowhere" }),
            &[],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(response).await["code"], "import_refused");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_listing_says_which_ground_truths_each_output_can_be_scored_on() {
    let datasets = scratch("endpoint_scorable");
    let app = server(FsRegistry::new(datasets, Vec::new()));

    let listing = body_json(send(app, get("/api/v1/benchmarks")).await).await;

    // `CarriedPieces::scorable`, asked of every ground truth: a pipeline that
    // ends elsewhere than in an answer cannot be scored against reference
    // answers (ADR-C30 § 5).
    assert_eq!(
        listing["scorable"],
        json!({
            "ending_in_answer": ["none", "qrels", "reference_answers", "both"],
            "ending_elsewhere": ["none", "qrels"],
        })
    );
}
