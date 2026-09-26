//! The status ⇄ `ComponentError` conversion, as ADR-C35 fixes it.
//!
//! § 1 and § 3: a service reports a failure with one of three codes. § 2: an
//! adapter maps every code it can receive back with one total function. The
//! round trip of § 3 over a real channel is in `adapters.rs`; this file holds
//! the table itself.

use std::error::Error as _;

use ragondin_contracts::ComponentError;
use ragondin_remote::{
    error_from_response, error_from_status, status_from_error, status_from_request, DecodeError,
};
use tonic::{Code, Status};

/// Every code gRPC defines, `Ok` included: the function is total over what a
/// `Status` can hold, not over what a well-behaved service sends.
const EVERY_CODE: [Code; 17] = [
    Code::Ok,
    Code::Cancelled,
    Code::Unknown,
    Code::InvalidArgument,
    Code::DeadlineExceeded,
    Code::NotFound,
    Code::AlreadyExists,
    Code::PermissionDenied,
    Code::ResourceExhausted,
    Code::FailedPrecondition,
    Code::Aborted,
    Code::OutOfRange,
    Code::Unimplemented,
    Code::Internal,
    Code::Unavailable,
    Code::DataLoss,
    Code::Unauthenticated,
];

#[derive(Debug, PartialEq)]
enum Variant {
    Unavailable,
    InvalidRequest,
    Backend,
}

fn variant(error: &ComponentError) -> Variant {
    match error {
        ComponentError::Unavailable(_) => Variant::Unavailable,
        ComponentError::InvalidRequest(_) => Variant::InvalidRequest,
        ComponentError::Backend(_) => Variant::Backend,
        other => panic!("a variant this test does not know: {other:?}"),
    }
}

/// ADR-C35 § 2's table, stated by code.
fn expected(code: Code) -> Variant {
    match code {
        Code::InvalidArgument => Variant::InvalidRequest,
        Code::Unavailable | Code::DeadlineExceeded | Code::Cancelled => Variant::Unavailable,
        _ => Variant::Backend,
    }
}

#[test]
fn every_code_maps_to_the_variant_adr_c35_gives_it() {
    for code in EVERY_CODE {
        let error = error_from_status(Status::new(code, "why"));
        assert_eq!(variant(&error), expected(code), "{code:?}");
    }
}

#[test]
fn a_mapped_message_keeps_the_status_message() {
    for code in [Code::InvalidArgument, Code::Unavailable, Code::Cancelled] {
        let error = error_from_status(Status::new(code, "the reason"));
        assert!(
            error.to_string().contains("the reason"),
            "{code:?}: {error}"
        );
    }
}

#[test]
fn every_other_code_is_backend_with_the_status_as_its_source() {
    for code in EVERY_CODE
        .into_iter()
        .filter(|code| expected(*code) == Variant::Backend)
    {
        let error = error_from_status(Status::new(code, "the reason"));
        let source = error.source().expect("Backend carries a source");
        let status = source
            .downcast_ref::<Status>()
            .unwrap_or_else(|| panic!("{code:?}: the source is not the Status"));
        assert_eq!(status.code(), code);
        assert_eq!(status.message(), "the reason");
    }
}

#[test]
fn a_service_reports_each_variant_with_its_one_code() {
    let cases = [
        (
            ComponentError::InvalidRequest("bad".into()),
            Code::InvalidArgument,
        ),
        (
            ComponentError::Unavailable("down".into()),
            Code::Unavailable,
        ),
        (ComponentError::Backend("boom".into()), Code::Internal),
    ];
    for (error, code) in cases {
        let status = status_from_error(&error);
        assert_eq!(status.code(), code, "{error:?}");
    }
}

#[test]
fn a_service_status_carries_the_error_message() {
    let status = status_from_error(&ComponentError::InvalidRequest("top_k of zero".into()));
    assert!(status.message().contains("top_k of zero"), "{status:?}");
    let status = status_from_error(&ComponentError::Backend("index corrupt".into()));
    assert!(status.message().contains("index corrupt"), "{status:?}");
}

#[test]
fn a_service_never_uses_resource_exhausted_or_any_fourth_code() {
    for error in [
        ComponentError::InvalidRequest("a".into()),
        ComponentError::Unavailable("b".into()),
        ComponentError::Backend("c".into()),
    ] {
        let code = status_from_error(&error).code();
        assert!(
            matches!(
                code,
                Code::InvalidArgument | Code::Unavailable | Code::Internal
            ),
            "{code:?}"
        );
    }
}

#[test]
fn a_round_trip_keeps_the_variant() {
    for error in [
        ComponentError::InvalidRequest("a".into()),
        ComponentError::Unavailable("b".into()),
        ComponentError::Backend("c".into()),
    ] {
        let back = error_from_status(status_from_error(&error));
        assert_eq!(variant(&back), variant(&error), "{error:?}");
    }
}

#[test]
fn a_request_the_service_cannot_decode_is_invalid_argument() {
    let status = status_from_request(DecodeError::Unspecified {
        message: "EmbedParams",
        field: "role",
    });
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(status.message().contains("EmbedParams.role"), "{status:?}");
}

#[test]
fn a_response_the_adapter_cannot_decode_is_backend() {
    let decode = DecodeError::Empty {
        message: "ModelIdentity",
        field: "identity",
    };
    let error = error_from_response(decode.clone());
    assert_eq!(variant(&error), Variant::Backend);
    let source = error.source().expect("Backend carries a source");
    assert_eq!(source.downcast_ref::<DecodeError>(), Some(&decode));
}
