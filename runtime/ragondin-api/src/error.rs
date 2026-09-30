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
    /// An `impl:` name this binary registers no local implementation for.
    #[error("this build has no local {family} named `{implementation}`")]
    ImplNotInBuild {
        /// The family the node is in.
        family: String,
        /// The `impl:` name.
        implementation: String,
    },
    /// A `Remote` service that did not answer, at a probe or a submission.
    #[error("the service at {uri} did not answer: {reason}")]
    ServiceUnreachable {
        /// The address it was reached at.
        uri: String,
        /// The network error, or what the identity read found.
        reason: String,
    },
    /// A submission whose run id the store or the queue already holds.
    #[error("run {run_id} already exists")]
    RunExists {
        /// The run id announced for the submission.
        run_id: String,
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
    /// Passage text asked for, and no dataset on disk to resolve it from.
    #[error("the dataset {dataset} is not on disk")]
    DatasetAbsent {
        /// The benchmark the run was evaluated on.
        dataset: String,
    },
    /// Passage text asked for, and the dataset on disk is not the run's: its
    /// digest is not the one the run recorded.
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
        "dataset_absent",
        "dataset_differs",
        "benchmark_not_found",
        "benchmark_exists",
        "download_failed",
        "import_refused",
        "backend_failed",
        "host_refused",
        "origin_refused",
        "route_not_found",
        "method_not_allowed",
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
            | Self::ImportRefused { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::ServiceUnreachable { .. } | Self::DownloadFailed { .. } => {
                StatusCode::BAD_GATEWAY
            }
            Self::RunExists { .. } | Self::DatasetDiffers { .. } | Self::BenchmarkExists { .. } => {
                StatusCode::CONFLICT
            }
            Self::RunUnreadable { .. } | Self::BackendFailed { .. } => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::RunNotFound { .. }
            | Self::DatasetAbsent { .. }
            | Self::BenchmarkNotFound { .. }
            | Self::RouteNotFound { .. } => StatusCode::NOT_FOUND,
            Self::HostRefused { .. } => StatusCode::MISDIRECTED_REQUEST,
            Self::OriginRefused { .. } => StatusCode::FORBIDDEN,
            Self::MethodNotAllowed { .. } => StatusCode::METHOD_NOT_ALLOWED,
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
            Self::DatasetAbsent { .. } => 6,
            Self::DatasetDiffers { .. } => 7,
            Self::BenchmarkNotFound { .. } => 8,
            Self::BenchmarkExists { .. } => 9,
            Self::DownloadFailed { .. } => 10,
            Self::ImportRefused { .. } => 11,
            Self::BackendFailed { .. } => 12,
            Self::HostRefused { .. } => 13,
            Self::OriginRefused { .. } => 14,
            Self::RouteNotFound { .. } => 15,
            Self::MethodNotAllowed { .. } => 16,
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
            Self::DatasetAbsent { .. } => "The dataset is absent",
            Self::DatasetDiffers { .. } => "The dataset differs from the run's",
            Self::BenchmarkNotFound { .. } => "No such benchmark",
            Self::BenchmarkExists { .. } => "The benchmark is already on disk",
            Self::DownloadFailed { .. } => "The download did not verify",
            Self::ImportRefused { .. } => "The import was refused",
            Self::BackendFailed { .. } => "A backend failed",
            Self::HostRefused { .. } => "Host refused",
            Self::OriginRefused { .. } => "Origin refused",
            Self::RouteNotFound { .. } => "No such endpoint",
            Self::MethodNotAllowed { .. } => "Method not allowed",
        }
    }

    fn hint(&self) -> String {
        match self {
            Self::PipelineInvalid { .. } => {
                "Correct the node or edge named in `location`, then validate again.".to_owned()
            }
            Self::ImplNotInBuild { family, implementation } => format!(
                "Rebuild with the feature that carries `{implementation}`, or bind a Remote {family} under this name."
            ),
            Self::ServiceUnreachable { uri, .. } => {
                format!("Start the service at {uri}, or bind this name to an address that answers.")
            }
            Self::RunExists { run_id } => {
                format!("Open run {run_id}: it was produced by the same inputs.")
            }
            Self::RunUnreadable { .. } => {
                "Read this run with the build that stored it; this build does not repair it."
                    .to_owned()
            }
            Self::RunNotFound { .. } => {
                "Check the run id: a run is named by 64 lowercase hex digits.".to_owned()
            }
            Self::DatasetAbsent { dataset } => {
                format!("Download or import {dataset} to see passage text; ids are shown meanwhile.")
            }
            Self::DatasetDiffers { dataset, .. } => format!(
                "Restore the version of {dataset} this run was evaluated on to see passage text; ids are shown meanwhile."
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
            Self::ImportRefused { .. } => {
                "Correct what the detail names — the name, the path, or the dataset's files — and import again."
                    .to_owned()
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
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.status(), axum::Json(self.problem())).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
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
            },
            ApiError::ServiceUnreachable {
                uri: String::new(),
                reason: String::new(),
            },
            ApiError::RunExists {
                run_id: String::new(),
            },
            ApiError::RunUnreadable {
                run_id: String::new(),
                reason: String::new(),
            },
            ApiError::RunNotFound { id: String::new() },
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
            ApiError::ImportRefused {
                name: String::new(),
                reason: String::new(),
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
        ];
        assert_eq!(samples.len(), ApiError::CODES.len());
        for (position, error) in samples.iter().enumerate() {
            assert_eq!(error.index(), position, "{error:?}");
        }
    }
}
