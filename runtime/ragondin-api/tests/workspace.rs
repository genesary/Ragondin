//! `GET /api/v1/workspace`: the path, the settings, the build identity and the
//! capabilities the launcher reports.

mod support;

use axum::http::StatusCode;
use ragondin_api::{family_ports, Capabilities, FamilyCapabilities, NotCarried};
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
            families: vec![
                FamilyCapabilities {
                    family: "reranker".to_owned(),
                    local: vec![],
                    ports: family_ports("reranker"),
                    not_carried: vec![NotCarried {
                        name: "cross_encoder".to_owned(),
                        reason: "needs the `onnx` feature".to_owned(),
                    }],
                },
                FamilyCapabilities {
                    family: "fusion".to_owned(),
                    local: vec!["rrf".to_owned()],
                    ports: family_ports("fusion"),
                    not_carried: Vec::new(),
                },
                FamilyCapabilities {
                    family: "embedder".to_owned(),
                    local: vec![],
                    ports: family_ports("embedder"),
                    not_carried: Vec::new(),
                },
            ],
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
            "families": [
                {
                    "family": "reranker",
                    "local": [],
                    "ports": {
                        "produces": "chunks",
                        "consumes": { "shape": "fixed", "kinds": ["query", "chunks"] },
                    },
                    "not_carried": [
                        { "name": "cross_encoder", "reason": "needs the `onnx` feature" },
                    ],
                },
                {
                    "family": "fusion",
                    "local": ["rrf"],
                    "ports": {
                        "produces": "chunks",
                        "consumes": { "shape": "variadic", "kind": "chunks" },
                    },
                    "not_carried": [],
                },
                { "family": "embedder", "local": [], "ports": null, "not_carried": [] },
            ],
            "remote": false,
        })
    );
}
