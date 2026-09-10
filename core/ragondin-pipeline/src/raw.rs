//! The permissive wire schema: `RawPipeline`, the level a user's text file
//! lands in before anything has been checked.
//!
//! This is where **INV-9** lives. The serialized format is **separate from the
//! in-memory representation and versioned independently**: these types are
//! hand-maintained to match `docs/system-architecture.md` §5.1, and the
//! internal [`LogicalPipeline`](crate::node) types are never derived into the
//! wire format. A refactor of the logical model must not silently invalidate
//! every stored configuration, and a change to the stored format must be a
//! deliberate version bump here.
//!
//! The separation is forced, not stylistic: a configuration writes
//! `top_k: 50`, and [`crate::ParamValue`] is externally tagged, so it cannot
//! read that at all. [`RawParamValue`] is the untagged counterpart.
//!
//! **This level is deliberately permissive.** An unknown key is dropped at the
//! parse — no type here denies unknown fields — so it never reaches a later
//! pass at all. Component families with no `LogicalNode` variant and
//! references to nodes that do not exist are carried through intact instead,
//! and rejecting *those* is [`crate::validate::validate`]'s job, the pass that
//! lowers this level into a `LogicalPipeline`. A parser that pre-empted it
//! would turn a user's diagnosable mistake into an opaque parse failure.
//!
//! Two things it does *not* tolerate, for different reasons.
//!
//! A **stated** schema version it cannot read is refused: permissiveness means
//! tolerating content one does not understand *within a grammar one does*, and
//! a version bump says the grammar itself may have changed. Continuing there
//! is not leniency, it is misinterpretation.
//!
//! That refusal reaches a caller two ways, and only one of them keeps its
//! type. Through `Deserialize`, [`SchemaVersion`] routes to
//! [`SchemaVersion::new`] and hands the error to `serde::de::Error::custom`,
//! which keeps the message and erases the type — so a caller could only tell
//! "this configuration needs a newer build" from "this file is malformed" by
//! matching on text. [`peek_schema_version`] reads the `version` field alone,
//! without the rest of the document being interpreted, and returns
//! [`UnsupportedSchemaVersion`] as a type the caller can match on. It is the
//! surface `ragondin-config` (#27) is meant to load a file through.
//!
//! Uninterpreted is not unread: the deserializer still walks the whole
//! document, so a syntax error anywhere in it outranks the version verdict and
//! comes back as [`SchemaVersionPeekError::Unreadable`]. The peek skips the
//! meaning of the rest of the text, not its bytes — which is all the refusal
//! needs, since what a version bump puts in doubt is meaning.
//!
//! An **absent** version reads as the version this build writes: nothing has
//! ever been serialized in an earlier one, so there is no older document for
//! a default to be wrong about, and a configuration that says nothing is
//! current by definition.
//!
//! A **parameter that is not a scalar or a list of scalars** — a nested map,
//! a null — fails to parse. That is now a decided limit rather than an
//! accident (ADR-C22): the parameter grammar is `String | Int | Float | Bool |
//! List` in both models, and [`RawParamValue`] mirrors [`crate::ParamValue`]
//! because admitting a shape on one side only would put the wire and logical
//! models out of step. A **null** is refused permanently — it means *absent*,
//! which omitting the key already says. A **nested map** is refused only until
//! a configuration demands one: both enums are extensible by design, so the
//! variant is additive *on the Rust boundary* when that day comes — it still
//! bumps [`SchemaVersion`] under INV-9, and still owes the content hash a
//! canonicalization one level deeper. A metadata filter
//! (`params: { filters: { lang: fr } }`) is the shape that will ask for it,
//! and it is not expressible today.
//!
//! Two pieces of that decision are not here yet. Neither enum carries
//! `#[non_exhaustive]`, and a refusal still surfaces as serde's opaque
//! untagged-enum message, which names neither the offending key nor what was
//! expected — the one place this level does the thing it exists to prevent.
//! Both land with ADR-C22's implementation. Tracked as #178.
//!
//! Nothing here is executed, and nothing here is hashed.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// The version of the wire schema a configuration is written in.
///
/// Absent from a configuration, it reads as [`SchemaVersion::SUPPORTED`]: a
/// file that states no version is written in the one this build writes. A
/// version this build does not understand is refused rather than guessed at.
///
/// ADR-C18 bumped the supported version to 2 when it added `inputs` to
/// [`RawGraph`], because a change to the wire schema's shape never leaves
/// this type untouched (INV-9). It does **not** follow that an absent version
/// means 1: nothing has ever been serialized in version 1, so guarding
/// against documents that do not exist would only cost every configuration a
/// `version:` line. The mechanism is here and versioned; what it discriminates
/// between starts mattering when a version is actually in use somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// The only schema version this build can read.
    pub const SUPPORTED: u32 = 2;

    /// [`SchemaVersion::SUPPORTED`] as a value, and what
    /// [`RawPipeline::version`] defaults to.
    ///
    /// Every inhabitant of this type is a version this build reads: [`new`]
    /// refuses the rest, `Deserialize` routes through it, and this constant
    /// and [`Default`] both yield [`Self::SUPPORTED`]. Holding a
    /// `SchemaVersion` therefore means it is supported — a promise worth more
    /// than distinguishing a version nothing was ever written in.
    ///
    /// [`new`]: Self::new
    pub const CURRENT: Self = Self(Self::SUPPORTED);

    /// Accepts a version this build understands, and refuses any other.
    pub fn new(version: u32) -> Result<Self, UnsupportedSchemaVersion> {
        if version == Self::SUPPORTED {
            Ok(Self(version))
        } else {
            Err(UnsupportedSchemaVersion { found: version })
        }
    }

    /// The version number.
    pub fn get(self) -> u32 {
        self.0
    }
}

impl Default for SchemaVersion {
    /// [`SchemaVersion::CURRENT`] — the version this build writes, which is
    /// what a configuration saying nothing is written in.
    fn default() -> Self {
        Self::CURRENT
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let found = u32::deserialize(deserializer)?;
        Self::new(found).map_err(serde::de::Error::custom)
    }
}

/// Reads **only** the `version` field of a configuration, and refuses an
/// unsupported one as a typed [`UnsupportedSchemaVersion`].
///
/// [`SchemaVersion`]'s own `Deserialize` also refuses an unreadable version,
/// but through `serde::de::Error::custom`, which takes an `impl Display` and
/// keeps the string: by the time the error reaches a caller, the type is gone
/// and "this configuration needs a newer build" is indistinguishable from
/// "this file is malformed" except by matching on the message text. Peeking
/// first is what makes the distinction a type rather than a substring.
///
/// Generic over the deserializer, so the **caller supplies the format** —
/// `serde_yaml::Deserializer::from_str(text)`, `&mut
/// serde_json::Deserializer::from_str(text)`, or any other. This crate carries
/// no format implementation of its own (INV-4), and reading files is
/// `ragondin-config`'s job (#27), not this crate's (INV-3).
///
/// Every key but `version` is stepped over, so a document this build could not
/// parse as a [`RawPipeline`] still yields its version — which is the point: a
/// version bump says the grammar itself may have changed, and the refusal must
/// land before the rest of the document is **interpreted** under a grammar
/// that may no longer apply.
///
/// Uninterpreted is not unread. The deserializer still walks the whole
/// document, so a syntax error anywhere in it — including well after the
/// `version` line — comes back as [`SchemaVersionPeekError::Unreadable`]
/// rather than as a version verdict. What the peek skips is meaning, not
/// bytes.
///
/// Only a mapping is accepted. `serde`'s derive would also read a struct out
/// of a sequence positionally, which would make the JSON `[7]` an unsupported
/// version 7 — a confident wrong diagnosis about a document that is not a
/// configuration at all.
///
/// An absent version reads as [`SchemaVersion::CURRENT`], exactly as it does
/// in a full parse. An explicit null does not: it is a `version` that is not a
/// number, which the full parse refuses too.
///
/// ```
/// use ragondin_pipeline::{peek_schema_version, SchemaVersionPeekError};
///
/// let text = "version: 7\npipeline:\n  nodes: []\n";
/// let err = peek_schema_version(serde_yaml::Deserializer::from_str(text)).unwrap_err();
/// assert!(matches!(err, SchemaVersionPeekError::Unsupported(_)));
/// ```
pub fn peek_schema_version<'de, D>(
    deserializer: D,
) -> Result<SchemaVersion, SchemaVersionPeekError<D::Error>>
where
    D: Deserializer<'de>,
{
    match deserializer
        .deserialize_map(VersionProbe)
        .map_err(SchemaVersionPeekError::Unreadable)?
    {
        Some(found) => Ok(SchemaVersion::new(found)?),
        None => Ok(SchemaVersion::CURRENT),
    }
}

/// The one key [`peek_schema_version`] is looking for.
const VERSION_KEY: &str = "version";

/// Reads a configuration document's `version`, and steps over everything else.
///
/// Hand-written rather than derived, for two things a derive cannot give.
/// `deserialize_map` refuses a sequence outright, where a derived struct reads
/// one positionally and takes `[7]` for version 7. And `expecting` names the
/// document rather than this type, so a private name never reaches a message a
/// user reads.
struct VersionProbe;

impl<'de> serde::de::Visitor<'de> for VersionProbe {
    type Value = Option<u32>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a configuration document")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut version = None;
        while let Some(key) = map.next_key::<ProbeKey>()? {
            match key {
                ProbeKey::Version if version.is_some() => {
                    return Err(serde::de::Error::duplicate_field(VERSION_KEY));
                }
                ProbeKey::Version => version = Some(map.next_value::<u32>()?),
                ProbeKey::Other => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }
        Ok(version)
    }
}

/// `version`, or one of the keys the probe steps over.
enum ProbeKey {
    Version,
    Other,
}

impl<'de> Deserialize<'de> for ProbeKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct KeyVisitor;

        impl serde::de::Visitor<'_> for KeyVisitor {
            type Value = ProbeKey;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a configuration key")
            }

            fn visit_str<E: serde::de::Error>(self, key: &str) -> Result<ProbeKey, E> {
                Ok(ProbeKey::of(key.as_bytes()))
            }

            // A format that hands keys over as bytes rather than as text. Any
            // other shape of key — a number, say — is left to fail, and a
            // failure here reads as `Unreadable`, which sends the caller to the
            // full parse rather than to a guess about the version.
            fn visit_bytes<E: serde::de::Error>(self, key: &[u8]) -> Result<ProbeKey, E> {
                Ok(ProbeKey::of(key))
            }
        }

        deserializer.deserialize_identifier(KeyVisitor)
    }
}

impl ProbeKey {
    /// Which key this is, by name.
    fn of(key: &[u8]) -> Self {
        if key == VERSION_KEY.as_bytes() {
            Self::Version
        } else {
            Self::Other
        }
    }
}

/// Why [`peek_schema_version`] could not hand back a supported version.
///
/// The two variants are the two diagnoses that must not be confused: a
/// configuration this build is too old to read, and text that does not parse.
/// Only the first is actionable by the person who wrote the file, and only by
/// upgrading rather than by hunting for a syntax error.
///
/// Deliberately carries no `Clone`, `Copy`, `PartialEq` or `Eq`: no real
/// deserializer error implements them, so on a stable boundary (INV-1) they
/// would be decoration that no caller could ever use and that could not be
/// withdrawn.
#[derive(Debug, thiserror::Error)]
pub enum SchemaVersionPeekError<E> {
    /// The configuration stated a version this build cannot read.
    #[error(transparent)]
    Unsupported(#[from] UnsupportedSchemaVersion),
    /// The text could not be read far enough to trust a version — a syntax
    /// error anywhere in the document, a `version` that is not a number, or a
    /// top-level shape that is not a mapping.
    ///
    /// The deserializer's own error, with whatever location it carries, is
    /// this variant's [`source`](std::error::Error::source). `thiserror` puts
    /// the `E: Error` bound on the generated `Error` impl and not on this
    /// type, so `SchemaVersionPeekError<E>` exists for any `E` and *is* an
    /// `Error` for every `'static` deserializer error — which is every real
    /// one.
    ///
    /// A caller that peeks before parsing can simply fall through to the full
    /// parse here: it will fail too, and with the better-located message.
    #[error("the configuration's schema version could not be read")]
    Unreadable(#[source] E),
}

/// A configuration states a schema version this build cannot read.
///
/// Derived via `thiserror` rather than hand-written: `ragondin-pipeline`'s
/// `ARCHITECTURE.md` used to read as forbidding `thiserror` outright — "one
/// error type is not reason enough to widen a core crate's dependencies" —
/// but that reading conflicted with ADR-C13, which requires typed errors via
/// `thiserror` in every library in this workspace. #84 corrected
/// `ARCHITECTURE.md` to permit it, so this type no longer needs a hand-rolled
/// `Display`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "unsupported pipeline schema version {found}: this build reads version {}",
    SchemaVersion::SUPPORTED
)]
pub struct UnsupportedSchemaVersion {
    found: u32,
}

impl UnsupportedSchemaVersion {
    /// The version the configuration stated.
    pub fn found(self) -> u32 {
        self.found
    }
}

/// A parameter value **as a configuration writes it**: a bare scalar.
///
/// Untagged, so `top_k: 50` reads as [`RawParamValue::Int`]. This is the wire
/// counterpart of [`crate::ParamValue`] and must not be confused with it
/// (INV-9); lowering one to the other is [`crate::validate::validate`]'s job.
///
/// Variant order is load-bearing: an untagged enum is tried in declaration
/// order, so `Bool` precedes `Int` precedes `Float`, and `50` reads as an
/// integer rather than as a float.
///
/// These five variants are the whole parameter grammar; a nested map and a
/// null are refused, and this enum is extensible by design rather than closed.
/// See ADR-C22 and the module documentation above for which of those refusals
/// is permanent and which is only current.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RawParamValue {
    /// A flag.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// A number with a fractional part.
    Float(f64),
    /// A text value.
    String(String),
    /// An ordered sequence.
    List(Vec<RawParamValue>),
}

/// One node, exactly as §5.1 writes it: unresolved strings throughout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawNode {
    /// The node's identifier. Not yet known to be unique.
    pub id: String,
    /// The component family, e.g. `retriever`. Not yet known to name a family
    /// this build has a `LogicalNode` variant for.
    pub component: String,
    /// The `impl:` value, e.g. `bm25`. Not yet resolved to a component.
    #[serde(rename = "impl")]
    pub implementation: String,
    /// The ids this node consumes, in port order. Not yet known to exist.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// The node's parameters. Not yet known to be meaningful for this `impl`.
    #[serde(default)]
    pub params: BTreeMap<String, RawParamValue>,
}

/// The graph, as it is nested under `pipeline:` in a configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawGraph {
    /// The ids of the values the pipeline receives from its caller, in the
    /// order the configuration lists them (ADR-C18). A node consumes one by
    /// naming it in its own `inputs`, exactly as it names another node.
    ///
    /// Permissive here, as everything at this level is: absent reads as
    /// empty, and how many a pipeline must declare — exactly one, of kind
    /// `Query`, for a serving graph — is [`crate::validate`]'s to enforce.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// The nodes, in the order the configuration lists them.
    pub nodes: Vec<RawNode>,
}

/// A whole configuration document, as `serde` reads it from YAML or protobuf.
///
/// **Never executed.** It may hold a graph that does not validate; producing a
/// `LogicalPipeline` from it is [`crate::validate::validate`]'s job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawPipeline {
    /// The wire schema version. Absent means [`SchemaVersion::CURRENT`], so a
    /// configuration only writes this line to pin a version deliberately.
    #[serde(default)]
    pub version: SchemaVersion,
    /// The pipeline itself.
    pub pipeline: RawGraph,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_version_reads_as_the_version_this_build_writes() {
        let doc: RawPipeline = serde_json::from_str(r#"{"pipeline":{"nodes":[]}}"#).unwrap();
        assert_eq!(doc.version, SchemaVersion::CURRENT);
        assert_eq!(doc.version.get(), SchemaVersion::SUPPORTED);
    }

    #[test]
    fn every_inhabitant_of_this_type_is_a_version_the_build_reads() {
        // The type's promise: holding a `SchemaVersion` means it is
        // supported. `new` refuses the rest, `Deserialize` routes through it,
        // and `Default`/`CURRENT` yield SUPPORTED — so no constructor can
        // produce a value that fails later.
        assert_eq!(SchemaVersion::CURRENT.get(), SchemaVersion::SUPPORTED);
        assert_eq!(SchemaVersion::default(), SchemaVersion::CURRENT);
        assert_eq!(
            SchemaVersion::new(SchemaVersion::SUPPORTED).unwrap(),
            SchemaVersion::CURRENT
        );
    }

    #[test]
    fn a_document_this_build_reads_survives_a_serialize_reparse_round_trip() {
        // `Serialize` is derived and transparent; `Deserialize` is hand-rolled
        // through `new`. The two must stay inverse, or any store or wire hop
        // that reads a configuration and writes it back turns a readable
        // document into an unreadable one, and the failure surfaces at a
        // later, unrelated read (INV-9).
        let doc: RawPipeline =
            serde_json::from_str(r#"{"pipeline":{"inputs":["question"],"nodes":[]}}"#)
                .expect("a document that states no version must parse");
        let text = serde_json::to_string(&doc).expect("it must serialize");
        let back: RawPipeline =
            serde_json::from_str(&text).expect("what we serialize, we must be able to re-parse");
        assert_eq!(back, doc);
    }

    #[test]
    fn absent_inputs_read_as_an_empty_declaration() {
        // Permissive at this level: how many a pipeline must declare is
        // `validate`'s business, not `serde`'s.
        let doc: RawPipeline = serde_json::from_str(r#"{"pipeline":{"nodes":[]}}"#).unwrap();
        assert!(doc.pipeline.inputs.is_empty());
    }

    #[test]
    fn declared_inputs_read_in_the_order_the_configuration_lists_them() {
        let doc: RawPipeline =
            serde_json::from_str(r#"{"pipeline":{"inputs":["question"],"nodes":[]}}"#).unwrap();
        assert_eq!(doc.pipeline.inputs, vec!["question".to_string()]);
    }

    #[test]
    fn the_supported_version_is_accepted_when_stated() {
        let doc: RawPipeline =
            serde_json::from_str(r#"{"version":2,"pipeline":{"nodes":[]}}"#).unwrap();
        assert_eq!(doc.version.get(), SchemaVersion::SUPPORTED);
    }

    #[test]
    fn an_unknown_version_is_a_typed_error() {
        let err = SchemaVersion::new(7).unwrap_err();
        assert_eq!(err.found(), 7);
        assert!(
            err.to_string().contains('7') && err.to_string().contains('2'),
            "the message must name both what was found and what is understood: {err}"
        );
    }

    #[test]
    fn an_unknown_version_stops_deserialization_rather_than_being_ignored() {
        let err = serde_json::from_str::<RawPipeline>(r#"{"version":7,"pipeline":{"nodes":[]}}"#)
            .expect_err("a version this build cannot read must not parse");
        assert!(
            err.to_string().contains('7'),
            "the parse error must carry the offending version: {err}"
        );
    }

    #[test]
    fn peeking_an_unsupported_version_is_a_typed_error_in_json() {
        // The contract this surface exists for: the caller matches on the
        // *type*, not on a substring of a message.
        let text = r#"{"version":7,"pipeline":{"nodes":[]}}"#;
        let err = peek_schema_version(&mut serde_json::Deserializer::from_str(text))
            .expect_err("a version this build cannot read must be refused");
        match err {
            SchemaVersionPeekError::Unsupported(unsupported) => {
                assert_eq!(unsupported.found(), 7);
            }
            other => panic!("an unsupported version must not read as unreadable text: {other:?}"),
        }
    }

    #[test]
    fn peeking_an_unsupported_version_is_a_typed_error_in_yaml() {
        // The same refusal in a different syntax: the peek is generic over the
        // deserializer, so the caller chooses the format and the typed error is
        // the same one.
        let text = "version: 7\npipeline:\n  nodes: []\n";
        let err = peek_schema_version(serde_yaml::Deserializer::from_str(text))
            .expect_err("a version this build cannot read must be refused");
        match err {
            SchemaVersionPeekError::Unsupported(unsupported) => {
                assert_eq!(unsupported.found(), 7);
            }
            other => panic!("an unsupported version must not read as unreadable text: {other:?}"),
        }
    }

    #[test]
    fn text_that_does_not_parse_peeks_as_unreadable_rather_than_unsupported() {
        // The whole point of the peek: "this configuration needs a newer build"
        // and "this file is malformed" are different diagnoses, and sending a
        // user looking for a syntax error that does not exist is the failure
        // this surface exists to stop. It has to hold in the other direction
        // too, or the peek just relabels every parse error.
        let json = peek_schema_version(&mut serde_json::Deserializer::from_str(r#"{"version":"#))
            .expect_err("truncated JSON states no version this build could act on");
        assert!(
            matches!(json, SchemaVersionPeekError::Unreadable(_)),
            "malformed text is not an unsupported version: {json:?}"
        );

        let yaml = peek_schema_version(serde_yaml::Deserializer::from_str(
            "version: 2\n\tnodes: []\n",
        ))
        .expect_err("a tab where YAML expects indentation is a syntax error");
        assert!(
            matches!(yaml, SchemaVersionPeekError::Unreadable(_)),
            "malformed text is not an unsupported version: {yaml:?}"
        );
    }

    #[test]
    fn a_syntax_error_after_the_version_still_reads_as_unreadable() {
        // The peek skips the *meaning* of the rest of the document, not its
        // bytes: the deserializer still walks to the end, so a syntax error
        // anywhere wins over the version verdict. Pinned because the docs
        // claim exactly this, and the opposite is the easy thing to assume.
        let err = peek_schema_version(serde_yaml::Deserializer::from_str(
            "version: 7\npipeline:\n\tnodes: []\n",
        ))
        .expect_err("text that does not parse cannot yield a version verdict");
        assert!(
            matches!(err, SchemaVersionPeekError::Unreadable(_)),
            "a broken document is unreadable even when its version line is fine: {err:?}"
        );
    }

    #[test]
    fn a_sequence_is_not_a_configuration_document() {
        // `serde`'s derive reads a struct from a sequence positionally, so a
        // derived probe takes `[7]` for version 7 — an unsupported-version
        // verdict on something that is not a configuration at all, which is
        // precisely the wrong diagnosis this surface exists to prevent. Only a
        // mapping is a configuration document.
        let json = peek_schema_version(&mut serde_json::Deserializer::from_str("[7]"))
            .expect_err("a JSON array is not a configuration document");
        assert!(
            matches!(json, SchemaVersionPeekError::Unreadable(_)),
            "a sequence must not read as a version: {json:?}"
        );

        let yaml = peek_schema_version(serde_yaml::Deserializer::from_str("- 7\n"))
            .expect_err("a YAML sequence is not a configuration document");
        assert!(
            matches!(yaml, SchemaVersionPeekError::Unreadable(_)),
            "a sequence must not read as a version: {yaml:?}"
        );
        let message = std::error::Error::source(&yaml)
            .expect("the deserializer's error is the source")
            .to_string();
        assert!(
            message.contains("a configuration document"),
            "the refusal must say what was expected, in the user's vocabulary: {message}"
        );
        assert!(
            !message.contains("Probe"),
            "no private type name may reach a user-facing message: {message}"
        );
    }

    #[test]
    fn the_deserializers_error_is_the_source_of_an_unreadable_peek() {
        // The caller reaches the format's own diagnosis — file, line, column —
        // through `source`, so the peek adds a verdict without swallowing the
        // detail underneath it.
        let err = peek_schema_version(&mut serde_json::Deserializer::from_str(r#"{"version":"#))
            .expect_err("truncated JSON does not parse");
        let SchemaVersionPeekError::Unreadable(ref inner) = err else {
            panic!("truncated text is unreadable, not an unsupported version: {err:?}");
        };
        let source =
            std::error::Error::source(&err).expect("the deserializer's error is the source");
        assert_eq!(source.to_string(), inner.to_string());
        assert!(
            !source.to_string().is_empty(),
            "a source that says nothing is not a diagnosis"
        );
    }

    #[test]
    fn the_peek_error_type_is_not_bounded_by_the_deserializers_error_type() {
        // `Unreadable` holds its `E` as a `#[source]`, which needs `E: Error`
        // — but `thiserror` puts that bound on the generated `Error` impl and
        // not on the type, so the type itself stays open. The doc comment says
        // so, and this is that claim compiled rather than asserted: `Opaque`
        // implements nothing at all, and the error still exists over it. If
        // this ever stops compiling, the doc comment has gone wrong.
        struct Opaque;
        let err: SchemaVersionPeekError<Opaque> = SchemaVersionPeekError::Unreadable(Opaque);
        assert!(matches!(err, SchemaVersionPeekError::Unreadable(_)));
    }

    #[test]
    fn the_version_is_peeked_without_the_rest_of_the_document_being_interpreted() {
        // The refusal has to land before the rest of the document is given
        // meaning, or a document written in a grammar this build does not know
        // is interpreted on the way to being refused. Here the body is not a
        // `RawGraph` at all, and the peek still reaches its verdict.
        let body = r#""pipeline":{"not-a-graph":true}"#;
        let unsupported = format!(r#"{{"version":7,{body}}}"#);
        let supported = format!(r#"{{"version":{},{body}}}"#, SchemaVersion::SUPPORTED);
        assert!(
            serde_json::from_str::<RawPipeline>(&supported).is_err(),
            "the body must be what a full parse chokes on, not the version, or this \
             test would prove nothing about the body"
        );

        let err = peek_schema_version(&mut serde_json::Deserializer::from_str(&unsupported))
            .expect_err("a version this build cannot read must be refused");
        assert!(
            matches!(err, SchemaVersionPeekError::Unsupported(_)),
            "the version is the verdict, whatever the rest of the document means: {err:?}"
        );

        // The complement: a body the full parse refuses does not stop a
        // supported version from peeking, which is what "uninterpreted" means.
        let peeked = peek_schema_version(&mut serde_json::Deserializer::from_str(&supported))
            .expect("a supported version peeks whatever the body means");
        assert_eq!(peeked, SchemaVersion::CURRENT);
    }

    #[test]
    fn peeking_agrees_with_the_full_parse_on_versions_this_build_reads() {
        // A peek that disagreed with `Deserialize` would refuse documents the
        // parser accepts, or wave through ones it refuses.
        let stated = peek_schema_version(&mut serde_json::Deserializer::from_str(
            r#"{"version":2,"pipeline":{"nodes":[]}}"#,
        ))
        .expect("the supported version must peek");
        assert_eq!(stated, SchemaVersion::CURRENT);

        let absent = peek_schema_version(serde_yaml::Deserializer::from_str(
            "pipeline:\n  nodes: []\n",
        ))
        .expect("an absent version must peek as the version this build writes");
        assert_eq!(absent, SchemaVersion::CURRENT);

        // An explicit null is not an absent version. The full parse refuses it,
        // so the peek must too — a peek that waved it through would report a
        // document readable that the parser then rejects, which is the
        // disagreement this test exists to forbid.
        let null = "version: null\npipeline:\n  nodes: []\n";
        assert!(
            serde_yaml::from_str::<RawPipeline>(null).is_err(),
            "the full parse refuses an explicit null version"
        );
        let err = peek_schema_version(serde_yaml::Deserializer::from_str(null))
            .expect_err("and so must the peek");
        assert!(
            matches!(err, SchemaVersionPeekError::Unreadable(_)),
            "a version that is not a number is unreadable, not unsupported: {err:?}"
        );
    }

    #[test]
    fn the_version_error_names_the_version_found_and_the_version_supported() {
        // The type is what a caller matches on; the message is what a human
        // reads, and it is useless unless it says both which version the
        // configuration states and which one this build reads. 7 is chosen so
        // that the two numbers are distinguishable in the text.
        let found = 7;
        let message = SchemaVersion::new(found).unwrap_err().to_string();
        assert!(
            message.contains(&found.to_string()),
            "the message must name the version found: {message}"
        );
        assert!(
            message.contains(&SchemaVersion::SUPPORTED.to_string()),
            "the message must name the version supported: {message}"
        );
    }

    #[test]
    fn wire_integers_keep_their_full_width() {
        // A narrowed `Int` would silently truncate on the way to
        // `ParamValue::Int(i64)`. No out-of-range Rust literal here, so the
        // narrowing fails this test rather than failing to compile it.
        let parsed: RawParamValue = serde_json::from_str("9223372036854775807").unwrap();
        assert_eq!(
            serde_json::to_string(&parsed).unwrap(),
            "9223372036854775807"
        );
        let parsed: RawParamValue = serde_json::from_str("-9223372036854775808").unwrap();
        assert_eq!(
            serde_json::to_string(&parsed).unwrap(),
            "-9223372036854775808"
        );
    }

    #[test]
    fn non_finite_floats_are_outside_the_documented_contract() {
        // YAML admits `.inf` and `.nan`, and this level parses them — but they
        // do not round trip, so the wire contract stops at finite values, as
        // `ParamValue`'s does. Pinned rather than hidden: this is the boundary
        // `validate` must reject at when it lowers this level.
        let infinite: RawParamValue = serde_yaml::from_str(".inf").unwrap();
        assert_eq!(
            serde_json::to_string(&infinite).unwrap(),
            "null",
            "a non-finite float is not representable in JSON"
        );
        assert!(
            serde_json::from_str::<RawParamValue>("null").is_err(),
            "and it does not read back, so the round trip is finite-only"
        );
        let nan: RawParamValue = serde_yaml::from_str(".nan").unwrap();
        assert_ne!(nan, nan, "NaN costs equality its reflexivity");
    }

    #[test]
    fn a_nested_map_parameter_is_not_representable() {
        // Pinned so the limit is a known one. `ParamValue` has no `Map`
        // variant either, so the wire type mirrors the logical model rather
        // than diverging from it — but a metadata filter is ordinary retriever
        // configuration, and today it fails to parse. See the module docs.
        assert!(
            serde_yaml::from_str::<RawParamValue>("{lang: fr}").is_err(),
            "if this starts parsing, the wire and logical param models have diverged"
        );
    }

    #[test]
    fn the_wire_form_of_a_param_is_not_the_internal_one() {
        // INV-9. This separation is not stylistic: `ParamValue` is externally
        // tagged, so it cannot read what a configuration actually contains.
        assert_eq!(
            serde_json::from_str::<RawParamValue>("50").unwrap(),
            RawParamValue::Int(50)
        );
        assert!(
            serde_json::from_str::<crate::ParamValue>("50").is_err(),
            "if ParamValue ever reads a bare scalar, this crate has two names for one \
             format and INV-9's separation has quietly collapsed"
        );
    }

    #[test]
    fn param_scalars_read_as_their_narrowest_kind() {
        let cases = [
            ("true", RawParamValue::Bool(true)),
            ("50", RawParamValue::Int(50)),
            ("-3", RawParamValue::Int(-3)),
            ("0.75", RawParamValue::Float(0.75)),
            (r#""cosine""#, RawParamValue::String("cosine".to_string())),
            (
                r#"[1,"a"]"#,
                RawParamValue::List(vec![
                    RawParamValue::Int(1),
                    RawParamValue::String("a".to_string()),
                ]),
            ),
        ];
        for (json, expected) in cases {
            assert_eq!(
                serde_json::from_str::<RawParamValue>(json).unwrap(),
                expected,
                "{json} read as the wrong kind"
            );
        }
    }

    #[test]
    fn a_node_may_omit_its_inputs_and_params() {
        let node: RawNode =
            serde_json::from_str(r#"{"id":"t","component":"query_transform","impl":"hyde"}"#)
                .unwrap();
        assert!(node.inputs.is_empty());
        assert!(node.params.is_empty());
        assert_eq!(node.implementation, "hyde");
    }

    #[test]
    fn an_unknown_node_field_is_tolerated() {
        // The permissive level tolerates what it does not know: an unknown
        // key is dropped at the parse, not carried to `validate`. This is the
        // opposite of the validated level, deliberately.
        let node: RawNode = serde_json::from_str(
            r#"{"id":"g","component":"retriever","impl":"bm25","next":"generate"}"#,
        )
        .expect("an unrecognised key must not stop the parse");
        assert_eq!(node.id, "g");
    }

    #[test]
    fn the_wire_form_round_trips() {
        let doc: RawPipeline = serde_json::from_str(
            r#"{"version":2,"pipeline":{"inputs":["question"],"nodes":[{"id":"d","component":"retriever","impl":"bm25","inputs":["t"],"params":{"top_k":50,"alpha":0.5}}]}}"#,
        )
        .unwrap();
        let back: RawPipeline =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(back, doc);
    }
}
