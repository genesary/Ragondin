//! The handlers of the workspace's endpoints — pipelines, benchmarks,
//! services — beside the read endpoints of `handlers.rs`. Each reads or
//! writes through the backends, converts into this crate's own types, and
//! answers; every failure is an [`ApiError`].

use axum::body::Bytes;
use serde::de::DeserializeOwned;

use crate::error::ApiError;

pub(crate) mod benchmarks;
pub(crate) mod pipelines;
pub(crate) mod services;

/// A request body read as the operation's JSON. Read by hand rather than by
/// axum's extractor, whose refusal is a plain-text body: a problem body says
/// which field was wrong, in `request_invalid`.
fn json_body<T: DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|error| ApiError::RequestInvalid {
        detail: format!("the body is not this operation's JSON: {error}"),
    })
}
