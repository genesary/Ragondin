//! The UI's static assets, handed in by the binary as data and served here,
//! beside the API and inside the same envelope.
//!
//! The binary owns the table — `rust-embed` over `ui/dist/`, or a notice page
//! when the UI was not built — and implements [`Assets`] over it; this crate
//! owns how a request reaches it. Taking the assets as data rather than as a
//! `Router` keeps every route the server answers written here, where the
//! layers are applied over them, and keeps `axum` out of the binary
//! (ADR-C36 § 6).

use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::header::{ALLOW, CONTENT_TYPE};
use axum::http::{Method, StatusCode, Uri};
use axum::response::Response;
use axum::routing::{any, MethodRouter};

/// One file of the UI: its bytes and the content type it is served with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// The file's content.
    pub bytes: Cow<'static, [u8]>,
    /// The `Content-Type` it is served with — [`content_type_for`] its path,
    /// ordinarily.
    pub content_type: &'static str,
}

/// A table of the UI's files, by path relative to the site root, with no
/// leading `/`: `index.html`, `assets/index-3f9a.js`.
///
/// `get` is asked for the path exactly as the request spelled it, never
/// resolved or decoded, so an implementation over an in-memory table cannot
/// be walked out of: a key with `..` in it is simply one it does not hold.
pub trait Assets: Send + Sync {
    /// The file at `path`, if the table holds one.
    fn get(&self, path: &str) -> Option<Asset>;
}

/// No assets: every path outside `/api` is a `404`.
pub struct NoAssets;

impl Assets for NoAssets {
    fn get(&self, _path: &str) -> Option<Asset> {
        None
    }
}

/// The page every client-side route is answered with.
const INDEX: &str = "index.html";

/// What the assets are served by: [`serve`] over `assets`, answering every
/// method, which `router` in `lib.rs` sets as the server's fallback for
/// every path the API does not answer. A `MethodRouter`, as a handler
/// fallback is, so a `HEAD` answer loses its body as it did.
pub(crate) fn endpoint(assets: Arc<dyn Assets>) -> MethodRouter {
    any(serve).with_state(assets)
}

/// The answer to a request for `uri` from `assets`.
///
/// `GET` and `HEAD` only; any other method is a `405` naming both. `/` is
/// `index.html`. A path naming a file of the table is that file. Any other
/// path whose last segment has no `.` is a client-side route — the UI routes
/// itself — and is answered with `index.html`. Any other path is a request for
/// a file that is not there, and is a `404`: answering it with the page would
/// hand a `<script>` tag HTML and hide the error.
async fn serve(State(assets): State<Arc<dyn Assets>>, method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(ALLOW, "GET, HEAD")
            .body(Body::empty())
            .expect("a static response is well formed");
    }
    let path = match uri.path().trim_start_matches('/') {
        "" => INDEX,
        path => path,
    };
    let last = path.rsplit('/').next().unwrap_or(path);
    let found = assets
        .get(path)
        .or_else(|| (!last.contains('.')).then(|| assets.get(INDEX)).flatten());
    match found {
        Some(asset) => Response::builder()
            .header(CONTENT_TYPE, asset.content_type)
            .body(Body::from(asset.bytes))
            .expect("a static response is well formed"),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Body::from(format!("no file at /{path}\n")))
            .expect("a static response is well formed"),
    }
}

/// The content type of a file at `path`, by its extension, ignoring case.
///
/// The extensions a Vite build emits, and a few a static asset may carry;
/// anything else is bytes. Text types name UTF-8, which is what Vite writes.
/// A fixed table this crate controls, rather than a MIME guesser at run time.
pub fn content_type_for(path: &str) -> &'static str {
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
