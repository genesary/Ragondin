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
    peek_schema_version, validate, LogicalPipeline, RawParamValue, RawPipeline,
    SchemaVersionPeekError, UnsupportedSchemaVersion, ValidationError,
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
/// **The rendering is block YAML, written by the rules of ADR-C41 § 1**:
/// `version: N` first; maps in block style, in the wire schema's key order,
/// a node's parameters in the byte order of their keys and a blank line
/// between nodes; lists in flow style, an empty one `[]`. A string — a key
/// as much as a value — is written plain only when no YAML 1.1 or 1.2 reader
/// can take it for anything else, and double-quoted otherwise. A float has a
/// fractional part (`60.0`) and a signed exponent when it has one
/// (`1.0e+20`); an integer has neither. Comments, key order and formatting a
/// person wrote are not in the wire schema, so a rendering does not keep
/// them.
pub fn render_document(document: &RawPipeline) -> Result<String, RenderError> {
    let text = write_document(document);
    // The guard that makes the promise above hold by construction: a
    // non-finite float, which the writer spells as no reader's float, or a
    // key too long once rendered for YAML to read as one, is found here
    // rather than by the next reader, and so would anything else that did
    // not read back.
    match read_document(&text) {
        Ok(read) if read == *document => Ok(text),
        _ => Err(RenderError),
    }
}

/// The writer of ADR-C41: the wire schema, and nothing else, as block YAML.
/// It reads nothing and edits no text in place; [`render_document`] checks
/// everything it writes through the one reader (ADR-C41 § 4).
fn write_document(document: &RawPipeline) -> String {
    let graph = &document.pipeline;
    let mut out = format!(
        "version: {}\npipeline:\n  inputs: {}\n",
        document.version.get(),
        flow(graph.inputs.iter().map(|input| scalar(input)))
    );
    if graph.nodes.is_empty() {
        out.push_str("  nodes: []\n");
        return out;
    }
    out.push_str("  nodes:\n");
    for (i, node) in graph.nodes.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("    - id: {}\n", scalar(&node.id)));
        out.push_str(&format!("      component: {}\n", scalar(&node.component)));
        out.push_str(&format!("      impl: {}\n", scalar(&node.implementation)));
        out.push_str(&format!(
            "      inputs: {}\n",
            flow(node.inputs.iter().map(|input| scalar(input)))
        ));
        // An empty map has no block form, and an absent `params` reads as
        // empty, so a node with none writes no key.
        if !node.params.is_empty() {
            out.push_str("      params:\n");
            for (key, value) in &node.params {
                out.push_str(&format!("        {}: {}\n", scalar(key), param(value)));
            }
        }
    }
    out
}

/// A list in flow style: `[a, b]`, and `[]` when it is empty.
fn flow(items: impl Iterator<Item = String>) -> String {
    format!("[{}]", items.collect::<Vec<_>>().join(", "))
}

fn param(value: &RawParamValue) -> String {
    match value {
        RawParamValue::Bool(flag) => flag.to_string(),
        RawParamValue::Int(int) => int.to_string(),
        RawParamValue::Float(float) => write_float(*float),
        RawParamValue::String(text) => scalar(text),
        RawParamValue::List(items) => flow(items.iter().map(param)),
    }
}

/// A float in the YAML 1.1 float form, which a YAML 1.2 reader reads too: a
/// fractional part always, and a signed exponent when there is one. Rust's
/// `Debug` gives the shortest digits that read back to the same value; this
/// adds the `.0` and the `+` it leaves out. A non-finite float comes out as
/// `inf` or `NaN`, which no reader takes for a float; the guard refuses it.
fn write_float(float: f64) -> String {
    let shortest = format!("{float:?}");
    let Some((mantissa, exponent)) = shortest.split_once('e') else {
        return shortest;
    };
    let point = if mantissa.contains('.') { "" } else { ".0" };
    let sign = if exponent.starts_with('-') { "" } else { "+" };
    format!("{mantissa}{point}e{sign}{exponent}")
}

/// The words YAML 1.1 reads as a boolean or a null, in any case, that the
/// plain-string rule would otherwise admit.
const RESERVED: [&str; 9] = ["y", "n", "yes", "no", "true", "false", "on", "off", "null"];

/// A string, plain when ADR-C41 § 1's allowlist admits it — an ASCII letter
/// or `_`, then `[A-Za-z0-9_./-]`, and not a [`RESERVED`] word in any case —
/// and double-quoted otherwise. Every other implicit type of YAML 1.1 and
/// 1.2 begins with a digit, a sign, `.`, `~`, `=` or `<`, so a string the
/// allowlist admits reads as a string under both. Keys take this rule too.
fn scalar(text: &str) -> String {
    let plain = text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-'))
        && !RESERVED.iter().any(|word| word.eq_ignore_ascii_case(text));
    if plain {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if needs_escape(c) => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            // Outside the Basic Multilingual Plane too: written as itself,
            // never as a surrogate pair.
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether a character is written as `\uXXXX` inside double quotes: every
/// character YAML does not count as printable — the C0 controls, DEL, the C1
/// controls, U+FFFE and U+FFFF — and the line breaks it would fold, U+0085,
/// U+2028 and U+2029, and U+FEFF, the byte-order mark.
fn needs_escape(c: char) -> bool {
    matches!(
        c,
        '\u{0}'..='\u{1f}'
            | '\u{7f}'..='\u{9f}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{feff}'
            | '\u{fffe}'
            | '\u{ffff}'
    )
}

/// A wire-schema document whose rendering would not read back as itself: a
/// non-finite float, which the wire schema's reader does not carry, or a
/// parameter name too long once rendered for YAML to read it as a key: the
/// limit counts the key as written, its quotes and escapes included —
/// about 1024 plain characters, about 511 `é` or newlines (`\n`).
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
