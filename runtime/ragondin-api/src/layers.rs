//! The server's defence of its origin (ADR-C36 § 1), as Tower layers on the
//! router — the network envelope, which is where ADR-C10 puts Tower and the
//! only place this crate uses it (INV-11): no handler and no trait is a
//! `tower::Service`.
//!
//! Four layers, outermost first, so that a refusal carries the two headers
//! every response carries:
//!
//! 1. the build identity header, on every response;
//! 2. the content security policy, on every response;
//! 3. the `Host` check: a request must name the address the server serves;
//! 4. the `Origin` check: a `POST`, `PUT`, `PATCH` or `DELETE` must come from
//!    the server's own origin.
//!
//! The listener and the loopback-only rule are the binary's; these layers
//! assume nothing about where the router is bound.

use std::convert::Infallible;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderName, HeaderValue, Method};
use axum::middleware::{from_fn_with_state, FromFnLayer, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::Route;
use tower::{Layer, Service, ServiceBuilder};

use crate::error::ApiError;

/// The header every response carries the build identity in. The UI compares
/// it with its own build's and reloads when they differ (ADR-C36 § 1).
pub const BUILD_HEADER: &str = "x-ragondin-build";

/// The content security policy: every source is the server's own origin, and
/// no page may frame this one — `default-src` does not govern framing, and a
/// page that launches runs is a clickjacking target. Nothing is added for
/// inline styles — `ARCHITECTURE.md` § The layers says why the UI needs no
/// `'unsafe-inline'`.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; frame-ancestors 'none'";

/// What the layers compare a request against, fixed when the router is built.
struct Envelope {
    /// The authority the server serves, e.g. `127.0.0.1:7878`.
    served: String,
    /// The origin a page the server served sends: `http://` and `served`.
    origin: String,
    /// The build identity, ready to be a header value.
    build: HeaderValue,
}

/// The four layers, as one stack, outermost first: what `router` in `lib.rs`
/// applies to the whole server with one `Router::layer`. This function adds
/// nothing to a router itself, so it needs no `Router` method.
///
/// A build identity that is not a valid header value is replaced by a
/// visible marker rather than dropped: a response without the header would
/// read to the UI as a build that cannot be compared, which is the case the
/// header exists to prevent.
pub(crate) fn envelope(
    served: &str,
    build: &str,
) -> impl Layer<
    Route,
    Service = impl Service<
        Request,
        Response = Response,
        Error = Infallible,
        Future = impl Send + 'static,
    > + Clone
                  + Send
                  + Sync
                  + 'static,
> + Clone
       + Send
       + Sync
       + 'static {
    let envelope = Arc::new(Envelope {
        served: served.to_owned(),
        origin: format!("http://{served}"),
        build: HeaderValue::from_str(build)
            .unwrap_or_else(|_| HeaderValue::from_static("invalid-build-identity")),
    });
    ServiceBuilder::new()
        .layer(middleware(envelope.clone(), build_identity))
        .layer(middleware(envelope.clone(), content_security_policy))
        .layer(middleware(envelope.clone(), check_host))
        .layer(middleware(envelope, check_origin))
}

/// One of the envelope's middleware functions over its state, as a layer.
// The envelope's middleware (ADR-C10) is the one place `clippy.toml` lets a
// middleware read the request, since anywhere else it would read it around
// the ADR-C37 § 2 guard. The allow covers this one call and nothing else, so
// a route, a nested router or a service added in `envelope` is still refused.
#[allow(clippy::disallowed_methods)]
fn middleware<F, T>(envelope: Arc<Envelope>, function: F) -> FromFnLayer<F, Arc<Envelope>, T> {
    from_fn_with_state(envelope, function)
}

async fn build_identity(
    State(envelope): State<Arc<Envelope>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        HeaderName::from_static(BUILD_HEADER),
        envelope.build.clone(),
    );
    response
}

async fn content_security_policy(
    State(_): State<Arc<Envelope>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    response
}

/// Refuses a request whose `Host` is not the served authority — the defence
/// against DNS rebinding, where a hostile page's name comes to resolve to the
/// loopback. The comparison ignores ASCII case, as host names do; it does not
/// resolve names, so `localhost` is refused by a server given `127.0.0.1`.
/// A request with no `Host` header is judged by its URI's authority, which is
/// where HTTP/2 carries it, and refused when it has neither; one whose `Host`
/// is present and not text is refused.
async fn check_host(
    State(envelope): State<Arc<Envelope>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    // The URI's authority is consulted only when the header is absent: a
    // header that is present and not text is refused, never skipped in favour
    // of whatever the URI says.
    let host = match request.headers().get(header::HOST) {
        Some(value) => value.to_str().ok().map(str::to_owned),
        None => request.uri().authority().map(|a| a.as_str().to_owned()),
    };
    match host {
        Some(host) if host.eq_ignore_ascii_case(&envelope.served) => next.run(request).await,
        host => ApiError::HostRefused { host }.into_response(),
    }
}

/// Refuses a state-changing request whose `Origin` is not the server's own.
/// A missing `Origin` is refused too: a browser sends one on every such
/// request, and the embedded UI is this API's only client (ADR-C36 § 2), so
/// a request without one is not the UI's.
async fn check_origin(
    State(envelope): State<Arc<Envelope>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let changes_state = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if !changes_state {
        return next.run(request).await;
    }
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    match origin {
        Some(origin) if origin.eq_ignore_ascii_case(&envelope.origin) => next.run(request).await,
        origin => ApiError::OriginRefused { origin }.into_response(),
    }
}
