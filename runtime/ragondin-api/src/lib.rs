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
//! - [`request`] — every request body the API reads, and the types its
//!   query parameters and request headers are read into; this crate's own
//!   too. A handler reads every input through the crate's own extractors,
//!   in one private module (ADR-C37), so every refusal is a problem body.
//! - [`error`] — [`ApiError`] and its `application/problem+json` rendering.
//! - [`description`] — the API description, kept as a golden file.
//! - [`fs`] — the workspace on disk and its file backends:
//!   [`fs::Workspace`], [`fs::FsSettings`], [`fs::FsPipelines`],
//!   [`fs::FsRegistry`].
//! - [`jobs`] — the job model and the queue: `jobs/<id>.json`, the run and
//!   download lanes with one worker each, the event stream.
//! - `conformance` — behind the `conformance` feature, the suite every
//!   [`Registry`] backend passes.
//!
//! Every path is under `/api/v1`, and [`description::OPERATIONS`] lists
//! them: the workspace, the runs, the comparison of runs, the pipelines and
//! their layouts, the benchmarks, the queue of jobs and the services.
//! `GET /runs/{id}/queries` and
//! `GET /runs/{id}/trace/{query}` serve derived data — per-query scores,
//! per-node metrics, passage text — computed on read against the run's own
//! dataset, cached under the workspace's `cache/`, and never written into the
//! run (`ARCHITECTURE.md` § Derived data).

#![warn(missing_docs)]

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use axum::routing::{any, IntoMakeService};
use axum::{Router, ServiceExt};
use tower::Service;

pub mod assets;
pub mod backends;
#[cfg(feature = "conformance")]
pub mod conformance;
pub mod description;
pub mod error;
pub mod fs;
pub mod jobs;
pub mod request;
pub mod response;

mod cache;
mod comparison;
mod convert;
mod derived;
mod endpoints;
mod extract;
mod handlers;
mod layers;
mod lineage;
mod matrix;
mod routes;
mod stages;
mod validation;

pub use assets::{content_type_for, Asset, Assets, NoAssets};
pub use backends::{
    Backends, Cancellation, DownloadProgress, Launcher, LauncherError, LoadedDataset,
    PinnedBenchmark, PipelineFile, PipelineSource, Precondition, ProgressSink, QueryProgress,
    Registry, Revision, RunDataset, RunObserver, ServiceIdentity, Settings, Submission,
    WorkspaceSettings,
};
pub use convert::family_ports;
pub use error::ApiError;
pub use layers::BUILD_HEADER;
pub use response::{
    BenchmarkEntry, BenchmarkState, Capabilities, ConsumedPorts, EdgeKind, EdgeLocation,
    FamilyCapabilities, FamilyPorts, GroundTruth, Layout, Location, NodePair, NotCarried, Pairing,
    Position, Problem, ServiceBinding,
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
    /// The workspace directory, as `GET /workspace` reports it; the queue
    /// keeps its jobs in its `jobs/`.
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
///
/// The job queue is read back from the workspace's `jobs/` here, before the
/// server answers anything: a job found running is failed as interrupted —
/// or done, for a run whose announced id the store holds — and the queued
/// ones wait in their stored order. Called inside a `tokio`
/// runtime, as `serve` needs anyway, the workers start on them at once.
// The one assembly site: the nest, the bare-prefix route, the naming
// fallback, the assets' fallback and the envelope's layer, none of them an
// /api handler reading input (ADR-C37 § 2's exceptions). `clippy.toml`
// refuses the methods that add a route, a service, a fallback or a layer
// everywhere but here and `routes::Builder::into_router`, so no handler
// reaches the server around the guard.
#[allow(clippy::disallowed_methods)]
pub fn router(backends: Backends, config: ServerConfig, assets: Arc<dyn Assets>) -> Server {
    // Every /api route is listed once, in `routes::api`, and registered only
    // through `routes::Routes::route`, whose bound refuses a handler that
    // takes anything but `State` and the crate's own extractors (ADR-C37
    // § 2).
    let envelope = layers::envelope(&config.served, &config.build);
    let mut routes = routes::Builder::default();
    routes::api(&mut routes);
    let api = routes
        .into_router()
        .fallback(handlers::route_not_found)
        .with_state(handlers::AppState::new(backends, config));
    let server = Router::new()
        .nest(handlers::API_PREFIX, api)
        // axum 0.7's `nest` leaves the prefix with a trailing slash to the
        // outer router, where the assets' fallback would answer it.
        .route("/api/", any(handlers::prefix_not_found))
        .fallback_service(assets::endpoint(assets))
        // Last, so it wraps every route above and the fallback.
        .layer(envelope);
    Server { router: server }
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
