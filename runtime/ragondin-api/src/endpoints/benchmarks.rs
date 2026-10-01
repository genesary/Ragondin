//! `GET /benchmarks` and `POST /benchmarks/import`, over `Registry`.
//!
//! A download is not here: it is a job on the queue's IO lane, and its route
//! arrives with the queue.

use std::path::PathBuf;

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;

use super::json_body;
use crate::error::ApiError;
use crate::handlers::AppState;
use crate::request::ImportRequest;
use crate::response::{BenchmarkEntry, BenchmarkListing};

/// `GET /benchmarks`. The registry verifies every dataset on disk on each
/// call; nothing is cached here.
pub(crate) async fn list(
    State(state): State<AppState>,
) -> Result<Json<BenchmarkListing>, ApiError> {
    Ok(Json(BenchmarkListing {
        benchmarks: state.backends.registry.benchmarks().await?,
    }))
}

/// `POST /benchmarks/import`: the corpus at `path`, imported as `name`.
pub(crate) async fn import(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<BenchmarkEntry>, ApiError> {
    let ImportRequest { name, path } = json_body(&body)?;
    Ok(Json(
        state
            .backends
            .registry
            .import(&name, &PathBuf::from(path))
            .await?,
    ))
}
