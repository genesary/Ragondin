//! The API description is a golden file: regenerated here and compared byte
//! for byte, so a change to the API is a reviewed diff. `just
//! gen-api-description` rewrites it.

mod support;

use std::path::Path;

use axum::http::StatusCode;
use support::{app, get, send, FakeRunStore};

#[test]
fn the_committed_description_is_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("api/v1.json");
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == ragondin_api::description::render(),
        "{} is stale: run `just gen-api-description` and review the diff",
        path.display()
    );
}

#[test]
fn the_description_lists_every_operation_with_its_schema() {
    let description: serde_json::Value =
        serde_json::from_str(&ragondin_api::description::render()).unwrap();

    for operation in ragondin_api::description::OPERATIONS {
        let entry = &description["paths"][operation.path][operation.method];
        let schema = &entry["responses"]["200"]["content"]["application/json"]["schema"]["$ref"];
        assert_eq!(
            schema,
            &format!("#/components/schemas/{}", operation.response),
            "{} {}",
            operation.method,
            operation.path
        );
        assert!(
            description["components"]["schemas"][operation.response].is_object(),
            "{} is defined under components/schemas",
            operation.response
        );
        assert_eq!(
            entry["responses"]["default"]["content"]["application/problem+json"]["schema"]["$ref"],
            "#/components/schemas/Problem"
        );
    }
}

/// The description is written from a table, and the router from code: this
/// is what keeps the two saying the same thing.
#[tokio::test]
async fn every_described_operation_is_routed() {
    for operation in ragondin_api::description::OPERATIONS {
        assert_eq!(operation.method, "get", "only reads are routed yet");
        // An axum path parameter is spelled `:id`, the description's `{id}`.
        let path = operation.path.replace(
            "{id}",
            "b41e0752792e728f5dd893043b42d2a2d71f0b0039157a177e0a267e0420ea6f",
        );
        let response = send(
            app(FakeRunStore::holding([support::fixture_run()])),
            get(&format!("/api/v1{path}")),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}
