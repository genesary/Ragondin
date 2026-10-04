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
//!
//! With `up_to`, the document is cut at that node first (`prefix.rs`): the
//! job's pipeline is the cut, announced under the cut's own identity, and the
//! submission carries the parent's name and canonical hash for the launch
//! record's `prefix_of` (ADR-C39 § 1, § 3). A cut that cannot be a prefix,
//! or that the chosen benchmark could not score, is refused before the
//! launcher is asked anything.

use std::collections::BTreeMap;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use ragondin_experiments::{lower_configuration, ConfigDocument, Trace, TraceDocument};
use ragondin_types::QueryId;

use crate::backends::{LauncherError, Submission};
use crate::endpoints::services::last_read_at;
use crate::error::ApiError;
use crate::extract::{ApiJson, ApiPath, ApiQuery, NoParameters};
use crate::handlers::AppState;
use crate::jobs::{summary, Partial};
use crate::request::{ReorderRequest, RunRequest};
use crate::response::{
    JobListing, JobSummary, Location, PartialQueries, PartialTrace, QueryScores, RunAccepted,
};
use crate::{convert, prefix, validation};

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
    // A prefix run is an ordinary run of the cut document: the job snapshots
    // the cut, and the parent is its name and hash, provenance beside it.
    let (document, parent_pipeline_hash) = match &up_to {
        None => {
            validation::lower(&file.document)?;
            (file.document, None)
        }
        Some(node) => {
            let cut = prefix::cut(&file.name, &file.document, node)?;
            let entry = state.backends.registry.verify(&benchmark).await?;
            prefix::scorable(node, cut.output, &entry)?;
            (cut.document, Some(cut.parent_hash))
        }
    };
    let bindings = state.backends.settings.read().await?.services;
    let submission = Submission {
        pipeline_name: file.name,
        pipeline: document,
        benchmark,
        bindings,
        up_to,
        parent_pipeline_hash,
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

/// `GET /jobs/{id}/queries`: the queries a failed or cancelled run job
/// executed, from the traces it kept under `jobs/<id>/partial/`, with the
/// graph its snapshotted document lowers to. Nothing is scored and no text is
/// read: the traces record no dataset digest to check one against (ADR-C36
/// § 4).
pub(crate) async fn queries(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    _: ApiQuery<NoParameters>,
) -> Result<Json<PartialQueries>, ApiError> {
    let Partial {
        job,
        pipeline,
        traces,
    } = state.jobs.partial(&id).await?;
    let lowered = lower_configuration(&ConfigDocument::new(pipeline)).map_err(|reason| {
        ApiError::BackendFailed {
            detail: format!("job {id}'s pipeline document no longer lowers: {reason}"),
        }
    })?;
    let mut failed_query = None;
    let mut queries = Vec::with_capacity(traces.len());
    for (query, document) in &traces {
        let trace = read_partial_trace(&id, query, document)?;
        if failed_query.is_none() && trace.nodes.iter().any(|node| node.error.is_some()) {
            failed_query = Some(query.as_str().to_owned());
        }
        queries.push(QueryScores {
            id: query.as_str().to_owned(),
            text: None,
            scores: BTreeMap::new(),
            duration_nanos: trace.latency_nanos(),
        });
    }
    Ok(Json(PartialQueries {
        job: summary(&job),
        graph: convert::graph(&lowered),
        queries,
        failed_query,
    }))
}

/// `GET /jobs/{id}/trace/{query}`: one query's trace from a failed or
/// cancelled run job's partial traces, in the node shape a stored run's
/// trace is served in, with nothing read against a dataset.
pub(crate) async fn trace(
    State(state): State<AppState>,
    ApiPath((id, query)): ApiPath<(String, String)>,
    _: ApiQuery<NoParameters>,
) -> Result<Json<PartialTrace>, ApiError> {
    let Partial { job, traces, .. } = state.jobs.partial(&id).await?;
    let query_id = QueryId::new(&query);
    let document = traces
        .get(&query_id)
        .ok_or_else(|| ApiError::QueryNotFound {
            owner: format!("job {}", job.id),
            listing: format!("/jobs/{}/queries", job.id),
            query: query.clone(),
        })?;
    let trace = read_partial_trace(&job.id, &query_id, document)?;
    let nodes = convert::trace_view(&trace, None, |_| None, |_| None, |_| None);
    Ok(Json(PartialTrace {
        job: job.id,
        query,
        nodes,
    }))
}

/// One kept trace, typed; one that does not read is reported, never
/// repaired, naming the query.
fn read_partial_trace(
    id: &str,
    query: &QueryId,
    document: &TraceDocument,
) -> Result<Trace, ApiError> {
    Trace::try_from(document).map_err(|error| ApiError::BackendFailed {
        detail: format!(
            "job {id}'s partial trace of query {} cannot be read: {error}",
            query.as_str()
        ),
    })
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
