//! The extractors through which every handler of the `/api` router reads
//! request input (ADR-C37 § 2): [`ApiPath`] for path parameters,
//! [`ApiQuery`] for the query string, [`ApiHeaders`] for every request header
//! a handler reads, and [`ApiJson`] for a JSON body. Each rejects with an
//! [`ApiError`], so every refusal is a problem body with a stable code —
//! never axum's plain text.
//!
//! This is the one module that names axum's raw `Path`, `Query`, the body's
//! `Bytes` and the request's header map: `clippy.toml` refuses the first two
//! and `HeaderMap` everywhere else in the crate.

// The raw extractors are this module's to wrap: everywhere else in the crate
// `clippy.toml` refuses them (ADR-C37 § 2), and this is where the refusal
// ends.
#![allow(clippy::disallowed_types)]

use std::error::Error;

use axum::async_trait;
use axum::body::Bytes;
use axum::extract::path::ErrorKind;
use axum::extract::rejection::{BytesRejection, FailedToBufferBody, PathRejection};
use axum::extract::{FromRequest, FromRequestParts, Path, Query, RawPathParams, Request};
use axum::http::request::Parts;
use schemars::{JsonSchema, SchemaGenerator};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::error::ApiError;

/// The path parameters, deserialized into `T`. A segment that does not
/// decode to UTF-8, or whose value `T` does not read, is `parameter_invalid`
/// naming the path parameter it fills (ADR-C37 § 4).
pub(crate) struct ApiPath<T>(pub(crate) T);

#[async_trait]
impl<S, T> FromRequestParts<S> for ApiPath<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        match Path::<T>::from_request_parts(parts, state).await {
            Ok(Path(value)) => Ok(Self(value)),
            Err(rejection) => Err(path_refused(parts, state, rejection).await),
        }
    }
}

/// What a path rejection is, as a problem: a value the request sent is
/// `parameter_invalid`, named when axum's rejection or the route's own
/// parameter list says which; a handler whose type does not fit its route is
/// this build's defect, `backend_failed`.
async fn path_refused<S: Send + Sync>(
    parts: &mut Parts,
    state: &S,
    rejection: PathRejection,
) -> ApiError {
    let PathRejection::FailedToDeserializePathParams(failed) = rejection else {
        return ApiError::BackendFailed {
            detail: format!("the route's path parameters were not read: {rejection}"),
        };
    };
    let kind = failed.into_kind();
    let reason = kind.to_string();
    let name = match kind {
        ErrorKind::InvalidUtf8InPathParam { key } => {
            return ApiError::ParameterInvalid {
                name: Some(key),
                reason: "it does not decode to UTF-8".to_owned(),
            }
        }
        ErrorKind::ParseErrorAtKey { key, .. } => Some(key),
        // A tuple reports a position; the route's own list names it.
        ErrorKind::ParseErrorAtIndex { index, .. } => {
            RawPathParams::from_request_parts(parts, state)
                .await
                .ok()
                .and_then(|raw| raw.iter().nth(index).map(|(key, _)| key.to_owned()))
        }
        ErrorKind::ParseError { .. } | ErrorKind::Message(_) => None,
        _ => {
            return ApiError::BackendFailed {
                detail: format!("the route's path parameters were not read: {reason}"),
            }
        }
    };
    ApiError::ParameterInvalid { name, reason }
}

/// The query string, validated as strict percent-encoded UTF-8 and then
/// deserialized into `T` by axum's `Query` (ADR-C37 § 3).
///
/// `T` is a closed struct, `#[serde(deny_unknown_fields)]`, whose values
/// are integers, enums or self-validating newtypes (ADR-C37 § 4) — so an
/// unknown, repeated or unreadable parameter is refused, `parameter_invalid`,
/// rather than ignored. An endpoint that takes none reads [`NoParameters`].
pub(crate) struct ApiQuery<T>(pub(crate) T);

#[async_trait]
impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        validate(parts.uri.query().unwrap_or(""))?;
        let Query(value) = Query::<T>::try_from_uri(&parts.uri).map_err(|rejection| {
            ApiError::ParameterInvalid {
                // serde's reason names no parameter it can be read from: a
                // value's own type describes itself instead.
                name: None,
                reason: innermost(&rejection),
            }
        })?;
        Ok(Self(value))
    }
}

/// The parameters of an endpoint that takes none: any parameter sent to it
/// is refused rather than ignored.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoParameters {}

/// Checks that every name and every value of `query` percent-decodes to
/// UTF-8, every `%` followed by two hexadecimal digits — what `Query`'s
/// lenient decoder would otherwise pass through literally or replace with
/// U+FFFD. `+` is a space, as a form encodes one. It produces no value: the
/// deserializer reads the query once this has passed.
fn validate(query: &str) -> Result<(), ApiError> {
    let refused = |name: Option<String>| ApiError::ParameterInvalid {
        name,
        reason: "it is not percent-encoded UTF-8".to_owned(),
    };
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = decode(name).ok_or_else(|| refused(None))?;
        decode(value).ok_or_else(|| refused(Some(name)))?;
    }
    Ok(())
}

/// One name or value of a query string, percent-decoded; `None` when it is
/// not percent-encoded UTF-8.
fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => decoded.push(b' '),
            b'%' => {
                // Two hexadecimal digits, checked here: `from_str_radix`
                // also accepts a leading sign, which would read `%+1` as 1.
                let hex = bytes.get(at + 1..at + 3)?;
                if !hex.iter().all(u8::is_ascii_hexdigit) {
                    return None;
                }
                decoded.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
                at += 2;
            }
            byte => decoded.push(byte),
        }
        at += 1;
    }
    String::from_utf8(decoded).ok()
}

/// The request headers `T` names, deserialized into it (ADR-C37 § 4).
///
/// `T` is a struct whose fields are the headers it reads, each renamed to
/// the header's wire spelling (`If-Match`), deriving `JsonSchema`: its
/// schema's properties are the names read, which is also what the
/// description declares as `in: header`. It is not closed — a request
/// carries headers no handler reads, and every header `T` does not name is
/// ignored. A header it names that is sent more than once is
/// `parameter_invalid`, naming it; one whose value is not text is
/// `request_invalid`, as it was before the headers were declared. No
/// admitted crate deserializes a header map, so each value is read here, as
/// trimmed text, into a JSON object `T` is deserialized from.
pub(crate) struct ApiHeaders<T>(pub(crate) T);

#[async_trait]
impl<S, T> FromRequestParts<S> for ApiHeaders<T>
where
    S: Send + Sync,
    T: DeserializeOwned + JsonSchema,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        let schema = SchemaGenerator::default().into_root_schema_for::<T>();
        let names = schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|properties| properties.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut read = Map::new();
        for name in names {
            let mut values = parts.headers.get_all(name.as_str()).iter();
            let Some(value) = values.next() else {
                continue;
            };
            if values.next().is_some() {
                return Err(ApiError::ParameterInvalid {
                    name: Some(name),
                    reason: "it is sent more than once".to_owned(),
                });
            }
            let text = value.to_str().map_err(|_| ApiError::RequestInvalid {
                detail: format!("the `{name}` header is not text"),
            })?;
            read.insert(name, Value::String(text.trim().to_owned()));
        }
        serde_json::from_value(Value::Object(read))
            .map(Self)
            .map_err(|error| ApiError::ParameterInvalid {
                name: None,
                reason: error.to_string(),
            })
    }
}

/// The request body, read as `T`'s JSON (ADR-C37 § 4).
///
/// The body is buffered and parsed here rather than by axum's `Json`, which
/// would refuse a body without `Content-Type: application/json` with a
/// plain-text `415`: a body that is not the operation's JSON is
/// `request_invalid`, saying which field was wrong. A body over axum's
/// default limit is `body_too_large` (413), and one that fails to buffer —
/// a connection that breaks off mid-body, say — `request_invalid`.
///
/// An empty body is no body: it reads as JSON `null`, so `ApiJson<Option<U>>`
/// reads it as `None`, and a type that takes no `null` refuses it. A body
/// that *is* `null` is refused whatever the type, since it is no operation's
/// JSON object.
pub(crate) struct ApiJson<T>(pub(crate) T);

#[async_trait]
impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, ApiError> {
        let body = Bytes::from_request(request, state)
            .await
            .map_err(body_refused)?;
        let invalid = |detail: String| ApiError::RequestInvalid { detail };
        if body.is_empty() {
            return serde_json::from_value(Value::Null).map(Self).map_err(|_| {
                invalid("the body is empty, and this operation reads JSON".to_owned())
            });
        }
        if body.trim_ascii() == b"null" {
            return Err(invalid(
                "the body is `null`, not this operation's JSON".to_owned(),
            ));
        }
        serde_json::from_slice(&body)
            .map(Self)
            .map_err(|error| invalid(format!("the body is not this operation's JSON: {error}")))
    }
}

/// A body that was not read, as a problem.
fn body_refused(rejection: BytesRejection) -> ApiError {
    match rejection {
        BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(limit)) => {
            ApiError::BodyTooLarge {
                detail: innermost(&limit),
            }
        }
        other => ApiError::RequestInvalid {
            detail: format!("the body could not be read: {}", innermost(&other)),
        },
    }
}

/// The deepest cause of an axum rejection: the decoder's or the body's own
/// words, which the rejection's `Display` leaves out.
fn innermost(error: &(dyn Error + 'static)) -> String {
    let mut cause = error;
    while let Some(source) = cause.source() {
        cause = source;
    }
    cause.to_string()
}
