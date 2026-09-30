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
//! [`router`] builds the whole thing from [`Backends`] and a
//! [`ServerConfig`], both passed in by the binary: nothing here is a static or
//! a global. Nothing here binds a port either — the listener is the binary's.
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

/// The router: the three read endpoints under `/api/v1`, wrapped in the
/// server's layers.
pub fn router(backends: Backends, config: ServerConfig) -> Router {
    let (served, build) = (config.served.clone(), config.build.clone());
    let routes = Router::new()
        .route("/api/v1/workspace", get(handlers::workspace))
        .route("/api/v1/runs", get(handlers::runs))
        // axum 0.7 spells a path parameter `:id`; the description's `{id}`.
        .route("/api/v1/runs/:id", get(handlers::run))
        .with_state(handlers::AppState {
            backends,
            config: Arc::new(config),
        });
    layers::wrap(routes, &served, &build)
}
