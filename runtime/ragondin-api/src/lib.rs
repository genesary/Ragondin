//! # ragondin-api
//!
//! The JSON API the front end consumes (ADR-C36 § 2): the router, its
//! handlers, the response types, the typed errors, and the traits the service
//! consumes. **Internal, and not an API boundary**: its one consumer is the UI
//! embedded in the same build, so it changes with the UI in the same pull
//! request (`ARCHITECTURE.md` says why, by ADR-C21's test).
//!
//! **INV-12**: this crate reaches no crate under `engine/` or `components/`,
//! nor `ragondin-remote`, directly or through any other crate. It cannot
//! execute a pipeline, construct a component or call one; the only path from
//! the UI to the data plane is [`Launcher`], implemented by the binary.
//!
//! [`router`] builds the whole server from [`Backends`], a [`ServerConfig`]
//! and the UI's [`Assets`], all passed in by the binary, applies the server's
//! layers last, and returns a [`Server`]: something [`serve`] listens with and
//! no route can be added to, so every route it answers is one the layers
//! wrap. Nothing here is a static or a global, and nothing binds a port — the
//! listener is the binary's, handed to [`serve`].
//!
//! The modules:
//!
//! - [`backends`] — the traits and the values they exchange.
//! - [`assets`] — the [`Assets`] table the binary hands in, and how a request
//!   reaches it: the single-page fallback, the content types.
//! - [`response`] — every type the API serializes; this crate's own, never a
//!   core type serialized directly.
//! - [`error`] — [`ApiError`] and its `application/problem+json` rendering.
//! - [`description`] — the API description, kept as a golden file.
//! - [`fs`] — the file backends: [`fs::FsRegistry`] today.
//! - `conformance` — behind the `conformance` feature, the suite every
//!   [`Registry`] backend passes.
//!
//! Every path is under `/api/v1`: `GET /workspace`, `GET /runs`,
//! `GET /runs/{id}`, `GET /runs/{id}/queries` and
//! `GET /runs/{id}/trace/{query}`. The last two serve derived data — per-query
//! scores, per-node metrics, passage text — computed on read against the run's
//! own dataset, cached under the workspace's `cache/`, and never written into
//! the run (`ARCHITECTURE.md` § Derived data).

#![warn(missing_docs)]

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use axum::routing::{any, get, IntoMakeService};
use axum::{Router, ServiceExt};
use tower::Service;

pub mod assets;
pub mod backends;
#[cfg(feature = "conformance")]
pub mod conformance;
pub mod description;
pub mod error;
pub mod fs;
pub mod response;

mod cache;
mod convert;
mod derived;
mod handlers;
mod layers;

pub use assets::{content_type_for, Asset, Assets, NoAssets};
pub use backends::{
    Backends, DownloadProgress, Job, JobState, Launcher, PipelineEntry, PipelineFile,
    PipelineSource, ProgressSink, Registry, Revision, RunDataset, ServiceIdentity, Settings,
    Submission, WorkspaceSettings,
};
pub use error::ApiError;
pub use layers::BUILD_HEADER;
pub use response::{
    BenchmarkEntry, BenchmarkState, Capabilities, EdgeLocation, FamilyCapabilities, GroundTruth,
    Location, Problem, ServiceBinding,
};

/// What the binary fixes when it builds the router.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerConfig {
    /// The authority the server serves, e.g. `127.0.0.1:7878`: the only
    /// `Host` it answers, and with `http://`, the only origin it accepts a
    /// state-changing request from.
    pub served: String,
    /// The build's identity — the crate version with a build hash, say — as
    /// `GET /workspace` and every response's `x-ragondin-build` header
    /// report it.
    ///
    /// **It must change with every change to the code** — the commit and a
    /// dirty flag, say. The derived-data cache under the workspace's `cache/`
    /// is keyed on it, since another build may score or derive differently:
    /// a build string that stayed the same across a code change would serve
    /// figures the new code would not compute.
    pub build: String,
    /// The workspace directory, as `GET /workspace` reports it.
    pub workspace: PathBuf,
}

/// The whole server: the API under `/api`, `assets` beside it, and the
/// server's layers around both.
///
/// `assets` is the UI's files, as data: every path outside `/api` is looked
/// up in it, with the single-page fallback [`assets`](mod@assets) describes.
/// The routes that serve them are written here, because `Router::layer`
/// wraps only the routes that exist when it is called: the envelope is
/// applied last, here, over everything the server answers, and what comes
/// back is a [`Server`], which has no method that adds a route. Pass
/// [`NoAssets`] for none.
///
/// `/api`, `/api/` and every path below them are the API's: an unknown one
/// is a `route_not_found` problem, never an asset. A path that merely starts
/// with the same letters, such as `/apix`, is not under `/api` and is the
/// assets' to answer.
pub fn router(backends: Backends, config: ServerConfig, assets: Arc<dyn Assets>) -> Server {
    let (served, build) = (config.served.clone(), config.build.clone());
    // Each route answers a method it does not serve with a problem body;
    // axum still sets `Allow`.
    let api = Router::new()
        .route(
            "/v1/workspace",
            get(handlers::workspace).fallback(handlers::method_not_allowed),
        )
        .route(
            "/v1/runs",
            get(handlers::runs).fallback(handlers::method_not_allowed),
        )
        // axum 0.7 spells a path parameter `:id`; the description's `{id}`.
        .route(
            "/v1/runs/:id",
            get(handlers::run).fallback(handlers::method_not_allowed),
        )
        .route(
            "/v1/runs/:id/queries",
            get(handlers::queries).fallback(handlers::method_not_allowed),
        )
        .route(
            "/v1/runs/:id/trace/:query",
            get(handlers::trace).fallback(handlers::method_not_allowed),
        )
        .fallback(handlers::route_not_found)
        .with_state(handlers::AppState {
            backends,
            config: Arc::new(config),
        });
    let server = Router::new()
        .nest(handlers::API_PREFIX, api)
        // axum 0.7's `nest` leaves the prefix with a trailing slash to the
        // outer router, where the assets' fallback would answer it.
        .route("/api/", any(handlers::prefix_not_found))
        .merge(assets::router(assets));
    Server {
        router: layers::wrap(server, &served, &build),
    }
}

/// Serves `server` on `listener` until the listener fails: the one place the
/// server meets a socket, so the binary names no HTTP stack.
///
/// Each connection gets a clone of `server`, and every request it answers is
/// inside the envelope [`router`] applied.
pub async fn serve(listener: tokio::net::TcpListener, server: Server) -> std::io::Result<()> {
    axum::serve(listener, server.into_make_service()).await
}

/// The server [`router`] builds, enveloped: what [`serve`] listens with, and
/// nothing more.
///
/// Opaque on purpose. A `Router` can be extended, and a route added to it
/// after the layers would answer outside them; a `Server` has no method that
/// adds a route, merges a router or sets a fallback, and does not convert
/// back into a `Router`. What it offers is what serving needs:
/// [`into_make_service`](Self::into_make_service), which [`serve`] hands
/// to `axum::serve`, and
/// `tower::Service` over one request, which is what a connection calls —
/// the network envelope, where ADR-C10 puts Tower (INV-11 is about
/// components, and this is none).
///
/// ```
/// # async fn run(server: ragondin_api::Server, listener: tokio::net::TcpListener) {
/// let _stopped = ragondin_api::serve(listener, server).await;
/// # }
/// ```
///
/// No route can be added to it:
///
/// ```compile_fail,E0599
/// # fn extend(server: ragondin_api::Server) {
/// let _ = server.route("/", axum::routing::get(|| async { "outside" }));
/// # }
/// ```
///
/// nor can it be merged into a router, which would take it as one:
///
/// ```compile_fail,E0277
/// # fn merge(server: ragondin_api::Server) {
/// let _ = axum::Router::new().merge(server);
/// # }
/// ```
///
/// A caller can still write a second router of its own around a `Server`,
/// answering routes it adds itself. That is a new server outside this
/// envelope, written by hand — and in the binary it would need `axum`, which
/// the binary does not depend on: a diff a reviewer sees in its manifest.
#[derive(Clone)]
pub struct Server {
    router: Router,
}

impl Server {
    /// The server as `axum::serve` takes it: each connection gets a clone.
    pub fn into_make_service(self) -> IntoMakeService<Server> {
        ServiceExt::<Request<Body>>::into_make_service(self)
    }
}

impl Service<Request<Body>> for Server {
    type Response = Response;
    type Error = Infallible;
    type Future = <Router as Service<Request<Body>>>::Future;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Service::<Request<Body>>::poll_ready(&mut self.router, context)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        self.router.call(request)
    }
}
