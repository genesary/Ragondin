//! # rag-pipeline
//!
//! The pipeline representation. Eventually three levels — `RawPipeline` →
//! `LogicalPipeline` → `PhysicalPipeline` — of which **only the logical node
//! model exists today**; see [`node`].
//!
//! This crate is a **stable API boundary** (INV-1) and holds **value types
//! only** (INV-3). The content hash will be computed over the **canonical
//! logical form**, never over source text (INV-8), and the wire format is kept
//! **separate** from this in-memory representation (INV-9) — so the `serde`
//! derives here are for internal round-tripping, not for the wire.
//!
//! Not here yet, each owned by its own issue: `RawPipeline` (the permissive
//! deserialization target), validation and canonicalization, content hashing,
//! `PhysicalPipeline`, and the `Branch`/`Loop` control-flow nodes.
//!
//! See `ARCHITECTURE.md`.

#![warn(missing_docs)]

pub mod node;
pub mod raw;

pub use node::{
    ExtensionNode, FusionNode, LogicalNode, NodeId, ParamValue, Params, RerankerNode, RetrieverNode,
};

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("rag-"), "unexpected crate name: {name}");
    }
}
