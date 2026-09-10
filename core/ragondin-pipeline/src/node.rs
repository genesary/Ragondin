//! The logical node model: the validated, in-memory node types.
//!
//! Per `docs/code-architecture.md` §6.2 the representation is a **closed enum
//! of primitive nodes plus one open `Extension` variant** (ADR-C3), so a new
//! technique can be added without changing the core. Nodes are value types
//! (INV-3): plain data, no trait objects, no I/O. Edges are data flow — a node
//! names in `inputs` the ids of the values it consumes: another node's output,
//! or one of the pipeline's declared inputs (ADR-C18).
//!
//! **`inputs` is positional and order-significant.** ADR-C16 derives a node's
//! consumed kinds from its variant, and a variant with heterogeneous ports — a
//! reranker consumes a query *and* a chunk list — can only address them by
//! position. Canonicalization (#10) must therefore **never reorder `inputs`**:
//! two orderings of the same legs are two configurations, even where the
//! component itself is commutative.
//!
//! **The variant is the sole source of a node's port kinds.** ADR-C16 derives
//! the `ValueKind` a node produces and consumes by matching on its
//! `LogicalNode` variant, so no port declaration ever appears in a
//! configuration and nothing enters the content hash (INV-8). A node type whose
//! output kind depended on its `params` or on its `implementation` string would
//! make that derivation impossible; do not introduce one.
//!
//! **On `serde` here.** These types derive `Serialize`/`Deserialize` for
//! internal round-tripping only. This is **not** the wire format: per INV-9 the
//! wire format is hand-maintained and versioned separately, in
//! `ragondin-config`/`ragondin-proto`.
//!
//! `Branch` and `Loop` (§6.2) are deliberately absent: [`LogicalNode`] reserves
//! no variant for either today. Control flow in the representation is settled
//! in principle — ADR-2 decides that the representation is a graph with
//! first-class branches and bounded loops — but a `Loop`'s mandatory
//! termination guard and a `Branch`'s predicate have no settled *shape*, and
//! inventing one here would decide it by accident. Adding either variant is a
//! deliberate act on a stable boundary (INV-1), so it waits for the issue that
//! owns that decision.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The stable identifier of a node within a pipeline — and, since ADR-C18, of
/// a declared pipeline input, which is not a node.
///
/// The two share **one namespace**, which is why this type is also the element
/// of [`crate::LogicalPipeline::inputs`] even though none of those elements is
/// a node: a node names a declared input in `inputs` exactly as it names
/// another node, so an id claimed by both would be ambiguous and validation
/// refuses it ([`crate::ValidationError::InputCollidesWithNode`]).
///
/// Edges are expressed by id: a node lists in its `inputs` the ids of what it
/// consumes — another node, whose output it takes, or one of the pipeline's
/// declared inputs.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Wraps an identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrows the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A single component parameter value.
///
/// Deliberately a small owned enum rather than `serde_json::Value`: the latter's
/// float and map ordering is not canonical, which would undermine the content
/// hash (INV-8) that #10 computes over the canonical logical form.
///
/// `ParamValue` implements no total order, because `f64` admits none. Canonical
/// ordering of *keys* comes from the [`Params`] `BTreeMap`, whose keys iterate
/// in sorted order regardless of insertion order. Canonicalizing the *values*
/// is a separate obligation, discharged as follows.
///
/// # The grammar
///
/// A parameter value is `String | Int | Float | Bool | List`, and those five
/// are the whole grammar (ADR-C22). Two shapes a configuration might reach for
/// are rejected, for different reasons and with different lifetimes:
///
/// - A **nested map** — a metadata filter, say — is rejected *for now*, not
///   forever. This enum is extensible by design, so the `Map` variant a real
///   configuration eventually demands is additive rather than breaking, and it
///   waits for that demand rather than being guessed at here.
/// - A **null** is rejected permanently. It means *absent*, which [`Params`]
///   already expresses by omitting the key, and two spellings of one
///   configuration on a content-addressed boundary (INV-8) is a trap rather
///   than a convenience.
///
/// This enum carries no `#[non_exhaustive]` today. ADR-C22 decides that it
/// should — the attribute, and the diagnostic that names a refused parameter's
/// key, land with that decision's implementation.
///
/// # The float contract
///
/// [`ParamValue::Float`] holds an IEEE-754 double, and **only finite values are
/// within contract**:
///
/// - **Non-finite values (`NaN`, `±∞`) are out of contract.** They do not round
///   trip — JSON encodes them as `null` and then refuses to read them back —
///   and `NaN` costs `PartialEq` its reflexivity, so a pipeline holding one
///   stops comparing equal to itself. `ParamValue` is a value type with public
///   variants (INV-3) and so has no constructor to guard; the rejection
///   therefore sits one level up, in this crate's own lowering pass, where
///   [`validate`](mod@crate::validate) refuses a non-finite float with
///   [`crate::ValidationError::NonFiniteParam`].
/// - **`-0.0` is canonicalized to `0.0`**, by that same lowering pass.
///   `Float(0.0) == Float(-0.0)` here, but their bit patterns differ, so
///   hashing a raw `to_bits()` would give two content hashes to two values this
///   crate calls equal — precisely INV-8's failure mode, which is why the
///   normalization happens before the hash (#10) rather than inside it.
/// - **`Int` and `Float` are distinct**, deliberately: `k: 60` and `k: 60.0`
///   are different configurations and hash differently.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ParamValue {
    /// A text value, e.g. a similarity metric name.
    String(String),
    /// A whole number, e.g. `top_k`.
    Int(i64),
    /// A finite IEEE-754 double — see the float contract above.
    Float(f64),
    /// A flag.
    Bool(bool),
    /// An ordered sequence; order is significant and preserved by hashing.
    List(Vec<ParamValue>),
}

/// Parameters of a node, keyed by name in canonical (sorted) order.
pub type Params = BTreeMap<String, ParamValue>;

/// A retrieval node: it retrieves candidate chunks for a query.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetrieverNode {
    /// This node's identifier, unique within the pipeline.
    pub id: NodeId,
    /// The `impl:` value naming the component to resolve, e.g. `"bm25"`.
    ///
    /// Part of the logical form, so it enters the content hash: two backends
    /// are two different configurations (ADR-C2 § Amendments).
    pub implementation: String,
    /// The ids of what this node consumes — another node, whose output it
    /// takes, or one of the pipeline's declared inputs (ADR-C18) — **in port
    /// order**.
    pub inputs: Vec<NodeId>,
    /// This node's parameters, in canonical key order.
    pub params: Params,
}

/// A fusion node: it merges several retrieval results into one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FusionNode {
    /// This node's identifier, unique within the pipeline.
    pub id: NodeId,
    /// The `impl:` value naming the component to resolve, e.g. `"bm25"`.
    ///
    /// Part of the logical form, so it enters the content hash: two backends
    /// are two different configurations (ADR-C2 § Amendments).
    pub implementation: String,
    /// The ids of what this node consumes — another node, whose output it
    /// takes, or one of the pipeline's declared inputs (ADR-C18) — **in port
    /// order**.
    pub inputs: Vec<NodeId>,
    /// This node's parameters, in canonical key order.
    pub params: Params,
}

/// A reranking node: it reorders retrieved chunks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RerankerNode {
    /// This node's identifier, unique within the pipeline.
    pub id: NodeId,
    /// The `impl:` value naming the component to resolve, e.g. `"bm25"`.
    ///
    /// Part of the logical form, so it enters the content hash: two backends
    /// are two different configurations (ADR-C2 § Amendments).
    pub implementation: String,
    /// The ids of what this node consumes — another node, whose output it
    /// takes, or one of the pipeline's declared inputs (ADR-C18) — **in port
    /// order**.
    pub inputs: Vec<NodeId>,
    /// This node's parameters, in canonical key order.
    pub params: Params,
}

/// A node defined outside the core — the escape hatch of ADR-C3.
///
/// A researcher who invents a genuinely new node type expresses it here,
/// without modifying the primitive enum. Repeated use of `Extension` for the
/// same shape is the signal to promote that shape to a primitive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtensionNode {
    /// This node's identifier, unique within the pipeline.
    pub id: NodeId,
    /// Names the *extension node type*, e.g. `"hyde"`.
    ///
    /// Unrelated to ADR-C16's `ValueKind`, which names what travels along an
    /// edge. An extension's port kinds are unknown to the core by construction,
    /// and are resolved at physical planning. *How* an extension is looked up
    /// there is not settled — §8.1 describes one registry per component family,
    /// keyed on the implementation name, and there is no extension family — so
    /// this type deliberately says nothing about it.
    pub kind: String,
    /// The ids of what this node consumes — another node, whose output it
    /// takes, or one of the pipeline's declared inputs (ADR-C18) — **in port
    /// order**.
    pub inputs: Vec<NodeId>,
    /// This node's parameters, in canonical key order.
    pub params: Params,
}

/// A node of a validated pipeline: closed over the primitives, open through
/// [`ExtensionNode`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LogicalNode {
    /// Retrieves candidate chunks.
    Retriever(RetrieverNode),
    /// Merges several retrieval results into one.
    Fusion(FusionNode),
    /// Reorders retrieved chunks.
    Reranker(RerankerNode),
    /// A node defined outside the core (ADR-C3).
    Extension(ExtensionNode),
}

impl LogicalNode {
    /// The node's identifier, whatever its variant.
    pub fn id(&self) -> &NodeId {
        match self {
            Self::Retriever(node) => &node.id,
            Self::Fusion(node) => &node.id,
            Self::Reranker(node) => &node.id,
            Self::Extension(node) => &node.id,
        }
    }

    /// The ids of what this node consumes: another node, whose output it
    /// takes, or one of the pipeline's declared inputs (ADR-C18).
    pub fn inputs(&self) -> &[NodeId] {
        match self {
            Self::Retriever(node) => &node.inputs,
            Self::Fusion(node) => &node.inputs,
            Self::Reranker(node) => &node.inputs,
            Self::Extension(node) => &node.inputs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap};

    fn params(pairs: &[(&str, ParamValue)]) -> Params {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn node_ids_expose_their_inner_value() {
        let id = NodeId::new("bm25_leg");
        assert_eq!(id.as_str(), "bm25_leg");
    }

    #[test]
    fn node_ids_encode_as_bare_strings() {
        let encoded = serde_json::to_string(&NodeId::new("dense_leg")).unwrap();
        assert_eq!(encoded, r#""dense_leg""#);
    }

    #[test]
    fn param_values_round_trip_through_serde() {
        let cases = vec![
            ParamValue::String("cosine".to_string()),
            ParamValue::Int(-42),
            ParamValue::Float(0.75),
            ParamValue::Bool(true),
            ParamValue::List(vec![
                ParamValue::String("title".to_string()),
                ParamValue::Int(3),
            ]),
        ];
        for case in cases {
            let json = serde_json::to_string(&case).unwrap();
            let back: ParamValue = serde_json::from_str(&json).unwrap();
            assert_eq!(back, case, "round trip lost information for {case:?}");
        }
    }

    #[test]
    fn params_iterate_in_canonical_key_order() {
        // #10 hashes the canonical logical form (INV-8): the same params
        // inserted in a different order must iterate identically.
        let forward = params(&[
            ("alpha", ParamValue::Int(1)),
            ("beta", ParamValue::Int(2)),
            ("gamma", ParamValue::Int(3)),
        ]);
        let reverse = params(&[
            ("gamma", ParamValue::Int(3)),
            ("beta", ParamValue::Int(2)),
            ("alpha", ParamValue::Int(1)),
        ]);
        let keys: Vec<&String> = forward.keys().collect();
        assert_eq!(keys, vec!["alpha", "beta", "gamma"]);
        assert_eq!(
            forward.keys().collect::<Vec<_>>(),
            reverse.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_retriever_reads_from_its_documented_shape() {
        let node = RetrieverNode {
            id: NodeId::new("bm25_leg"),
            implementation: "bm25".to_string(),
            inputs: vec![NodeId::new("question")],
            params: params(&[("k", ParamValue::Int(10))]),
        };
        assert_eq!(node.id.as_str(), "bm25_leg");
        assert_eq!(node.implementation, "bm25");
        assert_eq!(node.inputs, vec![NodeId::new("question")]);
        assert_eq!(node.params["k"], ParamValue::Int(10));
    }

    #[test]
    fn a_fusion_reads_from_its_documented_shape() {
        let node = FusionNode {
            id: NodeId::new("rrf"),
            implementation: "reciprocal_rank_fusion".to_string(),
            inputs: vec![NodeId::new("bm25_leg"), NodeId::new("dense_leg")],
            params: params(&[("k", ParamValue::Float(60.0))]),
        };
        assert_eq!(node.id.as_str(), "rrf");
        assert_eq!(node.inputs.len(), 2);
        assert_eq!(node.params["k"], ParamValue::Float(60.0));
    }

    #[test]
    fn a_reranker_reads_from_its_documented_shape() {
        let node = RerankerNode {
            id: NodeId::new("cross_encoder"),
            implementation: "bge_reranker".to_string(),
            // Two ports, in order: the query, then the chunks to reorder. The
            // module documentation derives consumed kinds positionally.
            inputs: vec![NodeId::new("question"), NodeId::new("rrf")],
            params: params(&[("top_n", ParamValue::Int(5))]),
        };
        assert_eq!(node.id.as_str(), "cross_encoder");
        assert_eq!(
            node.inputs,
            vec![NodeId::new("question"), NodeId::new("rrf")]
        );
    }

    #[test]
    fn an_extension_reads_from_its_documented_shape() {
        let node = ExtensionNode {
            id: NodeId::new("my_technique"),
            kind: "hyde".to_string(),
            inputs: vec![NodeId::new("question")],
            params: params(&[("prompt", ParamValue::String("expand".to_string()))]),
        };
        assert_eq!(node.id.as_str(), "my_technique");
        assert_eq!(node.kind, "hyde");
    }

    #[test]
    fn logical_node_dispatches_id_and_inputs_across_every_variant() {
        let nodes = [
            LogicalNode::Retriever(RetrieverNode {
                id: NodeId::new("r"),
                implementation: "bm25".to_string(),
                inputs: vec![NodeId::new("q")],
                params: Params::new(),
            }),
            LogicalNode::Fusion(FusionNode {
                id: NodeId::new("f"),
                implementation: "rrf".to_string(),
                inputs: vec![NodeId::new("r")],
                params: Params::new(),
            }),
            LogicalNode::Reranker(RerankerNode {
                id: NodeId::new("k"),
                implementation: "bge".to_string(),
                inputs: vec![NodeId::new("f")],
                params: Params::new(),
            }),
            LogicalNode::Extension(ExtensionNode {
                id: NodeId::new("x"),
                kind: "hyde".to_string(),
                inputs: vec![NodeId::new("k")],
                params: Params::new(),
            }),
        ];
        let ids: Vec<&str> = nodes.iter().map(|n| n.id().as_str()).collect();
        assert_eq!(ids, vec!["r", "f", "k", "x"]);
        let inputs: Vec<&str> = nodes.iter().map(|n| n.inputs()[0].as_str()).collect();
        assert_eq!(inputs, vec!["q", "r", "f", "k"]);
    }

    #[test]
    fn a_hybrid_graph_wires_two_retrievers_into_a_fusion() {
        let lexical = RetrieverNode {
            id: NodeId::new("bm25_leg"),
            implementation: "bm25".to_string(),
            inputs: vec![NodeId::new("question")],
            params: params(&[("k", ParamValue::Int(50))]),
        };
        let dense = RetrieverNode {
            id: NodeId::new("dense_leg"),
            implementation: "dense".to_string(),
            inputs: vec![NodeId::new("question")],
            params: params(&[("k", ParamValue::Int(50))]),
        };
        let fusion = FusionNode {
            id: NodeId::new("rrf"),
            implementation: "reciprocal_rank_fusion".to_string(),
            inputs: vec![lexical.id.clone(), dense.id.clone()],
            params: Params::new(),
        };
        let graph = [
            LogicalNode::Retriever(lexical),
            LogicalNode::Retriever(dense),
            LogicalNode::Fusion(fusion),
        ];

        let fused = graph.last().unwrap();
        assert_eq!(fused.id().as_str(), "rrf");
        // The fusion consumes both retrieval legs, by id — edges are data flow.
        assert_eq!(
            fused.inputs(),
            &[NodeId::new("bm25_leg"), NodeId::new("dense_leg")]
        );
    }

    #[test]
    fn logical_nodes_round_trip_through_serde() {
        // Every variant, because a `#[serde(skip)]` on any one field would
        // otherwise ship green while silently dropping a node's identity or
        // its entire edge set.
        let nodes = [
            LogicalNode::Retriever(RetrieverNode {
                id: NodeId::new("bm25_leg"),
                implementation: "bm25".to_string(),
                inputs: vec![NodeId::new("question")],
                params: params(&[("k", ParamValue::Int(50))]),
            }),
            LogicalNode::Fusion(FusionNode {
                id: NodeId::new("rrf"),
                implementation: "reciprocal_rank_fusion".to_string(),
                inputs: vec![NodeId::new("bm25_leg"), NodeId::new("dense_leg")],
                params: params(&[("k", ParamValue::Float(60.0))]),
            }),
            LogicalNode::Reranker(RerankerNode {
                id: NodeId::new("cross_encoder"),
                implementation: "bge_reranker".to_string(),
                inputs: vec![NodeId::new("question"), NodeId::new("rrf")],
                params: params(&[("top_n", ParamValue::Int(5))]),
            }),
            LogicalNode::Extension(ExtensionNode {
                id: NodeId::new("my_technique"),
                kind: "hyde".to_string(),
                inputs: vec![NodeId::new("question")],
                params: params(&[
                    ("temperature", ParamValue::Float(0.2)),
                    ("enabled", ParamValue::Bool(true)),
                ]),
            }),
        ];
        for node in nodes {
            let json = serde_json::to_string(&node).unwrap();
            let back: LogicalNode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, node, "round trip lost information for {node:?}");
        }
    }

    #[test]
    fn logical_nodes_keep_their_serialized_shape() {
        // INV-9 puts the wire format elsewhere, so this shape is internal —
        // but it is what a persisted logical form would be written as, and a
        // silent flip of the tagging discipline or a variant rename would make
        // every stored run unreadable with a green suite. One snapshot makes
        // such a change deliberate rather than accidental.
        let node = LogicalNode::Retriever(RetrieverNode {
            id: NodeId::new("bm25_leg"),
            implementation: "bm25".to_string(),
            inputs: vec![NodeId::new("question")],
            params: params(&[("k", ParamValue::Int(50))]),
        });
        assert_eq!(
            serde_json::to_string(&node).unwrap(),
            r#"{"Retriever":{"id":"bm25_leg","implementation":"bm25","inputs":["question"],"params":{"k":{"Int":50}}}}"#
        );
    }

    #[test]
    fn node_ids_serve_as_map_keys_and_sort() {
        // `validate` checks referential integrity with a `HashMap` keyed by
        // id, and any `BTreeMap`-keyed adjacency needs the ordering.
        let mut seen = HashMap::new();
        seen.insert(NodeId::new("rrf"), 1);
        assert_eq!(seen.get(&NodeId::new("rrf")), Some(&1));
        assert_eq!(seen.get(&NodeId::new("absent")), None);

        let sorted: BTreeSet<NodeId> = ["gamma", "alpha", "beta"]
            .into_iter()
            .map(NodeId::new)
            .collect();
        let sorted: Vec<&str> = sorted.iter().map(NodeId::as_str).collect();
        assert_eq!(sorted, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn param_values_keep_their_full_numeric_width() {
        // Deliberately no out-of-range Rust literal here: a narrowed `Int` or
        // `Float` must *fail this test*, not merely fail to compile it. A
        // narrower type would silently truncate a plausible parameter such as
        // a token budget, or saturate a threshold to infinity.
        for (json, reserialized) in [
            (
                r#"{"Int":9223372036854775807}"#,
                r#"{"Int":9223372036854775807}"#,
            ),
            (
                r#"{"Int":-9223372036854775808}"#,
                r#"{"Int":-9223372036854775808}"#,
            ),
            (r#"{"Float":1e300}"#, r#"{"Float":1e+300}"#),
        ] {
            let parsed: ParamValue =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("{json} did not parse: {e}"));
            assert_eq!(
                serde_json::to_string(&parsed).unwrap(),
                reserialized,
                "{json} did not survive at full width"
            );
        }
    }

    #[test]
    fn non_finite_floats_are_outside_the_documented_contract() {
        // Pinned rather than hidden: this is the boundary `validate` must
        // reject at, and the reason `ParamValue` cannot derive `Eq`.
        let encoded = serde_json::to_string(&ParamValue::Float(f64::NAN)).unwrap();
        assert_eq!(encoded, r#"{"Float":null}"#);
        assert!(
            serde_json::from_str::<ParamValue>(&encoded).is_err(),
            "a non-finite float must not silently read back"
        );
        // `NaN` costs equality its reflexivity, so such a node stops comparing
        // equal to itself.
        assert_ne!(ParamValue::Float(f64::NAN), ParamValue::Float(f64::NAN));
        // And these two compare equal while their bit patterns differ, which is
        // why #10 must canonicalize `-0.0` before hashing (INV-8).
        assert_eq!(ParamValue::Float(0.0), ParamValue::Float(-0.0));
        assert_ne!(0.0f64.to_bits(), (-0.0f64).to_bits());
    }
}
