//! `GET /api/v1/workspace`: the path, the settings, the build identity and the
//! capabilities the launcher reports.

mod support;

use axum::http::StatusCode;
use ragondin_api::{
    family_ports, Capabilities, ChoiceCase, FamilyCapabilities, ImplementationParameters,
    NotCarried, Parameter, ParameterChoice, ParameterKind, ParameterValue,
};
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
                    ports: family_ports("reranker"),
                    not_carried: vec![NotCarried {
                        name: "cross_encoder".to_owned(),
                        reason: "needs the `onnx` feature".to_owned(),
                    }],
                    parameters: Vec::new(),
                    bound: vec![Parameter {
                        name: "top_k".to_owned(),
                        kind: ParameterKind::NonNegativeInteger,
                        required: true,
                        description: "How many chunks.".to_owned(),
                        start: Some(ParameterValue::Int(10)),
                    }],
                },
                FamilyCapabilities {
                    family: "fusion".to_owned(),
                    ports: family_ports("fusion"),
                    not_carried: Vec::new(),
                    parameters: vec![ImplementationParameters {
                        name: "rrf".to_owned(),
                        parameters: vec![Parameter {
                            name: "k".to_owned(),
                            kind: ParameterKind::NonNegativeInteger,
                            required: false,
                            description: "The constant.".to_owned(),
                            start: None,
                        }],
                        choice: None,
                    }],
                    bound: Vec::new(),
                },
                FamilyCapabilities {
                    family: "retriever".to_owned(),
                    ports: family_ports("retriever"),
                    not_carried: Vec::new(),
                    parameters: vec![ImplementationParameters {
                        name: "dense".to_owned(),
                        parameters: Vec::new(),
                        choice: Some(ParameterChoice {
                            key: "embedder".to_owned(),
                            cases: vec![ChoiceCase {
                                value: None,
                                parameters: vec![Parameter {
                                    name: "served_model".to_owned(),
                                    kind: ParameterKind::String,
                                    required: true,
                                    description: "The served model.".to_owned(),
                                    start: None,
                                }],
                            }],
                        }),
                    }],
                    bound: Vec::new(),
                },
                FamilyCapabilities {
                    family: "embedder".to_owned(),
                    ports: family_ports("embedder"),
                    not_carried: Vec::new(),
                    parameters: Vec::new(),
                    bound: Vec::new(),
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
                    "ports": {
                        "produces": "chunks",
                        "consumes": { "shape": "fixed", "kinds": ["query", "chunks"] },
                    },
                    "not_carried": [
                        { "name": "cross_encoder", "reason": "needs the `onnx` feature" },
                    ],
                    "parameters": [],
                    "bound": [
                        {
                            "name": "top_k",
                            "kind": "non_negative_integer",
                            "required": true,
                            "description": "How many chunks.",
                            "start": { "kind": "int", "value": "10" },
                        },
                    ],
                },
                {
                    "family": "fusion",
                    "ports": {
                        "produces": "chunks",
                        "consumes": { "shape": "variadic", "kind": "chunks" },
                    },
                    "not_carried": [],
                    "parameters": [
                        {
                            "name": "rrf",
                            "parameters": [
                                {
                                    "name": "k",
                                    "kind": "non_negative_integer",
                                    "required": false,
                                    "description": "The constant.",
                                    "start": null,
                                },
                            ],
                            "choice": null,
                        },
                    ],
                    "bound": [],
                },
                {
                    "family": "retriever",
                    "ports": {
                        "produces": "chunks",
                        "consumes": { "shape": "fixed", "kinds": ["query"] },
                    },
                    "not_carried": [],
                    "parameters": [
                        {
                            "name": "dense",
                            "parameters": [],
                            "choice": {
                                "key": "embedder",
                                "cases": [
                                    {
                                        "value": null,
                                        "parameters": [
                                            {
                                                "name": "served_model",
                                                "kind": "string",
                                                "required": true,
                                                "description": "The served model.",
                                                "start": null,
                                            },
                                        ],
                                    },
                                ],
                            },
                        },
                    ],
                    "bound": [],
                },
                {
                    "family": "embedder",
                    "ports": null,
                    "not_carried": [],
                    "parameters": [],
                    "bound": [],
                },
            ],
            "remote": false,
        })
    );
}
