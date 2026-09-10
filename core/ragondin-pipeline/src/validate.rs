//! `ValidationError`, the raw-to-logical lowering, and [`validate`]: the pass
//! that produces a [`crate::LogicalPipeline`] from a [`crate::RawPipeline`].
//!
//! Turning a permissive [`crate::RawPipeline`] into a validated, canonical
//! [`crate::LogicalPipeline`] is four passes. First, lowering *one*
//! [`crate::RawNode`] into *one* [`crate::LogicalNode`], and *one*
//! [`crate::RawParamValue`] into *one* [`crate::ParamValue`], rejecting what
//! cannot be represented. Second, sorting the node list by id — the one
//! normalization this pass performs (see the contract on
//! [`crate::LogicalPipeline`]) — done before any check runs, so that which of
//! two faults a malformed graph reports never depends on the order the source
//! `RawPipeline` happened to list nodes in. Third, the structural checks over
//! the *whole* graph — duplicate ids, the pipeline's own input declaration,
//! dangling inputs, cycles — that only make sense once every node has
//! lowered. Fourth, the kind check across every edge
//! ([`ValidationError::KindMismatch`], ADR-C16), which needs the graph to be
//! acyclic and every input already known to resolve. [`validate`] runs all
//! four, in that order, behind a schema-version check that precedes them all.
//!
//! Since ADR-C18 a pipeline **declares its inputs**, and an id in a node's
//! `inputs` resolves either to another node or to one of those declarations.
//! A declared input is what gives [`crate::ValueKind::Query`] a producer — no
//! [`LogicalNode`] variant is one — so it is a source: never on a cycle, and
//! contributing `Query` to the kind check.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::kind::{consumed_kinds, produced_kind, PortSpec, ValueKind};
use crate::node::{
    ExtensionNode, FusionNode, LogicalNode, NodeId, ParamValue, Params, RerankerNode, RetrieverNode,
};
use crate::pipeline::LogicalPipeline;
use crate::raw::{RawNode, RawParamValue, RawPipeline};

/// A `RawPipeline` cannot be lowered into a validated logical form.
///
/// Derived via `thiserror` rather than hand-written: `ragondin-pipeline`'s
/// `ARCHITECTURE.md` used to read as forbidding `thiserror` outright, and #9
/// followed that precedent (the same one `raw.rs`'s
/// `UnsupportedSchemaVersion` set). But ADR-C13 requires typed errors via
/// `thiserror` in every library in this workspace, so #84 corrected
/// `ARCHITECTURE.md` to permit it, and this enum no longer needs a
/// hand-rolled `Display`.
///
/// The lowering variants
/// (`UnknownComponent`, `NonFiniteParam`) and the five structural checks over
/// a whole graph (`DuplicateId`, `InputArity`, `InputCollidesWithNode`,
/// `DanglingInput`, `Cycle`) cover well-formedness. `KindMismatch` is
/// additional: the edge-kind check ADR-C16 places at `LogicalPipeline`
/// validation. Each arrived as an additional variant, never a redesign of the
/// ones before it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    /// A node's `component` names no family this build has a [`LogicalNode`]
    /// variant for (settled reading A1).
    #[error("node `{}`: unknown component `{component}`", node.as_str())]
    UnknownComponent {
        /// The node whose `component` could not be resolved.
        node: NodeId,
        /// The offending `component` string.
        component: String,
    },
    /// A node's parameters hold a `NaN` or `±∞` float, directly or nested
    /// inside a `List` (settled reading A3).
    #[error(
        "node `{}`: parameter `{param}` is not a finite number",
        node.as_str()
    )]
    NonFiniteParam {
        /// The node whose parameters hold the non-finite value.
        node: NodeId,
        /// The parameter key under which the non-finite value was found.
        param: String,
    },
    /// Two nodes in the pipeline share the same id.
    #[error("duplicate node id `{}`", id.as_str())]
    DuplicateId {
        /// The id claimed by more than one node.
        id: NodeId,
    },
    /// A node's `inputs` names an id that neither a node nor a declared
    /// pipeline input defines.
    ///
    /// Per settled reading A2, an *empty* `inputs` list is never a dangling
    /// input — this fires only for an id that is actually listed and does
    /// not resolve. Since ADR-C18 an id resolves two ways: to a node, or to
    /// one of the pipeline's declared inputs.
    #[error(
        "node `{}`: input `{}` names neither a node nor a declared input",
        node.as_str(),
        missing.as_str()
    )]
    DanglingInput {
        /// The node whose `inputs` names the missing id.
        node: NodeId,
        /// The id named in `inputs` that nothing in the pipeline defines.
        missing: NodeId,
    },
    /// A pipeline does not declare exactly one input (ADR-C18).
    ///
    /// A serving graph takes exactly one value from its caller, the query.
    /// Declaring none leaves `ValueKind::Query` without the producer no
    /// [`LogicalNode`] variant is; declaring more than one describes a graph
    /// this build has no second kind of.
    ///
    /// This subsumes a repeated declaration: `[question, question]` declares
    /// two, and is reported here rather than as a distinct duplicate fault.
    #[error("a pipeline must declare exactly one input, found {declared}")]
    InputArity {
        /// How many inputs the configuration declared.
        declared: usize,
    },
    /// A declared input and a node claim the same id.
    ///
    /// Ids are one namespace — a node names an input in `inputs` exactly as
    /// it names another node — so an id resolving to both is ambiguous. The
    /// same reasoning as [`ValidationError::DuplicateId`], across the two
    /// kinds of thing an id can name.
    #[error("declared input `{}` is also a node id", id.as_str())]
    InputCollidesWithNode {
        /// The id claimed by both a declared input and a node.
        id: NodeId,
    },
    /// The graph of data edges (a node's `inputs`) is not acyclic.
    // `nodes` walks consumer -> producer (a node, then its input), the
    // reverse of data flow — rendered as "consumes" rather than "->" so the
    // direction cannot be misread as which way values travel.
    #[error(
        "cycle in the pipeline's data edges: {}",
        nodes.iter().map(NodeId::as_str).collect::<Vec<_>>().join(" consumes ")
    )]
    Cycle {
        /// The ids of the nodes on the cycle, in the order a depth-first
        /// traversal walked them. At least one node on the cycle is always
        /// present; a self-loop (`a` listing `a` in its own `inputs`)
        /// reports the single-element cycle `[a]`.
        nodes: Vec<NodeId>,
    },
    /// An edge's value kinds do not line up (ADR-C16).
    ///
    /// `consumer.inputs()[port]` names `producer`, but the kind
    /// [`produced_kind`] derives for `producer` does not match what
    /// [`consumed_kinds`] declares `consumer` expects at `port`.
    ///
    /// `expected` is `None` when `port` is beyond what a fixed-arity variant
    /// declares (settled reading A2): there is no port to compare against at
    /// all, only an edge that should not exist. That fires regardless of what
    /// produces the edge — the consumer's arity is fully known from its own
    /// variant — so an `Extension` producer can appear here with
    /// `found: ValueKind::Opaque`. An `Extension` node is never a
    /// *consumer* of this check (its `PortSpec` is unknown to the core), and
    /// an `Extension` *producer* is otherwise skipped whenever a port
    /// genuinely exists (`expected` is `Some`): its real kind is not something
    /// the core can state (ADR-C16). Nothing downstream states it either —
    /// `ragondin-engine`'s `plan_physical` refuses every `Extension` node
    /// (`PlanError::ExtensionUnsupported`) rather than guess, because there is
    /// no extension registry to resolve one through and how an extension is
    /// looked up is open decision #93.
    // `thiserror`'s `#[error(...)]` cannot branch on a field's value, and
    // `expected` renders differently for `Some` and `None` — so the
    // `Some`/`None` clause is built by `kind_mismatch_expected_clause` and
    // interpolated as one fragment rather than by a hand-written `Display`.
    #[error(
        "node `{}` port {port} (fed by `{}`): {}found `{found}`",
        consumer.as_str(),
        producer.as_str(),
        kind_mismatch_expected_clause(expected)
    )]
    KindMismatch {
        /// The node consuming the mismatched edge.
        consumer: NodeId,
        /// The position, within `consumer`'s `inputs`, of the mismatched
        /// edge.
        port: usize,
        /// The node — or, since ADR-C18, the declared pipeline input —
        /// producing the value on the mismatched edge.
        producer: NodeId,
        /// The kind `consumer` declares at `port`, or `None` when `port` is
        /// beyond what a fixed-arity variant declares.
        expected: Option<ValueKind>,
        /// The kind `producer` actually produces.
        found: ValueKind,
    },
}

/// The part of [`ValidationError::KindMismatch`]'s message that depends on
/// whether a port genuinely exists at all: `expected `X`, ` when it does, or
/// a note that none was declared at that position when it does not. Pulled
/// out of the `#[error(...)]` attribute because that attribute cannot branch
/// on a field's value the way a hand-written `Display` could.
fn kind_mismatch_expected_clause(expected: &Option<ValueKind>) -> String {
    match expected {
        Some(expected) => format!("expected `{expected}`, "),
        None => "no port declared at this position, ".to_string(),
    }
}

/// Lowers one [`RawParamValue`] into the matching [`ParamValue`], recursing
/// through `List`.
///
/// Rejects non-finite floats (`NaN`, `±∞`) with [`ValidationError::NonFiniteParam`]
/// and normalizes `-0.0` to `0.0` (settled reading A3): `#9` produces the
/// canonical form, `#10` only hashes it, and hashing a raw `to_bits()` would
/// otherwise give two content hashes to two values `ParamValue` calls equal
/// (INV-8).
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
/// 2. sorts the node list by [`NodeId`] — the only reordering this function
///    ever performs (see the canonicalization contract on
///    [`LogicalPipeline`] for exactly what does, and does not, converge) —
///    so every check below, and which of two faults a malformed graph
///    reports, runs in a deterministic order regardless of how the source
///    `RawPipeline` listed its nodes;
/// 3. checks every id is unique ([`ValidationError::DuplicateId`]) —
///    referential integrity is meaningless without unique ids;
/// 4. checks the pipeline declares exactly one input
///    ([`ValidationError::InputArity`]) and that no declaration claims a node's
///    id ([`ValidationError::InputCollidesWithNode`]), ADR-C18;
/// 5. checks every `inputs` entry names a node or a declared input
///    ([`ValidationError::DanglingInput`]);
/// 6. checks the graph of data edges is acyclic
///    ([`ValidationError::Cycle`]);
/// 7. checks every edge's value kinds line up ([`check_kinds`], ADR-C16),
///    skipping an `Extension` node on either side of an edge
///    ([`ValidationError::KindMismatch`]).
pub fn validate(raw: RawPipeline) -> Result<LogicalPipeline, ValidationError> {
    // No schema-version check here: every inhabitant of `SchemaVersion` is a
    // version this build reads, established by `SchemaVersion::new` and by
    // the `Deserialize` that routes through it, so a document that got this
    // far is in a grammar this build understands.
    let inputs: Vec<NodeId> = raw.pipeline.inputs.into_iter().map(NodeId::new).collect();

    let mut nodes = raw
        .pipeline
        .nodes
        .into_iter()
        .map(lower_node)
        .collect::<Result<Vec<_>, _>>()?;

    // Sorted first, before any check runs: two `RawPipeline`s listing the
    // same nodes in different order must canonicalize to the same
    // `LogicalPipeline` (INV-8), and every check below must report the same
    // fault regardless of the order the source `RawPipeline` happened to
    // list nodes in. This sorts the *node list* only, by `NodeId` — it never
    // touches a node's `inputs`, which stays exactly as the source listed it
    // (ADR-C16). A node's `params` is already canonical (`Params` is a
    // `BTreeMap`), and non-finite floats were already rejected and `-0.0`
    // already normalized during lowering (settled reading A3), so nothing
    // further needs doing here.
    nodes.sort_by(|a, b| a.id().cmp(b.id()));

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

    // The declared inputs, checked once the node ids are known so that the
    // collision below can be decided, and before the checks that would
    // otherwise resolve the ambiguous id silently. A colliding id is never
    // *dangling* — it resolves through `index` — so what the order actually
    // buys is that `check_kinds` cannot read it as the node, derive `Chunks`
    // from it, and report a kind fault caused by an ambiguity it never
    // mentioned. `find_cycle` likewise reads it as the node, and a
    // self-referential collision surfaces as a cycle that exists only
    // because of the ambiguity.
    if inputs.len() != 1 {
        return Err(ValidationError::InputArity {
            declared: inputs.len(),
        });
    }
    let declared: HashSet<&NodeId> = inputs.iter().collect();
    for input in &inputs {
        if index.contains_key(input) {
            return Err(ValidationError::InputCollidesWithNode { id: input.clone() });
        }
    }

    for node in &nodes {
        for input in node.inputs() {
            if !index.contains_key(input) && !declared.contains(input) {
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

    check_kinds(&nodes, &index, &declared)?;

    Ok(LogicalPipeline::new(inputs, nodes))
}

/// Checks every edge's value kinds line up (ADR-C16), given `nodes` is
/// already known to be acyclic and every `inputs` entry already known to
/// resolve (`index` maps a [`NodeId`] to its position in `nodes`).
///
/// An `inputs` entry `index` does not know is a **declared pipeline input**
/// (ADR-C18) rather than a fault: it produces [`ValueKind::Query`], which is
/// the whole reason the declaration exists, since no [`LogicalNode`] variant
/// produces one. `declared` is what says so — membership is tested, not
/// inferred from `index` missing, so this loop stays clause for clause the
/// same as `ragondin-engine`'s mirror of it.
///
/// For each node, for each `(position, input_id)` in its `inputs`, this first
/// derives `expected` — the kind [`consumed_kinds`] declares the consumer
/// wants at `position`, `None` when `position` is beyond what a fixed-arity
/// variant declares. That derivation depends only on the *consumer's* own
/// variant, so it runs **regardless of what feeds the port**: an
/// [`LogicalNode::Extension`] producer makes the producer's *kind* unknown to
/// the core (ADR-C16), but says nothing about the consumer's arity, which
/// stays fully known. A position `inputs` reaches but that a fixed-arity
/// variant does not declare is therefore always a
/// [`ValidationError::KindMismatch`] with `expected: None`, whatever produces
/// it — [`produced_kind`] is representable even for an `Extension` producer
/// ([`ValueKind::Opaque`]), so `found` is reported normally.
///
/// Only once a port is known to exist (`expected` is `Some`) does an
/// `Extension` producer's unknowable kind excuse the edge from the
/// *comparison* that follows: guessing which kind an `Extension` yields is
/// exactly what ADR-C16 reserves for physical planning. No build performs that
/// guess — `ragondin-engine`'s `plan_physical` refuses every `Extension` node
/// instead, there being no extension registry to resolve one through (open
/// decision #93). A missing input — a position [`PortSpec::Fixed`] declares but
/// `inputs` does not reach — is not checked here (settled reading A2). An
/// [`LogicalNode::Extension`] *consumer* is skipped entirely, at the top of the
/// outer loop: its [`PortSpec`] is [`PortSpec::Unknown`], which ADR-C16 says
/// the core cannot state.
fn check_kinds(
    nodes: &[LogicalNode],
    index: &HashMap<NodeId, usize>,
    declared: &HashSet<&NodeId>,
) -> Result<(), ValidationError> {
    for node in nodes {
        if matches!(node, LogicalNode::Extension(_)) {
            continue;
        }
        let spec = consumed_kinds(node);

        for (position, input_id) in node.inputs().iter().enumerate() {
            let expected = match &spec {
                PortSpec::Fixed(kinds) => kinds.get(position).copied(),
                PortSpec::Variadic(kind) => Some(*kind),
                PortSpec::Unknown => {
                    unreachable!(
                        "only Extension returns PortSpec::Unknown, and it is skipped above"
                    )
                }
            };

            let found = match index.get(input_id) {
                Some(&at) => {
                    let producer = &nodes[at];
                    // The consumer's arity is checked above regardless of the
                    // producer's variant. Only when a port genuinely exists
                    // does an Extension producer's kind stay unguessed.
                    if expected.is_some() && matches!(producer, LogicalNode::Extension(_)) {
                        continue;
                    }
                    produced_kind(producer)
                }
                // A declared pipeline input. Its kind follows from the kind
                // of graph, never from anything a configuration writes
                // (ADR-C16, ADR-C18).
                None if declared.contains(input_id) => ValueKind::Query,
                // The dangling check ran at the call site and left no third
                // possibility. Tested rather than inferred by elimination:
                // this keeps the clause identical to `ragondin-engine`'s
                // mirror of this loop, and keeps the coupling to the arity
                // rule local — the day `InputArity` relaxes and a declared
                // input may be some kind other than `Query`, whoever changes
                // the arm above is standing next to the reason it was safe.
                None => unreachable!(
                    "input `{}` of node `{}` resolves to neither a node nor a \
                     declared input, which the dangling check refuses first",
                    input_id.as_str(),
                    node.id().as_str()
                ),
            };

            if expected != Some(found) {
                return Err(ValidationError::KindMismatch {
                    consumer: node.id().clone(),
                    port: position,
                    producer: input_id.clone(),
                    expected,
                    found,
                });
            }
        }
    }
    Ok(())
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
/// before this is called), so a lookup that misses is a declared pipeline
/// input (ADR-C18) rather than a fault — and a source, which no cycle can
/// contain.
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
            // A declared pipeline input resolves (dangling inputs are
            // checked before this runs) but has no node, so it has no
            // `inputs` of its own and the walk stops there. Nothing is
            // masked: a cycle is a closed walk, so every element of one must
            // have a successor in this traversal.
            let Some(&next) = index.get(input) else {
                continue;
            };
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
    use crate::kind::ValueKind;
    use crate::raw::{RawGraph, SchemaVersion};

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

    /// A `RawPipeline` declaring the one input ADR-C18 requires of a serving
    /// graph, named `question`. Nothing is obliged to consume it.
    fn pipeline(nodes: Vec<RawNode>) -> RawPipeline {
        pipeline_declaring(&["question"], nodes)
    }

    /// A `RawPipeline` declaring exactly the inputs given — what the arity and
    /// collision checks need, and what `pipeline` hides behind the single
    /// declaration every other test wants.
    fn pipeline_declaring(inputs: &[&str], nodes: Vec<RawNode>) -> RawPipeline {
        RawPipeline {
            version: SchemaVersion::CURRENT,
            pipeline: RawGraph {
                inputs: inputs.iter().map(|s| s.to_string()).collect(),
                nodes,
            },
        }
    }

    // --- ADR-C18: the pipeline's own input declaration ---

    #[test]
    fn a_pipeline_declaring_no_input_is_rejected() {
        let raw = pipeline_declaring(&[], vec![node("r", "retriever", &[])]);
        let err = validate(raw).unwrap_err();
        assert_eq!(err, ValidationError::InputArity { declared: 0 });
        assert!(
            err.to_string().contains("exactly one"),
            "the message must say how many are required: {err}"
        );
    }

    #[test]
    fn a_pipeline_declaring_two_inputs_is_rejected() {
        let raw = pipeline_declaring(&["question", "corpus"], vec![node("r", "retriever", &[])]);
        assert_eq!(
            validate(raw).unwrap_err(),
            ValidationError::InputArity { declared: 2 }
        );
    }

    #[test]
    fn a_repeated_declaration_is_reported_as_arity_not_as_a_duplicate() {
        // `[question, question]` declares two inputs, so the arity check
        // catches it first and there is no reachable duplicate-input fault.
        // Pinned so that a later relaxation of the arity rule is forced to
        // decide what a repeat means rather than inheriting silence.
        let raw = pipeline_declaring(&["question", "question"], vec![node("r", "retriever", &[])]);
        assert_eq!(
            validate(raw).unwrap_err(),
            ValidationError::InputArity { declared: 2 }
        );
    }

    #[test]
    fn a_declared_input_that_is_also_a_node_id_is_rejected() {
        // Ids are one namespace: a node names an input exactly as it names
        // another node, so an id resolving to both is ambiguous.
        let raw = pipeline_declaring(&["shared"], vec![node("shared", "retriever", &[])]);
        let err = validate(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::InputCollidesWithNode {
                id: NodeId::new("shared")
            }
        );
        assert!(
            err.to_string().contains("shared"),
            "the message must name the colliding id: {err}"
        );
    }

    #[test]
    fn a_collision_is_reported_ahead_of_the_fault_it_causes() {
        // The pass order is load-bearing, not incidental. `shared` is both the
        // declaration and a node that names itself, so if the collision check
        // ran later the graph would be refused as `Cycle { [shared, shared] }`
        // — a self-loop that exists only *because* of the ambiguity, which is
        // the misdiagnosis the order exists to prevent.
        let raw = pipeline_declaring(&["shared"], vec![node("shared", "retriever", &["shared"])]);
        assert_eq!(
            validate(raw).unwrap_err(),
            ValidationError::InputCollidesWithNode {
                id: NodeId::new("shared")
            }
        );
    }

    #[test]
    fn a_declaration_nothing_consumes_is_not_an_error() {
        // `validate` rejects; it does not lint. Named so that the rule has a
        // failure message of its own: every other fixture in this file relies
        // on it silently through the `pipeline` helper, so without this test
        // breaking the rule fails two dozen unrelated tests inside a helper
        // and none of them says why.
        let raw = pipeline(vec![node("lonely", "retriever", &[])]);
        let logical = validate(raw).expect("an unconsumed declaration is not a fault");
        assert_eq!(logical.inputs(), &[NodeId::new("question")]);
    }

    #[test]
    fn a_declared_input_at_a_non_zero_port_is_resolved_by_id_not_by_position() {
        // Every other declared-input edge in this file sits at port 0. Here
        // the reranker's port 1 wants `Chunks` and is handed the declaration,
        // so a resolution keyed on the port's position rather than on the id
        // would report the wrong kind — or nothing at all.
        let raw = pipeline(vec![node("rank", "reranker", &["question", "question"])]);
        assert_eq!(
            validate(raw).unwrap_err(),
            ValidationError::KindMismatch {
                consumer: NodeId::new("rank"),
                port: 1,
                producer: NodeId::new("question"),
                expected: Some(ValueKind::Chunks),
                found: ValueKind::Query,
            }
        );
    }

    #[test]
    fn a_node_consuming_the_declared_input_validates() {
        let raw = pipeline(vec![node("r", "retriever", &["question"])]);
        let logical = validate(raw).expect("a retriever fed by the declared input must validate");
        assert_eq!(logical.inputs(), &[NodeId::new("question")]);
    }

    #[test]
    fn a_reranker_wired_to_the_declared_input_and_a_fusion_validates() {
        // The shape ADR-C18 exists for: port 0 is the query, port 1 the
        // chunks, and neither needs an `Extension` to be satisfied.
        let raw = pipeline(vec![
            node("fuse", "fusion", &["leg"]),
            node("leg", "retriever", &["question"]),
            node("rank", "reranker", &["question", "fuse"]),
        ]);
        validate(raw).expect("a reranker wired [question, fuse] must validate");
    }

    #[test]
    fn a_reranker_with_its_two_ports_swapped_is_rejected() {
        // `inputs` is positional (ADR-C16) and canonicalization never
        // reorders it, so [fuse, question] is a different, invalid pipeline.
        let raw = pipeline(vec![
            node("fuse", "fusion", &["leg"]),
            node("leg", "retriever", &["question"]),
            node("rank", "reranker", &["fuse", "question"]),
        ]);
        assert_eq!(
            validate(raw).unwrap_err(),
            ValidationError::KindMismatch {
                consumer: NodeId::new("rank"),
                port: 0,
                producer: NodeId::new("fuse"),
                expected: Some(ValueKind::Query),
                found: ValueKind::Chunks,
            }
        );
    }

    #[test]
    fn a_kind_fault_on_a_declared_input_does_not_call_it_a_node() {
        // The message is the whole diagnosis a user gets, and `question` is
        // precisely not a node — `InputCollidesWithNode` guarantees it never
        // is. Telling them to look for one sends them hunting for something
        // the validator forbids.
        let raw = pipeline(vec![node("fuse", "fusion", &["question"])]);
        let message = validate(raw).unwrap_err().to_string();
        assert!(
            message.contains("fed by `question`"),
            "the message must name the producer without calling it a node: {message}"
        );
        assert!(
            !message.contains("node `question`"),
            "`question` is a declared input, not a node: {message}"
        );
    }

    #[test]
    fn a_fusion_fed_by_the_declared_input_is_a_kind_mismatch() {
        // A declared input produces `Query`; a fusion consumes `Chunks`. The
        // declaration is not a hole in the check — it participates in it.
        let raw = pipeline(vec![node("fuse", "fusion", &["question"])]);
        assert_eq!(
            validate(raw).unwrap_err(),
            ValidationError::KindMismatch {
                consumer: NodeId::new("fuse"),
                port: 0,
                producer: NodeId::new("question"),
                expected: Some(ValueKind::Chunks),
                found: ValueKind::Query,
            }
        );
    }

    #[test]
    fn a_declared_input_is_never_reported_as_a_dangling_input() {
        let raw = pipeline(vec![node("r", "retriever", &["question"])]);
        assert!(validate(raw).is_ok());
    }

    #[test]
    fn a_declared_input_is_a_source_and_cannot_be_on_a_cycle() {
        // The cycle search walks a node to each id in its `inputs`; a
        // declared input has no node, so the walk must stop there rather
        // than index a position that does not exist.
        let raw = pipeline(vec![
            node("a", "retriever", &["question"]),
            node("b", "fusion", &["a"]),
        ]);
        validate(raw).expect("a graph rooted at the declared input is acyclic");
    }

    #[test]
    fn the_declaration_survives_canonicalization_unchanged() {
        let raw = pipeline(vec![
            node("zeta", "retriever", &["question"]),
            node("alpha", "retriever", &["question"]),
        ]);
        let logical = validate(raw).expect("the fixture must validate");
        // The node list is sorted; the declaration is not touched.
        let ids: Vec<&str> = logical.nodes().iter().map(|n| n.id().as_str()).collect();
        assert_eq!(ids, vec!["alpha", "zeta"]);
        assert_eq!(logical.inputs(), &[NodeId::new("question")]);
    }

    #[test]
    fn a_document_that_states_no_version_validates() {
        // A configuration writes `version:` only to pin one deliberately;
        // saying nothing means the version this build writes. Deserialized
        // rather than built, so the default is exercised through the door a
        // real configuration comes in by.
        let raw: RawPipeline = serde_yaml::from_str(
            "pipeline:\n  inputs: [question]\n  nodes:\n    - id: r\n      component: retriever\n      impl: bm25\n      inputs: [question]\n",
        )
        .expect("an unversioned document must parse");
        assert_eq!(raw.version, SchemaVersion::CURRENT);
        validate(raw).expect("and it must validate");
    }

    #[test]
    fn a_valid_hybrid_graph_validates() {
        // Raw deliberately lists `rrf` first, out of `NodeId` order: this
        // pins deterministic ordering as well as the original acceptance
        // criterion — the sorted output must not depend on the order the
        // source `RawPipeline` happened to list nodes in.
        let raw = pipeline(vec![
            node("rrf", "fusion", &["bm25_leg", "dense_leg"]),
            node("dense_leg", "retriever", &[]),
            node("bm25_leg", "retriever", &[]),
        ]);
        let logical = validate(raw).expect("a valid hybrid graph must validate");
        let ids: Vec<&str> = logical.nodes().iter().map(|n| n.id().as_str()).collect();
        assert_eq!(ids, vec!["bm25_leg", "dense_leg", "rrf"]);

        // The node list is sorted, but a fusion's `inputs` are positional and
        // order-significant (ADR-C16, INV-8) and must never be touched by
        // that sort. `assert_ne` elsewhere (below) catches a reordering that
        // swaps the two legs; this pins the actual, positive shape, which an
        // injective mangling such as reversal could still satisfy.
        let rrf = logical
            .nodes()
            .iter()
            .find(|n| n.id().as_str() == "rrf")
            .expect("rrf must be present");
        assert_eq!(
            rrf.inputs(),
            &[NodeId::new("bm25_leg"), NodeId::new("dense_leg")],
            "the fusion's inputs must stay exactly [bm25_leg, dense_leg]"
        );
    }

    #[test]
    fn an_empty_inputs_list_is_not_rejected() {
        // Settled reading A2: a node with no `inputs` is not an error — how
        // few a variant declares stays unchecked here, and is the executor's
        // (#16). ADR-C18 did not change that: it gave the query a producer to
        // be wired to, and did not start enforcing arity.
        let raw = pipeline(vec![node("lonely", "retriever", &[])]);
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
    fn which_of_two_dangling_inputs_is_reported_does_not_depend_on_source_order() {
        // Two nodes, each with its own dangling input. `zeta` is listed
        // first in the source `RawPipeline`; `alpha` sorts first by
        // `NodeId`. If the structural checks ran on source order (as they
        // did before nodes were sorted first), this would report `zeta`'s
        // fault instead — making which of two faults a malformed graph
        // reports depend on the order the YAML happened to list nodes in.
        let raw = pipeline(vec![
            node("zeta", "retriever", &["missing_z"]),
            node("alpha", "retriever", &["missing_a"]),
        ]);
        let err = validate(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::DanglingInput {
                node: NodeId::new("alpha"),
                missing: NodeId::new("missing_a"),
            },
            "the fault on the alphabetically-first node must be reported, regardless of \
             the order the source RawPipeline listed nodes in"
        );
    }

    #[test]
    fn a_self_loop_is_a_cycle_not_a_dangling_input() {
        // `a` names itself, which exists — so this must not read as a
        // dangling input, only as a cycle.
        let raw = pipeline(vec![node("a", "retriever", &["a"])]);
        let err = validate(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::Cycle {
                nodes: vec![NodeId::new("a")]
            },
            "a self-loop must report the single-element cycle [a], exactly"
        );
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

    // --- canonicalization ---

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
        // Deserialized from two YAML documents whose `params` keys are
        // written in opposite order and whose nodes are listed in opposite
        // order, exercising the whole path end to end. A `BTreeMap` built
        // directly in Rust (the previous form of this test) is already equal
        // before `validate` ever runs, so it cannot fail no matter what the
        // code does — proving nothing. Going through `serde_yaml` means the
        // key order in the source text is what differs, not two
        // already-equal maps.
        let forward = r#"
version: 2
pipeline:
  inputs: [question]
  nodes:
    - id: bm25_leg
      component: retriever
      impl: bm25
      params:
        top_k: 50
        alpha: 0.5
    - id: dense_leg
      component: retriever
      impl: dense
      params:
        top_k: 50
        alpha: 0.5
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [bm25_leg, dense_leg]
      params:
        top_k: 50
        alpha: 0.5
"#;
        // Same nodes, listed in reverse, each with its `params` keys written
        // in the opposite order.
        let reversed = r#"
version: 2
pipeline:
  inputs: [question]
  nodes:
    - id: rrf
      component: fusion
      impl: rrf
      inputs: [bm25_leg, dense_leg]
      params:
        alpha: 0.5
        top_k: 50
    - id: dense_leg
      component: retriever
      impl: dense
      params:
        alpha: 0.5
        top_k: 50
    - id: bm25_leg
      component: retriever
      impl: bm25
      params:
        alpha: 0.5
        top_k: 50
"#;

        let forward: RawPipeline = serde_yaml::from_str(forward).unwrap();
        let reversed: RawPipeline = serde_yaml::from_str(reversed).unwrap();

        let a = validate(forward).expect("forward pipeline must validate");
        let b = validate(reversed).expect("reversed pipeline must validate");
        assert_eq!(
            a, b,
            "node order and param key order must not affect the canonical value"
        );
    }

    #[test]
    fn a_fusions_inputs_are_never_reordered_by_canonicalization() {
        // ADR-C16: `inputs` is positional and order-significant. `[a, b]` and
        // `[b, a]` are two different configurations, not one canonicalized to
        // the other.
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

    // --- the kind check (ADR-C16) ---

    #[test]
    fn a_reranker_consuming_chunks_at_the_query_port_fails_with_kind_mismatch() {
        // `leg` produces `Chunks`; wiring it into the reranker's port 0
        // (which `consumed_kinds` declares `Query`) must fail.
        let raw = pipeline(vec![
            node("leg", "retriever", &[]),
            node("k", "reranker", &["leg", "leg"]),
        ]);
        let err = validate(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::KindMismatch {
                consumer: NodeId::new("k"),
                port: 0,
                producer: NodeId::new("leg"),
                expected: Some(ValueKind::Query),
                found: ValueKind::Chunks,
            }
        );
        let message = err.to_string();
        assert!(
            message.contains("`k`") && message.contains("`leg`"),
            "the message must name both node ids: {message}"
        );
        assert!(
            message.contains("query") && message.contains("chunks"),
            "the message must name both the expected and found kinds: {message}"
        );
    }

    #[test]
    fn a_reranker_wired_with_an_extension_at_the_query_port_validates() {
        // Nothing in the closed primitive set produces `ValueKind::Query`
        // (see `produced_kind`), and this test once carried the reason a
        // reranker's port 0 *had* to be an `Extension`: there was nothing
        // else to feed it. ADR-C18 removed that necessity — a declared
        // pipeline input produces `Query`, and
        // `a_reranker_wired_to_the_declared_input_and_a_fusion_validates`
        // is now the proof a reranker can be wired at all.
        //
        // What this test still pins is that the escape hatch stays open: a
        // query transform is written `component: extension` today (§5.1's own
        // `query_transform` names no family `lower_node` knows, so it is an
        // `UnknownComponent`), and an `Extension` producer at a port that
        // exists is skipped by the kind check (ADR-C16). Wiring a query
        // transform between the declared input and a reranker must keep
        // validating — that is the whole point of declaring the query rather
        // than making it ambient.
        let raw = pipeline(vec![
            node("qx", "extension", &["question"]),
            node("a", "retriever", &["qx"]),
            node("b", "retriever", &["qx"]),
            node("rrf", "fusion", &["a", "b"]),
            node("k", "reranker", &["qx", "rrf"]),
        ]);
        validate(raw).expect(
            "a reranker fed by an extension at port 0 and a fusion at port 1 must validate",
        );
    }

    #[test]
    fn a_retriever_given_two_inputs_fails_with_kind_mismatch_at_port_one() {
        // A retriever's `PortSpec` is `Fixed(vec![Query])` — one port. `ext`
        // is an `Extension` at position 0, which is skipped as a producer
        // (ADR-C16), so this pins the position-1 failure on the *second*
        // input being beyond the retriever's declared arity, not on the
        // first input (nothing in the closed node set produces `Query`, so a
        // primitive there would fail at port 0 instead and this test would
        // no longer isolate the excess-arity case).
        let raw = pipeline(vec![
            node("ext", "extension", &[]),
            node("b", "retriever", &[]),
            node("r", "retriever", &["ext", "b"]),
        ]);
        let err = validate(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::KindMismatch {
                consumer: NodeId::new("r"),
                port: 1,
                producer: NodeId::new("b"),
                expected: None,
                found: ValueKind::Chunks,
            }
        );
    }

    #[test]
    fn a_fixed_arity_consumer_fed_entirely_by_extension_producers_still_fails_the_arity_check() {
        // Regression for the bug where the `Extension`-producer skip ran
        // before `expected` was computed: both `ext_a` and `ext_b` are
        // `Extension` producers, so the buggy code skipped both edges
        // outright and this pipeline validated. ADR-C16 makes an
        // `Extension`'s *kind* unknown to the core, but says nothing about
        // the *consumer's* arity, which is derived from `r`'s own variant
        // alone and is fully known regardless of what feeds each port: `r`
        // is a retriever (`Fixed(vec![Query])`, one port), so port 1 is
        // beyond its declared arity no matter which node produces it.
        let raw = pipeline(vec![
            node("ext_a", "extension", &[]),
            node("ext_b", "extension", &[]),
            node("r", "retriever", &["ext_a", "ext_b"]),
        ]);
        let err = validate(raw).unwrap_err();
        assert_eq!(
            err,
            ValidationError::KindMismatch {
                consumer: NodeId::new("r"),
                port: 1,
                producer: NodeId::new("ext_b"),
                expected: None,
                found: ValueKind::Opaque,
            }
        );
        // The `expected: None` branch of `KindMismatch`'s `Display` is
        // otherwise never rendered by any test.
        let message = err.to_string();
        assert!(
            message.contains("no port declared at this"),
            "the expected: None branch must render its own message: {message}"
        );
        assert!(
            message.contains("`r`") && message.contains("`ext_b`") && message.contains("opaque"),
            "the message must name both node ids and the found (opaque) kind: {message}"
        );
    }

    #[test]
    fn a_fusion_consuming_three_retrievers_validates() {
        let raw = pipeline(vec![
            node("a", "retriever", &[]),
            node("b", "retriever", &[]),
            node("c", "retriever", &[]),
            node("rrf", "fusion", &["a", "b", "c"]),
        ]);
        validate(raw).expect("a fusion's variadic Chunks port accepts any number of inputs");
    }

    #[test]
    fn an_extension_feeding_a_primitive_and_fed_by_one_validates() {
        // `ext` consumes `src`'s output (an `Extension` consumer's
        // `PortSpec` is `Unknown`, so this edge is skipped) and feeds `fus`
        // (an `Extension` producer yields `Opaque`, also skipped). Neither
        // edge may be falsely rejected, and neither kind may be guessed
        // (ADR-C16).
        let raw = pipeline(vec![
            node("src", "retriever", &[]),
            node("ext", "extension", &["src"]),
            node("fus", "fusion", &["ext"]),
        ]);
        validate(raw).expect("an Extension node on either side of an edge must not be rejected");
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
