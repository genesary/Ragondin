//! A pipeline document loaded from its text: [`parse_document`], the
//! [`DocumentError`] it refuses with, and [`incompatible_wiring`], the report
//! for an edge of the wrong kind.
//!
//! This is the one definition of the load. Every caller that holds a
//! document runs it — [`LocalFile`](crate::LocalFile) over a file's contents,
//! `ragondin-api` over a request's body, `ragondin-experiments` over a stored
//! run's configuration — and none of them holds a copy. It takes text, not a
//! path, because only one of those callers has a path. What a caller adds is
//! its own words around the verdict: the file it read, a problem body, a
//! sentence about a stored run.
//!
//! # The load path
//!
//! ```text
//! text → RawPipeline (serde) → validate → LogicalPipeline
//! ```
//!
//! The wire schema is `ragondin-pipeline`'s [`RawPipeline`], hand-maintained
//! and independently versioned; the pass is that crate's [`validate()`].
//! Neither is re-implemented here, which is what INV-9 asks for: a document
//! lands in the wire schema and reaches the in-memory model only through the
//! pass, never by a deserializer pointed at an internal type.
//!
//! The version is read before the document is. `SchemaVersion`'s own
//! `Deserialize` refuses an unsupported version through
//! `serde::de::Error::custom`, which keeps the wording and erases the type, so
//! a plain parse would report *this build is too old* as a syntax error.
//! [`peek_schema_version`] exists for this caller, and reading the version
//! through it is what lets [`DocumentError`] keep the two apart.

use ragondin_pipeline::{
    peek_schema_version, validate, LogicalPipeline, RawPipeline, SchemaVersionPeekError,
    UnsupportedSchemaVersion, ValidationError,
};

/// Lowers a pipeline document's text to its validated [`LogicalPipeline`], or
/// says why it is not one.
///
/// Stops at `LogicalPipeline`: resolving implementations to components is
/// physical planning, which needs an `EngineContext` and belongs to the
/// engine (ADR-C2). The canonical hash is the pipeline's own
/// [`content_hash`](LogicalPipeline::content_hash), over the canonical logical
/// form and never over this text (INV-8).
pub fn parse_document(text: &str) -> Result<LogicalPipeline, DocumentError> {
    // The version first, for the reason this module documents. A peek that
    // comes back `Unreadable` is *not* reported: the deserializer walked a
    // document it could not make sense of, and `ragondin-pipeline` says what
    // to do about that — fall through to the full parse below, which fails too
    // and with the better-located message.
    if let Err(SchemaVersionPeekError::Unsupported(source)) =
        peek_schema_version(serde_yaml::Deserializer::from_str(text))
    {
        return Err(DocumentError::UnsupportedSchemaVersion(source));
    }

    // Into the hand-maintained wire schema, never into an internal type
    // (INV-9).
    let raw: RawPipeline = serde_yaml::from_str(text).map_err(DocumentError::Malformed)?;

    validate(raw).map_err(DocumentError::Invalid)
}

/// Why a document's text is not a [`LogicalPipeline`]: the three halves of
/// the load that can refuse it, each with its own cause intact.
///
/// The `Display` is a heading with no file in it, since a document need not
/// come from one; the cause carries the detail. `ragondin-api` renders a
/// refusal as the heading, a colon, and the cause. [`ConfigError`] is this
/// verdict with the path a file has, and its headings name the file.
///
/// Not `PartialEq`, for the reason [`ConfigError`] is not: `serde_yaml::Error`
/// is not. Match on the variant.
///
/// [`ConfigError`]: crate::ConfigError
#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    /// The document states a schema version this build cannot read. Editing
    /// the document will not help; a newer build will.
    #[error("the configuration is written in a schema version this build cannot read")]
    UnsupportedSchemaVersion(#[source] UnsupportedSchemaVersion),
    /// The text could not be read into the wire schema: a syntax error, a
    /// missing required key, or a parameter outside the flat grammar ADR-C22
    /// fixes. The deserializer's error carries the line and the column.
    #[error("could not parse configuration")]
    Malformed(#[source] serde_yaml::Error),
    /// The wire schema parsed, and the validation pass refuses the graph it
    /// describes.
    #[error("configuration is not a valid pipeline")]
    Invalid(#[source] ValidationError),
}

/// The report for an edge of the wrong kind, or `None` for any other verdict
/// of the validation pass.
///
/// `subject` names what wires the two nodes: the CLI passes the file, in
/// backquotes; the API, which has no file, passes `the configuration`. The
/// rest is the three things a reader has to know to fix the edge: which edge,
/// what the port expects, and what arrives there.
///
/// ADR-C16 places the kind check at `LogicalPipeline` validation, and it is
/// the check a person most needs help reading: unlike a syntax error, it
/// points at no single line of the document. That is why this verdict gets a
/// report of its own rather than going out as the error's `Display`.
///
/// `expected` is `None` when the consumer's variant declares no port at that
/// position at all — an edge that should not exist rather than a kind that
/// does not fit — so that case is worded as the absence of a port and not as
/// an absent kind.
///
/// **The two ends are named in the opposite order to the error's own
/// `Display`.** `ValidationError::KindMismatch` renders the consumer first
/// ("node `ranked` port 0 (fed by `legs`)"); this renders the producer first
/// ("`legs` feeds `ranked` at port 0"), which is the direction the value
/// travels. A reader who meets both renderings of one fault — a library
/// message in a log, this report on a terminal — would otherwise read the
/// swap as a second fault. The library's wording is not touched (INV-1); this
/// is a second rendering beside it.
pub fn incompatible_wiring(subject: &str, error: &ValidationError) -> Option<String> {
    let ValidationError::KindMismatch {
        consumer,
        port,
        producer,
        expected,
        found,
    } = error
    else {
        return None;
    };
    let expected = match expected {
        Some(kind) => kind.to_string(),
        None => format!(
            "nothing — `{}` declares no port at position {port}",
            consumer.as_str()
        ),
    };
    Some(format!(
        "{subject} wires two nodes incompatibly\n  \
         edge: `{}` feeds `{}` at port {port}\n  \
         expected: {expected}\n  \
         found: {found}",
        producer.as_str(),
        consumer.as_str(),
    ))
}
