//! [`PipelineHash`] and [`LogicalPipeline::content_hash`]: content addressing
//! over the canonical logical form (INV-8).
//!
//! The hash is computed from a [`LogicalPipeline`]'s fields directly — never
//! from source text, and never from a re-serialization of it. Two
//! configurations that differ only in formatting canonicalize to one
//! [`LogicalPipeline`] ([`crate::validate::validate`] sorts the node list; the
//! rest is canonical by construction), and one `LogicalPipeline` has one
//! digest. That chain is what makes INV-8 true, and every link of it is
//! tested: the encoder's framing here, the whole path from YAML in
//! `tests/content_hash.rs`.
//!
//! This is the *pipeline_config* component of run identity
//! (`docs/system-architecture.md` §7.1), and only that component. The rest of
//! the tuple — dataset, index, model and engine versions — is assembled by the
//! harness, which is where the other four are known.
//!
//! # Why a hand-written encoder
//!
//! A digest is only as canonical as the bytes fed to it, and a general-purpose
//! serializer canonicalizes nothing it was not asked to. `serde_json` renders
//! a float by a shortest-round-trip rule, orders a map by whatever the map
//! orders by, and would put this crate's identity at the mercy of a dependency
//! whose output is documented as readable, not as stable. The encoding below
//! is instead a property of this file, which is what the pinned digest in
//! `tests/content_hash.rs` guards.
//!
//! # The encoding
//!
//! A tagged, length-prefixed byte stream, fed to SHA-256 in one pass:
//!
//! - It opens with a domain separator, so this digest is a digest *of a
//!   pipeline* and cannot collide with some other structure that happens to
//!   encode to the same bytes.
//! - Every string and every sequence is prefixed with its length as a
//!   little-endian `u64`. Length prefixes are what make the encoding
//!   **injective**: without them a node whose id is `ab` and whose `impl` is
//!   `c` would feed the hasher the same bytes as one with id `a` and `impl`
//!   `bc`.
//! - Every enum is prefixed with a tag byte that is unique within its enum, so
//!   a [`ParamValue::Int`] never encodes as a [`ParamValue::Bool`] and a
//!   [`LogicalNode::Fusion`] never as a [`LogicalNode::Retriever`]. The tags
//!   are written down as constants because their *values* are part of the
//!   digest: renumbering one is as breaking as changing the canonical form.
//!
//! Injectivity is the whole requirement. Two distinct canonical logical forms
//! must produce two distinct byte streams; SHA-256 supplies the rest.
//!
//! # What the encoder relies on, and does not re-check
//!
//! It hashes what [`crate::validate::validate`] returns. Two properties of the
//! canonical form are therefore assumed rather than enforced here:
//!
//! - **Every [`ParamValue::Float`] is finite**, rejected otherwise during
//!   lowering ([`crate::ValidationError::NonFiniteParam`]).
//! - **`-0.0` has already been folded into `0.0`**, also during lowering. The
//!   encoder hashes `f64::to_bits` verbatim, which distinguishes the two bit
//!   patterns; `node.rs` states that the normalization belongs before the hash
//!   rather than inside it, and that is where it is.
//!
//! **ADR-C23 does not close this, and it says so.** It routes `Deserialize`
//! through the *structural* checks — the node sort and the six graph checks —
//! and its Consequences state that the lowering-only variants of
//! [`crate::ValidationError`] (`UnknownComponent`, `NonFiniteParam`) are
//! unreachable from that second path. Lowering is where both float properties
//! are established, so after that decision lands they remain a convention
//! about provenance rather than a property of the type: a `LogicalPipeline`
//! obtained by deserializing a logical-shaped document may still hold
//! `Float(-0.0)`, and two such values that this crate calls equal
//! (`Float(0.0) == Float(-0.0)`) then carry two digests.
//!
//! That is INV-8's failure mode, reachable through a public path, and it is
//! named here rather than papered over. It is not reachable through
//! [`crate::validate::validate`], which is the only door today and the one
//! this method is specified against; normalizing inside the encoder instead
//! would contradict `node.rs`, which places the normalization before the hash
//! deliberately, so moving it is a decision this issue does not own. The
//! residual gap belongs to ADR-C23's implementation (#179), and is raised
//! there.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::node::{
    ExtensionNode, FusionNode, LogicalNode, NodeId, ParamValue, Params, RerankerNode, RetrieverNode,
};
use crate::pipeline::LogicalPipeline;

/// The domain separator opening every encoding.
///
/// Its `-v1` names the *encoding*, not the wire schema: a change to either the
/// canonical form or the framing below changes every digest this crate
/// produces, and bumping this string is how that becomes visible rather than
/// silent. It is unrelated to [`crate::SchemaVersion`], which the logical form
/// deliberately does not carry.
const DOMAIN: &[u8] = b"ragondin-pipeline:logical-v1";

/// Tag bytes for [`LogicalNode`]'s variants. Part of the digest: renumbering
/// one changes every hash.
const TAG_RETRIEVER: u8 = 0x01;
const TAG_FUSION: u8 = 0x02;
const TAG_RERANKER: u8 = 0x03;
const TAG_EXTENSION: u8 = 0x04;

/// Tag bytes for [`ParamValue`]'s variants. Numbered in their own space,
/// which is unambiguous because a param value is only ever read where one is
/// expected.
const TAG_STRING: u8 = 0x01;
const TAG_INT: u8 = 0x02;
const TAG_FLOAT: u8 = 0x03;
const TAG_BOOL: u8 = 0x04;
const TAG_LIST: u8 = 0x05;

/// The content address of a canonical [`LogicalPipeline`]: a SHA-256 digest.
///
/// Renders and serializes as 64 lowercase hex digits, which is what a run id
/// (`docs/system-architecture.md` §7.1) is built from and what a user sees.
///
/// `Eq` and `Hash` are derived so this can key the content-addressed run cache
/// directly. There is deliberately no `Ord`: a digest has no meaningful order,
/// and sorting runs by one would be sorting by noise.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipelineHash([u8; 32]);

impl PipelineHash {
    /// The raw digest.
    ///
    /// This is what a *composite* hash folds in — run identity (§7.1) hashes
    /// this pipeline hash together with the dataset, index, model and engine
    /// versions — and folding in the bytes keeps that composition independent
    /// of how this type happens to render.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for PipelineHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for PipelineHash {
    /// Hand-written, because the derive would print 32 decimal numbers — and
    /// `Debug` is what an `assert_eq!` between two hashes reports.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PipelineHash({self})")
    }
}

impl Serialize for PipelineHash {
    /// Writes the hex form, not the 32 bytes: a digest in a run store is read
    /// by people as often as by programs.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PipelineHash {
    /// Reads back exactly what [`Display`](fmt::Display) writes, and nothing
    /// else. Uppercase is refused along with everything else malformed:
    /// accepting two spellings of one digest on a content-addressed boundary
    /// is the trap INV-8 exists to avoid, one level up.
    ///
    /// The failure is reported through `serde::de::Error::custom`, following
    /// [`crate::SchemaVersion`]: it keeps the wording and erases the type, and
    /// a caller that could act on a distinct error type has nothing to do
    /// with one here anyway.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;

        let hex = String::deserialize(deserializer)?;
        let bytes = hex.as_bytes();
        if bytes.len() != 64 {
            // Reported in bytes, because bytes is what was checked: `é` is one
            // character and two bytes, and a message that called a rejected
            // string the right length would be worse than no message.
            return Err(D::Error::custom(format!(
                "a pipeline hash is 64 lowercase hex digits, found {} bytes",
                bytes.len()
            )));
        }

        let mut digest = [0u8; 32];
        for (byte, pair) in digest.iter_mut().zip(bytes.chunks_exact(2)) {
            let mut value = 0u8;
            for &digit in pair {
                let nibble = match digit {
                    b'0'..=b'9' => digit - b'0',
                    b'a'..=b'f' => digit - b'a' + 10,
                    _ => {
                        return Err(D::Error::custom(
                            "a pipeline hash is 64 lowercase hex digits",
                        ))
                    }
                };
                value = value << 4 | nibble;
            }
            *byte = value;
        }
        Ok(Self(digest))
    }
}

impl LogicalPipeline {
    /// The content address of this pipeline: a digest over the **canonical
    /// logical form**, never over source text (INV-8).
    ///
    /// Two configurations that differ only in node order, param key order or
    /// whitespace reduce to one `LogicalPipeline` and so to one hash. Two that
    /// differ in anything the engine would act on — a node id, an `impl:`
    /// value, a param, the wiring, the order of a node's `inputs` — reduce to
    /// two.
    ///
    /// What is hashed is exactly the canonicalization contract on this type:
    /// the declared [`inputs`](LogicalPipeline::inputs) in declaration order
    /// and the [`nodes`](LogicalPipeline::nodes) in canonical order, each node
    /// by its variant, id, implementation, `inputs` **in port order**, and
    /// params in sorted key order. Nothing else — no port kinds (they are
    /// derived from the variant, ADR-C16) and no [`crate::SchemaVersion`]
    /// (which says what this build can read, not what pipeline this is).
    pub fn content_hash(&self) -> PipelineHash {
        let mut hasher = Sha256::new();
        hasher.update(DOMAIN);

        // The declared inputs (ADR-C18), in declaration order: they are the
        // graph's signature, positional exactly as a node's `inputs` are, and
        // sorting them here would change the configuration rather than
        // canonicalize it.
        feed_ids(&mut hasher, self.inputs());

        feed_len(&mut hasher, self.nodes().len());
        for node in self.nodes() {
            feed_node(&mut hasher, node);
        }

        PipelineHash(hasher.finalize().into())
    }
}

/// Feeds a length as a little-endian `u64` — the prefix that makes the
/// encoding injective.
fn feed_len(hasher: &mut Sha256, len: usize) {
    hasher.update((len as u64).to_le_bytes());
}

fn feed_str(hasher: &mut Sha256, value: &str) {
    feed_len(hasher, value.len());
    hasher.update(value.as_bytes());
}

fn feed_ids(hasher: &mut Sha256, ids: &[NodeId]) {
    feed_len(hasher, ids.len());
    for id in ids {
        feed_str(hasher, id.as_str());
    }
}

/// Feeds one node: its variant tag, then the fields that variant carries, in a
/// fixed order.
///
/// `implementation` and `kind` occupy the same slot, which is unambiguous
/// because the tag already fixed which one it is — an `Extension` named `hyde`
/// and a `Retriever` whose `impl` is `hyde` differ in their first byte.
fn feed_node(hasher: &mut Sha256, node: &LogicalNode) {
    // Destructured rather than bound whole, and that is load-bearing: a field
    // added to any of these structs is then a compile error here, at the one
    // place that must account for it. Bound whole, it would compile, warn
    // nothing, and silently stay out of the content hash — which no test could
    // catch, since the pinned digest does not move for a field never encoded.
    let (tag, id, name, inputs, params) = match node {
        LogicalNode::Retriever(RetrieverNode {
            id,
            implementation,
            inputs,
            params,
        }) => (TAG_RETRIEVER, id, implementation, inputs, params),
        LogicalNode::Fusion(FusionNode {
            id,
            implementation,
            inputs,
            params,
        }) => (TAG_FUSION, id, implementation, inputs, params),
        LogicalNode::Reranker(RerankerNode {
            id,
            implementation,
            inputs,
            params,
        }) => (TAG_RERANKER, id, implementation, inputs, params),
        LogicalNode::Extension(ExtensionNode {
            id,
            kind,
            inputs,
            params,
        }) => (TAG_EXTENSION, id, kind, inputs, params),
    };

    hasher.update([tag]);
    feed_str(hasher, id.as_str());
    feed_str(hasher, name);
    // In port order (ADR-C16): a node's consumed kinds are derived
    // positionally, so two orderings of the same legs are two configurations.
    feed_ids(hasher, inputs);
    feed_params(hasher, params);
}

/// Feeds a parameter map in sorted key order — which is simply iteration
/// order, `Params` being a `BTreeMap`.
fn feed_params(hasher: &mut Sha256, params: &Params) {
    feed_len(hasher, params.len());
    for (key, value) in params {
        feed_str(hasher, key);
        feed_param_value(hasher, value);
    }
}

fn feed_param_value(hasher: &mut Sha256, value: &ParamValue) {
    match value {
        ParamValue::String(value) => {
            hasher.update([TAG_STRING]);
            feed_str(hasher, value);
        }
        ParamValue::Int(value) => {
            hasher.update([TAG_INT]);
            hasher.update(value.to_le_bytes());
        }
        ParamValue::Float(value) => {
            hasher.update([TAG_FLOAT]);
            // `to_bits` verbatim: finite and sign-normalized is established by
            // lowering, per this module's documentation. A `Float` and an
            // `Int` of equal value differ in their tag, which is the point of
            // having one.
            hasher.update(value.to_bits().to_le_bytes());
        }
        ParamValue::Bool(value) => {
            hasher.update([TAG_BOOL]);
            hasher.update([u8::from(*value)]);
        }
        ParamValue::List(values) => {
            hasher.update([TAG_LIST]);
            // Order within a list is part of the value.
            feed_len(hasher, values.len());
            for value in values {
                feed_param_value(hasher, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retriever(id: &str, implementation: &str, inputs: &[&str], params: Params) -> LogicalNode {
        LogicalNode::Retriever(RetrieverNode {
            id: NodeId::new(id),
            implementation: implementation.to_string(),
            inputs: inputs.iter().copied().map(NodeId::new).collect(),
            params,
        })
    }

    fn pipeline(inputs: &[&str], nodes: Vec<LogicalNode>) -> LogicalPipeline {
        LogicalPipeline::new(inputs.iter().copied().map(NodeId::new).collect(), nodes)
    }

    /// A one-key parameter map.
    fn param(key: &str, value: ParamValue) -> Params {
        [(key.to_string(), value)].into_iter().collect()
    }

    #[test]
    fn the_encoding_is_injective_across_a_field_boundary() {
        // Without length prefixes these two feed the hasher the same bytes:
        // "ab" + "c" and "a" + "bc". This is the test that fails if a
        // `feed_len` is ever dropped as redundant.
        let left = pipeline(&["q"], vec![retriever("ab", "c", &["q"], Params::new())]);
        let right = pipeline(&["q"], vec![retriever("a", "bc", &["q"], Params::new())]);
        assert_ne!(
            left.content_hash(),
            right.content_hash(),
            "adjacent fields must not run together in the encoding"
        );
    }

    #[test]
    fn the_encoding_is_injective_across_a_sequence_boundary() {
        // One node with inputs [a, b] against one with inputs [a] and a
        // declaration of [b]: the same strings, differently grouped.
        let left = pipeline(
            &["q"],
            vec![retriever("r", "bm25", &["q", "x"], Params::new())],
        );
        let right = pipeline(
            &["q", "x"],
            vec![retriever("r", "bm25", &[], Params::new())],
        );
        assert_ne!(
            left.content_hash(),
            right.content_hash(),
            "sequence lengths must be part of the encoding"
        );
    }

    #[test]
    fn a_nodes_variant_is_part_of_the_hash() {
        // Same id, same name, same wiring — different kind of node. Only the
        // tag byte distinguishes them.
        let as_retriever = pipeline(&["q"], vec![retriever("n", "x", &["q"], Params::new())]);
        let as_fusion = pipeline(
            &["q"],
            vec![LogicalNode::Fusion(FusionNode {
                id: NodeId::new("n"),
                implementation: "x".to_string(),
                inputs: vec![NodeId::new("q")],
                params: Params::new(),
            })],
        );
        let as_extension = pipeline(
            &["q"],
            vec![LogicalNode::Extension(ExtensionNode {
                id: NodeId::new("n"),
                kind: "x".to_string(),
                inputs: vec![NodeId::new("q")],
                params: Params::new(),
            })],
        );
        assert_ne!(as_retriever.content_hash(), as_fusion.content_hash());
        assert_ne!(as_retriever.content_hash(), as_extension.content_hash());
        assert_ne!(as_fusion.content_hash(), as_extension.content_hash());
    }

    #[test]
    fn the_declared_inputs_are_hashed_in_declaration_order() {
        // The canonicalization contract on `LogicalPipeline` says the
        // declaration is never sorted or deduplicated. `validate` permits
        // exactly one input today, so this rule is unobservable through the
        // public path — which is exactly why it is pinned here, against the
        // day arity relaxes and a "tidy the canonical form" change sorts
        // them.
        let forward = pipeline(&["a", "b"], Vec::new());
        let reverse = pipeline(&["b", "a"], Vec::new());
        assert_ne!(
            forward.content_hash(),
            reverse.content_hash(),
            "declared inputs are positional, like a node's"
        );

        let deduplicated = pipeline(&["a"], Vec::new());
        let repeated = pipeline(&["a", "a"], Vec::new());
        assert_ne!(
            deduplicated.content_hash(),
            repeated.content_hash(),
            "declared inputs are never deduplicated"
        );
    }

    #[test]
    fn every_param_variant_is_distinguished_by_its_tag() {
        // A tag collision would make two grammars one. `String("1")`,
        // `Int(1)`, `Float(1.0)` and `Bool(true)` are four configurations.
        let values = [
            ParamValue::String("1".to_string()),
            ParamValue::Int(1),
            ParamValue::Float(1.0),
            ParamValue::Bool(true),
            ParamValue::List(vec![ParamValue::Int(1)]),
        ];
        let hashes: Vec<PipelineHash> = values
            .iter()
            .map(|value| {
                pipeline(
                    &["q"],
                    vec![retriever("r", "bm25", &["q"], param("p", value.clone()))],
                )
                .content_hash()
            })
            .collect();
        for (i, left) in hashes.iter().enumerate() {
            for right in &hashes[i + 1..] {
                assert_ne!(left, right, "two param variants collided: {values:?}");
            }
        }
    }

    #[test]
    fn a_list_params_order_is_part_of_the_value() {
        let forward = ParamValue::List(vec![
            ParamValue::String("title".to_string()),
            ParamValue::String("body".to_string()),
        ]);
        let reverse = ParamValue::List(vec![
            ParamValue::String("body".to_string()),
            ParamValue::String("title".to_string()),
        ]);
        let hash_of = |value: ParamValue| {
            pipeline(
                &["q"],
                vec![retriever("r", "bm25", &["q"], param("fields", value))],
            )
            .content_hash()
        };
        assert_ne!(hash_of(forward), hash_of(reverse));
    }

    #[test]
    fn an_absent_param_and_a_present_one_differ() {
        // ADR-C22 rejects a null parameter because *absent* already has a
        // spelling. That only holds if the two hash differently.
        let absent = pipeline(&["q"], vec![retriever("r", "bm25", &["q"], Params::new())]);
        let present = pipeline(
            &["q"],
            vec![retriever(
                "r",
                "bm25",
                &["q"],
                param("p", ParamValue::String(String::new())),
            )],
        );
        assert_ne!(absent.content_hash(), present.content_hash());
    }

    #[test]
    fn a_hash_is_a_pure_function_of_the_value() {
        // Two independently built equal values, hashed twice each: nothing
        // about a hash may depend on address, allocation or call order. This
        // is the in-process half of "stable across runs"; the pinned literal
        // in `tests/content_hash.rs` is the other half.
        let build = || {
            pipeline(
                &["q"],
                vec![retriever(
                    "r",
                    "bm25",
                    &["q"],
                    param("k", ParamValue::Int(10)),
                )],
            )
        };
        let (first, second) = (build(), build());
        assert_eq!(first, second, "the fixtures must be equal values");
        assert_eq!(first.content_hash(), first.content_hash());
        assert_eq!(first.content_hash(), second.content_hash());
    }

    #[test]
    fn the_empty_pipeline_has_a_hash() {
        // Degenerate but representable: `LogicalPipeline::new` is the door
        // the test above uses, and an encoder that indexed before checking a
        // length would panic here rather than answer.
        let hash = pipeline(&[], Vec::new()).content_hash();
        assert_eq!(hash.to_string().len(), 64);
        assert_ne!(
            hash,
            pipeline(&["q"], Vec::new()).content_hash(),
            "an empty declaration is not the same pipeline as a one-input one"
        );
    }

    #[test]
    fn as_bytes_agrees_with_the_rendered_form() {
        // Run identity (§7.1) folds in `as_bytes`; a user reads `Display`.
        // The two must be the same digest.
        let hash = pipeline(&["q"], Vec::new()).content_hash();
        let rendered: String = hash.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(rendered, hash.to_string());
    }
}
