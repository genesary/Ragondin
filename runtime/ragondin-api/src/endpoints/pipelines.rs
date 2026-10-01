//! `GET /pipelines`, `GET`/`PUT /pipelines/{name}`,
//! `GET`/`PUT /pipelines/{name}/layout` and `POST /pipelines/validate`.
//!
//! The etag is the document's [`Revision`], in hex: bare in a JSON body,
//! quoted in the `ETag` header, as HTTP spells an entity tag. A write names
//! the revision it read with `If-Match` — or `If-Match: *`, any stored
//! revision — or creates with `If-None-Match: *`; one that names neither is
//! refused, since nothing would then stop it overwriting a change it never
//! saw. A write is checked by the composition root
//! (`Launcher::check_document`) before it is stored; `validate` is not.

use std::time::UNIX_EPOCH;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::json_body;
use crate::backends::{PipelineFile, Precondition, Revision};
use crate::error::ApiError;
use crate::handlers::AppState;
use crate::request::PipelineDocument;
use crate::response::{
    Layout, PipelineDetail, PipelineError, PipelineLayout, PipelineListing, PipelineSummary,
    PipelineValidated, PipelineWritten,
};
use crate::validation;

/// `GET /pipelines`.
pub(crate) async fn list(State(state): State<AppState>) -> Result<Json<PipelineListing>, ApiError> {
    let files = state.backends.pipelines.list().await?;
    Ok(Json(PipelineListing {
        pipelines: files
            .into_iter()
            .map(|file| {
                let (hash, error) = verdict(&file.document);
                PipelineSummary {
                    modified_ms: file
                        .modified
                        .duration_since(UNIX_EPOCH)
                        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
                        .unwrap_or_default(),
                    name: file.name,
                    etag: file.revision.as_str().to_owned(),
                    hash,
                    error,
                }
            })
            .collect(),
    }))
}

/// `GET /pipelines/{name}`.
pub(crate) async fn read(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    let file = state.backends.pipelines.read(&name).await?;
    let (hash, error) = verdict(&file.document);
    let etag = file.revision.clone();
    Ok(with_etag(
        Json(PipelineDetail {
            name: file.name,
            document: file.document,
            etag: etag.as_str().to_owned(),
            hash,
            error,
        }),
        &etag,
    ))
}

/// `PUT /pipelines/{name}`: `If-Match` or `If-None-Match: *`, and the
/// document.
pub(crate) async fn write(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let precondition = precondition(&headers)?;
    let request: PipelineDocument = json_body(&body)?;
    // The composition root's key refusals, with the workspace's bindings
    // deciding which names are bound: what `bench` would refuse is not stored.
    let pipeline = validation::lower(&request.document)?;
    let settings = state.backends.settings.read().await?;
    state
        .backends
        .launcher
        .check_document(&pipeline, &settings.services)?;
    let PipelineFile {
        name,
        document,
        revision,
        ..
    } = state
        .backends
        .pipelines
        .write(&name, &request.document, &precondition)
        .await?;
    let hash = validation::check(&document)?;
    Ok(with_etag(
        Json(PipelineWritten {
            name,
            etag: revision.as_str().to_owned(),
            hash,
        }),
        &revision,
    ))
}

/// `POST /pipelines/validate`: the hash, or `pipeline_invalid`.
pub(crate) async fn validate(body: Bytes) -> Result<Json<PipelineValidated>, ApiError> {
    let request: PipelineDocument = json_body(&body)?;
    Ok(Json(PipelineValidated {
        hash: validation::check(&request.document)?,
    }))
}

/// `GET /pipelines/{name}/layout`.
pub(crate) async fn read_layout(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<PipelineLayout>, ApiError> {
    Ok(Json(PipelineLayout {
        layout: state.backends.pipelines.read_layout(&name).await?,
    }))
}

/// `PUT /pipelines/{name}/layout`: replaces the layout, and answers it.
pub(crate) async fn write_layout(
    State(state): State<AppState>,
    Path(name): Path<String>,
    body: Bytes,
) -> Result<Json<PipelineLayout>, ApiError> {
    let layout: Layout = json_body(&body)?;
    state
        .backends
        .pipelines
        .write_layout(&name, &layout)
        .await?;
    Ok(Json(PipelineLayout {
        layout: Some(layout),
    }))
}

/// The hash a document validates to, or why it does not.
fn verdict(document: &str) -> (Option<String>, Option<PipelineError>) {
    match validation::check(document) {
        Ok(hash) => (Some(hash), None),
        Err(ApiError::PipelineInvalid { detail, location }) => {
            (None, Some(PipelineError { detail, location }))
        }
        // `check` answers nothing else; were it to, the document is still
        // reported as not validating rather than dropped from the listing.
        Err(other) => (
            None,
            Some(PipelineError {
                detail: other.to_string(),
                location: crate::response::Location {
                    node: None,
                    edge: None,
                },
            }),
        ),
    }
}

/// What a write's headers expect of the stored document.
fn precondition(headers: &HeaderMap) -> Result<Precondition, ApiError> {
    let text = |name: header::HeaderName| -> Result<Option<String>, ApiError> {
        headers
            .get(&name)
            .map(|value| {
                value
                    .to_str()
                    .map(|value| value.trim().to_owned())
                    .map_err(|_| ApiError::RequestInvalid {
                        detail: format!("the `{name}` header is not text"),
                    })
            })
            .transpose()
    };
    match (text(header::IF_MATCH)?, text(header::IF_NONE_MATCH)?) {
        (Some(_), Some(_)) => Err(ApiError::RequestInvalid {
            detail: "a write states `If-Match` or `If-None-Match: *`, not both".to_owned(),
        }),
        (Some(star), None) if star == "*" => Ok(Precondition::Exists),
        (Some(etag), None) => Ok(Precondition::Matches(Revision::new(
            etag.strip_prefix("W/")
                .unwrap_or(&etag)
                .trim_matches('"')
                .to_owned(),
        ))),
        (None, Some(star)) if star == "*" => Ok(Precondition::Absent),
        (None, Some(other)) => Err(ApiError::RequestInvalid {
            detail: format!(
                "`If-None-Match: {other}`: a write creates with `If-None-Match: *` only"
            ),
        }),
        (None, None) => Ok(Precondition::Unstated),
    }
}

/// `body`, with the `ETag` header naming `revision`.
fn with_etag(body: impl IntoResponse, revision: &Revision) -> Response {
    let mut response = body.into_response();
    if let Ok(value) = HeaderValue::from_str(&format!("\"{}\"", revision.as_str())) {
        response.headers_mut().insert(header::ETAG, value);
    }
    response
}
