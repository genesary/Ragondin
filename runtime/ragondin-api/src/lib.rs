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
//! and the assets `Router`, all passed in by the binary, and applies the
//! server's layers last so that nothing it answers is outside them. Nothing
//! here is a static or a global, and nothing binds a port — the listener is
//! the binary's.
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

use std::path::PathBuf;
use std::sync::Arc;

use axum::routing::get;
use axum::Router;

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
/// fallback that serves them on every client-side route. It is taken here,
/// rather than merged by the caller into what this returns, because
/// `Router::layer` wraps only the routes that exist when it is called: a
/// route or a fallback added afterwards would answer a foreign `Host`
/// without the content security policy or the build identity. The envelope
/// is applied last, here, so nothing the server answers is outside it. Pass
/// `Router::new()` for none.
///
/// Every path under `/api` is the API's: an unknown one is a
/// `route_not_found` problem, never the assets' fallback.
pub fn router(backends: Backends, config: ServerConfig, assets: Router) -> Router {
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
    let server = Router::new().nest(handlers::API_PREFIX, api).merge(assets);
    layers::wrap(server, &served, &build)
}
