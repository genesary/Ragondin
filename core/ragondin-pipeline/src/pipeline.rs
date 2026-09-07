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
///
/// **Never normalized — reordering either would silently change the
/// configuration, not canonicalize it:**
/// - **A node's `inputs`.** Positional and order-significant (ADR-C16): a
///   fusion consuming `[a, b]` and one consuming `[b, a]` are two different
///   pipelines, never sorted or deduplicated into one.
/// - **A node's `params` values**, e.g. the elements of a `List` — order
///   within a list is part of the value.
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

    /// The pipeline's nodes, sorted by [`crate::NodeId`] (see the
    /// canonicalization contract above) regardless of the order the source
    /// `RawPipeline` listed them in.
    pub fn nodes(&self) -> &[LogicalNode] {
        &self.nodes
    }
}
