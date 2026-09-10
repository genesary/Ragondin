//! `ValueKind` and the port derivation (ADR-C16).
//!
//! ADR-C16 derives the kind of value flowing along each edge from the
//! consuming/producing node's [`LogicalNode`] variant alone, so no port
//! declaration ever appears in a configuration and nothing enters the
//! canonical form or the content hash (INV-8). This module is that
//! derivation, written once: `LogicalPipeline` validation here and
//! `ragondin-engine`'s physical planning both call the same functions.
//!
//! **One edge is not derived from a variant.** Since ADR-C18 a pipeline
//! declares its inputs, and a declared input produces [`ValueKind::Query`] —
//! fixed by the kind of graph, never written in a configuration. It has no
//! [`LogicalNode`], so it comes from neither [`produced_kind`] nor
//! [`consumed_kinds`]; the two callers supply it, and each tests membership
//! in the pipeline's declaration rather than inferring it.

use std::fmt;

use crate::node::LogicalNode;

/// The kind of value travelling along one edge of a pipeline graph.
///
/// Deliberately **coarse** and **parameterless** (ADR-C16): it names what
/// kind of thing an edge carries — never an embedding dimensionality, a
/// chunk provenance, or any other detail. It is derived from a node's
/// [`LogicalNode`] variant by [`produced_kind`] and [`consumed_kinds`] — with
/// the one exception named in this module's documentation, a declared
/// pipeline input, which has no variant to derive from — and
/// it never enters the canonical form: it is not `Serialize`/`Deserialize`,
/// never stored in `LogicalPipeline`, never hashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// The question travelling into a retriever or a reranker.
    Query,
    /// A list of retrieved chunks, scored or not.
    Chunks,
    /// The kind of a value produced or consumed by an
    /// [`ExtensionNode`](crate::node::ExtensionNode), unknown to the core by
    /// construction (ADR-C16).
    Opaque,
}

impl fmt::Display for ValueKind {
    /// Renders a stable, human-readable string. Pinned exactly, because a
    /// later `KindMismatch` validation error names the expected and found
    /// kinds with this rendering.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Query => "query",
            Self::Chunks => "chunks",
            Self::Opaque => "opaque",
        };
        f.write_str(s)
    }
}

/// The shape of the ports a [`LogicalNode`] variant consumes.
///
/// Describes, without checking anything itself, what a variant's `inputs`
/// must look like once matched against [`ValueKind`]s: an exact sequence, an
/// unbounded repetition of one kind, or — for
/// [`ExtensionNode`](crate::node::ExtensionNode) — a shape the core cannot
/// state at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortSpec {
    /// Exactly these kinds, in this order. An input beyond the last kind
    /// listed is a `KindMismatch`; a missing input is not this derivation's
    /// concern (that question belongs to #16).
    Fixed(Vec<ValueKind>),
    /// Any number of ports, all of the given kind.
    Variadic(ValueKind),
    /// The core cannot say: an [`ExtensionNode`](crate::node::ExtensionNode)'s
    /// consumed kinds are resolved only once its registry entry is known
    /// (ADR-C16), so nothing is checked at this level.
    Unknown,
}

/// The kind of value a node produces on its output edge, derived solely from
/// its [`LogicalNode`] variant (ADR-C16).
pub fn produced_kind(node: &LogicalNode) -> ValueKind {
    match node {
        LogicalNode::Retriever(_) => ValueKind::Chunks,
        LogicalNode::Fusion(_) => ValueKind::Chunks,
        LogicalNode::Reranker(_) => ValueKind::Chunks,
        LogicalNode::Extension(_) => ValueKind::Opaque,
    }
}

/// The shape of the ports a node consumes on its input edges, derived solely
/// from its [`LogicalNode`] variant (ADR-C16).
pub fn consumed_kinds(node: &LogicalNode) -> PortSpec {
    match node {
        LogicalNode::Retriever(_) => PortSpec::Fixed(vec![ValueKind::Query]),
        // Positional pair, query first: `node.rs`'s module doc states this
        // order, and canonicalization must never reorder `inputs` to match it.
        LogicalNode::Reranker(_) => PortSpec::Fixed(vec![ValueKind::Query, ValueKind::Chunks]),
        LogicalNode::Fusion(_) => PortSpec::Variadic(ValueKind::Chunks),
        LogicalNode::Extension(_) => PortSpec::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use crate::node::{ExtensionNode, FusionNode, NodeId, Params, RerankerNode, RetrieverNode};
    use crate::{consumed_kinds, produced_kind, LogicalNode, PortSpec, ValueKind};

    fn retriever() -> LogicalNode {
        LogicalNode::Retriever(RetrieverNode {
            id: NodeId::new("r"),
            implementation: "bm25".to_string(),
            inputs: vec![NodeId::new("question")],
            params: Params::new(),
        })
    }

    fn fusion() -> LogicalNode {
        LogicalNode::Fusion(FusionNode {
            id: NodeId::new("f"),
            implementation: "reciprocal_rank_fusion".to_string(),
            inputs: vec![NodeId::new("bm25_leg"), NodeId::new("dense_leg")],
            params: Params::new(),
        })
    }

    fn reranker() -> LogicalNode {
        LogicalNode::Reranker(RerankerNode {
            id: NodeId::new("k"),
            implementation: "bge_reranker".to_string(),
            inputs: vec![NodeId::new("question"), NodeId::new("f")],
            params: Params::new(),
        })
    }

    fn extension() -> LogicalNode {
        LogicalNode::Extension(ExtensionNode {
            id: NodeId::new("x"),
            kind: "hyde".to_string(),
            inputs: vec![NodeId::new("question")],
            params: Params::new(),
        })
    }

    #[test]
    fn a_retriever_produces_chunks() {
        assert_eq!(produced_kind(&retriever()), ValueKind::Chunks);
    }

    #[test]
    fn a_fusion_produces_chunks() {
        assert_eq!(produced_kind(&fusion()), ValueKind::Chunks);
    }

    #[test]
    fn a_reranker_produces_chunks() {
        assert_eq!(produced_kind(&reranker()), ValueKind::Chunks);
    }

    #[test]
    fn an_extension_produces_an_opaque_kind() {
        assert_eq!(produced_kind(&extension()), ValueKind::Opaque);
    }

    #[test]
    fn a_retriever_consumes_exactly_one_query_port() {
        assert_eq!(
            consumed_kinds(&retriever()),
            PortSpec::Fixed(vec![ValueKind::Query])
        );
    }

    #[test]
    fn a_reranker_consumes_a_query_then_a_chunks_port_in_that_order() {
        // The positional pair `node.rs`'s module doc requires: query first,
        // chunks second. Swapping the order here must fail this test.
        assert_eq!(
            consumed_kinds(&reranker()),
            PortSpec::Fixed(vec![ValueKind::Query, ValueKind::Chunks])
        );
    }

    #[test]
    fn a_fusion_consumes_any_number_of_chunks_ports() {
        assert_eq!(
            consumed_kinds(&fusion()),
            PortSpec::Variadic(ValueKind::Chunks)
        );
    }

    #[test]
    fn an_extensions_consumed_kinds_are_unknown_to_the_core() {
        assert_eq!(consumed_kinds(&extension()), PortSpec::Unknown);
    }

    #[test]
    fn value_kind_display_renders_stable_human_readable_strings() {
        // Pinned exactly: `KindMismatch`'s message depends on these strings,
        // so a drift here is a silent message change there.
        assert_eq!(ValueKind::Query.to_string(), "query");
        assert_eq!(ValueKind::Chunks.to_string(), "chunks");
        assert_eq!(ValueKind::Opaque.to_string(), "opaque");
    }
}
