//! The logical node model: the validated, in-memory node types.
//!
//! Per `docs/code-architecture.md` §6.2 the representation is a **closed enum
//! of primitive nodes plus one open `Extension` variant** (ADR-C3), so a new
//! technique can be added without changing the core. Nodes are value types
//! (INV-3): plain data, no trait objects, no I/O. Edges are data flow — a node
//! names the ids of the nodes it consumes, in `inputs`.
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
//! `rag-config`/`rag-proto`.
//!
//! `Branch` and `Loop` (§6.2) are deliberately absent. A `Loop`'s mandatory
//! termination guard and a `Branch`'s predicate have no settled representation,
//! and inventing one here would be a design decision this issue does not own.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The stable identifier of a node within a pipeline.
///
/// Edges are expressed by id: a node lists in its `inputs` the ids of the nodes
/// whose output it consumes.
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
/// ordering comes from the `BTreeMap` that holds the parameters, whose keys
/// iterate in sorted order regardless of insertion order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ParamValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<ParamValue>),
}

/// Parameters of a node, keyed by name in canonical (sorted) order.
pub type Params = BTreeMap<String, ParamValue>;

/// A retrieval node: it retrieves candidate chunks for a query.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetrieverNode {
    pub id: NodeId,
    /// The `impl:` value naming the component to resolve, e.g. `"bm25"`.
    pub implementation: String,
    pub inputs: Vec<NodeId>,
    pub params: Params,
}

/// A fusion node: it merges several retrieval results into one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FusionNode {
    pub id: NodeId,
    pub implementation: String,
    pub inputs: Vec<NodeId>,
    pub params: Params,
}

/// A reranking node: it reorders retrieved chunks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RerankerNode {
    pub id: NodeId,
    pub implementation: String,
    pub inputs: Vec<NodeId>,
    pub params: Params,
}

/// A node defined outside the core — the escape hatch of ADR-C3.
///
/// A researcher who invents a genuinely new node type expresses it here,
/// without modifying the primitive enum. Repeated use of `Extension` for the
/// same shape is the signal to promote that shape to a primitive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtensionNode {
    pub id: NodeId,
    /// Names the *extension node type*, e.g. `"hyde"`.
    ///
    /// Unrelated to ADR-C16's `ValueKind`, which names what travels along an
    /// edge. An extension's port kinds are unknown to the core by construction,
    /// and are resolved from the registry at physical planning.
    pub kind: String,
    pub inputs: Vec<NodeId>,
    pub params: Params,
}

/// A node of a validated pipeline: closed over the primitives, open through
/// [`ExtensionNode`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LogicalNode {
    Retriever(RetrieverNode),
    Fusion(FusionNode),
    Reranker(RerankerNode),
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

    /// The ids of the nodes whose output this node consumes.
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
    use std::collections::BTreeMap;

    fn params(pairs: &[(&str, ParamValue)]) -> BTreeMap<String, ParamValue> {
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
            inputs: vec![NodeId::new("rrf")],
            params: params(&[("top_n", ParamValue::Int(5))]),
        };
        assert_eq!(node.id.as_str(), "cross_encoder");
        assert_eq!(node.inputs, vec![NodeId::new("rrf")]);
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
                params: BTreeMap::new(),
            }),
            LogicalNode::Fusion(FusionNode {
                id: NodeId::new("f"),
                implementation: "rrf".to_string(),
                inputs: vec![NodeId::new("r")],
                params: BTreeMap::new(),
            }),
            LogicalNode::Reranker(RerankerNode {
                id: NodeId::new("k"),
                implementation: "bge".to_string(),
                inputs: vec![NodeId::new("f")],
                params: BTreeMap::new(),
            }),
            LogicalNode::Extension(ExtensionNode {
                id: NodeId::new("x"),
                kind: "hyde".to_string(),
                inputs: vec![NodeId::new("k")],
                params: BTreeMap::new(),
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
            params: BTreeMap::new(),
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
        let node = LogicalNode::Extension(ExtensionNode {
            id: NodeId::new("my_technique"),
            kind: "hyde".to_string(),
            inputs: vec![NodeId::new("question")],
            params: params(&[
                ("temperature", ParamValue::Float(0.2)),
                ("enabled", ParamValue::Bool(true)),
            ]),
        });
        let json = serde_json::to_string(&node).unwrap();
        let back: LogicalNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, node);
    }
}
