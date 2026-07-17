//! # rag-types
//!
//! The platform's core **value types**: `Document`, `Chunk`, `Query`,
//! `Embedding`, `ScoredChunk`, `Context`, `Generation`.
//!
//! This crate is a **stable API boundary** (INV-1) and holds **value types
//! only** (INV-3): no global context, no interner, no I/O. A value is fully
//! determined by its content. It carries **no heavy dependency** (INV-4) —
//! `serde` at most.
//!
//! The types themselves are introduced in a later issue; this is the compiling
//! skeleton. See `ARCHITECTURE.md`.

#[cfg(test)]
mod tests {
    /// Skeleton smoke test: the crate compiles, links, and runs a test, so
    /// `cargo test --workspace` is green and meaningful from day one.
    #[test]
    fn skeleton_links() {
        let name = env!("CARGO_PKG_NAME");
        assert!(name.starts_with("rag-"), "unexpected crate name: {name}");
    }
}
