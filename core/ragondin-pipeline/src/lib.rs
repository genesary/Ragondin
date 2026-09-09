//! # ragondin-pipeline
//!
//! The pipeline representation, in three levels — `RawPipeline` →
//! `LogicalPipeline` → `PhysicalPipeline`. Five of them exist today:
//!
//! - [`raw`] — the permissive wire schema a configuration file lands in.
//!   Hand-maintained and independently versioned (INV-9). Never executed.
//! - [`node`] — the logical node model: validated, canonical value types.
//! - [`kind`] — `ValueKind` and the port derivation (ADR-C16): the kind of
//!   value flowing along an edge, derived from a node's [`node::LogicalNode`]
//!   variant alone — except on an edge fed by one of the pipeline's declared
//!   inputs (ADR-C18), which has no variant and produces `Query`.
//! - [`validate`] — the `RawPipeline` → `LogicalPipeline` validation and
//!   canonicalization pass: lowering, the structural checks, the kind check,
//!   and sorting the node list into canonical order.
//! - [`pipeline`] — [`pipeline::LogicalPipeline`] itself, the validated,
//!   canonical value type this pass produces.
//!
//! This crate is a **stable API boundary** (INV-1) and holds **value types
//! only** (INV-3). The content hash will be computed over the **canonical
//! logical form**, never over source text (INV-8), and the wire schema in
//! [`raw`] is kept **separate** from the in-memory model in [`node`] (INV-9) —
//! so the `serde` derives on the latter are for internal round-tripping, not
//! for the wire.
//!
//! Not here yet, each owned by its own issue: content hashing,
//! `PhysicalPipeline`, and the `Branch`/`Loop` control-flow nodes.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

pub mod kind;
pub mod node;
pub mod pipeline;
pub mod raw;
pub mod validate;

pub use kind::{consumed_kinds, produced_kind, PortSpec, ValueKind};
pub use node::{
    ExtensionNode, FusionNode, LogicalNode, NodeId, ParamValue, Params, RerankerNode, RetrieverNode,
};
pub use pipeline::LogicalPipeline;
pub use raw::{
    RawGraph, RawNode, RawParamValue, RawPipeline, SchemaVersion, UnsupportedSchemaVersion,
};
pub use validate::{validate, ValidationError};

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(
            name.starts_with("ragondin-"),
            "unexpected crate name: {name}"
        );
    }
}
