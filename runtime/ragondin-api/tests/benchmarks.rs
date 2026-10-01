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
