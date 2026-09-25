//! `ragondin-context-concat` against the shared conformance suite.
//!
//! The same call a third-party context builder writes, against the same suite:
//! that is what makes "no privilege for built-ins" (INV-7) a fact rather than a
//! claim.

#![cfg(feature = "concat")]

use ragondin_conformance::assert_context_builder_conformance;
use ragondin_context_concat::ConcatContextBuilder;

#[tokio::test]
async fn concat_context_builder_is_conformant() {
    assert_context_builder_conformance(|| Box::new(ConcatContextBuilder::new("\n\n"))).await;
}

#[tokio::test]
async fn conformance_does_not_depend_on_the_separator() {
    // The separator is constructor configuration, so a build with an unusual
    // one is a different component as far as the suite is concerned, and has
    // to pass it too. The empty separator is the edge: nothing between chunks.
    assert_context_builder_conformance(|| Box::new(ConcatContextBuilder::new(""))).await;
}
