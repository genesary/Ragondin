//! `GET /benchmarks` and `POST /benchmarks/import`, over `Registry`.
//!
//! A download is not here: it is a job on the queue's IO lane, and its route
//! arrives with the queue.

use std::path::PathBuf;

use axum::extract::State;
use axum::Json;

use crate::error::ApiError;
use crate::extract::{ApiJson, ApiQuery, NoParameters};
use crate::handlers::AppState;
use crate::request::ImportRequest;
use crate::response::{BenchmarkEntry, BenchmarkListing};

/// `GET /benchmarks`. The registry verifies every dataset on disk on each
/// call; nothing is cached here.
pub(crate) async fn list(
    State(state): State<AppState>,
    _: ApiQuery<NoParameters>,
) -> Result<Json<BenchmarkListing>, ApiError> {
    Ok(Json(BenchmarkListing {
        benchmarks: state.backends.registry.benchmarks().await?,
    }))
}

/// `POST /benchmarks/import`: the corpus at `path`, imported as `name`.
pub(crate) async fn import(
    State(state): State<AppState>,
    _: ApiQuery<NoParameters>,
    ApiJson(ImportRequest { name, path }): ApiJson<ImportRequest>,
) -> Result<Json<BenchmarkEntry>, ApiError> {
    Ok(Json(
        state
            .backends
            .registry
            .import(&name, &PathBuf::from(path))
            .await?,
    ))
}
