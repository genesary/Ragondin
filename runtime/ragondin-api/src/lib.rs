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
//! and the assets `Router`, all passed in by the binary, applies the server's
//! layers last, and returns a [`Server`]: something `axum::serve` listens
//! with and no route can be added to, so every route it answers is one the
//! layers wrap. Nothing here is a static or a global, and nothing binds a
//! port — the listener is the binary's.
//!
//! The modules:
//!
//! - [`backends`] — the traits and the values they exchange.
//! - [`response`] — every type the API serializes; this crate's own, never a
//!   core type serialized directly.
//! - [`error`] — [`ApiError`] and its `application/problem+json` rendering.
//! - [`description`] — the API description, kept as a golden file.
//! - [`fs`] — the home of the file backends, empty today.
//!
//! Every path is under `/api/v1`: `GET /workspace`, `GET /runs` and
//! `GET /runs/{id}`.

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

pub mod backends;
pub mod description;
pub mod error;
pub mod fs;
pub mod response;

mod convert;
mod handlers;
mod layers;

pub use backends::{
    Backends, BenchmarkEntry, BenchmarkStatus, Job, JobState, Launcher, PipelineEntry,
    PipelineFile, PipelineSource, Registry, Revision, ServiceIdentity, Settings, Submission,
    WorkspaceSettings,
};
pub use error::ApiError;
pub use layers::BUILD_HEADER;
pub use response::{
    Capabilities, EdgeLocation, FamilyCapabilities, Location, Problem, ServiceBinding,
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
    pub build: String,
    /// The workspace directory, as `GET /workspace` reports it.
    pub workspace: PathBuf,
}

/// The whole server: the API under `/api`, `assets` beside it, and the
/// server's layers around both.
///
/// `assets` is whatever else the server answers — the UI's pages, and the
/// fallback that serves them on every client-side route. It is taken here
/// because `Router::layer` wraps only the routes that exist when it is
/// called: the envelope is applied last, here, over everything the server
/// answers, and what comes back is a [`Server`], which has no method that
/// adds a route. Pass `Router::new()` for no assets.
///
/// `/api`, `/api/` and every path below them are the API's: an unknown one
/// is a `route_not_found` problem, never the assets' fallback. A path that
/// merely starts with the same letters, such as `/apix`, is not under `/api`
/// and is the assets' to answer.
pub fn router(backends: Backends, config: ServerConfig, assets: Router) -> Server {
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
        .merge(assets);
    Server {
        router: layers::wrap(server, &served, &build),
    }
}

/// The server [`router`] builds, enveloped: what `axum::serve` listens with,
/// and nothing more.
///
/// Opaque on purpose. A `Router` can be extended, and a route added to it
/// after the layers would answer outside them; a `Server` has no method that
/// adds a route, merges a router or sets a fallback, and does not convert
/// back into a `Router`. What it offers is what serving needs:
/// [`into_make_service`](Self::into_make_service) for `axum::serve`, and
/// `tower::Service` over one request, which is what a connection calls —
/// the network envelope, where ADR-C10 puts Tower (INV-11 is about
/// components, and this is none).
///
/// ```
/// # fn serve(server: ragondin_api::Server, listener: tokio::net::TcpListener) {
/// let _serving = axum::serve(listener, server.into_make_service());
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
/// envelope, written by hand in the binary — a diff a reviewer sees, not a
/// method call that looks like extending this one.
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
