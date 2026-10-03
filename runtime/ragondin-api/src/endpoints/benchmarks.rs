//! `GET /benchmarks` and `POST /benchmarks/import`, over `Registry`, and
//! `POST /benchmarks/{name}/download`, which queues a download job on the
//! queue's download lane: the registry fetches and verifies it there, its
//! progress in bytes on the job stream.

use std::path::PathBuf;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use crate::error::ApiError;
use crate::extract::{ApiJson, ApiPath, ApiQuery, NoParameters};
use crate::handlers::AppState;
use crate::request::ImportRequest;
use crate::response::{BenchmarkEntry, BenchmarkListing, DownloadAccepted};

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

/// `POST /benchmarks/{name}/download`: `202` with the download job. Whether
/// the manifest names the benchmark, and whether it is already on disk, is
/// the registry's to say when the job runs: its refusal is the job's
/// `failed` state, in its words.
pub(crate) async fn download(
    State(state): State<AppState>,
    ApiPath(name): ApiPath<String>,
    _: ApiQuery<NoParameters>,
) -> Result<(StatusCode, Json<DownloadAccepted>), ApiError> {
    let job_id = state.jobs.submit_download(name).await?;
    Ok((StatusCode::ACCEPTED, Json(DownloadAccepted { job_id })))
}
