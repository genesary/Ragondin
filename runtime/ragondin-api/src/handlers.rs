//! The handlers of the three read endpoints. Each reads its backends, converts
//! what it read into this crate's response types, and answers; every failure
//! is an [`ApiError`].

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::Json;
use ragondin_experiments::{RunId, RunStore, RunStoreError};

use crate::backends::Backends;
use crate::convert;
use crate::error::ApiError;
use crate::response::{RunDetail, RunListing, SettingsSummary, UnreadableRun, Workspace};
use crate::ServerConfig;

/// What every handler reads: the backends and the fixed configuration.
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) backends: Backends,
    pub(crate) config: Arc<ServerConfig>,
}

/// `GET /workspace`.
pub(crate) async fn workspace(State(state): State<AppState>) -> Result<Json<Workspace>, ApiError> {
    let settings = state.backends.settings.read().await?;
    Ok(Json(Workspace {
        path: state.config.workspace.display().to_string(),
        settings: SettingsSummary {
            datasets: settings.datasets.display().to_string(),
            services: settings.services,
        },
        build: state.config.build.clone(),
        capabilities: state.backends.launcher.capabilities(),
    }))
}

/// `GET /runs`: every id the store lists, loaded; a run that does not load is
/// listed as unreadable, with the store's reason, rather than dropped.
pub(crate) async fn runs(State(state): State<AppState>) -> Result<Json<RunListing>, ApiError> {
    let listing = blocking(state.backends.runs, |store| {
        let ids = store.ids().map_err(|error| ApiError::BackendFailed {
            detail: format!("the run store cannot be listed: {error}"),
        })?;
        let mut listing = RunListing {
            runs: Vec::with_capacity(ids.len()),
            unreadable: Vec::new(),
        };
        for id in ids {
            match store.load(&id) {
                Ok(run) => listing.runs.push(convert::summary(&run)),
                Err(error) => listing.unreadable.push(UnreadableRun {
                    id: id.to_string(),
                    reason: error.to_string(),
                }),
            }
        }
        Ok(listing)
    })
    .await?;
    Ok(Json(listing))
}

/// `GET /runs/{id}`. An id that is not a run id names no run, so it is
/// `run_not_found` as an absent one is; a run that is there and does not
/// read is `run_unreadable`.
pub(crate) async fn run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RunDetail>, ApiError> {
    let run_id: RunId = id
        .parse()
        .map_err(|_| ApiError::RunNotFound { id: id.clone() })?;
    let run = blocking(state.backends.runs, move |store| {
        store.load(&run_id).map_err(|error| match error {
            RunStoreError::NotFound { .. } => ApiError::RunNotFound { id },
            other => ApiError::RunUnreadable {
                run_id: run_id.to_string(),
                reason: other.to_string(),
            },
        })
    })
    .await?;
    Ok(Json(convert::detail(&run)?))
}

/// Where the API is nested. Inside the nest, axum hands a handler the path
/// with this prefix stripped, so the two fallbacks below put it back to name
/// the path as it was requested.
pub(crate) const API_PREFIX: &str = "/api";

/// Any path under `/api` that names no endpoint.
pub(crate) async fn route_not_found(uri: Uri) -> ApiError {
    ApiError::RouteNotFound {
        path: format!("{API_PREFIX}{}", uri.path()),
    }
}

/// An endpoint asked for with a method it does not serve.
pub(crate) async fn method_not_allowed(method: Method, uri: Uri) -> ApiError {
    ApiError::MethodNotAllowed {
        method: method.to_string(),
        path: format!("{API_PREFIX}{}", uri.path()),
    }
}

/// Runs a synchronous store call on a blocking thread: the file backend
/// reads the disk, and an async worker must not wait on it.
async fn blocking<T, F>(store: Arc<dyn RunStore>, call: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&dyn RunStore) -> Result<T, ApiError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || call(store.as_ref()))
        .await
        .map_err(|error| ApiError::BackendFailed {
            detail: format!("the run store call did not complete: {error}"),
        })?
}
