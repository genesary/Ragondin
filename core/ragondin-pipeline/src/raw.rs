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
//! **This level is deliberately permissive.** It tolerates unknown keys,
//! component families with no `LogicalNode` variant, and references to nodes
//! that do not exist. Rejecting those is validation's job (#9), and a parser
//! that pre-empted it would turn a user's diagnosable mistake into an opaque
//! parse failure.
//!
//! Two things it does *not* tolerate, for different reasons.
//!
//! A **schema version it cannot read** is refused: permissiveness means
//! tolerating content one does not understand *within a grammar one does*, and
//! a version bump says the grammar itself may have changed. Continuing there
//! is not leniency, it is misinterpretation.
//!
//! A **parameter that is not a scalar or a list of scalars** — a nested map,
//! a null — fails to parse, and does so with serde's opaque untagged-enum
//! message rather than a diagnosis. That is a genuine limit, not a choice:
//! [`RawParamValue`] mirrors [`crate::ParamValue`], which has no `Map` or
//! `Null` variant either, so admitting one here would put the wire and logical
//! models out of step. It is worth knowing that a metadata filter
//! (`params: { filters: { lang: fr } }`) is ordinary retriever configuration
//! and is not expressible today.
//!
//! Nothing here is executed, and nothing here is hashed.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

/// The version of the wire schema a configuration is written in.
///
/// Absent from a configuration, it reads as [`SchemaVersion::PRE_VERSIONING`]
/// — version 1 predates the field, so a file written before versioning is
/// unambiguous. A configuration written in any later version must say so, and
/// a version this build does not understand is refused rather than guessed at.
///
/// The two constants were equal until ADR-C18 added `inputs` to
/// [`RawGraph`]: a change to the wire schema's shape never leaves this type
/// untouched (INV-9). They have been distinct ever since, which is why an
/// absent version is no longer a supported one — a v1 document cannot declare
/// the input ADR-C18 requires, so reading it as current would reject it for a
/// missing declaration rather than for the version it is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// The only schema version this build can read.
    pub const SUPPORTED: u32 = 2;

    /// The version a configuration written before the `version` field existed
    /// is in. What an absent `version` means, and no longer a version this
    /// build reads — see the note on this type.
    pub const PRE_VERSIONING: u32 = 1;

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
    /// [`SchemaVersion::PRE_VERSIONING`] — what an absent `version` field
    /// means, which is the only thing a default for this type can honestly
    /// be. It is deliberately **not** a version this build reads:
    /// [`crate::validate`] refuses it, so an unversioned document fails on
    /// the version it is written in rather than on a consequence of it.
    fn default() -> Self {
        Self(Self::PRE_VERSIONING)
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
/// (INV-9); lowering one to the other is #9's job.
///
/// Variant order is load-bearing: an untagged enum is tried in declaration
/// order, so `Bool` precedes `Int` precedes `Float`, and `50` reads as an
/// integer rather than as a float.
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
/// `LogicalPipeline` from it is #9.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawPipeline {
    /// The wire schema version. Absent means
    /// [`SchemaVersion::PRE_VERSIONING`], which this build does not read.
    #[serde(default)]
    pub version: SchemaVersion,
    /// The pipeline itself.
    pub pipeline: RawGraph,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_version_reads_as_the_version_that_predates_the_field() {
        let doc: RawPipeline = serde_json::from_str(r#"{"pipeline":{"nodes":[]}}"#).unwrap();
        assert_eq!(doc.version, SchemaVersion::default());
        assert_eq!(doc.version.get(), SchemaVersion::PRE_VERSIONING);
        assert_eq!(doc.version.get(), 1);
    }

    #[test]
    fn the_version_that_predates_the_field_is_no_longer_one_this_build_reads() {
        // ADR-C18 added `inputs` to the wire schema, so the two constants
        // parted company (INV-9). Stating version 1 is refused at
        // deserialization; an *absent* version reaches `validate` instead,
        // which is where it is refused.
        assert_ne!(SchemaVersion::PRE_VERSIONING, SchemaVersion::SUPPORTED);
        let err = SchemaVersion::new(SchemaVersion::PRE_VERSIONING).unwrap_err();
        assert_eq!(err.found(), 1);
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
    fn the_version_error_wording_is_pinned() {
        // #27 has no better way to tell "this config needs a newer build" from
        // "this YAML is broken": `serde::de::Error::custom` erases the type
        // into a string. Until that is addressed, the wording is the contract,
        // so a reword has to break a test rather than break `ragondin-config`.
        assert_eq!(
            SchemaVersion::new(3).unwrap_err().to_string(),
            "unsupported pipeline schema version 3: this build reads version 2"
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
        // #9 must reject at.
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
        // The permissive level tolerates what it does not know; rejecting is #9's
        // job. This is the opposite of the validated level, deliberately.
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
