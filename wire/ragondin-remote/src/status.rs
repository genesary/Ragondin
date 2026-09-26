//! How a `ComponentError` crosses the wire, in both directions (ADR-C35).
//!
//! The one place a status is mapped. Every adapter in this crate goes through
//! [`error_from_status`], [`error_from_response`] and
//! [`error_from_identity_response`], and a service written in
//! Rust over a `Local` component goes through [`status_from_error`] and
//! [`status_from_request`]; no adapter maps a status itself (ADR-C35 § 4).
//!
//! | `ComponentError` | status a service returns | statuses an adapter maps to it |
//! |---|---|---|
//! | `InvalidRequest` | `INVALID_ARGUMENT` | `INVALID_ARGUMENT` |
//! | `Unavailable` | `UNAVAILABLE` | `UNAVAILABLE`, `DEADLINE_EXCEEDED`, `CANCELLED` |
//! | `Backend` | `INTERNAL` | every other code, with the `Status` as its source; and an OK response the adapter cannot convert |
//!
//! One exception on the adapter's side: an empty identity in an OK
//! `GetModelIdentity` response is `InvalidRequest`, by ADR-C31 § 1's specific
//! rule ([`error_from_identity_response`]).
//!
//! A service uses no fourth code, `RESOURCE_EXHAUSTED` included: overload and
//! a quota are `UNAVAILABLE`, because the caller's remedy is to try later.

use ragondin_contracts::ComponentError;
use tonic::{Code, Status};

use crate::DecodeError;

/// The status a service returns for a component's error (ADR-C35 § 1, § 3):
/// `INVALID_ARGUMENT`, `UNAVAILABLE` or `INTERNAL`, and no other.
///
/// The message is the error's own. A `Backend`'s cause does not cross: only
/// its message does, and the adapter receives it as `INTERNAL`.
pub fn status_from_error(error: &ComponentError) -> Status {
    match error {
        ComponentError::InvalidRequest(message) => Status::invalid_argument(message.clone()),
        ComponentError::Unavailable(message) => Status::unavailable(message.clone()),
        ComponentError::Backend(cause) => Status::internal(cause.to_string()),
        // `ComponentError` is `#[non_exhaustive]`. A variant added later is
        // "any other failure" until ADR-C35's table names it.
        other => Status::internal(other.to_string()),
    }
}

/// The status a service returns for a request it cannot convert to the domain
/// types — a required message left out, `EMBED_ROLE_UNSPECIFIED`, an enum
/// number the domain has no value for: `INVALID_ARGUMENT` (ADR-C35 § 1).
pub fn status_from_request(error: DecodeError) -> Status {
    Status::invalid_argument(error.to_string())
}

/// The `ComponentError` an adapter makes of a status it received (ADR-C35
/// § 2). Total: every code, `OK` included, maps to a variant.
///
/// `INVALID_ARGUMENT` is `InvalidRequest`. `UNAVAILABLE`, `DEADLINE_EXCEEDED`
/// and `CANCELLED` are `Unavailable`: `tonic` reports a failed connect as
/// `UNAVAILABLE` and an expired call timeout as `CANCELLED`, and a caller that
/// abandons a call drops its future and receives no status at all. Every other
/// code is `Backend`, with the `Status` boxed as its source so that its code
/// and message reach a caller who walks the error chain.
pub fn error_from_status(status: Status) -> ComponentError {
    match status.code() {
        Code::InvalidArgument => ComponentError::InvalidRequest(status.message().to_owned()),
        code @ (Code::Unavailable | Code::DeadlineExceeded | Code::Cancelled) => {
            ComponentError::Unavailable(format!("{code:?}: {}", status.message()))
        }
        _ => ComponentError::Backend(Box::new(status)),
    }
}

/// The `ComponentError` an adapter makes of an OK response it cannot convert
/// to the domain types, or one that breaks the family's contract: `Backend`,
/// with the [`DecodeError`] as its source (ADR-C35 § 2). The service answered,
/// so the caller's request is not at fault, and nothing about it says to try
/// later. An identity response goes through [`error_from_identity_response`]
/// instead, for the one case a more specific rule governs.
pub fn error_from_response(error: DecodeError) -> ComponentError {
    ComponentError::Backend(Box::new(error))
}

/// The `ComponentError` an adapter makes of a `GetModelIdentity` response it
/// cannot convert.
///
/// An empty identity is `InvalidRequest`: ADR-C31 § 1 decides specifically
/// that an adapter receiving one "refuses it as an `InvalidRequest`-class
/// failure", and ADR-C35 § 2's general row for a contract-breaking response
/// does not displace it — ADR-C35 supersedes nothing. Every other refusal of
/// an identity response, such as the identity message left out, is
/// [`error_from_response`]'s `Backend`. Every adapter of a model-bearing
/// family maps its identity response through this, and through nothing else.
pub fn error_from_identity_response(error: DecodeError) -> ComponentError {
    match error {
        DecodeError::Empty {
            message: "ModelIdentity",
            ..
        } => ComponentError::InvalidRequest(error.to_string()),
        other => error_from_response(other),
    }
}
