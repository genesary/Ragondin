//! `ragondin-store-memory` against the shared conformance suite.
//!
//! The same call a third-party vector store writes, against the same suite:
//! that is what makes "no privilege for built-ins" (INV-7) a fact rather than a
//! claim.

#![cfg(feature = "memory")]

use ragondin_conformance::assert_vector_store_conformance;
use ragondin_store_memory::MemoryVectorStore;

#[tokio::test]
async fn the_memory_store_is_conformant() {
    assert_vector_store_conformance(|| Box::new(MemoryVectorStore::new()), 3).await;
}
