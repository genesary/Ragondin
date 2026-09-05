//! # ragondin-pipeline
//!
//! The pipeline representation, in three levels — `RawPipeline` →
//! `LogicalPipeline` → `PhysicalPipeline`. Two of them exist today:
//!
//! - [`raw`] — the permissive wire schema a configuration file lands in.
//!   Hand-maintained and independently versioned (INV-9). Never executed.
//! - [`node`] — the logical node model: validated, canonical value types.
//!
//! This crate is a **stable API boundary** (INV-1) and holds **value types
//! only** (INV-3). The content hash will be computed over the **canonical
//! logical form**, never over source text (INV-8), and the wire schema in
//! [`raw`] is kept **separate** from the in-memory model in [`node`] (INV-9) —
//! so the `serde` derives on the latter are for internal round-tripping, not
//! for the wire.
//!
//! Not here yet, each owned by its own issue: the `RawPipeline` →
//! `LogicalPipeline` validation and canonicalization pass, content hashing,
//! `PhysicalPipeline`, and the `Branch`/`Loop` control-flow nodes.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

pub mod node;
pub mod raw;

pub use node::{
    ExtensionNode, FusionNode, LogicalNode, NodeId, ParamValue, Params, RerankerNode, RetrieverNode,
};
pub use raw::{
    RawGraph, RawNode, RawParamValue, RawPipeline, SchemaVersion, UnsupportedSchemaVersion,
};

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
