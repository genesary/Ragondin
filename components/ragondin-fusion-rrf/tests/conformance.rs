//! `ragondin-fusion-rrf` against the shared conformance suite.
//!
//! The same call a third-party fusion writes, against the same suite: that is
//! what makes "no privilege for built-ins" (INV-7) a fact rather than a claim.

#![cfg(feature = "rrf")]

use ragondin_conformance::assert_fusion_conformance;
use ragondin_fusion_rrf::ReciprocalRankFusion;

#[tokio::test]
async fn reciprocal_rank_fusion_is_conformant() {
    assert_fusion_conformance(|| Box::new(ReciprocalRankFusion::default())).await;
}

#[tokio::test]
async fn conformance_does_not_depend_on_k() {
    // `k` is constructor configuration, so a build with an unusual one is a
    // different component as far as the suite is concerned, and has to pass it
    // too. `k = 0` is the edge the formula comes closest to breaking on.
    assert_fusion_conformance(|| Box::new(ReciprocalRankFusion::new(0))).await;
}
