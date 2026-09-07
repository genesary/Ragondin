//! `LogicalPipeline`: the validated, canonical value type (#9).
//!
//! This is the middle level of the three described in `ARCHITECTURE.md`: it
//! sits between the permissive [`crate::RawPipeline`] and the
//! not-yet-built `PhysicalPipeline`. Produced only by
//! [`crate::validate::validate`], which is what actually establishes the
//! invariants this type's name promises — unique node ids, no dangling
//! `inputs`, no cycle in the data-flow graph.

use serde::{Deserialize, Serialize};

use crate::node::LogicalNode;

/// A validated, canonical pipeline.
///
/// A value type (INV-3): no I/O, no global context, no interner, fully
/// determined by its content. Node order is exactly the order the source
/// `RawPipeline` listed the nodes in — canonical (e.g. topological)
/// ordering is a later task's job, so this type never sorts, reorders, or
/// deduplicates what it is given.
///
/// Carries no hash (content hashing is #10) and no port kinds: a node's
/// `ValueKind`s are derived from its [`LogicalNode`] variant
/// ([`crate::produced_kind`], [`crate::consumed_kinds`]) and never stored
/// here, never serialized, never hashed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogicalPipeline {
    nodes: Vec<LogicalNode>,
}

impl LogicalPipeline {
    /// Wraps an already-validated node list.
    ///
    /// Not exposed outside this crate: [`crate::validate::validate`] is the
    /// only place that establishes referential integrity and acyclicity, so
    /// it is the only legitimate way to produce a `LogicalPipeline` from a
    /// `RawPipeline`.
    pub(crate) fn new(nodes: Vec<LogicalNode>) -> Self {
        Self { nodes }
    }

    /// The pipeline's nodes, in the order the source `RawPipeline` listed
    /// them.
    pub fn nodes(&self) -> &[LogicalNode] {
        &self.nodes
    }
}
