//! `GET /api/v1/workspace`: the path, the settings, the build identity and the
//! capabilities the launcher reports.

mod support;

use axum::http::StatusCode;
use ragondin_api::{Capabilities, FamilyCapabilities};
use serde_json::json;
use support::{app, app_with, get, json, send, FakeLauncher, FakeRunStore, BUILD};

#[tokio::test]
async fn the_workspace_reports_its_path_settings_and_build() {
    let response = send(app(FakeRunStore::default()), get("/api/v1/workspace")).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["path"], "/workspace");
    assert_eq!(body["build"], BUILD);
    assert_eq!(
        body["settings"],
        json!({
            "datasets": "/workspace/datasets",
            "services": [
                { "family": "generator", "name": "qwen", "uri": "http://127.0.0.1:50051" },
            ],
        })
    );
}

#[tokio::test]
async fn the_workspace_reports_the_capabilities_the_launcher_returns() {
    let launcher = FakeLauncher {
        capabilities: Capabilities {
            families: vec![FamilyCapabilities {
                family: "reranker".to_owned(),
                local: vec!["cross_encoder".to_owned()],
            }],
            remote: false,
        },
    };
    let body = json(
        send(
            app_with(FakeRunStore::default(), launcher),
            get("/api/v1/workspace"),
        )
        .await,
    )
    .await;

    assert_eq!(
        body["capabilities"],
        json!({
            "families": [{ "family": "reranker", "local": ["cross_encoder"] }],
            "remote": false,
        })
    );
}
