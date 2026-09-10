//! `LogicalPipeline`: the validated, canonical value type that
//! [`crate::validate::validate`] returns.
//!
//! This is the middle level of the three described in `ARCHITECTURE.md`: it
//! sits between the permissive [`crate::RawPipeline`] and `PhysicalPipeline`.
//! That last one lives in `ragondin-engine`, not here, and always will: it
//! holds `Box<dyn Trait>`, which INV-3 forbids this crate, and the dependency
//! arrow runs from the engine to this crate — so it is named in prose rather
//! than linked.
//!
//! [`crate::validate::validate`] is what establishes the invariants this
//! type's name promises — unique node ids, exactly one declared input, no
//! declaration claiming a node's id, no dangling `inputs`, no cycle in the
//! data-flow graph — and it is the only thing that establishes them. It is
//! not the only way to obtain the type: `LogicalPipeline` derives
//! `Deserialize`, which is public and re-runs none of those checks. That is
//! not a wire path — anything arriving over one goes `RawPipeline` →
//! `validate` (INV-9) — but a value that did not come through `validate`
//! carries no guarantee, which is why `ragondin-engine`'s physical planning
//! checks every edge's kinds again rather than trusting them.

use serde::{Deserialize, Serialize};

use crate::node::{LogicalNode, NodeId};

/// A validated, canonical pipeline.
///
/// A value type (INV-3): no I/O, no global context, no interner, fully
/// determined by its content.
///
/// # The canonicalization contract
///
/// This is exactly what [`crate::validate::validate`] normalizes, and exactly
/// what it leaves alone — #10 hashes this value directly, so this statement
/// is what makes INV-8 ("two semantically equivalent configurations
/// formatted differently must hash identically") true.
///
/// **Normalized:**
/// - **Node order.** The node list is sorted by [`crate::NodeId`], regardless
///   of the order the source `RawPipeline` listed them in.
/// - **Param key order.** Already canonical before this type exists —
///   [`crate::Params`] is a `BTreeMap`, which iterates in sorted key order
///   independent of insertion order.
/// - **`-0.0`.** Normalized to `0.0` during lowering (not here, see
///   [`crate::validate::validate`]), so `Float(-0.0)` never reaches this
///   type.
/// - **The wire `SchemaVersion`.** Discarded during lowering and never
///   carried into this type. Deliberate, not an oversight: the schema version
///   says only what this crate can *read*, not what pipeline it produces, so
///   storing it here would rehash every unchanged pipeline on a schema bump
///   alone — the opposite of what INV-8 asks for.
///
/// **Never normalized — reordering either would silently change the
/// configuration, not canonicalize it:**
/// - **A node's `inputs`.** Positional and order-significant (ADR-C16): a
///   fusion consuming `[a, b]` and one consuming `[b, a]` are two different
///   pipelines, never sorted or deduplicated into one.
/// - **The pipeline's own `inputs`.** The same rule, for the same reason
///   (ADR-C18): the declared inputs are the graph's signature, positional
///   like a node's, and #10 hashes this value directly. A serving pipeline
///   declares exactly one today, so the ordering is not yet observable —
///   which is precisely why the rule is written down now rather than
///   discovered later.
/// - **A node's `params` values**, e.g. the elements of a `List` — order
///   within a list is part of the value.
///
/// Carries no hash (content hashing is #10) and no port kinds: a node's
/// `ValueKind`s are derived from its [`LogicalNode`] variant
/// ([`crate::produced_kind`], [`crate::consumed_kinds`]) and never stored
/// here, never serialized, never hashed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogicalPipeline {
    inputs: Vec<NodeId>,
    nodes: Vec<LogicalNode>,
}

impl LogicalPipeline {
    /// Wraps an already-validated declaration and node list.
    ///
    /// Not exposed outside this crate: [`crate::validate::validate`] is the
    /// only place that establishes referential integrity and acyclicity, so
    /// it is the only path from a `RawPipeline` to a `LogicalPipeline` that
    /// establishes them. It is not the only way to obtain the type — the
    /// public `Deserialize` derive produces one and checks nothing, as this
    /// module's documentation says.
    pub(crate) fn new(inputs: Vec<NodeId>, nodes: Vec<LogicalNode>) -> Self {
        Self { inputs, nodes }
    }

    /// The values this pipeline receives from its caller, in the order the
    /// source configuration declared them (ADR-C18).
    ///
    /// These are the ids a node may name in its `inputs` besides another
    /// node's: they are what gives [`crate::ValueKind::Query`] a producer,
    /// which no [`LogicalNode`] variant is. A serving pipeline declares
    /// exactly one, of kind `Query` — the kind follows from the graph, and is
    /// never written in a configuration (ADR-C16).
    pub fn inputs(&self) -> &[NodeId] {
        &self.inputs
    }

    /// The pipeline's nodes, sorted by [`crate::NodeId`] (see the
    /// canonicalization contract above) regardless of the order the source
    /// `RawPipeline` listed them in.
    pub fn nodes(&self) -> &[LogicalNode] {
        &self.nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constructor_preserves_the_declaration_verbatim() {
        // The canonicalization contract above says declared inputs are never
        // reordered or deduplicated. `validate` currently permits exactly one,
        // so that rule is unobservable through it — and a "tidy up the
        // canonical form" refactor that sorted them would pass every other
        // test in this crate. This pins the constructor instead, which is
        // where #10 hashes the value, so the rule is mechanical now rather
        // than the day arity relaxes.
        let pipeline = LogicalPipeline::new(
            vec![NodeId::new("b"), NodeId::new("a"), NodeId::new("b")],
            Vec::new(),
        );
        assert_eq!(
            pipeline.inputs(),
            &[NodeId::new("b"), NodeId::new("a"), NodeId::new("b")],
            "the declaration is the graph's signature: neither sorted nor deduplicated"
        );
    }
}
