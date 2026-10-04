//! A pipeline document loaded from its text: [`parse_document`], its first
//! half [`read_document`], the [`DocumentError`] they refuse with,
//! [`render_document`], which writes the wire schema back as text, and
//! [`incompatible_wiring`], the report for an edge of the wrong kind.
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
//!        └─ read_document ─┘
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
/// says why it is not one: [`read_document`], then the validation pass.
///
/// Stops at `LogicalPipeline`: resolving implementations to components is
/// physical planning, which needs an `EngineContext` and belongs to the
/// engine (ADR-C2). The canonical hash is the pipeline's own
/// [`content_hash`](LogicalPipeline::content_hash), over the canonical logical
/// form and never over this text (INV-8).
pub fn parse_document(text: &str) -> Result<LogicalPipeline, DocumentError> {
    validate(read_document(text)?).map_err(DocumentError::Invalid)
}

/// Reads a pipeline document's text into the wire schema, [`RawPipeline`],
/// whether or not the graph it describes validates: the first half of
/// [`parse_document`], and never a second load beside it.
///
/// A caller that shows a document a person is still editing needs this half
/// alone, since a document that does not validate has no lowered form
/// (ADR-C40 § 4).
pub fn read_document(text: &str) -> Result<RawPipeline, DocumentError> {
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
    serde_yaml::from_str(text).map_err(DocumentError::Malformed)
}

/// Renders a wire-schema document as the configuration format's text: the
/// one writer of the format, beside its one reader.
///
/// What it writes reads back, through [`read_document`], to the same
/// [`RawPipeline`], and rendering that again changes nothing (ADR-C40 § 5).
/// A document whose rendering would read back as anything else is refused
/// rather than rendered, so no caller ever holds text that means another
/// document.
///
/// **The rendering is JSON, which YAML reads**: indented, one key per line,
/// and every string in double quotes. That is how the renderer quotes the
/// strings another reader of the format could retype — a YAML 1.1 boolean
/// such as `yes` or `on`, a null, a number, a date: `serde_yaml`'s own writer
/// quotes only what its own reader would retype, and leaves `yes` plain. A
/// float keeps its fractional part (`60.0`), an integer has none. Comments,
/// key order and formatting a person wrote are not in the wire schema, so a
/// rendering does not keep them.
pub fn render_document(document: &RawPipeline) -> Result<String, RenderError> {
    // The wire schema's own `Serialize`, never an internal type's (INV-9).
    // It does not fail on this schema, whose every key is a string; a
    // non-finite float it writes as `null`, which the guard below refuses.
    let json = serde_json::to_string_pretty(document).map_err(|_| RenderError)?;
    let text = escape_for_yaml(&json) + "\n";
    // The guard that makes the promise above hold by construction: a
    // non-finite float, which JSON writes as `null`, or a key too long once
    // rendered for YAML to read as one, is found here rather than by the next reader, and
    // so would anything else that did not read back.
    match read_document(&text) {
        Ok(read) if read == *document => Ok(text),
        _ => Err(RenderError),
    }
}

/// Whether YAML folds or refuses this character bare in a double-quoted
/// string where JSON leaves it bare: DEL and the C1 controls, U+0085 among
/// them, and U+2028 and U+2029, which YAML reads as line breaks, and the two
/// noncharacters U+FFFE and U+FFFF, which it does not accept as printable.
fn yaml_needs_escape(c: char) -> bool {
    matches!(
        c,
        '\u{7f}'..='\u{9f}' | '\u{2028}' | '\u{2029}' | '\u{fffe}' | '\u{ffff}'
    )
}

/// The JSON with each character [`yaml_needs_escape`] names written as
/// `\uXXXX`, an escape JSON and YAML both read as the character, so a string
/// holding one reads back. JSON's structure is ASCII and these characters are
/// not, so each one found sits inside a string, where the escape is valid;
/// `serde_json` writes every backslash of a string as a pair, so none is left
/// open before it.
fn escape_for_yaml(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if yaml_needs_escape(c) {
            out.push_str(&format!("\\u{:04x}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

/// A wire-schema document whose rendering would not read back as itself: a
/// non-finite float, which neither JSON nor the wire schema's reader carries,
/// or a parameter name too long once rendered for YAML to read it as a key:
/// the limit counts the key as written, its quotes and escapes included —
/// about 1022 plain characters, about 511 `é` or newlines (`\n`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the document holds what the configuration format cannot carry: a number that is not finite, or a parameter name too long once rendered")]
pub struct RenderError;

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
