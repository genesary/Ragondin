//! Every `ApiError` variant renders as `application/problem+json` with its
//! stable code — the contract test the design document § 9 asks for, one test
//! per code, including the codes no handler raises yet.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use ragondin_api::{ApiError, EdgeLocation, Location};
use serde_json::{json, Value};

async fn render(error: ApiError) -> (StatusCode, Value) {
    let response = error.into_response();
    assert_eq!(
        response.headers()["content-type"],
        "application/problem+json",
        "every error is problem+json"
    );
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    (status, body)
}

/// The fields every problem carries, whatever its code.
fn assert_problem(body: &Value, status: StatusCode, code: &str) {
    assert_eq!(body["code"], code);
    assert_eq!(body["status"], status.as_u16());
    assert_eq!(body["type"], format!("urn:ragondin:problem:{code}"));
    for field in ["title", "detail", "hint"] {
        assert!(
            body[field].as_str().is_some_and(|text| !text.is_empty()),
            "{code}: `{field}` is a non-empty string"
        );
    }
}

#[tokio::test]
async fn pipeline_invalid_carries_its_location() {
    let (status, body) = render(ApiError::PipelineInvalid {
        detail: "node `answer` port 1 (fed by `leg_a`): expected `context`, found `chunks`"
            .to_owned(),
        location: Location {
            node: Some("answer".to_owned()),
            edge: Some(EdgeLocation {
                from: "leg_a".to_owned(),
                to: "answer".to_owned(),
                port: 1,
            }),
        },
    })
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_problem(&body, status, "pipeline_invalid");
    assert_eq!(
        body["location"],
        json!({ "node": "answer", "edge": { "from": "leg_a", "to": "answer", "port": 1 } })
    );
}

#[tokio::test]
async fn impl_not_in_build() {
    let (status, body) = render(ApiError::ImplNotInBuild {
        family: "retriever".to_owned(),
        implementation: "bm25".to_owned(),
        feature: None,
    })
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_problem(&body, status, "impl_not_in_build");
    assert!(body["hint"].as_str().unwrap().contains("Remote"));
    assert!(
        body.get("location").is_none(),
        "only a validation failure is located"
    );
}

#[tokio::test]
async fn impl_not_in_build_names_the_feature_when_one_is_known() {
    let (status, body) = render(ApiError::ImplNotInBuild {
        family: "generator".to_owned(),
        implementation: "qwen".to_owned(),
        feature: Some("remote".to_owned()),
    })
    .await;
    assert_problem(&body, status, "impl_not_in_build");
    assert!(body["detail"]
        .as_str()
        .unwrap()
        .contains("`remote` feature"));
    assert_eq!(body["hint"], "Rebuild with the `remote` feature.");
}

#[tokio::test]
async fn service_unreachable() {
    let (status, body) = render(ApiError::ServiceUnreachable {
        uri: "http://127.0.0.1:50051".to_owned(),
        reason: "connection refused".to_owned(),
        last_identity: Some("qwen@rev3".to_owned()),
    })
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_problem(&body, status, "service_unreachable");
    let detail = body["detail"].as_str().unwrap();
    assert!(detail.contains("127.0.0.1:50051"), "{detail}");
    assert!(detail.contains("connection refused"), "{detail}");
    assert!(detail.contains("qwen@rev3"), "{detail}");
}

#[tokio::test]
async fn run_exists() {
    let (status, body) = render(ApiError::RunExists {
        run_id: "ab".repeat(32),
    })
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_problem(&body, status, "run_exists");
    assert!(body["hint"].as_str().unwrap().contains(&"ab".repeat(32)));
}

#[tokio::test]
async fn run_unreadable() {
    let (status, body) = render(ApiError::RunUnreadable {
        run_id: "ab".repeat(32),
        reason: "stored under a schema version this build cannot read".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_problem(&body, status, "run_unreadable");
    assert!(body["detail"].as_str().unwrap().contains("schema version"));
}

#[tokio::test]
async fn run_not_found() {
    let (status, body) = render(ApiError::RunNotFound {
        id: "not-a-run".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem(&body, status, "run_not_found");
}

#[tokio::test]
async fn query_not_found() {
    let (status, body) = render(ApiError::QueryNotFound {
        run_id: "ab".repeat(32),
        query: "q-9".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem(&body, status, "query_not_found");
    assert!(body["detail"].as_str().unwrap().contains("q-9"));
}

#[tokio::test]
async fn parameter_invalid() {
    let (status, body) = render(ApiError::ParameterInvalid {
        name: "missing_gold_at".to_owned(),
        reason: "`ten` is not a positive integer".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem(&body, status, "parameter_invalid");
    assert!(body["detail"].as_str().unwrap().contains("missing_gold_at"));
}

#[tokio::test]
async fn dataset_absent() {
    let (status, body) = render(ApiError::DatasetAbsent {
        dataset: "beir/scifact".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem(&body, status, "dataset_absent");
}

#[tokio::test]
async fn dataset_differs() {
    let (status, body) = render(ApiError::DatasetDiffers {
        dataset: "beir/scifact".to_owned(),
        expected: "331a".to_owned(),
        found: "9f00".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_problem(&body, status, "dataset_differs");
    let detail = body["detail"].as_str().unwrap();
    assert!(detail.contains("331a") && detail.contains("9f00"));
}

#[tokio::test]
async fn benchmark_not_found() {
    let (status, body) = render(ApiError::BenchmarkNotFound {
        name: "beir/nowhere".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem(&body, status, "benchmark_not_found");
    assert!(body["detail"].as_str().unwrap().contains("beir/nowhere"));
}

#[tokio::test]
async fn benchmark_exists() {
    let (status, body) = render(ApiError::BenchmarkExists {
        name: "beir/scifact".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_problem(&body, status, "benchmark_exists");
    assert!(body["hint"].as_str().unwrap().contains("beir/scifact"));
}

#[tokio::test]
async fn download_failed() {
    let (status, body) = render(ApiError::DownloadFailed {
        name: "beir/scifact".to_owned(),
        reason: "corpus.jsonl digests to 9f00, the manifest pins 331a".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_problem(&body, status, "download_failed");
    let detail = body["detail"].as_str().unwrap();
    assert!(detail.contains("331a") && detail.contains("9f00"));
}

#[tokio::test]
async fn download_cancelled() {
    let (status, body) = render(ApiError::DownloadCancelled {
        name: "beir/scifact".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_problem(&body, status, "download_cancelled");
    assert!(body["detail"].as_str().unwrap().contains("beir/scifact"));
}

#[tokio::test]
async fn import_refused() {
    let (status, body) = render(ApiError::ImportRefused {
        name: "mine".to_owned(),
        reason: "corpus.jsonl:1: malformed JSON record".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_problem(&body, status, "import_refused");
    assert!(body["detail"].as_str().unwrap().contains("corpus.jsonl:1"));
}

#[tokio::test]
async fn pipeline_not_found() {
    let (status, body) = render(ApiError::PipelineNotFound {
        name: "hybrid".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem(&body, status, "pipeline_not_found");
    assert!(body["detail"].as_str().unwrap().contains("hybrid"));
}

#[tokio::test]
async fn precondition_failed_sends_the_current_etag() {
    let response = ApiError::PreconditionFailed {
        reason: "pipeline hybrid changed since it was read".to_owned(),
        current: Some("ab".repeat(32)),
    }
    .into_response();
    assert_eq!(
        response.headers()["etag"],
        format!("\"{}\"", "ab".repeat(32)).as_str()
    );

    let (status, body) = render(ApiError::PreconditionFailed {
        reason: "pipeline hybrid changed since it was read".to_owned(),
        current: None,
    })
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    assert_problem(&body, status, "precondition_failed");
}

#[tokio::test]
async fn binding_refused() {
    let (status, body) = render(ApiError::BindingRefused {
        detail: "`--remote store/qdrant=http://host`: `store` is not a family".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_problem(&body, status, "binding_refused");
    assert!(body["detail"].as_str().unwrap().contains("--remote"));
}

#[tokio::test]
async fn service_not_found() {
    let (status, body) = render(ApiError::ServiceNotFound {
        family: "generator".to_owned(),
        name: "qwen".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem(&body, status, "service_not_found");
    assert!(body["detail"].as_str().unwrap().contains("generator/qwen"));
}

#[tokio::test]
async fn request_invalid() {
    let (status, body) = render(ApiError::RequestInvalid {
        detail: "the body is not this operation's JSON".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem(&body, status, "request_invalid");
}

#[tokio::test]
async fn backend_failed() {
    let (status, body) = render(ApiError::BackendFailed {
        detail: "runs/: permission denied".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_problem(&body, status, "backend_failed");
}

#[tokio::test]
async fn host_refused() {
    let (status, body) = render(ApiError::HostRefused {
        host: Some("evil.example".to_owned()),
    })
    .await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
    assert_problem(&body, status, "host_refused");
}

#[tokio::test]
async fn origin_refused() {
    let (status, body) = render(ApiError::OriginRefused {
        origin: Some("http://evil.example".to_owned()),
    })
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_problem(&body, status, "origin_refused");
}

#[tokio::test]
async fn route_not_found() {
    let (status, body) = render(ApiError::RouteNotFound {
        path: "/api/v1/nowhere".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem(&body, status, "route_not_found");
    assert!(body["detail"].as_str().unwrap().contains("/api/v1/nowhere"));
}

#[tokio::test]
async fn method_not_allowed() {
    let (status, body) = render(ApiError::MethodNotAllowed {
        method: "DELETE".to_owned(),
        path: "/api/v1/runs".to_owned(),
    })
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_problem(&body, status, "method_not_allowed");
}

#[test]
fn every_variant_has_a_distinct_code() {
    let codes = ApiError::CODES;
    let mut sorted = codes.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), codes.len(), "codes are unique: {codes:?}");
    assert_eq!(
        codes.len(),
        25,
        "a variant added without a test here: {codes:?}"
    );
}
