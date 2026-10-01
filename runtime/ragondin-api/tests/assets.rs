//! The assets the binary hands in, served beside the API: `/` and every file
//! the table holds, the single-page fallback for a client-side route, a `404`
//! for a missing file, `GET` and `HEAD` only — all inside the envelope.

mod support;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use support::{app_with_assets, body, get, send, FakeAssets, BUILD, INDEX_PAGE};

fn assets() -> FakeAssets {
    FakeAssets::built()
}

#[tokio::test]
async fn the_root_is_the_index_page() {
    let response = send(app_with_assets(assets()), get("/")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert_eq!(body(response).await, INDEX_PAGE);
}

#[tokio::test]
async fn a_file_is_served_with_the_content_type_its_table_gives() {
    for (path, content_type, expected) in [
        (
            "/assets/index-abc123.js",
            "text/javascript; charset=utf-8",
            "console.log(1);",
        ),
        (
            "/assets/index-abc123.css",
            "text/css; charset=utf-8",
            "body{}",
        ),
        ("/favicon.svg", "image/svg+xml", "<svg/>"),
        ("/index.html", "text/html; charset=utf-8", INDEX_PAGE),
    ] {
        let response = send(app_with_assets(assets()), get(path)).await;

        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()["content-type"], content_type, "{path}");
        assert_eq!(body(response).await, expected, "{path}");
    }
}

#[tokio::test]
async fn a_client_side_route_is_answered_with_the_index_page() {
    for path in ["/runs", "/runs/3f9a", "/compare/a/b", "/apix"] {
        let response = send(app_with_assets(assets()), get(path)).await;

        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response.headers()["content-type"],
            "text/html; charset=utf-8",
            "{path}"
        );
        assert_eq!(body(response).await, INDEX_PAGE, "{path}");
    }
}

#[tokio::test]
async fn a_missing_file_is_not_found_rather_than_the_index_page() {
    // A last segment with a `.` names a file: answering the page would hand
    // a `<script>` tag HTML.
    for path in [
        "/assets/gone-123.js",
        "/robots.txt",
        "/../Cargo.toml",
        "/%2e%2e/Cargo.toml",
    ] {
        let response = send(app_with_assets(assets()), get(path)).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_ne!(body(response).await, INDEX_PAGE, "{path}");
    }
}

#[tokio::test]
async fn a_traversal_with_no_extension_is_a_client_route_and_never_a_file() {
    // Its last segment has no `.`, so it is answered with the index page —
    // never with anything outside the table.
    let response = send(app_with_assets(assets()), get("/assets/../../etc/passwd")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body(response).await, INDEX_PAGE);
}

#[tokio::test]
async fn a_path_is_looked_up_in_the_table_as_given_never_resolved() {
    // `..` is not collapsed: the table is asked for the path it was sent, and
    // it holds no such key.
    let assets = assets();
    let _ = send(
        app_with_assets(assets.clone()),
        get("/assets/../index.html"),
    )
    .await;

    assert_eq!(assets.asked(), ["assets/../index.html"]);
}

#[tokio::test]
async fn head_is_answered_and_other_methods_are_not_allowed() {
    let head = send(
        app_with_assets(assets()),
        Request::head("/")
            .header("host", support::SERVED)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()["content-type"], "text/html; charset=utf-8");

    for method in ["POST", "PUT", "DELETE"] {
        let response = send(
            app_with_assets(assets()),
            Request::builder()
                .method(method)
                .uri("/runs")
                .header("host", support::SERVED)
                .header("origin", support::OWN_ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method}"
        );
        assert_eq!(response.headers()["allow"], "GET, HEAD", "{method}");
    }
}

#[tokio::test]
async fn an_api_path_never_falls_through_to_the_assets() {
    for path in ["/api", "/api/", "/api/v1/nowhere", "/api/index.html"] {
        let response = send(app_with_assets(assets()), get(path)).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(
            response.headers()["content-type"],
            "application/problem+json",
            "{path}"
        );
    }
}

#[tokio::test]
async fn with_no_assets_the_root_is_not_found() {
    let response = send(app_with_assets(ragondin_api::NoAssets), get("/")).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[ragondin_api::BUILD_HEADER], BUILD);
}

#[test]
fn content_types_cover_what_a_vite_build_emits_and_default_to_bytes() {
    for (path, expected) in [
        ("a.html", "text/html; charset=utf-8"),
        ("a.js", "text/javascript; charset=utf-8"),
        ("a.mjs", "text/javascript; charset=utf-8"),
        ("a.css", "text/css; charset=utf-8"),
        ("a.json", "application/json"),
        ("a.js.map", "application/json"),
        ("a.svg", "image/svg+xml"),
        ("a.png", "image/png"),
        ("a.jpg", "image/jpeg"),
        ("a.jpeg", "image/jpeg"),
        ("a.webp", "image/webp"),
        ("a.ico", "image/x-icon"),
        ("a.woff2", "font/woff2"),
        ("a.woff", "font/woff"),
        ("a.txt", "text/plain; charset=utf-8"),
        ("a.wasm", "application/wasm"),
        ("a.bin", "application/octet-stream"),
        ("a", "application/octet-stream"),
        ("A.CSS", "text/css; charset=utf-8"),
    ] {
        assert_eq!(ragondin_api::content_type_for(path), expected, "{path}");
    }
}
