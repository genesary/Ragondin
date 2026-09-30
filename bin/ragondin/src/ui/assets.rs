//! The UI's static assets, embedded at build time, served at `/`.
//!
//! `build.rs` writes the `rust-embed` derive included below, over `ui/dist/`
//! when it has been built and over a generated notice page when not
//! (ADR-C36 § 5), so this
//! module never knows which: it serves whatever was embedded. `debug-embed`
//! makes a debug build embed the files too, so no build reads `ui/` at run
//! time.
//!
//! This is the `Router` handed to `ragondin_api::router` as its assets: that
//! function nests the API beside it and wraps both in the server's layers, so
//! every answer given here carries the build identity and the content security
//! policy and is refused on a foreign `Host`, exactly as the API's are.

use std::path::Path;

use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{StatusCode, Uri};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use rust_embed::RustEmbed;

// `struct Assets`, deriving `RustEmbed` over the folder `build.rs` chose:
// `ui/dist/`, or the generated notice page.
include!(concat!(env!("OUT_DIR"), "/assets.rs"));

/// The page every client-side route is answered with.
const INDEX: &str = "index.html";

/// The router the assets are served by: one fallback, for `GET` and `HEAD`,
/// that answers every path the API does not. Any other method is a `405`.
pub fn router() -> Router {
    Router::new().fallback_service(get(|uri: Uri| async move { answer::<Assets>(uri.path()) }))
}

/// The answer to a `GET` of `path` from the assets `A`.
///
/// `/` is the index page. A path naming an embedded file is that file, with
/// its content type. Any other path whose last segment has no `.` is a
/// client-side route — the UI routes itself — and is answered with the index
/// page. Any other path is a request for a file that is not there, and is a
/// `404`: answering it with the page would hand a `<script>` tag HTML.
/// Lookups are in the embedded table, never on disk, so a `..` segment
/// names nothing.
fn answer<A: RustEmbed>(path: &str) -> Response {
    let path = match path.trim_start_matches('/') {
        "" => INDEX,
        path => path,
    };
    if let Some(file) = A::get(path) {
        return file_response(path, file.data.into_owned());
    }
    let last = path.rsplit('/').next().unwrap_or(path);
    if !last.contains('.') {
        if let Some(index) = A::get(INDEX) {
            return file_response(INDEX, index.data.into_owned());
        }
    }
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(format!("no file at /{path}\n")))
        .expect("a static response is well formed")
}

fn file_response(path: &str, data: Vec<u8>) -> Response {
    Response::builder()
        .header(CONTENT_TYPE, content_type(path))
        .body(Body::from(data))
        .expect("a static response is well formed")
}

/// The content type of an asset at `path`, by its extension, ignoring case.
///
/// The extensions a Vite build emits, and a few a static asset may carry;
/// anything else is bytes. Text types name UTF-8, which is what Vite writes.
/// A table here rather than `rust-embed`'s `mime-guess` feature: the set is
/// small and fixed, and one table is less than another crate.
fn content_type(path: &str) -> &'static str {
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("txt") => "text/plain; charset=utf-8",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A built UI, as `vite build` lays one out.
    #[derive(RustEmbed)]
    #[folder = "tests/fixtures/ui-dist"]
    struct Fixture;

    async fn get(path: &str) -> (StatusCode, Option<String>, String) {
        let response = answer::<Fixture>(path);
        let status = response.status();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .map(|value| value.to_str().expect("ASCII").to_owned());
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body is read");
        (
            status,
            content_type,
            String::from_utf8(body.to_vec()).expect("UTF-8 fixtures"),
        )
    }

    fn fixture(path: &str) -> String {
        String::from_utf8(Fixture::get(path).expect("in the fixture").data.to_vec())
            .expect("UTF-8 fixtures")
    }

    #[tokio::test]
    async fn the_root_is_the_index_page() {
        let (status, content_type, body) = get("/").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type.as_deref(), Some("text/html; charset=utf-8"));
        assert_eq!(body, fixture("index.html"));
    }

    #[tokio::test]
    async fn an_asset_is_served_with_its_content_type() {
        for (path, expected) in [
            ("/assets/index-abc123.js", "text/javascript; charset=utf-8"),
            ("/assets/index-abc123.css", "text/css; charset=utf-8"),
            ("/favicon.svg", "image/svg+xml"),
            ("/assets/inter-latin.woff2", "font/woff2"),
            ("/index.html", "text/html; charset=utf-8"),
        ] {
            let (status, content_type, body) = get(path).await;

            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(content_type.as_deref(), Some(expected), "{path}");
            assert_eq!(body, fixture(path.trim_start_matches('/')), "{path}");
        }
    }

    #[tokio::test]
    async fn a_client_side_route_is_answered_with_the_index_page() {
        for path in ["/runs", "/runs/3f9a", "/compare/a/b"] {
            let (status, content_type, body) = get(path).await;

            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(content_type.as_deref(), Some("text/html; charset=utf-8"));
            assert_eq!(body, fixture("index.html"), "{path}");
        }
    }

    #[tokio::test]
    async fn a_missing_file_is_not_found_rather_than_the_index_page() {
        // A path whose last segment names a file is a request for that file:
        // answering the page would hand a script tag HTML.
        for path in ["/assets/gone-123.js", "/robots.txt", "/../Cargo.toml"] {
            let (status, _, body) = get(path).await;

            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
            assert_ne!(body, fixture("index.html"), "{path}");
        }
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
            assert_eq!(content_type(path), expected, "{path}");
        }
    }

    #[test]
    fn the_embedded_folder_always_has_an_index_page() {
        // `ui/dist/` or the notice: whichever `build.rs` chose, `/` answers.
        assert!(Assets::get("index.html").is_some());
    }
}
