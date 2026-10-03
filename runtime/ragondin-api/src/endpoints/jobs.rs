//! `POST /runs`, `GET /jobs`, `GET`/`PATCH`/`DELETE /jobs/{id}`, over the
//! queue (`jobs/`). The event stream, `GET /jobs/events`, is `jobs/stream.rs`;
//! a download's submission, `POST /benchmarks/{name}/download`, is with the
//! benchmarks.
//!
//! A submission announces its run id here, before it is queued: the pipeline
//! is read from the workspace and validated, the bindings in force are read
//! from the settings, and `Launcher::identity` — the composition root, which
//! constructs the components and reads the services' identities — computes
//! the id. So an existing run is refused and an unreachable service fails at
//! once, not when the worker reaches the job (ADR-C36 § 1).

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;

use crate::backends::{LauncherError, Submission};
use crate::endpoints::services::last_read_at;
use crate::error::ApiError;
use crate::extract::{ApiJson, ApiPath, ApiQuery, NoParameters};
use crate::handlers::AppState;
use crate::request::{ReorderRequest, RunRequest};
use crate::response::{JobListing, JobSummary, Location, RunAccepted};
use crate::validation;

/// `POST /runs`: `202` with the job and the run id it announced, or
/// `run_exists` linking the job or the run that holds that id.
pub(crate) async fn submit(
    State(state): State<AppState>,
    _: ApiQuery<NoParameters>,
    ApiJson(RunRequest {
        pipeline,
        benchmark,
        up_to,
    }): ApiJson<RunRequest>,
) -> Result<(StatusCode, Json<RunAccepted>), ApiError> {
    let file = state.backends.pipelines.read(&pipeline).await?;
    validation::lower(&file.document)?;
    let bindings = state.backends.settings.read().await?.services;
    let submission = Submission {
        pipeline_name: file.name,
        pipeline: file.document,
        benchmark,
        bindings,
        up_to,
    };
    let run_id = state
        .backends
        .launcher
        .identity(&submission)
        .await
        .map_err(|error| refusal(&state, error))?;
    let job_id = state.jobs.submit_run(submission, run_id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(RunAccepted {
            job_id,
            run_id: run_id.to_string(),
        }),
    ))
}

/// A launcher's refusal at submission, as the problem the UI shows inline.
fn refusal(state: &AppState, error: LauncherError) -> ApiError {
    match error {
        LauncherError::ImplNotInBuild {
            family,
            implementation,
            feature,
        } => ApiError::ImplNotInBuild {
            family,
            implementation,
            feature,
        },
        LauncherError::ServiceUnreachable { uri, reason } => ApiError::ServiceUnreachable {
            last_identity: last_read_at(&state.probes, &uri),
            uri,
            reason,
        },
        LauncherError::PipelineInvalid { detail, node } => ApiError::PipelineInvalid {
            detail,
            location: Location { node, edge: None },
        },
        other @ (LauncherError::Execution { .. } | LauncherError::Cancelled) => {
            ApiError::BackendFailed {
                detail: format!("the run's identity could not be computed: {other}"),
            }
        }
    }
}

/// `GET /jobs`.
pub(crate) async fn list(
    State(state): State<AppState>,
    _: ApiQuery<NoParameters>,
) -> Json<JobListing> {
    Json(state.jobs.listing().await)
}

/// `GET /jobs/{id}`.
pub(crate) async fn read(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    _: ApiQuery<NoParameters>,
) -> Result<Json<JobSummary>, ApiError> {
    Ok(Json(state.jobs.job(&id).await?))
}

/// `PATCH /jobs/{id}`: moves a queued job among its lane's queued jobs, and
/// answers the queue in its new order.
pub(crate) async fn reorder(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    _: ApiQuery<NoParameters>,
    ApiJson(ReorderRequest { position }): ApiJson<ReorderRequest>,
) -> Result<Json<JobListing>, ApiError> {
    Ok(Json(state.jobs.reorder(&id, position).await?))
}

/// `DELETE /jobs/{id}`: cancels a queued job at once, or sets the running
/// one's signal — it stays `running` until the launcher honours it between
/// two queries, and the `cancelled` event says when it has.
pub(crate) async fn cancel(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    _: ApiQuery<NoParameters>,
) -> Result<Json<JobSummary>, ApiError> {
    Ok(Json(state.jobs.cancel(&id).await?))
}
