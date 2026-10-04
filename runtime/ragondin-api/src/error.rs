//! [`ApiError`]: every way a request can fail, typed (ADR-C13), each with a
//! stable code and rendered as `application/problem+json`.
//!
//! The codes are listed in [`ApiError::CODES`] and in `ARCHITECTURE.md`. A
//! code no handler raises yet still exists here, so an endpoint added later
//! adds a handler, not a code.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::response::{Location, Problem};

/// Why a request failed.
///
/// `Display` is the problem's `detail`; the code, the status, the title and
/// the hint are derived from the variant.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    /// A pipeline document the validation pass refused.
    #[error("the pipeline does not validate: {detail}")]
    PipelineInvalid {
        /// What the validation pass said, in the words `ragondin validate`
        /// uses.
        detail: String,
        /// The node and the edge it concerns.
        location: Location,
    },
    /// An `impl:` name this binary registers no local implementation for —
    /// or, with `feature`, a component this build cannot construct at all
    /// without that feature, such as any `Remote` one without `remote`.
    #[error("{}", impl_not_in_build(family, implementation, feature.as_deref()))]
    ImplNotInBuild {
        /// The family the node is in.
        family: String,
        /// The `impl:` name.
        implementation: String,
        /// The build feature that would carry it, when one is known.
        feature: Option<String>,
    },
    /// A `Remote` service that did not answer, at a probe or a submission.
    #[error("the service at {uri} did not answer: {reason}{}", last_read(last_identity.as_deref()))]
    ServiceUnreachable {
        /// The address it was reached at.
        uri: String,
        /// The network error, or what the identity read found.
        reason: String,
        /// The identity this server last read under the same binding, if it
        /// read one, with the address it read it at when that was another.
        last_identity: Option<String>,
    },
    /// A submission whose run id the store or the queue already holds.
    #[error("run {run_id} already exists")]
    RunExists {
        /// The run id announced for the submission.
        run_id: String,
        /// Where what holds it is read: `/api/v1/jobs/<id>` for a job
        /// queued or running under it, `/api/v1/runs/<id>` for a stored run.
        link: String,
    },
    /// A run the store holds and this build cannot read: a configuration
    /// under a schema version it does not read, a trace that does not parse,
    /// a torn record. Reported, never repaired.
    #[error("run {run_id} cannot be read: {reason}")]
    RunUnreadable {
        /// The run's id.
        run_id: String,
        /// What was read, and what this build expected.
        reason: String,
    },
    /// No run is stored under this id — or the text is not a run id at all,
    /// which names no run either.
    #[error("no run {id} in this store")]
    RunNotFound {
        /// The id as the request spelled it.
        id: String,
    },
    /// A query whose trace is not there: a run's traces, or a job's partial
    /// traces, hold none under this id.
    #[error("{owner} holds no trace of query {query}")]
    QueryNotFound {
        /// Whose traces were looked in: `run <id>`, or `job <id>`.
        owner: String,
        /// The path that lists the queries it holds.
        listing: String,
        /// The query id as the request spelled it.
        query: String,
    },
    /// A request parameter this endpoint does not take, or a value it cannot
    /// read: in the query string, the path, or a header the endpoint reads.
    #[error("{}", parameter_invalid(name.as_deref(), reason))]
    ParameterInvalid {
        /// The parameter, path parameter or header, as the request spelled
        /// it — or as the description declares it, for a header — when it
        /// is known; `None` when the reason does not say which.
        name: Option<String>,
        /// What is wrong with it.
        reason: String,
    },
    /// Passage text or scores asked for, and no dataset on disk to resolve
    /// them from.
    #[error("the dataset {dataset} is not on disk")]
    DatasetAbsent {
        /// The benchmark the run was evaluated on.
        dataset: String,
    },
    /// Passage text or scores asked for, and the dataset on disk is not the
    /// run's: a digest is not the one the run recorded.
    #[error(
        "the dataset {dataset} on disk digests to {found}, the run was evaluated on {expected}"
    )]
    DatasetDiffers {
        /// The benchmark the run was evaluated on.
        dataset: String,
        /// The digest the run recorded.
        expected: String,
        /// The digest the dataset on disk has.
        found: String,
    },
    /// A benchmark name the registry does not know — or, for a download, one
    /// the manifest does not hold.
    #[error("no benchmark {name}")]
    BenchmarkNotFound {
        /// The name as the request spelled it.
        name: String,
    },
    /// A download or an import whose destination is already a dataset.
    #[error("the benchmark {name} is already on disk")]
    BenchmarkExists {
        /// The benchmark's name.
        name: String,
    },
    /// A download that did not verify: a fetch that failed, or bytes whose
    /// digest is not the manifest's. Nothing was left on disk.
    #[error("downloading {name} failed: {reason}")]
    DownloadFailed {
        /// The benchmark.
        name: String,
        /// What failed, digests included when one differed.
        reason: String,
    },
    /// A download cancelled before it finished. Nothing was left on disk.
    #[error("downloading {name} was cancelled")]
    DownloadCancelled {
        /// The benchmark.
        name: String,
    },
    /// An import refused: a name that is not one directory name, a path that
    /// cannot be read, or a corpus its adapter does not load. Nothing was
    /// registered.
    #[error("importing {name} was refused: {reason}")]
    ImportRefused {
        /// The name the import asked for.
        name: String,
        /// Why, in the adapter's words when it refused the corpus.
        reason: String,
    },
    /// No pipeline of this name in the workspace — or a name that is not one
    /// file name, which names no pipeline either.
    #[error("no pipeline {name} in this workspace")]
    PipelineNotFound {
        /// The name as the request spelled it.
        name: String,
    },
    /// A write whose precondition does not hold: an `If-Match` naming
    /// another revision than the stored one, an `If-None-Match: *` over a
    /// pipeline that exists, or neither header. Nothing was written.
    #[error("{reason}")]
    PreconditionFailed {
        /// Which precondition failed, and the current etag when there is one.
        reason: String,
        /// The stored revision's etag, sent back in the `ETag` header; `None`
        /// when nothing is stored.
        current: Option<String>,
    },
    /// A service binding refused: a family, name or address the composition
    /// root would refuse on `--remote`, in its words — or one the settings
    /// backend cannot store so that it reads back as the same binding, in
    /// its own words.
    #[error("{detail}")]
    BindingRefused {
        /// The composition root's refusal.
        detail: String,
    },
    /// No service bound under this family and name.
    #[error("no service is bound as {family}/{name}")]
    ServiceNotFound {
        /// The family.
        family: String,
        /// The name.
        name: String,
    },
    /// A request this API cannot read: a body that is not the operation's
    /// JSON, a name that is not one file name, a layout of another version,
    /// a probe missing what its family needs.
    #[error("{detail}")]
    RequestInvalid {
        /// What is wrong with it.
        detail: String,
    },
    /// A backend failed for a reason that is none of the above — an I/O
    /// error listing the store, say.
    #[error("{detail}")]
    BackendFailed {
        /// What the backend reported.
        detail: String,
    },
    /// A request whose `Host` is not the address the server serves.
    #[error("this server does not answer for host {}", host.as_deref().unwrap_or("(none)"))]
    HostRefused {
        /// The `Host` the request named, if any.
        host: Option<String>,
    },
    /// A state-changing request whose `Origin` is not the server's own.
    #[error("this server does not accept a state-changing request from origin {}", origin.as_deref().unwrap_or("(none)"))]
    OriginRefused {
        /// The `Origin` the request named, if any.
        origin: Option<String>,
    },
    /// A path under `/api/` that names no endpoint.
    #[error("no endpoint at {path}")]
    RouteNotFound {
        /// The path as requested.
        path: String,
    },
    /// An endpoint asked for with a method it does not serve.
    #[error("{path} does not answer {method}")]
    MethodNotAllowed {
        /// The method as requested.
        method: String,
        /// The path as requested.
        path: String,
    },
    /// Runs that cannot be compared: evaluated on different benchmarks, or
    /// more of them than a comparison holds.
    #[error("{detail}")]
    RunsNotComparable {
        /// Why, naming both benchmark versions or the ceiling.
        detail: String,
    },
    /// A request body longer than the server reads.
    #[error("the request body is larger than this server reads: {detail}")]
    BodyTooLarge {
        /// What the body reader reported.
        detail: String,
    },
    /// No job under this id in the queue.
    #[error("no job {id} in this queue")]
    JobNotFound {
        /// The id as the request spelled it.
        id: String,
    },
    /// A reorder of a job that is no longer waiting.
    #[error("job {id} is {state}, and only a queued job is reordered")]
    JobNotQueued {
        /// The job's id.
        id: String,
        /// The state it is in.
        state: String,
    },
    /// A cancellation of a job that already ended.
    #[error("job {id} already ended: it is {state}")]
    JobFinished {
        /// The job's id.
        id: String,
        /// The terminal state it is in.
        state: String,
    },
    /// A read of the partial traces of a job still queued or running: a
    /// run's traces are written when it stops.
    #[error("job {id} is {state}: its traces are kept only once it fails or is cancelled")]
    JobNotEnded {
        /// The job's id.
        id: String,
        /// The state it is in.
        state: String,
    },
    /// A read of the partial traces of a job that ended without any: done,
    /// a download, or a run that kept none.
    #[error("job {id} kept no partial traces: {reason}")]
    NoPartialTraces {
        /// The job's id.
        id: String,
        /// Why it has none.
        reason: String,
    },
}

impl ApiError {
    /// Every stable code, one per variant, in declaration order.
    pub const CODES: &'static [&'static str] = &[
        "pipeline_invalid",
        "impl_not_in_build",
        "service_unreachable",
        "run_exists",
        "run_unreadable",
        "run_not_found",
        "query_not_found",
        "parameter_invalid",
        "dataset_absent",
        "dataset_differs",
        "benchmark_not_found",
        "benchmark_exists",
        "download_failed",
        "download_cancelled",
        "import_refused",
        "pipeline_not_found",
        "precondition_failed",
        "binding_refused",
        "service_not_found",
        "request_invalid",
        "backend_failed",
        "host_refused",
        "origin_refused",
        "route_not_found",
        "method_not_allowed",
        "runs_not_comparable",
        "body_too_large",
        "job_not_found",
        "job_not_queued",
        "job_finished",
        "job_not_ended",
        "no_partial_traces",
    ];

    /// The stable code a client matches on.
    pub fn code(&self) -> &'static str {
        Self::CODES[self.index()]
    }

    /// The HTTP status this error is answered with.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::PipelineInvalid { .. }
            | Self::ImplNotInBuild { .. }
            | Self::ImportRefused { .. }
            | Self::BindingRefused { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::PreconditionFailed { .. } => StatusCode::PRECONDITION_FAILED,
            Self::ParameterInvalid { .. } | Self::RequestInvalid { .. } => StatusCode::BAD_REQUEST,
            Self::ServiceUnreachable { .. } | Self::DownloadFailed { .. } => {
                StatusCode::BAD_GATEWAY
            }
            Self::RunExists { .. }
            | Self::DatasetDiffers { .. }
            | Self::BenchmarkExists { .. }
            | Self::DownloadCancelled { .. }
            | Self::RunsNotComparable { .. }
            | Self::JobNotQueued { .. }
            | Self::JobFinished { .. }
            | Self::JobNotEnded { .. } => StatusCode::CONFLICT,
            Self::RunUnreadable { .. } | Self::BackendFailed { .. } => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::RunNotFound { .. }
            | Self::QueryNotFound { .. }
            | Self::DatasetAbsent { .. }
            | Self::BenchmarkNotFound { .. }
            | Self::PipelineNotFound { .. }
            | Self::ServiceNotFound { .. }
            | Self::RouteNotFound { .. }
            | Self::JobNotFound { .. }
            | Self::NoPartialTraces { .. } => StatusCode::NOT_FOUND,
            Self::HostRefused { .. } => StatusCode::MISDIRECTED_REQUEST,
            Self::OriginRefused { .. } => StatusCode::FORBIDDEN,
            Self::MethodNotAllowed { .. } => StatusCode::METHOD_NOT_ALLOWED,
            Self::BodyTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        }
    }

    /// The problem body this error renders as.
    pub fn problem(&self) -> Problem {
        let code = self.code();
        Problem {
            problem_type: format!("urn:ragondin:problem:{code}"),
            title: self.title().to_owned(),
            status: self.status().as_u16(),
            detail: self.to_string(),
            code: code.to_owned(),
            hint: self.hint(),
            location: match self {
                Self::PipelineInvalid { location, .. } => Some(location.clone()),
                _ => None,
            },
            etag: match self {
                Self::PreconditionFailed { current, .. } => current.clone(),
                _ => None,
            },
            name: match self {
                Self::ParameterInvalid { name, .. } => name.clone(),
                _ => None,
            },
            link: match self {
                Self::RunExists { link, .. } => Some(link.clone()),
                _ => None,
            },
        }
    }

    fn index(&self) -> usize {
        match self {
            Self::PipelineInvalid { .. } => 0,
            Self::ImplNotInBuild { .. } => 1,
            Self::ServiceUnreachable { .. } => 2,
            Self::RunExists { .. } => 3,
            Self::RunUnreadable { .. } => 4,
            Self::RunNotFound { .. } => 5,
            Self::QueryNotFound { .. } => 6,
            Self::ParameterInvalid { .. } => 7,
            Self::DatasetAbsent { .. } => 8,
            Self::DatasetDiffers { .. } => 9,
            Self::BenchmarkNotFound { .. } => 10,
            Self::BenchmarkExists { .. } => 11,
            Self::DownloadFailed { .. } => 12,
            Self::DownloadCancelled { .. } => 13,
            Self::ImportRefused { .. } => 14,
            Self::PipelineNotFound { .. } => 15,
            Self::PreconditionFailed { .. } => 16,
            Self::BindingRefused { .. } => 17,
            Self::ServiceNotFound { .. } => 18,
            Self::RequestInvalid { .. } => 19,
            Self::BackendFailed { .. } => 20,
            Self::HostRefused { .. } => 21,
            Self::OriginRefused { .. } => 22,
            Self::RouteNotFound { .. } => 23,
            Self::MethodNotAllowed { .. } => 24,
            Self::RunsNotComparable { .. } => 25,
            Self::BodyTooLarge { .. } => 26,
            Self::JobNotFound { .. } => 27,
            Self::JobNotQueued { .. } => 28,
            Self::JobFinished { .. } => 29,
            Self::JobNotEnded { .. } => 30,
            Self::NoPartialTraces { .. } => 31,
        }
    }

    fn title(&self) -> &'static str {
        match self {
            Self::PipelineInvalid { .. } => "The pipeline is invalid",
            Self::ImplNotInBuild { .. } => "An implementation is not in this build",
            Self::ServiceUnreachable { .. } => "A service is unreachable",
            Self::RunExists { .. } => "The run already exists",
            Self::RunUnreadable { .. } => "The run cannot be read",
            Self::RunNotFound { .. } => "No such run",
            Self::QueryNotFound { .. } => "No trace of this query",
            Self::ParameterInvalid { .. } => "A parameter is invalid",
            Self::DatasetAbsent { .. } => "The dataset is absent",
            Self::DatasetDiffers { .. } => "The dataset differs from the run's",
            Self::BenchmarkNotFound { .. } => "No such benchmark",
            Self::BenchmarkExists { .. } => "The benchmark is already on disk",
            Self::DownloadFailed { .. } => "The download did not verify",
            Self::DownloadCancelled { .. } => "The download was cancelled",
            Self::ImportRefused { .. } => "The import was refused",
            Self::PipelineNotFound { .. } => "No such pipeline",
            Self::PreconditionFailed { .. } => "The pipeline changed since it was read",
            Self::BindingRefused { .. } => "The binding was refused",
            Self::ServiceNotFound { .. } => "No such service",
            Self::RequestInvalid { .. } => "The request is invalid",
            Self::BackendFailed { .. } => "A backend failed",
            Self::HostRefused { .. } => "Host refused",
            Self::OriginRefused { .. } => "Origin refused",
            Self::RouteNotFound { .. } => "No such endpoint",
            Self::MethodNotAllowed { .. } => "Method not allowed",
            Self::RunsNotComparable { .. } => "The runs cannot be compared",
            Self::BodyTooLarge { .. } => "The request body is too large",
            Self::JobNotFound { .. } => "No such job",
            Self::JobNotQueued { .. } => "The job is not queued",
            Self::JobFinished { .. } => "The job already ended",
            Self::JobNotEnded { .. } => "The job has not ended",
            Self::NoPartialTraces { .. } => "The job kept no partial traces",
        }
    }

    fn hint(&self) -> String {
        match self {
            Self::PipelineInvalid { .. } => {
                "Correct the node or edge named in `location`, then validate again.".to_owned()
            }
            Self::ImplNotInBuild {
                feature: Some(feature),
                ..
            } => format!("Rebuild with the `{feature}` feature."),
            Self::ImplNotInBuild {
                family,
                implementation,
                feature: None,
            } => format!(
                "Rebuild with the feature that carries `{implementation}`, or bind a Remote {family} under this name."
            ),
            Self::ServiceUnreachable { uri, .. } => {
                format!("Start the service at {uri}, or bind this name to an address that answers.")
            }
            Self::RunExists { run_id, link } => format!(
                "Open {link}: run {run_id} has the same inputs, and is stored or already in the queue."
            ),
            Self::RunUnreadable { .. } => {
                "Read this run with the build that stored it; this build does not repair it."
                    .to_owned()
            }
            Self::RunNotFound { .. } => {
                "Check the run id: a run is named by 64 lowercase hex digits.".to_owned()
            }
            Self::QueryNotFound { listing, .. } => format!("Pick a query from GET {listing}."),
            Self::ParameterInvalid { .. } => {
                "Check the parameter against the API description, api/v1.json.".to_owned()
            }
            Self::DatasetAbsent { dataset } => {
                format!("Download or import {dataset} to read passage text and scores against it; ids are shown meanwhile.")
            }
            Self::DatasetDiffers { dataset, .. } => format!(
                "Restore the version of {dataset} this run was evaluated on to read passage text and scores against it; ids are shown meanwhile."
            ),
            Self::BenchmarkNotFound { .. } => {
                "Check the name against the benchmark list: `<format>/<name>`.".to_owned()
            }
            Self::BenchmarkExists { name } => format!(
                "Use {name} as it is, or remove its directory from the datasets directory first."
            ),
            Self::DownloadFailed { .. } => {
                "Retry the download; if the digest differs again, the source changed and this build's manifest no longer matches it."
                    .to_owned()
            }
            Self::DownloadCancelled { name } => {
                format!("Download {name} again to obtain it; nothing was kept.")
            }
            Self::ImportRefused { .. } => {
                "Correct what the detail names — the name, the path, or the dataset's files — and import again."
                    .to_owned()
            }
            Self::PipelineNotFound { .. } => {
                "Check the name against the pipeline list.".to_owned()
            }
            Self::PreconditionFailed { .. } => {
                "Read the pipeline again, reapply the change to what it holds now, and write it with the etag that read returned."
                    .to_owned()
            }
            Self::BindingRefused { .. } => {
                "Correct the family, the name or the address the detail names.".to_owned()
            }
            Self::ServiceNotFound { .. } => {
                "Bind the name first, with PUT /services/{family}/{name}.".to_owned()
            }
            Self::RequestInvalid { .. } => {
                "Correct the request against the API description, api/v1.json.".to_owned()
            }
            Self::BackendFailed { .. } => {
                "Check the workspace on disk: the detail names what failed.".to_owned()
            }
            Self::HostRefused { .. } => {
                "Open the UI at the address `ragondin ui` printed.".to_owned()
            }
            Self::OriginRefused { .. } => {
                "Send state-changing requests from the UI this server serves.".to_owned()
            }
            Self::RouteNotFound { .. } => {
                "Check the path against the API description, api/v1.json.".to_owned()
            }
            Self::MethodNotAllowed { .. } => {
                "Use one of the methods the `Allow` header lists.".to_owned()
            }
            Self::RunsNotComparable { .. } => {
                "Compare runs of one benchmark, a baseline and at most four others.".to_owned()
            }
            Self::BodyTooLarge { .. } => {
                "Send a smaller body: no request this API reads needs one this large.".to_owned()
            }
            Self::JobNotFound { .. } => "Check the id against GET /jobs.".to_owned(),
            Self::JobNotQueued { .. } => {
                "Reorder only queued jobs; a running or ended job keeps its place.".to_owned()
            }
            Self::JobFinished { .. } => {
                "Nothing to cancel: submit it again to run it again.".to_owned()
            }
            Self::JobNotEnded { .. } => {
                "Wait for the job to end: a run that fails or is cancelled keeps the traces of the queries it completed.".to_owned()
            }
            Self::NoPartialTraces { .. } => {
                "Nothing to replay from the job: a done run is replayed from the store, and a crash leaves no traces.".to_owned()
            }
        }
    }
}

/// `ImplNotInBuild`'s detail: a local name this build lacks, or a component
/// it cannot construct without a feature.
pub(crate) fn impl_not_in_build(
    family: &str,
    implementation: &str,
    feature: Option<&str>,
) -> String {
    match feature {
        Some(feature) => format!(
            "this build cannot construct the {family} `{implementation}` without the `{feature}` feature"
        ),
        None => format!("this build has no local {family} named `{implementation}`"),
    }
}

/// `ParameterInvalid`'s detail: the parameter named when it is known, and
/// never a guessed one.
fn parameter_invalid(name: Option<&str>, reason: &str) -> String {
    match name {
        Some(name) => format!("the parameter `{name}` is invalid: {reason}"),
        None => format!("a parameter is invalid: {reason}"),
    }
}

/// `ServiceUnreachable`'s tail: the identity last read, when there is one.
fn last_read(identity: Option<&str>) -> String {
    identity
        .map(|identity| format!("; the identity last read under this name was {identity}"))
        .unwrap_or_default()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let etag = match &self {
            Self::PreconditionFailed {
                current: Some(current),
                ..
            } => HeaderValue::from_str(&format!("\"{current}\"")).ok(),
            _ => None,
        };
        let mut response = (self.status(), axum::Json(self.problem())).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if let Some(etag) = etag {
            response.headers_mut().insert(header::ETAG, etag);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CODES` and `index` are two lists that must agree: each variant's code
    /// is found at its own position, and no position is shared.
    #[test]
    fn each_variant_indexes_its_own_code() {
        let samples = [
            ApiError::PipelineInvalid {
                detail: String::new(),
                location: Location {
                    node: None,
                    edge: None,
                },
            },
            ApiError::ImplNotInBuild {
                family: String::new(),
                implementation: String::new(),
                feature: None,
            },
            ApiError::ServiceUnreachable {
                uri: String::new(),
                reason: String::new(),
                last_identity: None,
            },
            ApiError::RunExists {
                run_id: String::new(),
                link: String::new(),
            },
            ApiError::RunUnreadable {
                run_id: String::new(),
                reason: String::new(),
            },
            ApiError::RunNotFound { id: String::new() },
            ApiError::QueryNotFound {
                owner: String::new(),
                listing: String::new(),
                query: String::new(),
            },
            ApiError::ParameterInvalid {
                name: None,
                reason: String::new(),
            },
            ApiError::DatasetAbsent {
                dataset: String::new(),
            },
            ApiError::DatasetDiffers {
                dataset: String::new(),
                expected: String::new(),
                found: String::new(),
            },
            ApiError::BenchmarkNotFound {
                name: String::new(),
            },
            ApiError::BenchmarkExists {
                name: String::new(),
            },
            ApiError::DownloadFailed {
                name: String::new(),
                reason: String::new(),
            },
            ApiError::DownloadCancelled {
                name: String::new(),
            },
            ApiError::ImportRefused {
                name: String::new(),
                reason: String::new(),
            },
            ApiError::PipelineNotFound {
                name: String::new(),
            },
            ApiError::PreconditionFailed {
                reason: String::new(),
                current: None,
            },
            ApiError::BindingRefused {
                detail: String::new(),
            },
            ApiError::ServiceNotFound {
                family: String::new(),
                name: String::new(),
            },
            ApiError::RequestInvalid {
                detail: String::new(),
            },
            ApiError::BackendFailed {
                detail: String::new(),
            },
            ApiError::HostRefused { host: None },
            ApiError::OriginRefused { origin: None },
            ApiError::RouteNotFound {
                path: String::new(),
            },
            ApiError::MethodNotAllowed {
                method: String::new(),
                path: String::new(),
            },
            ApiError::RunsNotComparable {
                detail: String::new(),
            },
            ApiError::BodyTooLarge {
                detail: String::new(),
            },
            ApiError::JobNotFound { id: String::new() },
            ApiError::JobNotQueued {
                id: String::new(),
                state: String::new(),
            },
            ApiError::JobFinished {
                id: String::new(),
                state: String::new(),
            },
            ApiError::JobNotEnded {
                id: String::new(),
                state: String::new(),
            },
            ApiError::NoPartialTraces {
                id: String::new(),
                reason: String::new(),
            },
        ];
        assert_eq!(samples.len(), ApiError::CODES.len());
        for (position, error) in samples.iter().enumerate() {
            assert_eq!(error.index(), position, "{error:?}");
        }
    }
}
