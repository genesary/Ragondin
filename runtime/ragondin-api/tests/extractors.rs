//! Request input reaches a handler of the `/api` router only through the
//! crate's own extractors (ADR-C37), so every refusal of a path, a query
//! string, a header or a body is a problem body with a stable code. These
//! tests hold that over every operation the description declares, and pin
//! the query validator and the body's buffering refusals.

mod support;

use std::io::{Read, Write};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ragondin_api::description::OPERATIONS;
use serde_json::{json, Value};
use support::{
    app, fixture_run, get, json as body_json, send, write_request, FakeRunStore, FIXTURE_RUN,
    OWN_ORIGIN, SERVED,
};

/// A valid value for each path parameter the description names.
fn valid(parameter: &str) -> &'static str {
    match parameter {
        "id" => FIXTURE_RUN,
        "query" => "q-1",
        "name" => "hybrid",
        "family" => "generator",
        other => panic!("no valid value for the path parameter `{other}`"),
    }
}

/// The path parameters of a description path, in order.
fn path_parameters(template: &str) -> Vec<&str> {
    template
        .split('/')
        .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
        .collect()
}

/// `template` with every parameter valid but `invalid`, which is `value`.
fn fill(template: &str, invalid: Option<(&str, &str)>) -> String {
    let mut path = template.to_owned();
    for parameter in path_parameters(template) {
        let value = match invalid {
            Some((name, value)) if name == parameter => value,
            _ => valid(parameter),
        };
        path = path.replace(&format!("{{{parameter}}}"), value);
    }
    format!("/api/v1{path}")
}

/// The operation's request at `path`, with an empty JSON object as its body
/// when its method sends one.
fn request(method: &str, path: &str) -> Request<Body> {
    match method {
        "get" => get(path),
        method => write_request(&method.to_uppercase(), path, &json!({}), &[]),
    }
}

async fn answer(method: &str, path: &str) -> (StatusCode, Option<String>, Value) {
    let response = send(
        app(FakeRunStore::holding([fixture_run()])),
        request(method, path),
    )
    .await;
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|value| value.to_str().unwrap().to_owned());
    (status, content_type, body_json(response).await)
}

/// No endpoint ignores a parameter it does not take: each answers
/// `parameter_invalid`, the endpoints that take none (`NoParameters`)
/// included.
#[tokio::test(flavor = "multi_thread")]
async fn an_undeclared_query_parameter_is_refused_by_every_operation() {
    for operation in OPERATIONS {
        let path = format!("{}?undeclared=1", fill(operation.path, None));
        let (status, content_type, body) = answer(operation.method, &path).await;
        let what = format!("{} {path}", operation.method);
        assert_eq!(status, StatusCode::BAD_REQUEST, "{what}: {body}");
        assert_eq!(
            content_type.as_deref(),
            Some("application/problem+json"),
            "{what}"
        );
        assert_eq!(body["code"], "parameter_invalid", "{what}: {body}");
    }
}

/// A segment that does not decode to UTF-8 is refused as the path parameter
/// it fills, by name, on every operation that has one — never axum's
/// plain-text 400.
#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_path_value_is_parameter_invalid_naming_the_parameter() {
    let mut checked = 0;
    for operation in OPERATIONS {
        for parameter in path_parameters(operation.path) {
            let path = fill(operation.path, Some((parameter, "%FF")));
            let (status, content_type, body) = answer(operation.method, &path).await;
            let what = format!("{} {path}", operation.method);
            assert_eq!(status, StatusCode::BAD_REQUEST, "{what}: {body}");
            assert_eq!(
                content_type.as_deref(),
                Some("application/problem+json"),
                "{what}"
            );
            assert_eq!(body["code"], "parameter_invalid", "{what}: {body}");
            assert_eq!(body["name"], parameter, "{what}: {body}");
            checked += 1;
        }
    }
    assert!(checked > 0, "the description declares path parameters");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_undecodable_run_id_is_parameter_invalid_naming_id() {
    let (status, content_type, body) = answer("get", "/api/v1/runs/%FF").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(content_type.as_deref(), Some("application/problem+json"));
    assert_eq!(body["code"], "parameter_invalid");
    assert_eq!(body["name"], "id");
}

/// A query string is strict percent-encoded UTF-8, checked before it is
/// deserialized: a malformed escape, or bytes that are not UTF-8, in a name
/// or a value, is refused as such — never passed through literally, never
/// replaced by U+FFFD.
#[tokio::test(flavor = "multi_thread")]
async fn a_query_that_is_not_percent_encoded_utf8_is_refused_as_one() {
    let malformed = ["%", "%4", "%zz", "%+1", "%FF", "%C0%AF"];
    for bad in malformed {
        for query in [format!("{bad}=1"), format!("missing_gold_at={bad}")] {
            let path = format!("/api/v1/runs/{FIXTURE_RUN}?{query}");
            let (status, content_type, body) = answer("get", &path).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
            assert_eq!(content_type.as_deref(), Some("application/problem+json"));
            assert_eq!(body["code"], "parameter_invalid", "{query}: {body}");
            assert!(
                body["detail"]
                    .as_str()
                    .unwrap()
                    .contains("not percent-encoded UTF-8"),
                "{query}: {}",
                body["detail"]
            );
        }
    }
}

/// `+` is a space, as a form encodes one: the unknown parameter is named
/// with its space.
#[tokio::test(flavor = "multi_thread")]
async fn a_plus_reads_as_a_space() {
    let path = format!("/api/v1/runs/{FIXTURE_RUN}?not+taken=1");
    let (status, _, body) = answer("get", &path).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["detail"].as_str().unwrap().contains("`not taken`"),
        "{}",
        body["detail"]
    );
}

/// A refusal serde does not attribute to one parameter names none: the
/// problem body has no `name`, rather than a guessed one.
#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_serde_does_not_attribute_has_no_name() {
    let path = format!("/api/v1/runs/{FIXTURE_RUN}?undeclared=1");
    let (_, _, body) = answer("get", &path).await;
    assert_eq!(body["code"], "parameter_invalid");
    assert!(body.get("name").is_none(), "{body}");
}

/// The body limit, axum's default, answers a problem body, 413, rather than
/// axum's plain text.
#[tokio::test]
async fn a_body_over_the_limit_is_a_413_problem() {
    let oversized = format!("{{\"document\": \"{}\"}}", "x".repeat(2 * 1024 * 1024 + 1));
    let request = Request::post("/api/v1/pipelines/validate")
        .header("host", SERVED)
        .header("origin", OWN_ORIGIN)
        .header("content-type", "application/json")
        .body(Body::from(oversized))
        .unwrap();
    let response = send(app(FakeRunStore::default()), request).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
    assert_eq!(body_json(response).await["code"], "body_too_large");
}

/// No handler takes axum's `Json` extractor. `clippy.toml` cannot refuse
/// it: `axum::extract::Json` is `axum::Json`, the response every handler
/// returns, and `disallowed-types` sees one type. So this scans the source
/// for the shape a `Json` argument has — a binding typed `Json<…>`, which a
/// response never is — outside the extractor module. A source scan, and so
/// best-effort: an alias (`use axum::Json as J;`) is what it does not see.
#[test]
fn no_handler_takes_axum_s_json_extractor() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut pending = vec![src];
    let mut scanned = 0;
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(
                std::fs::read_dir(&path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs")
            || path.ends_with("extract.rs")
        {
            continue;
        }
        scanned += 1;
        let source: String = std::fs::read_to_string(&path)
            .unwrap()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        for typed in [":Json<", ":axum::Json<", ":axum::extract::Json<"] {
            assert!(
                !source.contains(typed),
                "{}: a binding typed `Json<…>` reads a body around `ApiJson` (ADR-C37 § 2)",
                path.display()
            );
        }
    }
    assert!(scanned > 10, "the scan reads the crate's sources");
}

/// A body that breaks off while it is read — here a chunk whose size is not
/// hexadecimal, over a real connection — is a problem body too.
#[tokio::test(flavor = "multi_thread")]
async fn a_body_that_fails_to_buffer_is_a_problem() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("bound");
    let serving = tokio::spawn(ragondin_api::serve(listener, app(FakeRunStore::default())));

    let response = tokio::task::spawn_blocking(move || {
        let mut stream = std::net::TcpStream::connect(address).expect("the server accepts");
        write!(
            stream,
            "POST /api/v1/pipelines/validate HTTP/1.1\r\nHost: {SERVED}\r\nOrigin: {OWN_ORIGIN}\r\n\
             Content-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n\
             zz\r\n{{}}\r\n0\r\n\r\n"
        )
        .expect("the request is written");
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    })
    .await
    .expect("the client runs");

    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert!(
        response
            .to_ascii_lowercase()
            .contains("content-type: application/problem+json"),
        "{response}"
    );
    assert!(
        response.contains("\"code\":\"request_invalid\""),
        "{response}"
    );
    serving.abort();
}
