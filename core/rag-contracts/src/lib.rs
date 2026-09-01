//! # rag-contracts
//!
//! The **component contract**: the traits an external contributor implements —
//! `Chunker`, `Embedder`, `Indexer`, `Retriever`, `Fusion`, `Reranker`,
//! `ContextBuilder`, `Generator`, `Grader`, `VectorStore`.
//!
//! This crate is a **stable API boundary** (INV-1) and **the crate a
//! contributor compiles against**. It must stay light (INV-4): implementing a
//! component requires only this crate and `rag-types`, never the engine.
//!
//! The traits themselves land in a later issue. What exists today is the
//! compiling skeleton plus the placeholder error type every boundary will
//! return. See `ARCHITECTURE.md`.

use thiserror::Error;

/// The error type a component returns.
///
/// Placeholder for now: concrete, per-boundary variants are added alongside the
/// traits that produce them, in a later issue. `#[non_exhaustive]` so that
/// adding those variants is not a breaking change to this stable API boundary.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ComponentError {
    /// An unspecified component failure.
    #[error("component error: {0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_error_displays_its_message() {
        let err = ComponentError::Other("boom".to_string());
        assert_eq!(err.to_string(), "component error: boom");
    }
}
