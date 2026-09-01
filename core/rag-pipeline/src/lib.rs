//! # rag-pipeline
//!
//! The pipeline representation, in three levels:
//! `RawPipeline` → `LogicalPipeline` → `PhysicalPipeline`. Holds the node
//! graph, first-class control flow (`Branch`, `Loop`), the open `Extension`
//! variant, and canonical content hashing.
//!
//! This crate is a **stable API boundary** (INV-1) and holds **value types
//! only** (INV-3). The content hash is computed over the **canonical logical
//! form**, never over source text (INV-8), and the wire format is kept
//! **separate** from this in-memory representation (INV-9).
//!
//! The representation itself is introduced in a later issue; this is the
//! compiling skeleton. See `ARCHITECTURE.md`.

#[cfg(test)]
mod tests {
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("rag-"), "unexpected crate name: {name}");
    }
}
