//! The server's defence of its origin (ADR-C36 § 1), as Tower layers on the
//! router: the `Host` check, the `Origin` check on a state-changing method,
//! the content security policy and the build identity on every response.

mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use support::{
    app, app_with_assets, get, json, send, FakeAssets, FakeRunStore, BUILD, OWN_ORIGIN, SERVED,
};

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
    // The empty body is no submission, so the handler refuses it: what
    // matters is that the origin check let the request reach it.
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(response).await["code"], "request_invalid");
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
            "default-src 'self'; frame-ancestors 'none'",
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

#[tokio::test]
async fn a_host_header_that_is_not_text_is_refused_not_read_from_the_uri() {
    // The URI names the served authority; the header, present and unreadable,
    // must not be skipped in its favour.
    let response = send(
        app(FakeRunStore::default()),
        request("GET", &format!("http://{SERVED}/api/v1/runs"))
            .header(
                "host",
                axum::http::HeaderValue::from_bytes(b"127.0.0.1:7878\xff").unwrap(),
            )
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
    assert_eq!(json(response).await["code"], "host_refused");
}

/// The UI's pages as the binary hands them in: an index page, answered at `/`
/// and on every path the application routes on the client.
fn assets() -> FakeAssets {
    FakeAssets::built()
}

#[tokio::test]
async fn the_index_page_and_a_client_route_carry_both_headers() {
    for path in ["/", "/runs/some/client/route"] {
        let response = send(app_with_assets(assets()), get(path)).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response.headers()["content-security-policy"],
            "default-src 'self'; frame-ancestors 'none'",
            "{path}"
        );
        assert_eq!(
            response.headers()[ragondin_api::BUILD_HEADER],
            BUILD,
            "{path}"
        );
    }
}

#[tokio::test]
async fn the_index_page_and_a_client_route_are_refused_on_a_foreign_host() {
    for path in ["/", "/runs/some/client/route"] {
        let response = send(
            app_with_assets(assets()),
            request("GET", path)
                .header("host", "evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST, "{path}");
        assert_eq!(
            response.headers()[ragondin_api::BUILD_HEADER],
            BUILD,
            "{path}"
        );
    }
}

#[tokio::test]
async fn an_unknown_api_path_is_a_problem_even_beside_an_assets_fallback() {
    let response = send(app_with_assets(assets()), get("/api/v1/nowhere")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers()["content-type"],
        "application/problem+json"
    );
    assert_eq!(json(response).await["code"], "route_not_found");
}

/// axum 0.7's `nest` leaves the prefix itself, with and without a trailing
/// slash, to the outer router — where the assets' fallback would answer it
/// with a page. Both are the API's.
#[tokio::test]
async fn the_api_prefix_itself_is_route_not_found_beside_an_assets_fallback() {
    for path in ["/api/", "/api"] {
        let response = send(app_with_assets(assets()), get(path)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        let body = json(response).await;
        assert_eq!(body["code"], "route_not_found", "{path}");
        assert_eq!(body["detail"], format!("no endpoint at {path}"), "{path}");
    }
}

/// `/apix` is not under `/api`: it is the assets' to answer.
#[tokio::test]
async fn a_path_that_only_starts_with_the_prefix_is_not_the_apis() {
    let response = send(app_with_assets(assets()), get("/apix")).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn an_unknown_api_path_is_route_not_found() {
    let response = send(app(FakeRunStore::default()), get("/api/v1/nowhere")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(response).await["code"], "route_not_found");
}

#[tokio::test]
async fn a_method_a_route_does_not_serve_is_method_not_allowed_with_allow() {
    let response = send(
        app(FakeRunStore::default()),
        request("DELETE", "/api/v1/runs")
            .header("host", SERVED)
            .header("origin", OWN_ORIGIN)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(response.headers()["allow"]
        .to_str()
        .unwrap()
        .contains("GET"));
    assert_eq!(json(response).await["code"], "method_not_allowed");
}
