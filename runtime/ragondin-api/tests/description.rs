//! The API description is a golden file: regenerated here and compared byte
//! for byte, so a change to the API is a reviewed diff. `just
//! gen-api-description` rewrites it.

mod support;

use std::path::Path;

use axum::http::StatusCode;
use support::{app, get, send, write_request, FakeRunStore};

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

fn schemas() -> serde_json::Value {
    let description: serde_json::Value =
        serde_json::from_str(&ragondin_api::description::render()).unwrap();
    description["components"]["schemas"].clone()
}

fn required(schema: &serde_json::Value) -> Vec<&str> {
    schema["required"]
        .as_array()
        .map(|names| names.iter().map(|name| name.as_str().unwrap()).collect())
        .unwrap_or_default()
}

/// A field serialized on every response is required in the schema, nullable
/// when it can be null — so a generated client types it `T | null`, never
/// `T | undefined`, which the JSON never is.
#[test]
fn a_field_always_serialized_is_required_even_when_nullable() {
    let schemas = schemas();
    assert!(required(&schemas["RunDetail"]).contains(&"prefix_of"));
    assert_eq!(
        schemas["RunDetail"]["properties"]["prefix_of"]["nullable"],
        true
    );
    for field in ["node", "edge"] {
        assert!(required(&schemas["Location"]).contains(&field), "{field}");
    }
    // `location` is omitted when absent, so it stays optional.
    assert!(!required(&schemas["Problem"]).contains(&"location"));
}

/// A client narrows on `code`, so the schema lists the codes.
#[test]
fn the_problem_code_is_an_enum_of_every_stable_code() {
    let codes: Vec<String> = schemas()["Problem"]["properties"]["code"]["enum"]
        .as_array()
        .expect("`code` is an enum")
        .iter()
        .map(|code| code.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(codes, ragondin_api::ApiError::CODES);
}

/// The description is written from a table, and the router from code: this
/// is what keeps the two saying the same thing. Each operation is sent with
/// its method and an empty JSON body; whatever it answers, it is neither the
/// API's "no such endpoint" nor its "not this method".
#[tokio::test]
async fn every_described_operation_is_routed() {
    for operation in ragondin_api::description::OPERATIONS {
        // An axum path parameter is spelled `:id`, the description's `{id}`.
        let path = operation
            .path
            .replace(
                "{id}",
                "b41e0752792e728f5dd893043b42d2a2d71f0b0039157a177e0a267e0420ea6f",
            )
            .replace("{query}", "q-1")
            .replace("{name}", "hybrid")
            .replace("{family}", "generator");
        let request = match operation.method {
            "get" => get(&format!("/api/v1{path}")),
            method => write_request(
                &method.to_uppercase(),
                &format!("/api/v1{path}"),
                &serde_json::json!({}),
                &[],
            ),
        };
        let response = send(
            app(FakeRunStore::holding([support::fixture_run()])),
            request,
        )
        .await;
        let status = response.status();
        let body = support::body(response).await;
        assert!(
            !body.contains("\"route_not_found\"") && !body.contains("\"method_not_allowed\""),
            "{} {path}: {status} {body}",
            operation.method
        );
        // The reads that name nothing the fakes lack answer outright.
        if operation.method == "get" && !path.contains("hybrid") {
            assert_eq!(status, StatusCode::OK, "{path}: {body}");
        }
    }
}

/// Every path parameter an operation reads is declared, and the one query
/// parameter the API takes is stated in its operation's description — the
/// UI's type generator refuses a declared query parameter.
#[test]
fn every_parameter_is_declared_or_described() {
    let description: serde_json::Value =
        serde_json::from_str(&ragondin_api::description::render()).unwrap();
    let declared = |path: &str| -> Vec<(String, String)> {
        description["paths"][path]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|parameter| {
                (
                    parameter["name"].as_str().unwrap().to_owned(),
                    parameter["in"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };
    let pair = |name: &str, place: &str| (name.to_owned(), place.to_owned());
    assert_eq!(
        declared("/runs/{id}/trace/{query}"),
        [pair("id", "path"), pair("query", "path")]
    );
    assert_eq!(declared("/runs/{id}/queries"), [pair("id", "path")]);
    assert_eq!(declared("/runs"), Vec::<(String, String)>::new());
    assert!(
        description["paths"]["/runs/{id}/queries"]["get"]["description"]
            .as_str()
            .unwrap()
            .contains("missing_gold_at")
    );
}
