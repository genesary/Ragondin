//! `GET /pipelines`, `GET`/`PUT /pipelines/{name}`,
//! `GET`/`PUT /pipelines/{name}/layout`, `POST /pipelines/validate`, and
//! `GET /runs/{id}/layout`, the layout a fork from a run copies.
//!
//! The etag is the document's [`Revision`], in hex: bare in a JSON body,
//! quoted in the `ETag` header, as HTTP spells an entity tag. A write names
//! the revision it read with `If-Match` — or `If-Match: *`, any stored
//! revision — or creates with `If-None-Match: *`; one that names neither is
//! refused, since nothing would then stop it overwriting a change it never
//! saw. A write is checked by the composition root
//! (`Launcher::check_document`) before it is stored; `validate` is not.

use std::time::SystemTime;

use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use ragondin_config::read_document;
use ragondin_experiments::UnixMillis;
use ragondin_pipeline::LogicalPipeline;

use crate::backends::{PipelineFile, Precondition, Revision};
use crate::convert;
use crate::derived;
use crate::error::ApiError;
use crate::extract::{ApiHeaders, ApiJson, ApiPath, ApiQuery, NoParameters};
use crate::handlers::{load_run, AppState};
use crate::request::{PipelineDocument, PreconditionHeaders, ValidationRequest};
use crate::response::{
    Layout, PipelineDetail, PipelineError, PipelineLayout, PipelineListing, PipelineSummary,
    PipelineValidated, PipelineWritten,
};
use crate::validation;

/// `GET /pipelines`.
pub(crate) async fn list(
    State(state): State<AppState>,
    _: ApiQuery<NoParameters>,
) -> Result<Json<PipelineListing>, ApiError> {
    let files = state.backends.pipelines.list().await?;
    Ok(Json(PipelineListing {
        pipelines: files
            .into_iter()
            .map(|file| {
                let lowered = validation::lower(&file.document);
                // What it ends in, read off the pipeline the hash is of: the
                // harness scores an answer only when its terminal node
                // produces one.
                let ends_in_answer = lowered.as_ref().ok().map(derived::ends_in_answer);
                let (hash, error) = verdict(lowered);
                PipelineSummary {
                    modified_ms: modified_ms(file.modified),
                    name: file.name,
                    etag: file.revision.as_str().to_owned(),
                    hash,
                    error,
                    ends_in_answer,
                }
            })
            .collect(),
    }))
}

/// A file's modification time in milliseconds since the epoch, by the one
/// rule a time follows everywhere in the API (`UnixMillis`): before the
/// epoch it is unknown, `None` — never `0`, which is a real time and would
/// sort as the oldest there is.
fn modified_ms(modified: SystemTime) -> Option<u64> {
    UnixMillis::from_system_time(modified).map(UnixMillis::get)
}

/// `GET /pipelines/{name}`.
pub(crate) async fn read(
    State(state): State<AppState>,
    ApiPath(name): ApiPath<String>,
    _: ApiQuery<NoParameters>,
) -> Result<Response, ApiError> {
    let file = state.backends.pipelines.read(&name).await?;
    let lowered = validation::lower(&file.document);
    // What each cut ends in — `POST /runs`' `up_to` — so a client offers a
    // prefix the benchmarks it can be scored on without reading node kinds.
    let ends_in_answer_up_to = lowered.as_ref().ok().map(|pipeline| {
        pipeline
            .nodes()
            .iter()
            .map(|node| (node.id().as_str().to_owned(), derived::answers(node)))
            .collect()
    });
    let (hash, error) = verdict(lowered);
    // The load's first half alone: a document that does not validate still
    // reads, and the editor opens it (ADR-C40 § 4).
    let typed = read_document(&file.document)
        .ok()
        .and_then(|raw| convert::typed_document(&raw));
    let canonical = validation::is_rendering(&file.document);
    let etag = file.revision.clone();
    Ok(with_etag(
        Json(PipelineDetail {
            name: file.name,
            document: file.document,
            etag: etag.as_str().to_owned(),
            hash,
            error,
            typed,
            canonical,
            ends_in_answer_up_to,
        }),
        &etag,
    ))
}

/// `PUT /pipelines/{name}`: `If-Match` or `If-None-Match: *`, and the
/// document — a text, stored byte for byte, or the editor's typed document,
/// stored as the server renders it.
pub(crate) async fn write(
    State(state): State<AppState>,
    ApiPath(name): ApiPath<String>,
    _: ApiQuery<NoParameters>,
    ApiHeaders(headers): ApiHeaders<PreconditionHeaders>,
    ApiJson(request): ApiJson<PipelineDocument>,
) -> Result<Response, ApiError> {
    let precondition = precondition(headers)?;
    let text = match request {
        PipelineDocument::Document(text) => text,
        PipelineDocument::Typed(typed) => validation::render_typed(&typed)?,
    };
    // The composition root's key refusals, with the workspace's bindings
    // deciding which names are bound: what `bench` would refuse is not stored.
    let pipeline = validation::lower(&text)?;
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
        .write(&name, &text, &precondition)
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

/// `POST /pipelines/validate`: the hash and the rendering, or
/// `pipeline_invalid` — of the text as sent, or of the typed document's
/// rendering.
pub(crate) async fn validate(
    _: ApiQuery<NoParameters>,
    ApiJson(request): ApiJson<ValidationRequest>,
) -> Result<Json<PipelineValidated>, ApiError> {
    let (hash, rendering) = match request {
        ValidationRequest::Document(text) => {
            (validation::check(&text)?, validation::rendering(&text))
        }
        ValidationRequest::Typed(typed) => {
            let text = validation::render_typed(&typed)?;
            (validation::check(&text)?, Some(text))
        }
    };
    Ok(Json(PipelineValidated { hash, rendering }))
}

/// `GET /pipelines/{name}/layout`.
pub(crate) async fn read_layout(
    State(state): State<AppState>,
    ApiPath(name): ApiPath<String>,
    _: ApiQuery<NoParameters>,
) -> Result<Json<PipelineLayout>, ApiError> {
    Ok(Json(PipelineLayout {
        layout: state.backends.pipelines.read_layout(&name).await?,
    }))
}

/// `GET /runs/{id}/layout`: the layout copied at launch for the run's
/// pipeline, `layouts/<hash>.json` by its canonical hash, if any — what a
/// fork from the run copies beside its new document.
pub(crate) async fn run_layout(
    State(state): State<AppState>,
    ApiPath(id): ApiPath<String>,
    _: ApiQuery<NoParameters>,
) -> Result<Json<PipelineLayout>, ApiError> {
    let run = load_run(&state, id).await?;
    Ok(Json(PipelineLayout {
        layout: state
            .backends
            .pipelines
            .read_launched_layout(&run.inputs.pipeline.to_string())
            .await?,
    }))
}

/// `PUT /pipelines/{name}/layout`: replaces the layout, and answers it.
pub(crate) async fn write_layout(
    State(state): State<AppState>,
    ApiPath(name): ApiPath<String>,
    _: ApiQuery<NoParameters>,
    ApiJson(layout): ApiJson<Layout>,
) -> Result<Json<PipelineLayout>, ApiError> {
    state
        .backends
        .pipelines
        .write_layout(&name, &layout)
        .await?;
    Ok(Json(PipelineLayout {
        layout: Some(layout),
    }))
}

/// The hash a document validates to, or why it does not, from its lowering.
fn verdict(lowered: Result<LogicalPipeline, ApiError>) -> (Option<String>, Option<PipelineError>) {
    match lowered.map(|pipeline| pipeline.content_hash().to_string()) {
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
fn precondition(headers: PreconditionHeaders) -> Result<Precondition, ApiError> {
    match (headers.if_match, headers.if_none_match) {
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn a_pre_epoch_modification_time_is_null() {
        assert_eq!(modified_ms(UNIX_EPOCH - Duration::from_millis(1)), None);
        assert_eq!(modified_ms(UNIX_EPOCH), Some(0));
        assert_eq!(
            modified_ms(UNIX_EPOCH + Duration::from_micros(1999)),
            Some(1)
        );
    }
}
