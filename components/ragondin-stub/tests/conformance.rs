//! `ragondin-stub` against the shared conformance suite.
//!
//! A stub is held to the contract exactly as a real component is: the same call
//! a third-party crate writes, against the same suite (INV-7). That matters
//! more here than elsewhere — a fixture that quietly broke the contract would
//! make every pipeline built on it prove the wrong thing.

#![cfg(feature = "stub")]

use ragondin_conformance::{assert_fusion_conformance, assert_retriever_conformance};
use ragondin_stub::{StubFusion, StubRetriever};

#[tokio::test]
async fn the_stub_retriever_is_conformant() {
    assert_retriever_conformance(|| Box::new(StubRetriever::new("conformance"))).await;
}

#[tokio::test]
async fn the_stub_fusion_is_conformant() {
    assert_fusion_conformance(|| Box::new(StubFusion)).await;
}
