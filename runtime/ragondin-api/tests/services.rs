//! The service endpoints over `workspace.toml`: `GET /services`,
//! `PUT`/`DELETE /services/{family}/{name}` and the probe, with a fake
//! launcher standing in for the composition root. Also `GET /workspace`'s
//! counts, which read the same state.

mod support;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::StatusCode;
use ragondin_api::fs::{FsPipelines, FsSettings, Workspace};
use serde_json::{json, Value};
use support::{
    app_with_backends, fakes, get, json as body_json, send, write_request, FakeLauncher,
    FakeRunStore, PROBED_IDENTITY,
};

fn scratch(test_name: &str) -> Workspace {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("services")
        .join(test_name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("the scratch directory is creatable");
    Workspace::open(&path).expect("a workspace opens")
}

/// One server over `workspace`, kept across requests: the probe's memory is
/// the server's.
fn server(workspace: &Workspace) -> ragondin_api::Server {
    let mut backends = fakes(FakeRunStore::holding([support::fixture_run()]));
    backends.settings = Arc::new(FsSettings::new(workspace));
    backends.pipelines = Arc::new(FsPipelines::new(workspace));
    backends.launcher = Arc::new(FakeLauncher::default());
    app_with_backends(backends)
}

fn put_service(path: &str, uri: &str) -> axum::http::Request<axum::body::Body> {
    write_request("PUT", path, &json!({ "uri": uri }), &[])
}

#[tokio::test]
async fn a_service_put_is_stored_in_workspace_toml_and_read_back() {
    let workspace = scratch("put");
    let app = server(&workspace);

    let response = send(
        app.clone(),
        put_service("/api/v1/services/generator/qwen", "http://[::1]:8080"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let text = fs::read_to_string(workspace.settings_file()).unwrap();
    assert!(
        text.contains("\"generator/qwen\" = \"http://[::1]:8080\""),
        "{text}"
    );
    let listing = body_json(send(app.clone(), get("/api/v1/services")).await).await;
    assert_eq!(
        listing,
        json!({ "services": [{
            "family": "generator", "name": "qwen", "uri": "http://[::1]:8080",
            "connected": false, "identity": null,
        }] })
    );

    // A second PUT replaces the address; it does not bind the name twice.
    send(
        app.clone(),
        put_service("/api/v1/services/generator/qwen", "http://127.0.0.1:9000"),
    )
    .await;
    let listing = body_json(send(app, get("/api/v1/services")).await).await;
    assert_eq!(listing["services"].as_array().unwrap().len(), 1);
    assert_eq!(listing["services"][0]["uri"], "http://127.0.0.1:9000");
}

#[tokio::test]
async fn a_service_with_a_malformed_family_name_or_uri_is_refused_with_the_cli_words() {
    let workspace = scratch("refused");
    let app = server(&workspace);
    let before = fs::read(workspace.settings_file()).unwrap();

    for path in [
        "/api/v1/services/store/qdrant",
        "/api/v1/services/generator/qwen",
    ] {
        let uri = if path.contains("store") {
            "http://host"
        } else {
            "https://host"
        };
        let response = send(app.clone(), put_service(path, uri)).await;

        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{path}"
        );
        let problem = body_json(response).await;
        assert_eq!(problem["code"], "binding_refused");
        // The launcher's words, passed through untouched.
        assert!(
            problem["detail"].as_str().unwrap().contains("`--remote "),
            "{problem}"
        );
    }
    assert_eq!(fs::read(workspace.settings_file()).unwrap(), before);
}

#[tokio::test]
async fn deleting_a_service_unbinds_it_and_an_unbound_one_is_not_found() {
    let workspace = scratch("delete");
    let app = server(&workspace);
    send(
        app.clone(),
        put_service("/api/v1/services/generator/qwen", "http://host:1"),
    )
    .await;

    let response = send(
        app.clone(),
        write_request("DELETE", "/api/v1/services/generator/qwen", &json!({}), &[]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await, json!({ "services": [] }));

    let again = send(
        app,
        write_request("DELETE", "/api/v1/services/generator/qwen", &json!({}), &[]),
    )
    .await;
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(again).await["code"], "service_not_found");
}

#[tokio::test]
async fn a_put_and_a_delete_keep_the_comments_of_a_commented_workspace_toml() {
    let workspace = scratch("commented");
    let commented = "\
# Our workspace.

datasets = 'benchmarks' # shared

[services]
# The judge.
\"generator/judge\" = \"http://judge:1\" # do not remove
";
    fs::write(workspace.settings_file(), commented).unwrap();
    let app = server(&workspace);

    let put = send(
        app.clone(),
        put_service("/api/v1/services/generator/qwen", "http://host:1"),
    )
    .await;
    assert_eq!(put.status(), StatusCode::OK);
    assert_eq!(
        fs::read_to_string(workspace.settings_file()).unwrap(),
        format!("{commented}\"generator/qwen\" = \"http://host:1\"\n")
    );

    let delete = send(
        app,
        write_request("DELETE", "/api/v1/services/generator/qwen", &json!({}), &[]),
    )
    .await;
    assert_eq!(delete.status(), StatusCode::OK);
    assert_eq!(
        fs::read_to_string(workspace.settings_file()).unwrap(),
        commented
    );
}

#[tokio::test]
async fn a_service_change_leaves_the_pipelines_and_the_runs_untouched() {
    let workspace = scratch("untouched");
    let pipeline = workspace.pipelines().join("hybrid.yaml");
    fs::write(&pipeline, "pipeline:\n  inputs: [question]\n  nodes: []\n").unwrap();
    let run = workspace.runs().join("some-run");
    fs::create_dir_all(&run).unwrap();
    fs::write(run.join("inputs.json"), "{\"pipeline\":\"x\"}").unwrap();
    let app = server(&workspace);

    send(
        app.clone(),
        put_service("/api/v1/services/generator/qwen", "http://host:1"),
    )
    .await;
    send(
        app,
        write_request("DELETE", "/api/v1/services/generator/qwen", &json!({}), &[]),
    )
    .await;

    assert_eq!(
        fs::read_to_string(&pipeline).unwrap(),
        "pipeline:\n  inputs: [question]\n  nodes: []\n"
    );
    assert_eq!(
        fs::read_to_string(run.join("inputs.json")).unwrap(),
        "{\"pipeline\":\"x\"}"
    );
}

#[tokio::test]
async fn a_probe_returns_the_identity_and_marks_the_service_connected() {
    let workspace = scratch("probe");
    let app = server(&workspace);
    send(
        app.clone(),
        put_service("/api/v1/services/generator/qwen", "http://127.0.0.1:8080"),
    )
    .await;

    let response = send(
        app.clone(),
        write_request(
            "POST",
            "/api/v1/services/generator/qwen/probe",
            &json!({ "served_model": "qwen2.5" }),
            &[],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({ "identity": format!("{PROBED_IDENTITY}:generator/qwen@http://127.0.0.1:8080#qwen2.5") })
    );
    let listing = body_json(send(app.clone(), get("/api/v1/services")).await).await;
    assert_eq!(listing["services"][0]["connected"], true);
    let workspace_body = body_json(send(app, get("/api/v1/workspace")).await).await;
    assert_eq!(workspace_body["counts"]["services_connected"], 1);
}

#[tokio::test]
async fn an_unreachable_probe_names_the_address_and_the_identity_last_read() {
    let workspace = scratch("unreachable");
    let app = server(&workspace);
    send(
        app.clone(),
        put_service(
            "/api/v1/services/context_builder/lines",
            "http://127.0.0.1:8080",
        ),
    )
    .await;
    let probe = || {
        write_request(
            "POST",
            "/api/v1/services/context_builder/lines/probe",
            &json!({}),
            &[],
        )
    };
    assert_eq!(send(app.clone(), probe()).await.status(), StatusCode::OK);

    // The fake launcher answers an address on port 1 as unreachable.
    send(
        app.clone(),
        put_service(
            "/api/v1/services/context_builder/lines",
            "http://127.0.0.1:1",
        ),
    )
    .await;
    let response = send(app.clone(), probe()).await;

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let problem = body_json(response).await;
    assert_eq!(problem["code"], "service_unreachable");
    let detail = problem["detail"].as_str().unwrap();
    assert!(detail.contains("http://127.0.0.1:1"), "{detail}");
    assert!(detail.contains("connection refused"), "{detail}");
    assert!(detail.contains(PROBED_IDENTITY), "{detail}");
    let listing = body_json(send(app, get("/api/v1/services")).await).await;
    assert_eq!(listing["services"][0]["connected"], false);
}

/// A probe needs nothing but the binding for a context builder, so a client
/// may send no body at all: it reads as `{}`.
#[tokio::test]
async fn a_probe_with_no_body_reads_as_one_with_no_served_model() {
    let workspace = scratch("probe_empty_body");
    let app = server(&workspace);
    send(
        app.clone(),
        put_service(
            "/api/v1/services/context_builder/lines",
            "http://127.0.0.1:8080",
        ),
    )
    .await;
    let request = axum::http::Request::post("/api/v1/services/context_builder/lines/probe")
        .header("host", support::SERVED)
        .header("origin", support::OWN_ORIGIN)
        .body(axum::body::Body::empty())
        .unwrap();

    let response = send(app, request).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["identity"],
        format!("{PROBED_IDENTITY}:context_builder/lines@http://127.0.0.1:8080")
    );
}

/// No body is no served model, but a body that is `null` is no probe
/// request: it is refused, as it was when the body was read as bytes.
#[tokio::test]
async fn a_probe_whose_body_is_null_is_request_invalid() {
    let workspace = scratch("probe_null_body");
    let app = server(&workspace);
    send(
        app.clone(),
        put_service(
            "/api/v1/services/context_builder/lines",
            "http://127.0.0.1:8080",
        ),
    )
    .await;

    let response = send(
        app,
        write_request(
            "POST",
            "/api/v1/services/context_builder/lines/probe",
            &Value::Null,
            &[],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(response).await["code"], "request_invalid");
}

/// A misspelled field is refused, not dropped: dropped, `served_modle`
/// would probe without the served model the client meant.
#[tokio::test]
async fn a_probe_body_field_this_api_does_not_read_is_request_invalid() {
    let workspace = scratch("probe_unknown_field");
    let app = server(&workspace);
    send(
        app.clone(),
        put_service("/api/v1/services/generator/qwen", "http://127.0.0.1:8080"),
    )
    .await;

    for (path, body) in [
        (
            "/api/v1/services/generator/qwen/probe",
            json!({ "served_modle": "qwen2.5" }),
        ),
        (
            "/api/v1/services/generator/qwen",
            json!({ "uri": "http://127.0.0.1:8080", "url": "x" }),
        ),
    ] {
        let method = if path.ends_with("probe") {
            "POST"
        } else {
            "PUT"
        };
        let response = send(app.clone(), write_request(method, path, &body, &[])).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        let problem = body_json(response).await;
        assert_eq!(problem["code"], "request_invalid");
        assert!(
            problem["detail"]
                .as_str()
                .unwrap()
                .contains("unknown field"),
            "{problem}"
        );
    }
}

#[tokio::test]
async fn probing_a_name_that_is_not_bound_is_service_not_found() {
    let workspace = scratch("probe_unbound");

    let response = send(
        server(&workspace),
        write_request(
            "POST",
            "/api/v1/services/generator/qwen/probe",
            &json!({}),
            &[],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(response).await["code"], "service_not_found");
}

#[tokio::test]
async fn the_workspace_counts_what_it_holds() {
    let workspace = scratch("counts");
    fs::write(
        workspace.pipelines().join("a.yaml"),
        "pipeline:\n  inputs: [question]\n  nodes: []\n",
    )
    .unwrap();
    let app = server(&workspace);

    let body: Value = body_json(send(app, get("/api/v1/workspace")).await).await;

    assert_eq!(
        body["counts"],
        json!({ "pipelines": 1, "runs": 1, "benchmarks_ready": 0, "services_connected": 0 })
    );
    assert_eq!(
        body["settings"]["datasets"],
        workspace.default_datasets().display().to_string()
    );
}
