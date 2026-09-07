//! `ValidationError`, the raw-to-logical lowering, and [`validate`] (#9).
//!
//! Turning a permissive [`crate::RawPipeline`] into a validated, canonical
//! [`crate::LogicalPipeline`] is three passes. First, lowering *one*
//! [`crate::RawNode`] into *one* [`crate::LogicalNode`], and *one*
//! [`crate::RawParamValue`] into *one* [`crate::ParamValue`], rejecting what
//! cannot be represented. Second, the structural checks over the *whole*
//! graph — duplicate ids, dangling inputs, cycles — that only make sense once
//! every node has lowered. Third, canonicalization: sorting the node list by
//! id, the one remaining step that makes two differently-formatted but
//! equivalent configurations converge (see the contract on
//! [`crate::LogicalPipeline`]). [`validate`] runs all three, in that order.
//! The kind check across an edge (`KindMismatch`) is a later task in this
//! issue, and extends [`ValidationError`] rather than replacing it.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::node::{
    ExtensionNode, FusionNode, LogicalNode, NodeId, ParamValue, Params, RerankerNode, RetrieverNode,
};
use crate::pipeline::LogicalPipeline;
use crate::raw::{RawNode, RawParamValue, RawPipeline};

/// A `RawPipeline` cannot be lowered into a validated logical form.
///
/// Written out by hand rather than derived: `ragondin-pipeline`'s
/// `ARCHITECTURE.md` permits `ragondin-types`, `serde` and a hashing crate and
/// nothing else, and one error type is not reason enough to widen a core
/// crate's dependencies — the same precedent `raw.rs`'s
/// `UnsupportedSchemaVersion` sets.
///
/// The lowering variants (`UnknownComponent`, `NonFiniteParam`) came from an
/// earlier task in #9. This task adds the three structural checks over a
/// whole graph: `DuplicateId`, `DanglingInput` and `Cycle`. `KindMismatch`
/// (the edge kind check) is later still; the enum is shaped so each is an
/// additional variant, not a redesign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    /// A node's `component` names no family this build has a [`LogicalNode`]
    /// variant for (settled reading A1).
    UnknownComponent {
        /// The node whose `component` could not be resolved.
        node: NodeId,
        /// The offending `component` string.
        component: String,
    },
    /// A node's parameters hold a `NaN` or `±∞` float, directly or nested
    /// inside a `List` (settled reading A3).
    NonFiniteParam {
        /// The node whose parameters hold the non-finite value.
        node: NodeId,
        /// The parameter key under which the non-finite value was found.
        param: String,
    },
    /// Two nodes in the pipeline share the same id.
    DuplicateId {
        /// The id claimed by more than one node.
        id: NodeId,
    },
    /// A node's `inputs` names an id that no node in the pipeline defines.
    ///
    /// Per settled reading A2, an *empty* `inputs` list is never a dangling
    /// input — this fires only for an id that is actually listed and does
    /// not resolve.
    DanglingInput {
        /// The node whose `inputs` names the missing id.
        node: NodeId,
        /// The id named in `inputs` that no node in the pipeline defines.
        missing: NodeId,
    },
    /// The graph of data edges (a node's `inputs`) is not acyclic.
    Cycle {
        /// The ids of the nodes on the cycle, in the order a depth-first
        /// traversal walked them. At least one node on the cycle is always
        /// present; a self-loop (`a` listing `a` in its own `inputs`)
        /// reports the single-element cycle `[a]`.
        nodes: Vec<NodeId>,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownComponent { node, component } => write!(
                f,
                "node `{}`: unknown component `{component}`",
                node.as_str()
            ),
            Self::NonFiniteParam { node, param } => write!(
                f,
                "node `{}`: parameter `{param}` is not a finite number",
                node.as_str()
            ),
            Self::DuplicateId { id } => {
                write!(f, "duplicate node id `{}`", id.as_str())
            }
            Self::DanglingInput { node, missing } => write!(
                f,
                "node `{}`: input `{}` names no node in the pipeline",
                node.as_str(),
                missing.as_str()
            ),
            Self::Cycle { nodes } => {
                let path = nodes
                    .iter()
                    .map(NodeId::as_str)
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "cycle in the pipeline's data edges: {path}")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Lowers one [`RawParamValue`] into the matching [`ParamValue`], recursing
/// through `List`.
///
/// Rejects non-finite floats (`NaN`, `±∞`) with [`ValidationError::NonFiniteParam`]
/// and normalizes `-0.0` to `0.0` (settled reading A3): `#9` produces the
/// canonical form, `#10` only hashes it, and hashing a raw `to_bits()` would
/// otherwise give two content hashes to two values `ParamValue` calls equal
/// (INV-8).
#[allow(dead_code)] // Wired into the public entry point by a later task in #9.
fn lower_param_value(
    node: &NodeId,
    param: &str,
    raw: RawParamValue,
) -> Result<ParamValue, ValidationError> {
    match raw {
        RawParamValue::Bool(b) => Ok(ParamValue::Bool(b)),
        RawParamValue::Int(i) => Ok(ParamValue::Int(i)),
        RawParamValue::Float(f) => {
            if f.is_finite() {
                // `f == 0.0` holds for both `0.0` and `-0.0`; folding through
                // the comparison is how `-0.0` loses its sign bit here.
                let normalized = if f == 0.0 { 0.0 } else { f };
                Ok(ParamValue::Float(normalized))
            } else {
                Err(ValidationError::NonFiniteParam {
                    node: node.clone(),
                    param: param.to_string(),
                })
            }
        }
        RawParamValue::String(s) => Ok(ParamValue::String(s)),
        RawParamValue::List(items) => {
            let lowered = items
                .into_iter()
                .map(|item| lower_param_value(node, param, item))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ParamValue::List(lowered))
        }
    }
}

/// Lowers a whole `params` map, preserving its (already canonical, `BTreeMap`)
/// key order.
fn lower_params(
    node: &NodeId,
    raw: BTreeMap<String, RawParamValue>,
) -> Result<Params, ValidationError> {
    raw.into_iter()
        .map(|(key, value)| {
            let lowered = lower_param_value(node, &key, value)?;
            Ok((key, lowered))
        })
        .collect()
}

/// Lowers one [`RawNode`] into the [`LogicalNode`] variant its `component`
/// names (settled reading A1): `retriever` -> `RetrieverNode`, `fusion` ->
/// `FusionNode`, `reranker` -> `RerankerNode`, `extension` ->
/// `ExtensionNode { kind: <impl> }`. Any other `component` is
/// [`ValidationError::UnknownComponent`].
///
/// Per settled reading A2, a node with no `inputs` is not rejected here — a
/// source node is written exactly this way, and whether an edge is missing is
/// a later task's question (#16). `inputs` is carried across positionally,
/// never reordered (ADR-C16).
fn lower_node(raw: RawNode) -> Result<LogicalNode, ValidationError> {
    let RawNode {
        id,
        component,
        implementation,
        inputs,
        params,
    } = raw;
    let id = NodeId::new(id);
    let inputs: Vec<NodeId> = inputs.into_iter().map(NodeId::new).collect();

    match component.as_str() {
        "retriever" => Ok(LogicalNode::Retriever(RetrieverNode {
            params: lower_params(&id, params)?,
            id,
            implementation,
            inputs,
        })),
        "fusion" => Ok(LogicalNode::Fusion(FusionNode {
            params: lower_params(&id, params)?,
            id,
            implementation,
            inputs,
        })),
        "reranker" => Ok(LogicalNode::Reranker(RerankerNode {
            params: lower_params(&id, params)?,
            id,
            implementation,
            inputs,
        })),
        "extension" => Ok(LogicalNode::Extension(ExtensionNode {
            params: lower_params(&id, params)?,
            id,
            kind: implementation,
            inputs,
        })),
        other => Err(ValidationError::UnknownComponent {
            node: id,
            component: other.to_string(),
        }),
    }
}

/// Lowers and validates a whole [`RawPipeline`] into a canonical
/// [`LogicalPipeline`].
///
/// Runs, in order — each presupposes the last:
///
/// 1. lowers every node ([`lower_node`]), rejecting an unknown `component`
///    or a non-finite param;
/// 2. checks every id is unique ([`ValidationError::DuplicateId`]) —
///    referential integrity is meaningless without unique ids;
/// 3. checks every `inputs` entry names a node that exists
///    ([`ValidationError::DanglingInput`]);
/// 4. checks the graph of data edges is acyclic
///    ([`ValidationError::Cycle`]);
/// 5. sorts the node list by [`NodeId`], the only reordering this function
///    ever performs — see the canonicalization contract on
///    [`LogicalPipeline`] for exactly what does, and does not, converge.
pub fn validate(raw: RawPipeline) -> Result<LogicalPipeline, ValidationError> {
    let mut nodes = raw
        .pipeline
        .nodes
        .into_iter()
        .map(lower_node)
        .collect::<Result<Vec<_>, _>>()?;

    // Referential integrity is meaningless without unique ids, so this runs
    // first. The map doubles as the id -> position index the later checks
    // need.
    let mut index: HashMap<NodeId, usize> = HashMap::with_capacity(nodes.len());
    for (position, node) in nodes.iter().enumerate() {
        if index.insert(node.id().clone(), position).is_some() {
            return Err(ValidationError::DuplicateId {
                id: node.id().clone(),
            });
        }
    }

    for node in &nodes {
        for input in node.inputs() {
            if !index.contains_key(input) {
                return Err(ValidationError::DanglingInput {
                    node: node.id().clone(),
                    missing: input.clone(),
                });
            }
        }
    }

    if let Some(cycle) = find_cycle(&nodes, &index) {
        return Err(ValidationError::Cycle { nodes: cycle });
    }

    // Deterministic ordering (Task 4): two `RawPipeline`s listing the same
    // nodes in different order must canonicalize to the same
    // `LogicalPipeline` (INV-8). This sorts the *node list* only, by
    // `NodeId` — it never touches a node's `inputs`, which stays exactly as
    // the source listed it (Global Constraint 4 / ADR-C16). A node's `params`
    // is already canonical (`Params` is a `BTreeMap`), and non-finite floats
    // were already rejected and `-0.0` already normalized during lowering
    // (settled reading A3), so nothing further needs doing here.
    nodes.sort_by(|a, b| a.id().cmp(b.id()));

    Ok(LogicalPipeline::new(nodes))
}

/// The three-colour marking a depth-first search over `nodes` uses to spot
/// a back edge.
#[derive(Clone, Copy, PartialEq)]
enum Color {
    /// Not yet visited.
    White,
    /// On the current DFS path — a step into a `Gray` node is a back edge,
    /// hence a cycle.
    Gray,
    /// Fully explored; cannot be part of a cycle discovered from here on.
    Black,
}

/// Depth-first search, with three-colour marking, over the "consumes" edges
/// (a node to each id in its `inputs`) for the first cycle. `index` maps
/// every node's id to its position in `nodes`; by the time this runs, every
/// `inputs` entry is already known to resolve (dangling inputs are checked
/// before this is called), so the lookup below cannot miss.
fn find_cycle(nodes: &[LogicalNode], index: &HashMap<NodeId, usize>) -> Option<Vec<NodeId>> {
    fn visit(
        position: usize,
        nodes: &[LogicalNode],
        index: &HashMap<NodeId, usize>,
        color: &mut [Color],
        path: &mut Vec<NodeId>,
    ) -> Option<Vec<NodeId>> {
        color[position] = Color::Gray;
        path.push(nodes[position].id().clone());

        for input in nodes[position].inputs() {
            let next = index[input];
            match color[next] {
                Color::White => {
                    if let Some(cycle) = visit(next, nodes, index, color, path) {
                        return Some(cycle);
                    }
                }
                Color::Gray => {
                    // A back edge to a node still on the current path: the
                    // cycle is everything from that node's first occurrence
                    // onward, which already loops back to it.
                    let start = path
                        .iter()
                        .position(|id| id == nodes[next].id())
                        .expect("a Gray node's id is always on the current DFS path");
                    return Some(path[start..].to_vec());
                }
                Color::Black => {}
            }
        }

        color[position] = Color::Black;
        path.pop();
        None
    }

    let mut color = vec![Color::White; nodes.len()];
    let mut path = Vec::new();
    for position in 0..nodes.len() {
        if color[position] == Color::White {
            if let Some(cycle) = visit(position, nodes, index, &mut color, &mut path) {
                return Some(cycle);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::RawGraph;

    fn raw_node(component: &str, params: BTreeMap<String, RawParamValue>) -> RawNode {
        RawNode {
            id: "n".to_string(),
            component: component.to_string(),
            implementation: "impl_name".to_string(),
            inputs: vec!["a".to_string(), "b".to_string()],
            params,
        }
    }

    fn fixture_params() -> BTreeMap<String, RawParamValue> {
        let mut params = BTreeMap::new();
        params.insert("top_k".to_string(), RawParamValue::Int(50));
        params.insert("alpha".to_string(), RawParamValue::Float(0.5));
        params
    }

    fn expected_params() -> Params {
        let mut params = Params::new();
        params.insert("top_k".to_string(), ParamValue::Int(50));
        params.insert("alpha".to_string(), ParamValue::Float(0.5));
        params
    }

    #[test]
    fn a_retriever_lowers_with_its_shape_intact_and_in_order() {
        let raw = raw_node("retriever", fixture_params());
        let node = lower_node(raw).unwrap();
        let LogicalNode::Retriever(node) = node else {
            panic!("expected Retriever, got {node:?}")
        };
        assert_eq!(node.id, NodeId::new("n"));
        assert_eq!(node.implementation, "impl_name");
        assert_eq!(node.inputs, vec![NodeId::new("a"), NodeId::new("b")]);
        assert_eq!(node.params, expected_params());
    }

    #[test]
    fn a_fusion_lowers_with_its_shape_intact_and_in_order() {
        let raw = raw_node("fusion", fixture_params());
        let node = lower_node(raw).unwrap();
        let LogicalNode::Fusion(node) = node else {
            panic!("expected Fusion, got {node:?}")
        };
        assert_eq!(node.id, NodeId::new("n"));
        assert_eq!(node.implementation, "impl_name");
        assert_eq!(node.inputs, vec![NodeId::new("a"), NodeId::new("b")]);
        assert_eq!(node.params, expected_params());
    }

    #[test]
    fn a_reranker_lowers_with_its_shape_intact_and_in_order() {
        let raw = raw_node("reranker", fixture_params());
        let node = lower_node(raw).unwrap();
        let LogicalNode::Reranker(node) = node else {
            panic!("expected Reranker, got {node:?}")
        };
        assert_eq!(node.id, NodeId::new("n"));
        assert_eq!(node.implementation, "impl_name");
        assert_eq!(node.inputs, vec![NodeId::new("a"), NodeId::new("b")]);
        assert_eq!(node.params, expected_params());
    }

    #[test]
    fn an_extension_lowers_its_impl_value_into_kind() {
        let raw = raw_node("extension", fixture_params());
        let node = lower_node(raw).unwrap();
        let LogicalNode::Extension(node) = node else {
            panic!("expected Extension, got {node:?}")
        };
        assert_eq!(node.id, NodeId::new("n"));
        assert_eq!(node.kind, "impl_name");
        assert_eq!(node.inputs, vec![NodeId::new("a"), NodeId::new("b")]);
        assert_eq!(node.params, expected_params());
    }

    #[test]
    fn an_unknown_component_names_the_node_and_the_offending_string() {
        let raw = RawNode {
            id: "bad".to_string(),
            component: "query_transform".to_string(),
            implementation: "hyde".to_string(),
            inputs: vec![],
            params: BTreeMap::new(),
        };
        let err = lower_node(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::UnknownComponent {
                node: NodeId::new("bad"),
                component: "query_transform".to_string(),
            }
        );
        assert!(
            err.to_string().contains("bad") && err.to_string().contains("query_transform"),
            "the message must name both the node and the offending component: {err}"
        );
    }

    #[test]
    fn a_bare_nan_param_is_rejected_naming_the_node_and_the_key() {
        let mut params = BTreeMap::new();
        params.insert("threshold".to_string(), RawParamValue::Float(f64::NAN));
        let raw = raw_node("retriever", params);
        let err = lower_node(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::NonFiniteParam {
                node: NodeId::new("n"),
                param: "threshold".to_string(),
            }
        );
        assert!(
            err.to_string().contains("`n`") && err.to_string().contains("threshold"),
            "the message must name both the node and the parameter key: {err}"
        );
    }

    #[test]
    fn bare_infinities_are_rejected() {
        for inf in [f64::INFINITY, f64::NEG_INFINITY] {
            let mut params = BTreeMap::new();
            params.insert("threshold".to_string(), RawParamValue::Float(inf));
            let raw = raw_node("retriever", params);
            let err = lower_node(raw).unwrap_err();
            assert_eq!(
                err,
                ValidationError::NonFiniteParam {
                    node: NodeId::new("n"),
                    param: "threshold".to_string(),
                }
            );
        }
    }

    #[test]
    fn a_non_finite_float_nested_inside_a_list_is_rejected() {
        let mut params = BTreeMap::new();
        params.insert(
            "weights".to_string(),
            RawParamValue::List(vec![
                RawParamValue::Float(1.0),
                RawParamValue::Float(f64::NAN),
            ]),
        );
        let raw = raw_node("retriever", params);
        let err = lower_node(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::NonFiniteParam {
                node: NodeId::new("n"),
                param: "weights".to_string(),
            }
        );
    }

    #[test]
    fn negative_zero_normalizes_to_positive_zero_bits() {
        let mut params = BTreeMap::new();
        params.insert("bias".to_string(), RawParamValue::Float(-0.0));
        let raw = raw_node("retriever", params);
        let node = lower_node(raw).unwrap();
        let LogicalNode::Retriever(node) = node else {
            panic!("expected Retriever, got {node:?}")
        };
        match node.params.get("bias").unwrap() {
            ParamValue::Float(f) => {
                assert_eq!(f.to_bits(), 0.0f64.to_bits());
            }
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn every_raw_param_value_variant_round_trips_into_the_matching_param_value_variant() {
        let id = NodeId::new("n");
        let cases = [
            (RawParamValue::Bool(true), ParamValue::Bool(true)),
            (RawParamValue::Int(42), ParamValue::Int(42)),
            (RawParamValue::Float(0.5), ParamValue::Float(0.5)),
            (
                RawParamValue::String("cosine".to_string()),
                ParamValue::String("cosine".to_string()),
            ),
            (
                RawParamValue::List(vec![RawParamValue::Int(1), RawParamValue::Bool(false)]),
                ParamValue::List(vec![ParamValue::Int(1), ParamValue::Bool(false)]),
            ),
        ];
        for (raw, expected) in cases {
            let got = lower_param_value(&id, "p", raw.clone()).unwrap();
            assert_eq!(got, expected, "{raw:?} did not lower to {expected:?}");
        }
    }

    // --- `validate`: structural checks over a whole `RawPipeline` ---

    fn node(id: &str, component: &str, inputs: &[&str]) -> RawNode {
        RawNode {
            id: id.to_string(),
            component: component.to_string(),
            implementation: format!("{component}_impl"),
            inputs: inputs.iter().map(|s| s.to_string()).collect(),
            params: BTreeMap::new(),
        }
    }

    fn pipeline(nodes: Vec<RawNode>) -> RawPipeline {
        RawPipeline {
            version: Default::default(),
            pipeline: RawGraph { nodes },
        }
    }

    #[test]
    fn a_valid_hybrid_graph_validates() {
        // Raw deliberately lists `rrf` first, out of `NodeId` order: this
        // pins deterministic ordering (Task 4) as well as the original
        // acceptance criterion — the sorted output must not depend on the
        // order the source `RawPipeline` happened to list nodes in.
        let raw = pipeline(vec![
            node("rrf", "fusion", &["bm25_leg", "dense_leg"]),
            node("dense_leg", "retriever", &[]),
            node("bm25_leg", "retriever", &[]),
        ]);
        let logical = validate(raw).expect("a valid hybrid graph must validate");
        let ids: Vec<&str> = logical.nodes().iter().map(|n| n.id().as_str()).collect();
        assert_eq!(ids, vec!["bm25_leg", "dense_leg", "rrf"]);
    }

    #[test]
    fn an_empty_inputs_list_validates_as_a_source_node() {
        // Settled reading A2: a node with no `inputs` is a source node, not
        // an error — the executor supplies the query.
        let raw = pipeline(vec![node("question", "retriever", &[])]);
        let logical = validate(raw).expect("an empty inputs list must not be rejected");
        assert_eq!(logical.nodes().len(), 1);
    }

    #[test]
    fn a_duplicate_id_is_rejected() {
        let raw = pipeline(vec![
            node("dup", "retriever", &[]),
            node("dup", "retriever", &[]),
        ]);
        let err = validate(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::DuplicateId {
                id: NodeId::new("dup")
            }
        );
        assert!(
            err.to_string().contains("dup"),
            "the message must name the duplicated id: {err}"
        );
    }

    #[test]
    fn a_dangling_input_is_rejected() {
        let raw = pipeline(vec![node("rrf", "fusion", &["missing"])]);
        let err = validate(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::DanglingInput {
                node: NodeId::new("rrf"),
                missing: NodeId::new("missing"),
            }
        );
        assert!(
            err.to_string().contains("rrf") && err.to_string().contains("missing"),
            "the message must name both the node and the missing input: {err}"
        );
    }

    #[test]
    fn a_self_loop_is_a_cycle_not_a_dangling_input() {
        // `a` names itself, which exists — so this must not read as a
        // dangling input, only as a cycle.
        let raw = pipeline(vec![node("a", "retriever", &["a"])]);
        let err = validate(raw).unwrap_err();
        match err {
            ValidationError::Cycle { nodes } => {
                assert!(
                    nodes.contains(&NodeId::new("a")),
                    "the cycle must name `a`: {nodes:?}"
                );
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn a_multi_node_cycle_is_detected() {
        let raw = pipeline(vec![
            node("a", "retriever", &["b"]),
            node("b", "retriever", &["a"]),
        ]);
        let err = validate(raw).unwrap_err();
        match err {
            ValidationError::Cycle { nodes } => {
                assert!(nodes.contains(&NodeId::new("a")));
                assert!(nodes.contains(&NodeId::new("b")));
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    // --- Task 4: canonicalization ---

    fn node_with_params(
        id: &str,
        component: &str,
        inputs: &[&str],
        params: BTreeMap<String, RawParamValue>,
    ) -> RawNode {
        RawNode {
            id: id.to_string(),
            component: component.to_string(),
            implementation: format!("{component}_impl"),
            inputs: inputs.iter().map(|s| s.to_string()).collect(),
            params,
        }
    }

    #[test]
    fn two_pipelines_differing_only_in_node_order_and_param_key_order_canonicalize_equal() {
        // Same params, built with the keys inserted in two different orders.
        // `Params` is a `BTreeMap`, so this must already be a non-event, but
        // the acceptance criterion asks it be proven rather than assumed.
        let mut params_forward = BTreeMap::new();
        params_forward.insert("top_k".to_string(), RawParamValue::Int(50));
        params_forward.insert("alpha".to_string(), RawParamValue::Float(0.5));

        let mut params_reverse = BTreeMap::new();
        params_reverse.insert("alpha".to_string(), RawParamValue::Float(0.5));
        params_reverse.insert("top_k".to_string(), RawParamValue::Int(50));

        let forward = pipeline(vec![
            node_with_params("bm25_leg", "retriever", &[], params_forward.clone()),
            node_with_params("dense_leg", "retriever", &[], params_forward.clone()),
            node_with_params("rrf", "fusion", &["bm25_leg", "dense_leg"], params_forward),
        ]);
        // Same nodes, listed in reverse, each with its params built in the
        // opposite key-insertion order.
        let reversed = pipeline(vec![
            node_with_params(
                "rrf",
                "fusion",
                &["bm25_leg", "dense_leg"],
                params_reverse.clone(),
            ),
            node_with_params("dense_leg", "retriever", &[], params_reverse.clone()),
            node_with_params("bm25_leg", "retriever", &[], params_reverse),
        ]);

        let a = validate(forward).expect("forward pipeline must validate");
        let b = validate(reversed).expect("reversed pipeline must validate");
        assert_eq!(
            a, b,
            "node order and param key order must not affect the canonical value"
        );
    }

    #[test]
    fn a_fusions_inputs_are_never_reordered_by_canonicalization() {
        // Global Constraint 4 / ADR-C16: `inputs` is positional and
        // order-significant. `[a, b]` and `[b, a]` are two different
        // configurations, not one canonicalized to the other.
        let ab = pipeline(vec![
            node("a", "retriever", &[]),
            node("b", "retriever", &[]),
            node("rrf", "fusion", &["a", "b"]),
        ]);
        let ba = pipeline(vec![
            node("a", "retriever", &[]),
            node("b", "retriever", &[]),
            node("rrf", "fusion", &["b", "a"]),
        ]);
        let logical_ab = validate(ab).expect("ab pipeline must validate");
        let logical_ba = validate(ba).expect("ba pipeline must validate");
        assert_ne!(
            logical_ab, logical_ba,
            "swapping a fusion's inputs must change the canonical value"
        );
    }

    #[test]
    fn negative_zero_survives_into_the_canonical_pipeline() {
        let mut params = BTreeMap::new();
        params.insert("bias".to_string(), RawParamValue::Float(-0.0));
        let raw = pipeline(vec![node_with_params("n", "retriever", &[], params)]);
        let logical = validate(raw).expect("a valid pipeline must validate");
        match &logical.nodes()[0] {
            LogicalNode::Retriever(node) => match node.params.get("bias").unwrap() {
                ParamValue::Float(f) => assert_eq!(
                    f.to_bits(),
                    0.0f64.to_bits(),
                    "-0.0 must normalize to 0.0 in the canonical pipeline"
                ),
                other => panic!("expected Float, got {other:?}"),
            },
            other => panic!("expected Retriever, got {other:?}"),
        }
    }

    #[test]
    fn a_logical_pipeline_serde_round_trip_changes_nothing() {
        // The executable form of "canonicalization is idempotent" (Ruling
        // R5): `validate` cannot be handed a `LogicalPipeline` back, but
        // serializing and re-deriving one must reproduce it exactly, which is
        // exactly what #10 needs from this type.
        let raw = pipeline(vec![
            node("bm25_leg", "retriever", &[]),
            node("dense_leg", "retriever", &[]),
            node("rrf", "fusion", &["bm25_leg", "dense_leg"]),
        ]);
        let logical = validate(raw).expect("a valid pipeline must validate");
        let json = serde_json::to_string(&logical).unwrap();
        let back: LogicalPipeline = serde_json::from_str(&json).unwrap();
        assert_eq!(back, logical, "a serde round trip must change nothing");
    }
}
