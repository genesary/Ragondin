//! `ValidationError` and the raw-to-logical lowering (#9).
//!
//! This is the first half of turning a permissive [`crate::RawPipeline`] into
//! a validated [`crate::LogicalNode`] graph: lowering *one* [`crate::RawNode`]
//! into *one* [`crate::LogicalNode`], and *one* [`crate::RawParamValue`] into
//! *one* [`crate::ParamValue`], rejecting what cannot be represented. The
//! structural checks over a whole graph — duplicate ids, dangling inputs,
//! cycles — are a later task in this issue, and the kind check across an edge
//! (`KindMismatch`) is later still; both extend [`ValidationError`] rather
//! than replace it.
//!
//! Kept private to this module and exercised directly by its own tests: a
//! public `validate` entry point over a whole [`crate::RawPipeline`] needs the
//! structural checks to be worth anything, and those are the later task's.

use std::collections::BTreeMap;
use std::fmt;

use crate::node::{
    ExtensionNode, FusionNode, LogicalNode, NodeId, ParamValue, Params, RerankerNode, RetrieverNode,
};
use crate::raw::{RawNode, RawParamValue};

/// A `RawPipeline` cannot be lowered into a validated logical form.
///
/// Written out by hand rather than derived: `ragondin-pipeline`'s
/// `ARCHITECTURE.md` permits `ragondin-types`, `serde` and a hashing crate and
/// nothing else, and one error type is not reason enough to widen a core
/// crate's dependencies — the same precedent `raw.rs`'s
/// `UnsupportedSchemaVersion` sets.
///
/// This task adds the two variants the lowering itself needs. Later tasks in
/// #9 add `DuplicateId`, `DanglingInput` and `Cycle` (structural checks over
/// the whole graph) and `KindMismatch` (the edge kind check); the enum is
/// shaped so each is an additional variant, not a redesign.
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
#[allow(dead_code)] // Wired into the public entry point by a later task in #9.
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
#[allow(dead_code)] // Wired into the public entry point by a later task in #9.
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
        "retriever" | "fusion" | "reranker" | "extension" => {}
        _ => {
            return Err(ValidationError::UnknownComponent {
                node: id,
                component,
            })
        }
    }
    let params = lower_params(&id, params)?;

    Ok(match component.as_str() {
        "retriever" => LogicalNode::Retriever(RetrieverNode {
            id,
            implementation,
            inputs,
            params,
        }),
        "fusion" => LogicalNode::Fusion(FusionNode {
            id,
            implementation,
            inputs,
            params,
        }),
        "reranker" => LogicalNode::Reranker(RerankerNode {
            id,
            implementation,
            inputs,
            params,
        }),
        _ => LogicalNode::Extension(ExtensionNode {
            id,
            kind: implementation,
            inputs,
            params,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
