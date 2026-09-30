//! The server's defence of its origin (ADR-C36 § 1), as Tower layers on the
//! router: the `Host` check, the `Origin` check on a state-changing method,
//! the content security policy and the build identity on every response.

mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use support::{app, get, json, send, FakeRunStore, BUILD, OWN_ORIGIN, SERVED};

fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder().method(method).uri(path)
}

#[tokio::test]
async fn a_request_naming_another_host_is_refused() {
    let response = send(
        app(FakeRunStore::default()),
        request("GET", "/api/v1/runs")
            .header("host", "evil.example:7878")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
    assert_eq!(json(response).await["code"], "host_refused");
}

#[tokio::test]
async fn a_request_naming_no_host_is_refused() {
    let response = send(
        app(FakeRunStore::default()),
        request("GET", "/api/v1/runs").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn the_host_is_compared_without_regard_to_case() {
    let response = send(
        app(FakeRunStore::default()),
        request("GET", "/api/v1/runs")
            .header("host", "LOCALHOST:7878")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    // `localhost` is not the served address, whatever its case: the check
    // compares the authority the server was given, not what resolves to it.
    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);

    let response = send(
        app(FakeRunStore::default()),
        request("GET", "/api/v1/runs")
            .header("host", SERVED.to_uppercase())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_post_from_another_origin_is_refused() {
    let response = send(
        app(FakeRunStore::default()),
        request("POST", "/api/v1/runs")
            .header("host", SERVED)
            .header("origin", "http://evil.example")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json(response).await["code"], "origin_refused");
}

#[tokio::test]
async fn a_state_changing_request_with_no_origin_is_refused() {
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let response = send(
            app(FakeRunStore::default()),
            request(method, "/api/v1/runs")
                .header("host", SERVED)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method}");
    }
}

#[tokio::test]
async fn a_post_from_its_own_origin_passes_the_check() {
    let response = send(
        app(FakeRunStore::default()),
        request("POST", "/api/v1/runs")
            .header("host", SERVED)
            .header("origin", OWN_ORIGIN)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    // No `POST /runs` route exists yet, so the router answers that the
    // method is not allowed: what matters is that the origin check let the
    // request reach it.
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn a_get_needs_no_origin() {
    let response = send(app(FakeRunStore::default()), get("/api/v1/runs")).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn every_response_carries_the_content_security_policy() {
    for request in [
        get("/api/v1/runs"),
        get("/api/v1/nowhere"),
        request("GET", "/api/v1/runs")
            .header("host", "evil.example")
            .body(Body::empty())
            .unwrap(),
    ] {
        let response = send(app(FakeRunStore::default()), request).await;
        assert_eq!(
            response.headers()["content-security-policy"],
            "default-src 'self'",
            "status {}",
            response.status()
        );
    }
}

#[tokio::test]
async fn every_response_carries_the_build_identity() {
    for request in [
        get("/api/v1/workspace"),
        get("/api/v1/nowhere"),
        request("DELETE", "/api/v1/runs")
            .header("host", SERVED)
            .body(Body::empty())
            .unwrap(),
    ] {
        let response = send(app(FakeRunStore::default()), request).await;
        assert_eq!(
            response.headers()[ragondin_api::BUILD_HEADER],
            BUILD,
            "status {}",
            response.status()
        );
    }
}
